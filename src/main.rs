use clap::Parser;
use rayon::prelude::*;
use std::collections::HashSet;
use std::fs;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const LEGACY_WEBUI_OUTPUT_DIR_NAME: &str = "webui_image";

#[derive(Parser)]
#[command(name = "sd_meta_sorter", version = "2.2.0")]
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
mod collect;
mod report;

use classifier::{ClassifierSink, TargetType};
use collect::Collector;
use report::{ProcessResult, Report};

fn main() {
    let args = Args::parse();

    // REQ-COL-06: 分類先フォルダ名は処理開始前に検証し、1 ファイルも移動せずに終了コード 2。
    if let Err(message) = validate_output_folder_names(&args) {
        eprintln!("[ERROR] Invalid output folder name: {}", message);
        eprintln!(
            "Usage: sd_meta_sorter <inputs...> [-c <name>] [-w <name>] [-n <name>] [-s <name>] [-i <name>] [--copy]"
        );
        std::process::exit(2);
    }

    println!("Scanning inputs (migrating old output folders first)...");

    let mut collector = Collector::new(&args);
    for input in &args.inputs {
        let path = Path::new(input);
        // REQ-COL-03: 存在しない入力は無言で無視せず、他の入力の処理を継続する。
        if !path.exists() {
            eprintln!("[WARN] Input path does not exist: {}", input);
            collector.input_errors += 1;
            continue;
        }
        collector.collect_input(path);
    }

    let (target_files, duplicates) = collector.finish();
    let total = target_files.len();
    println!("Found {} images. Processing...", total);

    let report = Report {
        input_errors: collector.input_errors,
        collection_errors: collector.collection_errors,
        excluded: collector.excluded,
        duplicates,
        links_followed: collector.links_followed,
        links_skipped: collector.links_skipped,
        ..Report::default()
    };

    let created_dirs = DirectoryCache::default();
    let unavailable_reported: Mutex<HashSet<PathBuf>> = Mutex::new(HashSet::new());
    target_files.par_iter().for_each(|file_path| {
        match process_image(file_path, &args, &created_dirs, &unavailable_reported) {
            Ok(result) => report.record(&result),
            Err(error) => {
                eprintln!("[ERROR] {:?}: {}", file_path, error);
                report.record_error();
            }
        }
    });

    // §12: 入力ファイル総数 ＝ 5 種の和（デバッグビルドで不変条件を確認する）。
    debug_assert_eq!(report.accounted_files(), total, "result tally must sum to the total");
    report.print(total);
    if report.failed() {
        std::process::exit(1);
    }
}

/// REQ-COL-06: 分類先フォルダ名の開始前検証。
fn validate_output_folder_names(args: &Args) -> Result<(), String> {
    let names = [
        ("comfy", &args.comfy_dir_name),
        ("webui", &args.webui_dir_name),
        ("novelai", &args.novelai_dir_name),
        ("swarm", &args.swarm_dir_name),
        ("invoke", &args.invoke_dir_name),
    ];
    for (label, name) in names {
        if name.is_empty() {
            return Err(format!("{}: must not be empty", label));
        }
        if name == "." || name == ".." {
            return Err(format!("{}: `{}` is not a folder name", label, name));
        }
        if name.contains('/') || name.contains('\\') {
            return Err(format!("{}: must not contain a path separator (`{}`)", label, name));
        }
        if Path::new(name).is_absolute() || name.contains(':') {
            return Err(format!("{}: must be a relative folder name (`{}`)", label, name));
        }
    }
    for (index, (label, name)) in names.iter().enumerate() {
        for (other_label, other_name) in names.iter().skip(index + 1) {
            if name.eq_ignore_ascii_case(other_name) {
                return Err(format!(
                    "{} and {} must be different folder names (`{}`)",
                    label, other_label, name
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn is_supported_image_path(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        let extension = extension.to_string_lossy();
        ["png", "jpg", "jpeg", "webp", "avif"]
            .iter()
            .any(|supported| extension.eq_ignore_ascii_case(supported))
    })
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

pub(crate) fn legacy_target_name<'a>(name: &str, args: &'a Args) -> Option<&'a str> {
    if name.eq_ignore_ascii_case(LEGACY_WEBUI_OUTPUT_DIR_NAME) {
        Some(&args.webui_dir_name)
    } else {
        None
    }
}

pub(crate) fn is_current_output_name(name: &str, args: &Args) -> bool {
    name.eq_ignore_ascii_case(&args.comfy_dir_name) ||
    name.eq_ignore_ascii_case(&args.webui_dir_name) ||
    name.eq_ignore_ascii_case(&args.novelai_dir_name) ||
    name.eq_ignore_ascii_case(&args.swarm_dir_name) ||
    name.eq_ignore_ascii_case(&args.invoke_dir_name)
}

fn process_image(
    file_path: &Path,
    args: &Args,
    created_dirs: &DirectoryCache,
    unavailable_reported: &Mutex<HashSet<PathBuf>>,
) -> Result<ProcessResult, String> {
    let mut sink = ClassifierSink::default();
    // 子プロセス時代の隔離性を、ファイル単位の catch_unwind で代替する。
    let scan_result = catch_unwind(AssertUnwindSafe(|| {
        fast_meta::scan_file(file_path, &fast_meta::ScanOptions::default(), &mut sink)
    }))
    .map_err(|_| "scan panicked".to_string())?;
    scan_result.map_err(|error| error.to_string())?;

    let target = sink.decide().unwrap_or(TargetType::None);
    if target == TargetType::None {
        return Ok(ProcessResult::SkippedNoMetadata);
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
    if let Err(error) = created_dirs.ensure(&dest_dir) {
        // REQ-TRN-03: 出力フォルダを作成できない場合は「dest 作成不可」として別集計する。
        // 同じ分類先についてのメッセージは 1 回までに抑える。
        let first = unavailable_reported.lock().unwrap().insert(dest_dir.clone());
        if first {
            eprintln!(
                "[WARN] Cannot create destination folder {:?} ({}); those files are left in place",
                dest_dir, error
            );
        }
        return Ok(ProcessResult::DestUnavailable);
    }

    let file_name = file_path.file_name().ok_or("Cannot get file name")?;
    let dest_path = dest_dir.join(file_name);
    match transfer_no_replace(file_path, &dest_path, args.copy)? {
        Transfer::Done => Ok(ProcessResult::Sorted(target)),
        // REQ-TRN-01: dest 同名衝突は「メタデータ無し」と区別して集計する。
        Transfer::DestinationExists => Ok(ProcessResult::DestConflict),
        // REQ-TRN-02: 仕分けは完了したが元ファイルが残った（安全側）。警告として別集計。
        Transfer::SourceRemains(warning) => Ok(ProcessResult::SortedWithWarning(target, warning)),
    }
}

/// 転送の結果。`DestinationExists` は dest を変更していない（既存ファイルを守る）。
#[derive(Debug, Eq, PartialEq)]
pub enum Transfer {
    Done,
    DestinationExists,
    /// dest への転送は完了したが、元ファイルを削除できなかった（REQ-TRN-02）。
    SourceRemains(String),
}

// Hard links provide an atomic no-replace move on the same filesystem. If unavailable,
// create_new reserves the destination before copying; never truncate an existing file.
fn transfer_no_replace(source: &Path, dest: &Path, copy: bool) -> Result<Transfer, String> {
    let metadata = fs::symlink_metadata(source).map_err(|e| format!("Source metadata failed: {}", e))?;
    if !metadata.file_type().is_file() {
        return Err("Source is not a regular file".to_string());
    }
    if !copy {
        match fs::hard_link(source, dest) {
            Ok(()) => {
                return match fs::remove_file(source) {
                    Ok(()) => Ok(Transfer::Done),
                    Err(e) => Ok(Transfer::SourceRemains(format!(
                        "Source removal failed after linking: {}",
                        e
                    ))),
                };
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                return Ok(Transfer::DestinationExists)
            }
            Err(_) => {} // Cross-device or unsupported: use an exclusive copy.
        }
    }
    let mut input = fs::File::open(source).map_err(|e| format!("Open source failed: {}", e))?;
    let mut output = match fs::OpenOptions::new().write(true).create_new(true).open(dest) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return Ok(Transfer::DestinationExists)
        }
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
        return match fs::remove_file(source) {
            Ok(()) => Ok(Transfer::Done),
            Err(e) => Ok(Transfer::SourceRemains(format!(
                "Source removal failed after copying: {}",
                e
            ))),
        };
    }
    Ok(Transfer::Done)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

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
            assert_eq!(
                transfer_no_replace(&source, &dest, copy).unwrap(),
                Transfer::DestinationExists
            );
            assert_eq!(fs::read(&source).unwrap(), b"source");
            assert_eq!(fs::read(&dest).unwrap(), b"existing");
        }
        fs::remove_file(&dest).unwrap();
        assert_eq!(
            transfer_no_replace(&source, &dest, true).unwrap(),
            Transfer::Done
        );
        assert_eq!(fs::read(&source).unwrap(), b"source");
        fs::remove_file(&dest).unwrap();
        assert_eq!(
            transfer_no_replace(&source, &dest, false).unwrap(),
            Transfer::Done
        );
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
            let first = one.join().unwrap();
            let second = two.join().unwrap();
            // どちらか一方だけが dest を確保する（既存ファイルは上書きしない）。
            assert_ne!(
                matches!(first, Transfer::DestinationExists),
                matches!(second, Transfer::DestinationExists)
            );
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
        let mut collector = Collector::new(&args);
        collector.collect_input(&root);
        let (files, duplicates) = collector.finish();

        assert_eq!(files, vec![root.join("batch/a.png")]);
        assert_eq!(duplicates, 0);
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
