use clap::Parser;
use rayon::prelude::*;
use std::collections::HashSet;
use std::fs;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

const LEGACY_WEBUI_OUTPUT_DIR_NAME: &str = "webui_image";

#[derive(Parser)]
#[command(name = "sd_meta_sorter", version = "2.0.1b")]
struct Args {
    #[arg(required = true)]
    inputs: Vec<String>,

    #[arg(short = 'c', long, default_value = "comfyui_img")]
    comfy_dir_name: String,

    #[arg(short = 'w', long, default_value = "webui_img")]
    webui_dir_name: String,

    #[arg(
        short = 'n',
        long = "novelai-dir-name",
        alias = "novelai-dir",
        default_value = "NovelAI_img"
    )]
    novelai_dir_name: String,

    #[arg(short = 's', long = "swarm-dir-name", default_value = "SwarmUI_img")]
    swarm_dir_name: String,

    #[arg(short = 'i', long = "invoke-dir-name", default_value = "InvokeAI_img")]
    invoke_dir_name: String,

    #[arg(long)]
    copy: bool,
}

mod classifier;

use classifier::{ClassifierSink, TargetType};

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

    println!("Scanning inputs (migrating old output folders first)...");

    // 走査と旧フォルダ移行を同じ再帰走査で行い、ディレクトリツリーを二度歩かない。
    for path in &scan_inputs {
        if path.is_dir() {
            if let Err(e) = collect_directory_images(path, &args, &mut target_files) {
                eprintln!("[ERROR] Failed to scan input {:?}: {}", path, e);
                std::process::exit(1);
            }
        } else if path.is_file() {
            target_files.push(path.to_path_buf());
        }
    }

    let total = target_files.len();
    println!("Found {} images. Processing...", total);

    let created_dirs = DirectoryCache::default();
    target_files.par_iter().for_each(|file_path| {
        match process_image(file_path, &args, &created_dirs) {
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


fn is_supported_image_path(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        let extension = extension.to_string_lossy();
        ["png", "jpg", "jpeg", "webp", "avif"]
            .iter()
            .any(|supported| extension.eq_ignore_ascii_case(supported))
    })
}

/// 1 回の再帰走査で、旧 `webui_image` の移行と画像ファイルの収集を行う。
/// 出力フォルダと旧出力フォルダの配下は走査しない。
fn collect_directory_images(
    dir: &Path,
    args: &Args,
    target_files: &mut Vec<PathBuf>,
) -> Result<(), String> {
    if let Some(name) = dir.file_name().map(|name| name.to_string_lossy()) {
        if let Some(target_name) = legacy_target_name(&name, args) {
            if !name.eq_ignore_ascii_case(target_name) {
                let parent = dir.parent().ok_or("Legacy output folder has no parent")?;
                migrate_legacy_directory(dir, &parent.join(target_name))?;
                return Ok(());
            }
        }
        if is_current_output_name(&name, args) {
            return Ok(());
        }
    }

    let entries = fs::read_dir(dir).map_err(|e| format!("Read directory {:?} failed: {}", dir, e))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("Read directory entry in {:?} failed: {}", dir, e))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|e| format!("Read file type for {:?} failed: {}", path, e))?;
        if file_type.is_dir() {
            collect_directory_images(&path, args, target_files)?;
        } else if file_type.is_file() && is_supported_image_path(&path) {
            target_files.push(path);
        }
    }
    Ok(())
}

/// 同じ整理先フォルダへの `create_dir_all` を 1 回に集約する。
#[derive(Default)]
struct DirectoryCache {
    created: Mutex<HashSet<PathBuf>>,
}

impl DirectoryCache {
    fn ensure(&self, path: &Path) -> Result<(), String> {
        if self.created.lock().unwrap().contains(path) {
            return Ok(());
        }
        fs::create_dir_all(path).map_err(|e| format!("Create dir failed: {}", e))?;
        self.created.lock().unwrap().insert(path.to_path_buf());
        Ok(())
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

fn process_image(
    file_path: &Path,
    args: &Args,
    created_dirs: &DirectoryCache,
) -> Result<TargetType, String> {
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

    created_dirs.ensure(&dest_dir)?;

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

    fn default_args() -> Args {
        Args {
            inputs: Vec::new(),
            comfy_dir_name: "comfyui_img".to_string(),
            webui_dir_name: "webui_img".to_string(),
            novelai_dir_name: "NovelAI_img".to_string(),
            swarm_dir_name: "SwarmUI_img".to_string(),
            invoke_dir_name: "InvokeAI_img".to_string(),
            copy: false,
        }
    }

    #[test]
    fn collects_images_and_migrates_legacy_directory_in_one_pass() {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "sd-sorter-scan-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("batch")).unwrap();
        fs::create_dir_all(root.join("webui_image")).unwrap();
        fs::create_dir_all(root.join("comfyui_img")).unwrap();
        fs::write(root.join("batch/a.png"), b"a").unwrap();
        fs::write(root.join("batch/note.txt"), b"note").unwrap();
        fs::write(root.join("webui_image/b.png"), b"b").unwrap();
        fs::write(root.join("comfyui_img/c.png"), b"c").unwrap();

        let args = default_args();
        let mut files = Vec::new();
        collect_directory_images(&root, &args, &mut files).unwrap();

        assert_eq!(files, vec![root.join("batch/a.png")]);
        assert!(!root.join("webui_image").exists());
        assert!(root.join("webui_img/b.png").is_file());
        assert!(root.join("comfyui_img/c.png").is_file());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn directory_cache_is_idempotent() {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "sd-sorter-cache-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let cache = DirectoryCache::default();
        cache.ensure(&path).unwrap();
        cache.ensure(&path).unwrap();
        assert!(path.is_dir());
        fs::remove_dir_all(path).unwrap();
    }
}
