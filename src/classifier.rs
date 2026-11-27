//! 分類段（sorter）のシグナル収集と決定手順。
//!
//! 3層分離の原則（監査 §0）に従い、ここでは `fast_meta` が渡すレコードの
//! 「キー＋値のパターン」だけで判定する。表示ラベルや生成元の推定は使わない。
//! 存在だけのシグナル（Software の有無、XMP の有無）は数えない（REQ-CLS-11／02）。

use fast_meta::{RecordSink, RecordView, Source};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TargetType {
    ComfyUI,
    WebUI,
    NovelAI,
    SwarmUI,
    InvokeAI,
    None,
}

/// REQ-CLS-10: Software 本文・XMP CreatorTool の照合に使う AI 生成ツール名。
/// 監査 §11-1 の列挙がそのまま初期値。非AIツール名（Adobe／Photoshop／Celsys／
/// ImageReady／GIMP 等）は **入れない**（表に入れないことでシグナル化しない）。
/// 表への追加は、実サンプルでそのツールが当該フィールドに自名を書くことを確認した場合に限る。
pub const AI_TOOL_NAMES: &[&str] = &[
    "Midjourney",
    "NovelAI",
    "Stable Diffusion",
    "AUTOMATIC1111",
    "Forge",
    "InvokeAI",
    "SwarmUI",
    "Draw Things",
];

/// U-1（Phase 0 で確定）: NovelAI 設定 JSON のキー集合。
/// 2 つ以上をトップレベルに含むことを検証条件にする（REQ-CLS-01）。
const NOVELAI_SETTING_KEYS: &[&str] = &[
    "uc",
    "sampler",
    "scale",
    "seed",
    "steps",
    "strength",
    "noise",
    "noise_schedule",
    "sm",
    "sm_dyn",
    "n_samples",
    "cfg_rescale",
];

/// U-2（Phase 0 で確定）: Draw Things JSON の生成パラメータ系キー。
/// `c` に加えてこのうち 2 つ以上を持つことを検証条件にする（REQ-CLS-07）。
const DRAW_THINGS_KEYS: &[&str] = &["sampler", "steps", "seed", "scale", "model", "size"];

/// REQ-CLS-08 (b): UserComment／ImageDescription の部分一致走査範囲。
/// 範囲外の `Steps: ` 等は検出しない（仕様上の制限として T-36 で固定する）。
const TEXT_SCAN_LIMIT: usize = 64 * 1024;

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    needle.is_empty() || haystack.windows(needle.len()).any(|window| window == needle)
}

fn contains_ignore_ascii_case(haystack: &str, needle: &[u8]) -> bool {
    let haystack = haystack.as_bytes();
    needle.is_empty()
        || haystack
            .windows(needle.len())
            .any(|window| window.eq_ignore_ascii_case(needle))
}

fn limited_bytes(text: &str, limit: usize) -> &[u8] {
    let bytes = text.as_bytes();
    &bytes[..bytes.len().min(limit)]
}

/// REQ-CLS-09 (b)／REQ-EXT-14: UTF-8 BOM を取り除いてから JSON として読む。
fn parse_json(body: &str) -> Option<Value> {
    let body = body.trim_start();
    let body = body.strip_prefix('\u{feff}').unwrap_or(body);
    serde_json::from_str(body).ok()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AiToolHit {
    NovelAi,
    OtherAi,
}

/// REQ-CLS-10: AI 生成ツール名の照合（ASCII 大文字小文字を区別しない部分一致）。
/// AI 名が複数ある場合は NovelAI を優先する（§8.3 手順5が手順6より上）。
fn match_ai_tool(body: &str) -> Option<AiToolHit> {
    let mut hit = None;
    for name in AI_TOOL_NAMES {
        if contains_ignore_ascii_case(body, name.as_bytes()) {
            if *name == "NovelAI" {
                return Some(AiToolHit::NovelAi);
            }
            hit = Some(AiToolHit::OtherAi);
        }
    }
    hit
}

/// REQ-CLS-01／U-1: 本文が NovelAI 設定 JSON として検証を通るか。
fn is_novelai_json(body: &str) -> bool {
    let Some(Value::Object(map)) = parse_json(body) else {
        return false;
    };
    let matched = NOVELAI_SETTING_KEYS
        .iter()
        .filter(|key| map.contains_key(**key))
        .count();
    if matched >= 2 {
        return true;
    }
    // U-8: Stealth の NovelAI は Comment（JSON 文字列）＋ Description のパックでも成立する。
    map.get("Comment").is_some_and(Value::is_string) && map.contains_key("Description")
}

/// REQ-CLS-05／U-8: Stealth 本文が ComfyUI の構造（workflow JSON／API 形式 prompt）か。
fn is_comfy_structure_json(body: &str) -> bool {
    let Some(Value::Object(map)) = parse_json(body) else {
        return false;
    };
    let has_workflow_arrays = map.get("nodes").is_some_and(Value::is_array)
        && map.get("links").is_some_and(Value::is_array);
    if has_workflow_arrays {
        return true;
    }
    !map.is_empty()
        && map
            .values()
            .all(|value| value.get("class_type").is_some())
}

/// REQ-CLS-07／U-2: XMP UserComment が Draw Things の生成パラメータ JSON か。
fn is_draw_things_json(body: &str) -> bool {
    let Some(Value::Object(map)) = parse_json(body) else {
        return false;
    };
    if !map.get("c").is_some_and(Value::is_string) {
        return false;
    }
    DRAW_THINGS_KEYS
        .iter()
        .filter(|key| map.contains_key(**key))
        .count()
        >= 2
}

/// REQ-CLS-12／U-3: InvokeAI 旧式 Dream 構文（引用符付きプロンプト＋フラグ2つ以上）。
fn is_dream_syntax(body: &str) -> bool {
    let body = body.trim_start();
    let Some(rest) = body.strip_prefix('"') else {
        return false;
    };
    let Some(close) = rest.find('"') else {
        return false;
    };
    let args = rest[close + 1..].as_bytes();
    let mut flags = 0usize;
    let mut index = 0usize;
    while index < args.len() {
        if !args[index].is_ascii_whitespace() {
            index += 1;
            continue;
        }
        index += 1;
        if index >= args.len() || args[index] != b'-' {
            continue;
        }
        let name_start = index + 1;
        let mut cursor = name_start;
        while cursor < args.len() && args[cursor].is_ascii_alphabetic() {
            cursor += 1;
        }
        if cursor == name_start || cursor >= args.len() || !args[cursor].is_ascii_whitespace() {
            continue;
        }
        while cursor < args.len() && args[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        let value_start = cursor;
        while cursor < args.len() && !args[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor > value_start {
            flags += 1;
            index = cursor;
        }
    }
    flags >= 2
}

fn is_xmp_source(source: Source) -> bool {
    matches!(
        source,
        Source::PngXmp | Source::JpegXmp | Source::WebpXmp | Source::TiffXmp
    )
}

/// §8.1 のシグナルだけを収集する。本文の全文コピーや小文字化コピーは作らない。
#[derive(Default)]
pub struct ClassifierSink {
    // ---- 強シグナル ----
    sui: bool,
    invokeai: bool,
    easy_diffusion: bool,
    comfy_strong: bool,
    novelai_strong: bool,
    webui_strong: bool,
    // ---- 中シグナル（WebUI） ----
    webui_medium: bool,
    // ---- 弱シグナル（方向別。種類ごとに 1 票） ----
    weak_webui_steps: bool,
    weak_webui_negprompt: bool,
    weak_webui_stealth_present: bool,
    weak_webui_trailing: bool,
    weak_novelai_trailing: bool,
    weak_novelai_stealth: bool,
    /// ComfyUI 方向の弱（`usercomment_workflow_substr`／`stealth_workflow_substr`）。
    /// §8.3 の注記により複合でも確定させないため、記録のみで `decide` では使わない。
    #[allow(dead_code)]
    weak_comfy_recorded: bool,
}

impl ClassifierSink {
    /// REQ-CLS-03: UserComment／ImageDescription の A1111 本文判定（段階付け）。
    fn observe_text_block(&mut self, body: &str) {
        let window = limited_bytes(body, TEXT_SCAN_LIMIT);
        let negative = contains_bytes(window, b"Negative prompt:");
        let steps = contains_bytes(window, b"Steps: ");
        match (negative, steps) {
            (true, true) => self.webui_medium = true,
            (false, true) => self.weak_webui_steps = true,
            (true, false) => self.weak_webui_negprompt = true,
            (false, false) => {}
        }
    }

    fn observe_ai_tool(&mut self, body: &str) {
        match match_ai_tool(body) {
            // REQ-CLS-08 (a): 複製上限に依存させず、借用した本文へ直接照合する。
            Some(AiToolHit::NovelAi) => self.novelai_strong = true,
            Some(AiToolHit::OtherAi) => self.webui_medium = true,
            None => {}
        }
    }
}

impl RecordSink for ClassifierSink {
    fn on_record(&mut self, record: &RecordView<'_>) {
        let body = record.body.as_ref();

        // H-4: Trailing は trailing 系シグナル以外に効かせない。
        if record.source == Source::Trailing {
            if contains_ignore_ascii_case(body, b"novelai") {
                self.weak_novelai_trailing = true;
            }
            // REQ-CLS-06: `Software` 部分一致は外し、構造のない本文はすべて弱 1 票に統一する。
            if [
                "parameters",
                "Stable Diffusion",
                "Steps: ",
                "Negative prompt:",
            ]
            .iter()
            .any(|needle| body.contains(needle))
            {
                self.weak_webui_trailing = true;
            }
            return;
        }

        if let Some(keyword) = record.keyword.as_deref() {
            match keyword {
                "sui_image_params" | "sui_extra_data" => self.sui = true,
                "invokeai_metadata" | "sd-metadata" => self.invokeai = true,
                "Dream" => {
                    // REQ-CLS-12: 値検証を通る Dream だけを強シグナルにする。
                    if is_dream_syntax(body) {
                        self.invokeai = true;
                    }
                }
                "use_stable_diffusion_model" => self.easy_diffusion = true,
                "workflow" | "generation_data" => self.comfy_strong = true,
                "parameters" => {
                    self.webui_strong = true;
                    if body.contains("\"sui_image_params\"") {
                        self.sui = true;
                    }
                }
                "Model" => {
                    if body.contains("sui_image_params") {
                        self.sui = true;
                    }
                }
                "prompt" => {
                    // REQ-CLS-09: 同一レコードの本文だけで JSON 判定する。
                    if parse_json(body).is_some() {
                        self.comfy_strong = true;
                    }
                }
                "Comment" => {
                    // REQ-CLS-01: PNG Comment は値検証を通過したときだけ強シグナル。
                    if is_novelai_json(body) {
                        self.novelai_strong = true;
                    }
                }
                "Software" => self.observe_ai_tool(body),
                "CreatorTool" => self.observe_ai_tool(body),
                "AIPromptInformation" => {
                    if !body.trim().is_empty() {
                        self.webui_strong = true;
                    }
                }
                "DigitalSourceType" => {
                    if contains_ignore_ascii_case(body, b"trainedAlgorithmicMedia") {
                        self.webui_strong = true;
                    }
                }
                "UserComment" => {
                    // REQ-CLS-02 (d): XMP UserComment の Draw Things 検証。
                    if is_xmp_source(record.source) && is_draw_things_json(body) {
                        self.webui_strong = true;
                    }
                    // H-7: NovelAI パック（Comment＋Description）は強シグナルのまま。
                    if contains_bytes(body.as_bytes(), b"\"Comment\":")
                        && contains_bytes(body.as_bytes(), b"\"Description\":")
                    {
                        self.novelai_strong = true;
                    }
                    // REQ-CLS-03: UserComment の "workflow" 部分一致は弱（ComfyUI 方向・記録のみ）。
                    if contains_bytes(limited_bytes(body, TEXT_SCAN_LIMIT), b"\"workflow\"") {
                        self.weak_comfy_recorded = true;
                    }
                    self.observe_text_block(body);
                }
                "ImageDescription" => self.observe_text_block(body),
                // REQ-CLS-02 (e)／REQ-INV-01: description・title は分類に使わない（無効）。
                _ => {}
            }
        }

        if record.source == Source::JpegComment {
            // Phase 1 暫定: fast_meta の構造化印（REQ-API-01）が未実装のため、
            // 暫定で label 判定を残す。Phase 2 で detail の印へ置き換え、label 依存を撤去する。
            if record.label == "Stable Diffusion (A1111)" {
                self.webui_medium = true;
            }
        }

        if record.source == Source::PngStealth {
            // REQ-CLS-05: 存在そのものは WebUI 方向の弱シグナル（方向ごとに最大1票）。
            self.weak_webui_stealth_present = true;
            if is_novelai_json(body) {
                self.novelai_strong = true;
            } else if is_comfy_structure_json(body) {
                self.comfy_strong = true;
            } else {
                if contains_ignore_ascii_case(body, b"novelai") {
                    self.weak_novelai_stealth = true;
                }
                if contains_ignore_ascii_case(body, b"workflow") {
                    self.weak_comfy_recorded = true;
                }
            }
        }
    }
}

impl ClassifierSink {
    fn weak_novelai_count(&self) -> usize {
        usize::from(self.weak_novelai_trailing) + usize::from(self.weak_novelai_stealth)
    }

    fn weak_webui_count(&self) -> usize {
        usize::from(self.weak_webui_steps)
            + usize::from(self.weak_webui_negprompt)
            + usize::from(self.weak_webui_stealth_present)
            + usize::from(self.weak_webui_trailing)
    }

    /// §8.3 の決定手順。上が優先。
    pub fn decide(&self) -> Option<TargetType> {
        // 1. SwarmUI（固有キー）
        if self.sui {
            return Some(TargetType::SwarmUI);
        }
        // 2. InvokeAI（Dream は値検証済みのみ）
        if self.invokeai {
            return Some(TargetType::InvokeAI);
        }
        // 3. Easy Diffusion → WebUI
        if self.easy_diffusion {
            return Some(TargetType::WebUI);
        }
        // 4. ComfyUI（強のみ）
        if self.comfy_strong {
            return Some(TargetType::ComfyUI);
        }
        // 5. NovelAI（強のみ。検証済み Comment はここへ移動した）
        if self.novelai_strong {
            return Some(TargetType::NovelAI);
        }
        // 6. WebUI（強・中）
        if self.webui_strong || self.webui_medium {
            return Some(TargetType::WebUI);
        }
        // W. 弱の複合（方向を混ぜず、方向ごとに異なる種類 2 つ以上で確定する）
        // 同点規則: 同時成立時は NovelAI を優先する（§13.1 の意図した変更 (6)）。
        if self.weak_novelai_count() >= 2 {
            return Some(TargetType::NovelAI);
        }
        if self.weak_webui_count() >= 2 {
            return Some(TargetType::WebUI);
        }
        // ComfyUI 方向の弱は複合でも確定させない（§8.3 の注記）。記録のみ。
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;

    fn record(
        source: Source,
        label: &'static str,
        body: &'static str,
        keyword: Option<&'static str>,
    ) -> RecordView<'static> {
        RecordView {
            source,
            label: Cow::Borrowed(label),
            body: Cow::Borrowed(body),
            keyword: keyword.map(Cow::Borrowed),
            detail: None,
        }
    }

    fn classify(records: &[RecordView<'_>]) -> TargetType {
        let mut sink = ClassifierSink::default();
        for record in records {
            sink.on_record(record);
        }
        sink.decide().unwrap_or(TargetType::None)
    }

    fn png<'a>(label: &'a str, body: &'a str, keyword: &'a str) -> RecordView<'a> {
        RecordView {
            source: Source::PngText,
            label: Cow::Borrowed(label),
            body: Cow::Borrowed(body),
            keyword: Some(Cow::Borrowed(keyword)),
            detail: None,
        }
    }

    /// §10.1／§10.2 のフィクスチャ（レコード単位）。期待値は仕様書の行き先。
    /// `N-11` は REQ-CLS-04 の構造化印が Phase 2 のため除外し、個別テストで暫定挙動を固定する。
    fn fixtures() -> Vec<(&'static str, Vec<RecordView<'static>>, TargetType)> {
        use Source::*;
        vec![
            // ---- 負例（§10.1） ----
            ("N-01", vec![
                record(JpegExif, "EXIF Software", "Adobe Photoshop 25.0 (Windows)", Some("Software")),
                record(JpegXmp, "XMP CreatorTool", "Adobe Photoshop 25.0 (Windows)", Some("CreatorTool")),
            ], TargetType::None),
            ("N-02", vec![png("Software", "Celsys Studio Tool", "Software")], TargetType::None),
            ("N-03", vec![
                png("Software", "Adobe ImageReady", "Software"),
                record(PngXmp, "XMP Description", "a caption", Some("description")),
                record(PngXmp, "XMP Title", "a title", Some("title")),
            ], TargetType::None),
            ("N-04", vec![], TargetType::None),
            ("N-05", vec![record(JpegXmp, "XMP Description", "a photo caption", Some("description"))], TargetType::None),
            ("N-06", vec![record(WebpXmp, "XMP CreatorTool", "Adobe Photoshop", Some("CreatorTool"))], TargetType::None),
            ("N-07", vec![png("Comment", "edited in MyPainter 2.0", "Comment")], TargetType::None),
            ("N-08", vec![record(JpegExif, "EXIF ImageDescription", "A staircase with Steps: 3", Some("ImageDescription"))], TargetType::None),
            ("N-09", vec![record(PngStealth, "Stealth PNG Info (Alpha) [stealth_pnginfo]", "a workflow diagram, watercolor", None)], TargetType::None),
            ("N-10", vec![record(Trailing, "Trailing Data (after IEND)", "{\"Software\":\"Celsys Studio Tool\"}", None)], TargetType::None),
            ("N-12", vec![png("Comment", "{\"width\":512,\"height\":512}", "Comment")], TargetType::None),
            ("N-13", vec![record(JpegExif, "EXIF UserComment", "the workflow is described here", Some("UserComment"))], TargetType::None),
            ("N-14", vec![record(JpegExif, "EXIF ImageDescription", "Negative prompt: none used", Some("ImageDescription"))], TargetType::None),
            // ---- 正例（§10.2） ----
            ("P-01", vec![png("Stable Diffusion (A1111)", "a cat\nSteps: 20", "parameters")], TargetType::WebUI),
            ("P-02", vec![record(JpegExif, "EXIF UserComment", "a cat\nNegative prompt: blur\nSteps: 20", Some("UserComment"))], TargetType::WebUI),
            ("P-03", vec![png("workflow", "{}", "workflow")], TargetType::ComfyUI),
            ("P-03b", vec![png("prompt", "{\"nodes\":[]}", "prompt")], TargetType::ComfyUI),
            ("P-03c", vec![png("prompt", "\u{feff}{\"nodes\":[]}", "prompt")], TargetType::ComfyUI),
            ("P-04", vec![
                png("NovelAI [Settings]", "{\"steps\":28,\"sampler\":\"k_euler_ancestral\",\"seed\":1}", "Comment"),
                png("Software", "NovelAI", "Software"),
            ], TargetType::NovelAI),
            ("P-04b", vec![png("Software", "NovelAI", "Software")], TargetType::NovelAI),
            ("P-05", vec![record(JpegExif, "EXIF UserComment", "{\"Comment\":\"{}\",\"Description\":\"a cat\"}", Some("UserComment"))], TargetType::NovelAI),
            ("P-06", vec![png("SwarmUI [Params]", "{\"prompt\":\"cat\"}", "sui_image_params")], TargetType::SwarmUI),
            ("P-07", vec![png("InvokeAI", "{\"prompt\":\"cat\"}", "invokeai_metadata")], TargetType::InvokeAI),
            ("P-07b", vec![png("Dream", "\"a cat\" -s 50 -S 12345 -C 7.5 -A k_lms", "Dream")], TargetType::InvokeAI),
            ("P-08", vec![png("Easy Diffusion", "sd_xl_base_1.0", "use_stable_diffusion_model")], TargetType::WebUI),
            ("P-09", vec![record(PngStealth, "Stealth PNG Info (Alpha) [stealth_pnginfo]", "{\"Description\":\"a cat\",\"Source\":\"NovelAI Diffusion 4E7A\",\"sampler\":\"k_euler_ancestral\",\"seed\":1,\"steps\":28,\"scale\":11}", None)], TargetType::NovelAI),
            ("P-10", vec![record(PngStealth, "Stealth PNG Info (RGB) [stealth_rgbcomp]", "{\"nodes\":[],\"links\":[]}", None)], TargetType::ComfyUI),
            ("P-11", vec![
                png("Software", "Midjourney", "Software"),
                record(PngXmp, "XMP Description", "a cat --ar 1:1", Some("description")),
            ], TargetType::WebUI),
            ("P-12", vec![record(PngXmp, "XMP UserComment", "{\"c\":\"a cat\",\"uc\":\"blur\",\"steps\":20,\"sampler\":\"euler\",\"seed\":1,\"size\":\"512x512\"}", Some("UserComment"))], TargetType::WebUI),
            ("P-13", vec![record(PngXmp, "XMP AIPromptInformation", "a cat", Some("AIPromptInformation"))], TargetType::WebUI),
            ("P-13b", vec![record(JpegXmp, "XMP DigitalSourceType", "http://cv.iptc.org/newscodes/digitalsourcetype/trainedAlgorithmicMedia", Some("DigitalSourceType"))], TargetType::WebUI),
            ("P-14", vec![record(JpegComment, "Stable Diffusion (A1111)", "a cat\nNegative prompt: blur\nSteps: 20", None)], TargetType::WebUI),
            ("P-15", vec![
                record(Trailing, "Trailing Data (after IEND)", "a cat\nNegative prompt: blur\nSteps: 20", None),
                record(JpegExif, "EXIF UserComment", "a cat\nSteps: 20", Some("UserComment")),
            ], TargetType::WebUI),
            ("P-15b", vec![record(Trailing, "Trailing Data (after IEND)", "a cat\nNegative prompt: blur\nSteps: 20", None)], TargetType::None),
            ("P-16", vec![
                png("Software", "Celsys Studio Tool", "Software"),
                record(PngStealth, "Stealth PNG Info (Alpha) [stealth_pnginfo]", "{\"Description\":\"a cat\",\"Source\":\"NovelAI Diffusion 4E7A\",\"sampler\":\"k_euler_ancestral\",\"seed\":1,\"steps\":28,\"scale\":11}", None),
            ], TargetType::NovelAI),
            ("P-17", vec![png("Dream", "a cat -s 50 -S 12345", "Dream")], TargetType::None),
            ("P-18", vec![record(JpegXmp, "XMP Description", "a cat --ar 1:1 --v 6", Some("description"))], TargetType::None),
            ("P-19", vec![record(PngXmp, "XMP CreatorTool", "Midjourney", Some("CreatorTool"))], TargetType::WebUI),
            ("P-20", vec![
                png("NovelAI [Settings]", "{\"steps\":28,\"sampler\":\"k_euler_ancestral\"}", "Comment"),
                png("workflow", "{}", "workflow"),
            ], TargetType::ComfyUI),
            ("P-21", vec![
                png("NovelAI [Settings]", "{\"steps\":28,\"sampler\":\"k_euler_ancestral\"}", "Comment"),
                png("SwarmUI [Params]", "{\"prompt\":\"cat\"}", "sui_image_params"),
            ], TargetType::SwarmUI),
            ("P-22", vec![
                record(JpegExif, "EXIF UserComment", "a cat\nNegative prompt: blur", Some("UserComment")),
                record(Trailing, "Trailing Data (after IEND)", "a cat\nNegative prompt: blur\nSteps: 20", None),
            ], TargetType::WebUI),
            ("P-23", vec![
                record(PngStealth, "Stealth PNG Info (Alpha) [stealth_pnginfo]", "novelai style, best quality", None),
                record(Trailing, "Trailing Data (after IEND)", "novelai\nSteps: 20", None),
            ], TargetType::NovelAI),
        ]
    }

    #[test]
    fn fixures_match_their_expected_destinations() {
        let mut failures = Vec::new();
        for (id, records, expected) in fixtures() {
            let actual = classify(&records);
            if actual != expected {
                failures.push(format!("{id}: expected {expected:?}, got {actual:?}"));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// Phase 1 の暫定（label 判定）。Phase 2 で REQ-CLS-04 の構造化印へ置き換える。
    #[test]
    fn phase1_provisional_jpeg_comment_still_uses_label() {
        assert_eq!(
            classify(&[record(
                Source::JpegComment,
                "Stable Diffusion (A1111)",
                "Steps: 20 photo comment",
                None
            )]),
            TargetType::WebUI
        );
        assert_eq!(
            classify(&[record(Source::JpegComment, "JPEG Comment", "plain", None)]),
            TargetType::None
        );
    }

    /// T-41: AI 生成ツール名の表の内容を固定する。
    #[test]
    fn t41_ai_tool_table_has_no_non_ai_names() {
        assert_eq!(
            AI_TOOL_NAMES,
            &[
                "Midjourney",
                "NovelAI",
                "Stable Diffusion",
                "AUTOMATIC1111",
                "Forge",
                "InvokeAI",
                "SwarmUI",
                "Draw Things",
            ]
        );
        for non_ai in [
            "Adobe",
            "Photoshop",
            "Celsys",
            "ImageReady",
            "GIMP",
            "ComfyUI",
        ] {
            assert!(
                !AI_TOOL_NAMES.iter().any(|name| name.eq_ignore_ascii_case(non_ai)),
                "{non_ai} must not be in the table"
            );
        }
        // 表に無い名前はシグナルにならない（N-02 と同じ経路）。
        assert_eq!(
            classify(&[png("Software", "GIMP 2.10", "Software")]),
            TargetType::None
        );
    }

    /// T-07: Software 本文が複製上限（256 バイト）を超えても AI 名照合が働く。
    #[test]
    fn t07_software_match_ignores_the_256_byte_copy_limit() {
        let mut body = String::from("NovelAI ");
        body.push_str(&"x".repeat(300));
        let records = [RecordView {
            source: Source::PngText,
            label: Cow::Borrowed("Software"),
            body: Cow::Owned(body),
            keyword: Some(Cow::Borrowed("Software")),
            detail: None,
        }];
        assert_eq!(classify(&records), TargetType::NovelAI);

        let mut other = String::from("AUTOMATIC1111 ");
        other.push_str(&"y".repeat(300));
        let records = [RecordView {
            source: Source::PngText,
            label: Cow::Borrowed("Software"),
            body: Cow::Owned(other),
            keyword: Some(Cow::Borrowed("Software")),
            detail: None,
        }];
        assert_eq!(classify(&records), TargetType::WebUI);
    }

    /// T-08／REQ-CLS-09: prompt の JSON 判定を別レコードと合成しない。
    #[test]
    fn t08_prompt_json_is_judged_per_record() {
        assert_eq!(
            classify(&[
                png("prompt", "a plain prompt", "prompt"),
                png("prompt", "{\"nodes\":[]}", "prompt"),
            ]),
            TargetType::ComfyUI
        );
        // JSON でない prompt と、別種の JSON 本文だけでは ComfyUI にならない。
        assert_eq!(
            classify(&[
                png("prompt", "a plain prompt", "prompt"),
                record(
                    Source::JpegExif,
                    "EXIF ImageDescription",
                    "{\"nodes\":[]}",
                    Some("ImageDescription")
                ),
            ]),
            TargetType::None
        );
    }

    /// T-36／REQ-CLS-08 (b): 先頭 64KB の外の `Steps: ` 等では判定しない。
    #[test]
    fn t36_text_scan_is_limited_to_the_first_64_kib() {
        let mut body = String::from("a cat\n");
        body.push_str(&"z".repeat(TEXT_SCAN_LIMIT));
        body.push_str("\nNegative prompt: blur\nSteps: 20");
        let records = [RecordView {
            source: Source::JpegExif,
            label: Cow::Borrowed("EXIF UserComment"),
            body: Cow::Owned(body),
            keyword: Some(Cow::Borrowed("UserComment")),
            detail: None,
        }];
        assert_eq!(classify(&records), TargetType::None);

        let mut workflow = String::from("a cat\n");
        workflow.push_str(&"z".repeat(TEXT_SCAN_LIMIT));
        workflow.push_str("{\"workflow\":{}}");
        let records = [RecordView {
            source: Source::JpegExif,
            label: Cow::Borrowed("EXIF UserComment"),
            body: Cow::Owned(workflow),
            keyword: Some(Cow::Borrowed("UserComment")),
            detail: None,
        }];
        assert_eq!(classify(&records), TargetType::None);
    }

    /// REQ-CLS-10 の読み替え: AI 名と非AI名が併存しても AI 名側のシグナルは生きる。
    #[test]
    fn ai_tool_signal_survives_alongside_non_ai_names() {
        assert_eq!(
            classify(&[png(
                "Software",
                "Adobe Photoshop 25.0, Midjourney v6",
                "Software"
            )]),
            TargetType::WebUI
        );
        assert_eq!(
            classify(&[png("Software", "photoshop, novelai", "Software")]),
            TargetType::NovelAI
        );
    }


    /// N-11 は Phase 2 で確定する（REQ-CLS-04 の構造化印が必要）。
    #[test]
    fn n11_is_pending_until_phase2() {
        // Phase 1 の暫定 label 判定では WebUI に落ちることを明示的に記録する。
        assert_eq!(
            classify(&[record(
                Source::JpegComment,
                "Stable Diffusion (A1111)",
                "Steps: 20 photo comment",
                None
            )]),
            TargetType::WebUI
        );
    }
}
