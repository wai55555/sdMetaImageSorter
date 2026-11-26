use clap::Parser;
use fast_meta::{RecordSink, RecordView, Source};
use rayon::prelude::*;
use std::fs;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use walkdir::WalkDir;

const LEGACY_WEBUI_OUTPUT_DIR_NAME: &str = "webui_image";

#[derive(Parser)]
#[command(name = "sd_meta_sorter", version)]
struct Args {
    #[arg(required = true)]
    inputs: Vec<String>,

    #[arg(short = 'c', long, default_value = "comfyui_img")]
    comfy_dir_name: String,

    #[arg(short = 'w', long, default_value = "webui_img")]
    webui_dir_name: String,

    #[arg(short = 'n', long = "novelai-dir", default_value = "NovelAI_img")]
    novelai_dir_name: String,

    #[arg(short = 's', long = "swarm-dir-name", default_value = "SwarmUI_img")]
    swarm_dir_name: String,

    #[arg(short = 'i', long = "invoke-dir-name", default_value = "InvokeAI_img")]
    invoke_dir_name: String,

    #[arg(long)]
    copy: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TargetType {
    ComfyUI,
    WebUI,
    NovelAI,
    SwarmUI,
    InvokeAI,
    None,
}

fn main() {
    let args = Args::parse();

    let count_comfy = AtomicUsize::new(0);
    let count_webui = AtomicUsize::new(0);
    let count_novelai = AtomicUsize::new(0);
    let count_swarm = AtomicUsize::new(0);
    let count_invoke = AtomicUsize::new(0);
    let count_skip = AtomicUsize::new(0);
    let count_error = AtomicUsize::new(0);

    let mut scan_inputs = Vec::with_capacity(args.inputs.len());
    for input in &args.inputs {
        match prepare_input_path(Path::new(input), &args) {
            Ok(path) => scan_inputs.push(path),
            Err(e) => {
                eprintln!("[ERROR] Could not migrate existing output folders: {}", e);
                std::process::exit(1);
            }
        }
    }

    let mut target_files = Vec::new();
    let supported_extensions = ["png", "jpg", "jpeg", "webp", "avif"];

    println!("Scanning inputs (migrating old output folders first)...");
    
    for path in &scan_inputs {
        if path.is_dir() {
            // ★修正箇所: .into_iter() を追加しました
            let walker = WalkDir::new(path).into_iter().filter_entry(|entry| {
                if entry.file_type().is_dir() {
                    let name = entry.file_name().to_string_lossy();
                    if is_current_output_name(&name, &args) || legacy_target_name(&name, &args).is_some() {
                        return false;
                    }
                }
                true
            });

            for entry in walker.filter_map(|e| e.ok()) {
                if entry.file_type().is_file() {
                    if let Some(ext) = entry.path().extension() {
                        let ext_str = ext.to_string_lossy().to_lowercase();
                        if supported_extensions.contains(&ext_str.as_str()) {
                            target_files.push(entry.path().to_path_buf());
                        }
                    }
                }
            }
        } else if path.is_file() {
            target_files.push(path.to_path_buf());
        }
    }

    let total = target_files.len();
    println!("Found {} images. Processing...", total);

    target_files.par_iter().for_each(|file_path| {
        match process_image(file_path, &args) {
            Ok(TargetType::ComfyUI) => { count_comfy.fetch_add(1, Ordering::Relaxed); },
            Ok(TargetType::WebUI) => { count_webui.fetch_add(1, Ordering::Relaxed); },
            Ok(TargetType::NovelAI) => { count_novelai.fetch_add(1, Ordering::Relaxed); },
            Ok(TargetType::SwarmUI) => { count_swarm.fetch_add(1, Ordering::Relaxed); },
            Ok(TargetType::InvokeAI) => { count_invoke.fetch_add(1, Ordering::Relaxed); },
            Ok(TargetType::None) => { count_skip.fetch_add(1, Ordering::Relaxed); },
            Err(e) => {
                eprintln!("[ERROR] {:?}: {}", file_path, e);
                count_error.fetch_add(1, Ordering::Relaxed);
            }
        }
    });

    println!("--------------------------------------------------");
    println!("Total Scanned: {}", total);
    println!("  -> ComfyUI: {}", count_comfy.load(Ordering::Relaxed));
    println!("  -> WebUI: {}", count_webui.load(Ordering::Relaxed));
    println!("  -> NovelAI: {}", count_novelai.load(Ordering::Relaxed));
    println!("  -> SwarmUI: {}", count_swarm.load(Ordering::Relaxed));
    println!("  -> InvokeAI: {}", count_invoke.load(Ordering::Relaxed));
    println!("  -> Skipped: {}", count_skip.load(Ordering::Relaxed));
    println!("  -> Errors: {}", count_error.load(Ordering::Relaxed));
    println!("--------------------------------------------------");
    if count_error.load(Ordering::Relaxed) > 0 {
        std::process::exit(1);
    }
}


fn legacy_target_name<'a>(name: &str, args: &'a Args) -> Option<&'a str> {
    if name.eq_ignore_ascii_case(LEGACY_WEBUI_OUTPUT_DIR_NAME) {
        Some(&args.webui_dir_name)
    } else {
        None
    }
}

fn is_current_output_name(name: &str, args: &Args) -> bool {
    name.eq_ignore_ascii_case(&args.comfy_dir_name) ||
    name.eq_ignore_ascii_case(&args.webui_dir_name) ||
    name.eq_ignore_ascii_case(&args.novelai_dir_name) ||
    name.eq_ignore_ascii_case(&args.swarm_dir_name) ||
    name.eq_ignore_ascii_case(&args.invoke_dir_name)
}

fn prepare_input_path(path: &Path, args: &Args) -> Result<std::path::PathBuf, String> {
    if path.is_dir() {
        if let Some(name) = path.file_name().map(|name| name.to_string_lossy()) {
            if let Some(target_name) = legacy_target_name(&name, args) {
                if !name.eq_ignore_ascii_case(target_name) {
                    let parent = path.parent().ok_or("Legacy output folder has no parent")?;
                    let destination = parent.join(target_name);
                    migrate_legacy_directory(path, &destination)?;
                    migrate_legacy_output_dirs(parent, args)?;
                    return Ok(destination);
                }
            }
            if is_current_output_name(&name, args) {
                return Ok(path.to_path_buf());
            }
        }
        migrate_legacy_output_dirs(path, args)?;
        return Ok(path.to_path_buf());
    }

    if path.is_file() {
        let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        let mut ancestor = Some(parent);
        while let Some(dir) = ancestor {
            if let Some(name) = dir.file_name().map(|name| name.to_string_lossy()) {
                if let Some(target_name) = legacy_target_name(&name, args) {
                    if !name.eq_ignore_ascii_case(target_name) {
                        let old_root = dir.to_path_buf();
                        let new_root = dir.parent().ok_or("Legacy output folder has no parent")?.join(target_name);
                        let relative = path.strip_prefix(&old_root).map_err(|e| format!("Resolve legacy file path failed: {}", e))?;
                        migrate_legacy_directory(&old_root, &new_root)?;
                        if let Some(parent) = new_root.parent() {
                            migrate_legacy_output_dirs(parent, args)?;
                        }
                        return Ok(new_root.join(relative));
                    }
                }
            }
            ancestor = dir.parent();
        }
        migrate_legacy_output_dirs(parent, args)?;
        return Ok(path.to_path_buf());
    }

    Ok(path.to_path_buf())
}

fn migrate_legacy_output_dirs(root: &Path, args: &Args) -> Result<(), String> {
    if let Some(name) = root.file_name().map(|name| name.to_string_lossy()) {
        if let Some(target_name) = legacy_target_name(&name, args) {
            if !name.eq_ignore_ascii_case(target_name) {
                let parent = root.parent().ok_or("Legacy output folder has no parent")?;
                return migrate_legacy_directory(root, &parent.join(target_name));
            }
        }
        if is_current_output_name(&name, args) {
            return Ok(());
        }
    }

    let mut pending = vec![root.to_path_buf()];
    while let Some(parent) = pending.pop() {
        let entries = fs::read_dir(&parent)
            .map_err(|e| format!("Read directory {:?} failed: {}", parent, e))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("Read directory entry in {:?} failed: {}", parent, e))?;
            let file_type = entry.file_type()
                .map_err(|e| format!("Read file type for {:?} failed: {}", entry.path(), e))?;
            if !file_type.is_dir() {
                continue;
            }

            let child = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(target_name) = legacy_target_name(&name, args) {
                if !name.eq_ignore_ascii_case(target_name) {
                    migrate_legacy_directory(&child, &parent.join(target_name))?;
                }
            } else if !is_current_output_name(&name, args) {
                pending.push(child);
            }
        }
    }
    Ok(())
}

fn migrate_legacy_directory(source: &Path, destination: &Path) -> Result<(), String> {
    if source == destination {
        return Ok(());
    }
    match fs::symlink_metadata(destination) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            fs::rename(source, destination)
                .map_err(|e| format!("Rename {:?} to {:?} failed: {}", source, destination, e))?;
            println!("[INFO] Renamed legacy folder {:?} -> {:?}", source, destination);
        }
        Err(e) => return Err(format!("Inspect destination {:?} failed: {}", destination, e)),
        Ok(metadata) if metadata.file_type().is_dir() => {
            merge_legacy_directory_contents(source, destination)?;
            fs::remove_dir(source)
                .map_err(|e| format!("Remove migrated legacy folder {:?} failed: {}", source, e))?;
            println!("[INFO] Merged legacy folder {:?} into {:?}", source, destination);
        }
        Ok(_) => return Err(format!("Migration destination exists and is not a directory: {:?}", destination)),
    }
    Ok(())
}

fn merge_legacy_directory_contents(source: &Path, destination: &Path) -> Result<(), String> {
    let entries = fs::read_dir(source)
        .map_err(|e| format!("Read legacy folder {:?} failed: {}", source, e))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("Read legacy folder entry in {:?} failed: {}", source, e))?;
        let source_entry = entry.path();
        let destination_entry = destination.join(entry.file_name());
        let source_type = entry.file_type()
            .map_err(|e| format!("Read file type for {:?} failed: {}", source_entry, e))?;
        let destination_metadata = match fs::symlink_metadata(&destination_entry) {
            Ok(metadata) => Some(metadata),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(format!("Inspect {:?} failed: {}", destination_entry, e)),
        };

        if source_type.is_dir() && destination_metadata.as_ref().is_some_and(|m| m.file_type().is_dir()) {
            merge_legacy_directory_contents(&source_entry, &destination_entry)?;
            fs::remove_dir(&source_entry)
                .map_err(|e| format!("Remove merged directory {:?} failed: {}", source_entry, e))?;
        } else if destination_metadata.is_none() {
            if source_type.is_file() {
                if !transfer_no_replace(&source_entry, &destination_entry, false)? {
                    move_legacy_conflict(&source_entry, destination, &entry.file_name())?;
                }
            } else {
                fs::rename(&source_entry, &destination_entry)
                    .map_err(|e| format!("Move {:?} to {:?} failed: {}", source_entry, destination_entry, e))?;
            }
        } else {
            move_legacy_conflict(&source_entry, destination, &entry.file_name())?;
        }
    }
    Ok(())
}

fn move_legacy_conflict(source: &Path, destination_dir: &Path, original_name: &std::ffi::OsStr) -> Result<(), String> {
    let source_metadata = fs::symlink_metadata(source)
        .map_err(|e| format!("Inspect conflict source {:?} failed: {}", source, e))?;
    for index in 1..=10_000usize {
        let destination = legacy_conflict_path(destination_dir, original_name, index);
        if source_metadata.file_type().is_file() {
            if transfer_no_replace(source, &destination, false)? {
                println!("[INFO] Preserved filename conflict as {:?}", destination);
                return Ok(());
            }
        } else if source_metadata.file_type().is_dir() {
            match fs::create_dir(&destination) {
                Ok(()) => {
                    merge_legacy_directory_contents(source, &destination)?;
                    fs::remove_dir(source)
                        .map_err(|e| format!("Remove merged conflict folder {:?} failed: {}", source, e))?;
                    println!("[INFO] Preserved folder conflict as {:?}", destination);
                    return Ok(());
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(format!("Create conflict folder {:?} failed: {}", destination, e)),
            }
        } else {
            match fs::rename(source, &destination) {
                Ok(()) => {
                    println!("[INFO] Preserved filename conflict as {:?}", destination);
                    return Ok(());
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(format!("Move conflict {:?} to {:?} failed: {}", source, destination, e)),
            }
        }
    }
    Err(format!("Could not find a free conflict name for {:?}", source))
}

fn legacy_conflict_path(parent: &Path, name: &std::ffi::OsStr, index: usize) -> std::path::PathBuf {
    let name = name.to_string_lossy();
    let path = Path::new(name.as_ref());
    if let Some(extension) = path.extension() {
        let stem = path.file_stem().unwrap_or(std::ffi::OsStr::new(name.as_ref()));
        parent.join(format!("{}__legacy_{}.{}", stem.to_string_lossy(), index, extension.to_string_lossy()))
    } else {
        parent.join(format!("{}__legacy_{}", name, index))
    }
}

fn process_image(file_path: &Path, args: &Args) -> Result<TargetType, String> {
    let mut sink = ClassifierSink::default();
    // 子プロセス時代の隔離性を、ファイル単位の catch_unwind で代替する。
    let scan_result = catch_unwind(AssertUnwindSafe(|| {
        fast_meta::scan_file(
            file_path,
            &fast_meta::ScanOptions::default(),
            &mut sink,
        )
    }))
    .map_err(|_| "scan panicked".to_string())?;
    scan_result.map_err(|error| error.to_string())?;

    let target = sink.decide().unwrap_or(TargetType::None);
    if target == TargetType::None {
        return Ok(TargetType::None);
    }

    // 親ディレクトリ取得
    let parent_dir = file_path.parent().ok_or("Cannot get parent dir")?;

    let folder_name = match target {
        TargetType::ComfyUI => &args.comfy_dir_name,
        TargetType::WebUI => &args.webui_dir_name,
        TargetType::NovelAI => &args.novelai_dir_name,
        TargetType::SwarmUI => &args.swarm_dir_name,
        TargetType::InvokeAI => &args.invoke_dir_name,
        TargetType::None => unreachable!(),
    };

    // 親ディレクトリ内にフォルダを作成
    let dest_dir = parent_dir.join(folder_name);

    if let Err(e) = fs::create_dir_all(&dest_dir) {
        return Err(format!("Create dir failed: {}", e));
    }

    let file_name = file_path.file_name().ok_or("Cannot get file name")?;
    let dest_path = dest_dir.join(file_name);
    if transfer_no_replace(file_path, &dest_path, args.copy)? {
        Ok(target)
    } else {
        Ok(TargetType::None)
    }
}

// Hard links provide an atomic no-replace move on the same filesystem. If unavailable,
// create_new reserves the destination before copying; never truncate an existing file.
fn transfer_no_replace(source: &Path, dest: &Path, copy: bool) -> Result<bool, String> {
    let metadata = fs::symlink_metadata(source).map_err(|e| format!("Source metadata failed: {}", e))?;
    if !metadata.file_type().is_file() {
        return Err("Source is not a regular file".to_string());
    }
    if !copy {
        match fs::hard_link(source, dest) {
            Ok(()) => {
                fs::remove_file(source).map_err(|e| format!("Source removal failed after linking: {}", e))?;
                return Ok(true);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Ok(false),
            Err(_) => {} // Cross-device or unsupported: use an exclusive copy.
        }
    }
    let mut input = fs::File::open(source).map_err(|e| format!("Open source failed: {}", e))?;
    let mut output = match fs::OpenOptions::new().write(true).create_new(true).open(dest) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Ok(false),
        Err(e) => return Err(format!("Create destination failed: {}", e)),
    };
    if let Err(e) = std::io::copy(&mut input, &mut output) {
        drop(output);
        return match fs::remove_file(dest) {
            Ok(()) => Err(format!("Copy failed: {}", e)),
            Err(cleanup) => Err(format!("Copy failed: {}; cannot remove incomplete destination: {}", e, cleanup)),
        };
    }
    drop(output);
    if !copy {
        fs::remove_file(source).map_err(|e| format!("Source removal failed after copying: {}", e))?;
    }
    Ok(true)
}

#[derive(Default)]
struct KeywordFlags {
    comment: bool,
    sui_image_params: bool,
    sui_extra_data: bool,
    invokeai_metadata: bool,
    sd_metadata: bool,
    dream: bool,
    use_stable_diffusion_model: bool,
    workflow: bool,
    generation_data: bool,
    prompt: bool,
    parameters: bool,
}

impl KeywordFlags {
    fn observe(&mut self, keyword: &str) {
        match keyword {
            "Comment" => self.comment = true,
            "sui_image_params" => self.sui_image_params = true,
            "sui_extra_data" => self.sui_extra_data = true,
            "invokeai_metadata" => self.invokeai_metadata = true,
            "sd-metadata" => self.sd_metadata = true,
            "Dream" => self.dream = true,
            "use_stable_diffusion_model" => self.use_stable_diffusion_model = true,
            "workflow" => self.workflow = true,
            "generation_data" => self.generation_data = true,
            "prompt" => self.prompt = true,
            "parameters" => self.parameters = true,
            _ => {}
        }
    }
}

fn limited_bytes(text: &str, limit: usize) -> &[u8] {
    let bytes = text.as_bytes();
    &bytes[..bytes.len().min(limit)]
}

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

/// §8.2 のシグナルだけを収集する。本文の全文コピーや小文字化コピーは作らない。
#[derive(Default)]
struct ClassifierSink {
    keywords: KeywordFlags,
    trailing_novelai: bool,
    trailing_webui: bool,
    has_software: bool,
    software_bodies: Vec<String>,
    model_body_has_sui: bool,
    parameters_has_sui: bool,
    prompt_is_json: bool,
    usercomment_has_workflow: bool,
    usercomment_is_novelai_pack: bool,
    steps_in_text_fields: bool,
    jpeg_comment_a1111: bool,
    has_stealth: bool,
    stealth_novelai: bool,
    stealth_comfy: bool,
    has_xmp: bool,
}

impl RecordSink for ClassifierSink {
    fn on_record(&mut self, record: &RecordView<'_>) {
        // §8.2 包括規則: Trailing は trailing 系シグナル以外に効かせない。
        if record.source == Source::Trailing {
            if contains_ignore_ascii_case(&record.body, b"novelai") {
                self.trailing_novelai = true;
            }
            if ["parameters", "Stable Diffusion", "Software", "Steps: "]
                .iter()
                .any(|needle| record.body.contains(needle))
            {
                self.trailing_webui = true;
            }
            return;
        }

        if let Some(keyword) = record.keyword.as_deref() {
            self.keywords.observe(keyword);
            match keyword {
                "Software" => {
                    self.has_software = true;
                    if record.body.len() <= 256 {
                        self.software_bodies.push(record.body.to_string());
                    }
                }
                "Model" => {
                    if record.body.contains("sui_image_params") {
                        self.model_body_has_sui = true;
                    }
                }
                "parameters" => {
                    if record.body.contains("\"sui_image_params\"") {
                        self.parameters_has_sui = true;
                    }
                }
                "prompt" => {
                    if record.body.trim_start().starts_with('{') {
                        self.prompt_is_json = true;
                    }
                }
                "UserComment" => {
                    let body = limited_bytes(&record.body, 64 * 1024);
                    if contains_bytes(body, b"\"workflow\"") {
                        self.usercomment_has_workflow = true;
                    }
                    if contains_bytes(body, b"\"Comment\":")
                        && contains_bytes(body, b"\"Description\":")
                    {
                        self.usercomment_is_novelai_pack = true;
                    }
                }
                _ => {}
            }
            if keyword == "UserComment" || keyword == "ImageDescription" {
                if contains_bytes(limited_bytes(&record.body, 64 * 1024), b"Steps: ") {
                    self.steps_in_text_fields = true;
                }
            }
        }

        if record.source == Source::JpegComment
            && record.label == "Stable Diffusion (A1111)"
        {
            self.jpeg_comment_a1111 = true;
        }
        if record.source == Source::PngStealth {
            self.has_stealth = true;
            if contains_ignore_ascii_case(&record.body, b"novelai") {
                self.stealth_novelai = true;
            }
            if contains_ignore_ascii_case(&record.body, b"workflow") {
                self.stealth_comfy = true;
            }
        }
        if matches!(
            record.source,
            Source::PngXmp | Source::JpegXmp | Source::WebpXmp | Source::TiffXmp
        ) {
            self.has_xmp = true;
        }
    }
}

impl ClassifierSink {
    /// §8.3 の決定手順。上が優先。
    fn decide(&self) -> Option<TargetType> {
        // 1. NovelAI の見出し相当（FR-CLS-01: Description 単独では確定しない）
        if self.keywords.comment {
            return Some(TargetType::NovelAI);
        }
        // 2. SwarmUI（F-07）
        if self.keywords.sui_image_params
            || self.keywords.sui_extra_data
            || self.parameters_has_sui
            || self.model_body_has_sui
        {
            return Some(TargetType::SwarmUI);
        }
        // 3. InvokeAI（F-04 / F-05）
        if self.keywords.invokeai_metadata || self.keywords.sd_metadata || self.keywords.dream {
            return Some(TargetType::InvokeAI);
        }
        // 4. Easy Diffusion（F-06）→ WebUI キャッチオール
        if self.keywords.use_stable_diffusion_model {
            return Some(TargetType::WebUI);
        }
        // 5. ComfyUI（FR-CLS-02 / FR-CLS-07）
        if self.keywords.workflow
            || self.keywords.generation_data
            || (self.keywords.prompt && self.prompt_is_json)
            || self.usercomment_has_workflow
            || self.stealth_comfy
        {
            return Some(TargetType::ComfyUI);
        }
        // 5b. 本文由来の NovelAI（旧順序: Comfy 見出し相当 → novelai 本文 → WebUI 本文）
        if self.software_bodies.iter().any(|body| body.contains("NovelAI"))
            || self.usercomment_is_novelai_pack
            || self.stealth_novelai
            || self.trailing_novelai
        {
            return Some(TargetType::NovelAI);
        }
        // 6. WebUI（A1111 / 汎用キャッチオール、FR-CLS-04 / FR-CLS-05）
        if self.keywords.parameters
            || self.steps_in_text_fields
            || self.jpeg_comment_a1111
            || self.has_software
            || self.has_stealth
            || self.has_xmp
            || self.trailing_webui
        {
            return Some(TargetType::WebUI);
        }
        // 7. どれにも当たらない
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfers_never_replace_existing_files() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("sd-sorter-test-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir(&dir).unwrap();
        let source = dir.join("source");
        let dest = dir.join("dest");
        fs::write(&source, b"source").unwrap();
        fs::write(&dest, b"existing").unwrap();
        for copy in [true, false] {
            assert!(!transfer_no_replace(&source, &dest, copy).unwrap());
            assert_eq!(fs::read(&source).unwrap(), b"source");
            assert_eq!(fs::read(&dest).unwrap(), b"existing");
        }
        fs::remove_file(&dest).unwrap();
        assert!(transfer_no_replace(&source, &dest, true).unwrap());
        assert_eq!(fs::read(&source).unwrap(), b"source");
        fs::remove_file(&dest).unwrap();
        assert!(transfer_no_replace(&source, &dest, false).unwrap());
        assert!(!source.exists());
        assert_eq!(fs::read(&dest).unwrap(), b"source");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn concurrent_copies_do_not_overwrite() {
        let dir = std::env::temp_dir().join(format!("sd-sorter-race-{}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let a = dir.join("a");
        let b = dir.join("b");
        let dest = dir.join("dest");
        fs::write(&a, b"first").unwrap();
        fs::write(&b, b"second").unwrap();
        std::thread::scope(|scope| {
            let one = scope.spawn(|| transfer_no_replace(&a, &dest, true).unwrap());
            let two = scope.spawn(|| transfer_no_replace(&b, &dest, true).unwrap());
            assert_ne!(one.join().unwrap(), two.join().unwrap());
        });
        let result = fs::read(&dest).unwrap();
        assert!(result == b"first" || result == b"second");
        assert!(transfer_no_replace(&dir.join("missing"), &dir.join("other"), true).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

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

    fn classify(records: &[RecordView<'static>]) -> TargetType {
        let mut sink = ClassifierSink::default();
        for record in records {
            sink.on_record(record);
        }
        sink.decide().unwrap_or(TargetType::None)
    }

    #[test]
    fn classifies_novelai_before_generic_metadata() {
        assert_eq!(
            classify(&[
                record(Source::PngText, "NovelAI [Settings]", "{\"steps\": 28}", Some("Comment")),
                record(
                    Source::PngText,
                    "Stable Diffusion (A1111)",
                    "Steps: 20",
                    Some("parameters")
                ),
            ]),
            TargetType::NovelAI
        );
        assert_eq!(
            classify(&[record(
                Source::PngStealth,
                "Stealth PNG Info (Alpha) [stealth_pnginfo]",
                "{\"Software\":\"NovelAI\"}",
                None
            )]),
            TargetType::NovelAI
        );
        assert_eq!(
            classify(&[record(
                Source::PngText,
                "Stable Diffusion (A1111)",
                "Steps: 20",
                Some("parameters")
            )]),
            TargetType::WebUI
        );
    }

    #[test]
    fn classifies_stealth_and_ignores_the_input_path() {
        assert_eq!(classify(&[]), TargetType::None);
        assert_eq!(
            classify(&[record(
                Source::PngStealth,
                "Stealth PNG Info (Alpha) [stealth_pnginfo]",
                "opaque data",
                None
            )]),
            TargetType::WebUI
        );
        assert_eq!(
            classify(&[record(
                Source::PngStealth,
                "Stealth PNG Info (RGB) [stealth_rgbcomp]",
                "ComfyUI workflow",
                None
            )]),
            TargetType::ComfyUI
        );
        assert_eq!(
            classify(&[record(Source::PngText, "workflow", "{}", Some("workflow"))]),
            TargetType::ComfyUI
        );
    }

    #[test]
    fn classifies_swarmui_variants() {
        assert_eq!(
            classify(&[record(
                Source::PngText,
                "SwarmUI [Params]",
                "{\"prompt\":\"cat\"}",
                Some("sui_image_params")
            )]),
            TargetType::SwarmUI
        );
        assert_eq!(
            classify(&[record(
                Source::PngText,
                "SwarmUI [Extra]",
                "{}",
                Some("sui_extra_data")
            )]),
            TargetType::SwarmUI
        );
        assert_eq!(
            classify(&[record(
                Source::PngText,
                "Stable Diffusion (A1111)",
                "{\"sui_image_params\":{\"prompt\":\"cat\"}}",
                Some("parameters")
            )]),
            TargetType::SwarmUI
        );
        assert_eq!(
            classify(&[record(
                Source::JpegExif,
                "EXIF Model",
                "{\"sui_image_params\":{\"prompt\":\"cat\"}}",
                Some("Model")
            )]),
            TargetType::SwarmUI
        );
    }

    #[test]
    fn classifies_invokeai_keys_as_invokeai() {
        for keyword in ["invokeai_metadata", "sd-metadata", "Dream"] {
            assert_eq!(
                classify(&[record(
                    Source::PngText,
                    "InvokeAI",
                    "{\"prompt\":\"cat\"}",
                    Some(keyword)
                )]),
                TargetType::InvokeAI
            );
        }
    }

    #[test]
    fn classifies_easy_diffusion_marker_as_webui() {
        assert_eq!(
            classify(&[record(
                Source::PngText,
                "Easy Diffusion",
                "sd_xl_base_1.0",
                Some("use_stable_diffusion_model")
            )]),
            TargetType::WebUI
        );
    }

    #[test]
    fn description_alone_is_not_novelai() {
        assert_eq!(
            classify(&[record(
                Source::PngText,
                "NovelAI [Prompt]",
                "a cat",
                Some("Description")
            )]),
            TargetType::None
        );
        assert_eq!(
            classify(&[
                record(Source::PngText, "NovelAI [Prompt]", "a cat", Some("Description")),
                record(Source::PngText, "Software", "Midjourney", Some("Software")),
            ]),
            TargetType::WebUI
        );
        assert_eq!(
            classify(&[
                record(Source::PngText, "NovelAI [Prompt]", "a cat", Some("Description")),
                record(Source::PngText, "NovelAI [Settings]", "{}", Some("Comment")),
            ]),
            TargetType::NovelAI
        );
    }

    #[test]
    fn prompt_requires_json_to_be_comfyui() {
        assert_eq!(
            classify(&[record(Source::PngText, "prompt", "a plain prompt", Some("prompt"))]),
            TargetType::None
        );
        assert_eq!(
            classify(&[record(
                Source::PngText,
                "prompt",
                "  {\"nodes\":[]}",
                Some("prompt")
            )]),
            TargetType::ComfyUI
        );
    }

    #[test]
    fn xmp_only_is_webui_catchall() {
        for source in [
            Source::PngXmp,
            Source::JpegXmp,
            Source::WebpXmp,
            Source::TiffXmp,
        ] {
            assert_eq!(
                classify(&[record(source, "XMP Description", "a cat --ar 1:1", None)]),
                TargetType::WebUI
            );
        }
    }

    #[test]
    fn software_body_distinguishes_novelai_and_midjourney() {
        assert_eq!(
            classify(&[record(Source::PngText, "Software", "NovelAI", Some("Software"))]),
            TargetType::NovelAI
        );
        assert_eq!(
            classify(&[record(Source::PngText, "Software", "Midjourney", Some("Software"))]),
            TargetType::WebUI
        );
    }

    #[test]
    fn jpeg_comment_uses_label_not_body_steps() {
        assert_eq!(
            classify(&[record(
                Source::JpegComment,
                "Stable Diffusion (A1111)",
                "Negative prompt: blur",
                None
            )]),
            TargetType::WebUI
        );
        assert_eq!(
            classify(&[record(
                Source::JpegComment,
                "Stable Diffusion (A1111)",
                "plain comment",
                None
            )]),
            TargetType::WebUI
        );
        assert_eq!(
            classify(&[record(Source::JpegComment, "JPEG Comment", "plain", None)]),
            TargetType::None
        );
    }

    #[test]
    fn trailing_records_only_use_body_substring_rules() {
        assert_eq!(
            classify(&[record(Source::Trailing, "Trailing Data (after IEND)", "novelai", None)]),
            TargetType::NovelAI
        );
        assert_eq!(
            classify(&[record(
                Source::Trailing,
                "Trailing Data (after IEND)",
                "Steps: 20",
                None
            )]),
            TargetType::WebUI
        );
        assert_eq!(
            classify(&[record(
                Source::Trailing,
                "Trailing Data (after IEND)",
                "{}",
                Some("workflow")
            )]),
            TargetType::None
        );
    }

    #[test]
    fn comfy_signal_wins_over_software_novelai_body() {
        assert_eq!(
            classify(&[
                record(Source::PngText, "workflow", "{}", Some("workflow")),
                record(Source::PngText, "Software", "NovelAI", Some("Software")),
            ]),
            TargetType::ComfyUI
        );
    }

    #[test]
    fn usercomment_signals_are_limited_to_known_patterns() {
        assert_eq!(
            classify(&[record(
                Source::JpegExif,
                "EXIF UserComment",
                "{\"workflow\":{}}",
                Some("UserComment")
            )]),
            TargetType::ComfyUI
        );
        assert_eq!(
            classify(&[record(
                Source::JpegExif,
                "EXIF UserComment",
                "{\"Comment\":\"{}\",\"Description\":\"cat\"}",
                Some("UserComment")
            )]),
            TargetType::NovelAI
        );
        assert_eq!(
            classify(&[record(
                Source::JpegExif,
                "EXIF UserComment",
                "parameters=cat\nSteps: 20",
                Some("UserComment")
            )]),
            TargetType::WebUI
        );
    }

}
