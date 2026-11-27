# Phase 0 記録（261127 誤分類改修）— フィクスチャ固定・現行挙動・未決事項の解消

対象仕様書: `doc/261127_plan-false-classification-fix.md`
実装変更: なし（本フェーズはフィクスチャと計測のみ）。分類ロジックを `src/classifier.rs` へ
移動しただけで、判定内容は改修前と同一である。

## 1. フィクスチャ

§10.1（負例 N-01〜N-14）／§10.2（正例 P-01〜P-23）をレコード単位の合成データとして
`src/classifier.rs` の `fixtures()` に固定した。画像ファイルではなく `RecordView` を直接
構築するのは、既存テストと同じ流儀であり、§10.3 の T 番号テストと 1 対 1 で対応させるため。

- `P-09`／`P-10` の Stealth 本文は U-8 の結論（§4）に沿った合成 JSON とした。
- `P-18` は U-7 の結論（§4）により期待値 None とした。
- `P-13` の本体（AIPromptInformation／DigitalSourceType）を固定した。tEXt 格納 PNG の変種は
  REQ-EXT-06 が前提のため Phase 2 で固定する。
- `N-11`（JPEG COM 片方マーカー）は REQ-CLS-04 の構造化印が Phase 2 のため、Phase 1 の
  完了条件には含めない（§11.1 の注記どおり）。

## 2. 現行挙動（改修前）

`cargo test phase0_dump -- --nocapture` の出力（左から ID｜現行判定｜仕様の期待値）。

```
N-01|WebUI|None            P-01|WebUI|WebUI
N-02|WebUI|None            P-02|WebUI|WebUI
N-03|WebUI|None            P-03|ComfyUI|ComfyUI
N-04|None|None             P-03b|ComfyUI|ComfyUI
N-05|WebUI|None            P-03c|None|ComfyUI      ← BOM 付き prompt が JSON 判定に失敗
N-06|WebUI|None            P-04|NovelAI|NovelAI
N-07|NovelAI|None          P-04b|NovelAI|NovelAI
N-08|WebUI|None            P-05|NovelAI|NovelAI
N-09|ComfyUI|None          P-06|SwarmUI|SwarmUI
N-10|WebUI|None            P-07|InvokeAI|InvokeAI
N-11|WebUI|None(Phase 2)   P-07b|InvokeAI|InvokeAI
N-12|NovelAI|None          P-08|WebUI|WebUI
N-13|None|None             P-09|NovelAI|NovelAI
N-14|None|None             P-10|WebUI|ComfyUI     ← Stealth の workflow 部分一致で WebUI が先に立つ
                           P-11|WebUI|WebUI
                           P-12|WebUI|WebUI
                           P-13|WebUI|WebUI
                           P-13b|WebUI|WebUI
                           P-14|WebUI|WebUI
                           P-15|WebUI|WebUI
                           P-15b|WebUI|None       ← Trailing 単独で確定
                           P-16|NovelAI|NovelAI
                           P-17|InvokeAI|None     ← 未検証の Dream 風キーで確定
                           P-18|WebUI|None        ← XMP 有無のみで確定
                           P-19|WebUI|WebUI
                           P-20|NovelAI|ComfyUI   ← Comment が最優先
                           P-21|NovelAI|SwarmUI   ← Comment が最優先
                           P-22|WebUI|WebUI
                           P-23|NovelAI|NovelAI
```

負例 14 件中 11 件が誤って仕分けられており（N-04／N-13／N-14 のみ正解）、監査の指摘
（存在シグナルによる確定）がそのまま再現した。正例側の未達は P-03c（BOM）、P-10（Stealth の
ComfyUI 構造）、P-15b（Trailing 単独）、P-17（未検証 Dream）、P-18（XMP のみ）。

## 3. 計測条件

| 項目 | 内容 |
|---|---|
| 日時 | 2026-11-27 |
| ツール | Windows MSVC 版 `cargo 1.91.1` |
| sorter | `sd_meta_tools` 2.0.1-b（main 6e29a2b） |
| fast_meta | v2.1.0（git tag） |
| 計測 | `cargo test --locked phase0_dump -- --nocapture`（レコード単位のため I/O なし） |

## 4. 未決事項の解消（§14 へ追記した内容）

実サンプルは手元に無いため、参照実装（d3x-at/sd-parsers、receyuki/stable-diffusion-prompt-reader）
の形式定義と公開されている仕様記述から確定した。閾値は「単一キーに依存しない」ことを優先し、
複合条件（2 つ以上）で固定する。

### U-1 NovelAI 設定 JSON のキー集合（REQ-CLS-01）

- キー集合: `uc` / `sampler` / `scale` / `seed` / `steps` / `strength` / `noise` /
  `noise_schedule` / `sm` / `sm_dyn` / `n_samples` / `cfg_rescale`
- 条件: トップレベル object が上記のうち **2 つ以上** を含むこと。
- 根拠: receyuki `format/novelai.py` の `SETTING_KEY_LEGACY`（sampler／seed／scale／steps）と
  `uc`、d3x-at `parsers/_novelai.py` の `SAMPLER_PARAMS`（seed／strength／noise／scale）＋必須
  `sampler`＋任意 `uc`。両実装が共通して使う sampler／seed／scale／steps／uc を核とし、
  NovelAI が実際に書き出す strength／noise／noise_schedule／sm／sm_dyn／n_samples／cfg_rescale
  を加えた。`prompt` は NovelAI では Description 側に出るため集合に含めない。
- Fooocus の `Comment` JSON（prompt／negative_prompt／style／performance／resolution 等）とは
  キーが重ならないため、2 つ以上の閾値で誤爆しないことを確認した。

### U-2 Draw Things JSON の固有キー集合（REQ-CLS-07）

- 条件: トップレベル object に `c`（文字列。ポジティブプロンプト）があり、かつ
  `sampler` / `steps` / `seed` / `scale` / `model` / `size` のうち **2 つ以上** を含むこと。
- 根拠: receyuki `format/drawthings.py` が `c`／`uc` を取り出し、`SETTING_KEY =
  ["model", "sampler", "seed", "scale", "steps", "size"]` を設定として読む。`c` 単独では
  NovelAI の `uc` と区別できないため、生成パラメータ系キーの併存を必須にした。

### U-3 Dream 旧式構文の検証パターン（REQ-CLS-12）

- 条件: 本文が `"` で始まり、対応する閉じ `"` の後ろに ` -<英字> <値>` 形式のフラグが
  **2 つ以上** 続くこと（末尾に余分な文字列があってよい）。
- 根拠: d3x-at `parsers/_invokeai/_variant_dream.py` の `^"(.*?)"(.*)$` と
  `\s+-(\w+)\s+([^\s]+)`。フラグは `-s`（steps）／`-S`（seed）／`-C`（cfg_scale）／`-W`／`-H`／
  `-A`（sampler）を使う。1 つだけの場合は他ツールの本文との区別が付かないため 2 つ以上を要求する。

### U-5 弱シグナルの方向跨ぎ（裏付け確認）

- §10.1／§10.2 の全フィクスチャで、方向跨ぎを数えないことによる反例は無かった。
  反例が生じ得るのは「WebUI 方向の弱 1 種類＋NovelAI 方向の弱 1 種類」だけを持つ画像だが、
  その組み合わせは P-23 の同時成立ケースに吸収され、同点規則で NovelAI になる。
  本書の改訂は不要。

### U-7 Midjourney の XMP のみ画像の捕捉条件（REQ-CLS-02 (f)、P-18）

- 結論: **XMP のみでは捕捉しない。** Midjourney は PNG tEXt の `Description`（プロンプト）と
  `Software = "Midjourney"` を書き、XMP に固有の生成宣言フィールドを持たない。参照実装
  （d3x-at、receyuki）にも Midjourney 用の XMP パーサは存在しない。汎用の `dc:description` を
  Midjourney とみなす条件を作ると N-05（Lightroom）を再び誤検知するため、条件は作らない。
- したがって P-18 の期待値は None とし、REQ-CLS-02 (f) は実装しない。

### U-8 Stealth 本文 JSON の構造判定のキー集合（REQ-CLS-05、P-09／P-10）

- NovelAI 構造: JSON パース成功かつ、U-1 のキー集合を 2 つ以上含む、または
  `Comment`（文字列）と `Description` を併せ持つ（receyuki `_nai_stealth` が
  `Comment` を展開して Description／prompt／uc を読む形に対応）。
- ComfyUI 構造: JSON パース成功かつ、トップレベルに `nodes` と `links` の両配列を持つ、または
  トップレベルの全値が `class_type` を持つ object である（ComfyUI の workflow JSON と API
  形式 prompt の構造）。
- どちらにも一致しない場合は構造判定をせず、方向別の部分一致（`novelai`／`workflow`）と
  Stealth レコードの存在だけを弱シグナルとして立てる。
- 実サンプルの ComfyUI Stealth は得られていないが、ComfyUI が書き出す JSON のトップレベル
  構造は上記で安定しており、テキスト部分一致より誤爆が小さい。判別できない本文は弱に留まる
  ため、誤分類側の危険は増えない。
