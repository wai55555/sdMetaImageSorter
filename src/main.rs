use clap::Parser;
use rayon::prelude::*;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::env;
use walkdir::WalkDir;
use std::sync::atomic::{AtomicUsize, Ordering};

const LEGACY_OUTPUT_DIR_NAMES: [&str; 2] = ["webui_image", "NovelAI"];

#[derive(Parser)]
#[command(name = "sd_meta_sorter", version = "4.4")]
struct Args {
    #[arg(required = true)]
    inputs: Vec<String>,

    #[arg(short = 'c', long, default_value = "comfyui_img")]
    comfy_dir_name: String,

    #[arg(short = 'w', long, default_value = "webui_img")]
    webui_dir_name: String,

    #[arg(short = 'n', long = "novelai-dir", default_value = "NovelAI_img")]
    novelai_dir_name: String,

    #[arg(long)]
    copy: bool,
}

#[derive(Debug, PartialEq)]
enum TargetType {
    ComfyUI,
    WebUI,
    NovelAI,
    None,
}

fn main() {
    let args = Args::parse();

    let count_comfy = AtomicUsize::new(0);
    let count_webui = AtomicUsize::new(0);
    let count_novelai = AtomicUsize::new(0);
    let count_skip = AtomicUsize::new(0);
    let count_error = AtomicUsize::new(0);

    let exe_path = env::current_exe().unwrap_or_default();
    let exe_dir = exe_path.parent().unwrap_or(Path::new("."));
    let fast_meta_path = exe_dir.join("fast_meta.exe");

    if !fast_meta_path.exists() {
        eprintln!("[ERROR] fast_meta.exe not found at {:?}", fast_meta_path);
        std::process::exit(1);
    }

    let mut target_files = Vec::new();
    let supported_extensions = ["png", "jpg", "jpeg", "webp", "avif"];

    println!("Scanning inputs (Skipping output folders)...");
    
    for input in &args.inputs {
        let path = Path::new(input);
        if path.is_dir() {
            // ★修正箇所: .into_iter() を追加しました
            let walker = WalkDir::new(path).into_iter().filter_entry(|entry| {
                // ディレクトリかつ名前が仕分け先と同じならスキップ(再帰しない)
                if entry.file_type().is_dir() {
                    let name = entry.file_name().to_string_lossy();
                    let is_current_output = [&args.comfy_dir_name, &args.webui_dir_name, &args.novelai_dir_name]
                        .iter()
                        .any(|output_name| name.eq_ignore_ascii_case(output_name));
                    let is_legacy_output = LEGACY_OUTPUT_DIR_NAMES
                        .iter()
                        .any(|output_name| name.eq_ignore_ascii_case(output_name));
                    if is_current_output || is_legacy_output {
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
        match process_image(file_path, &args, &fast_meta_path) {
            Ok(TargetType::ComfyUI) => { count_comfy.fetch_add(1, Ordering::Relaxed); },
            Ok(TargetType::WebUI) => { count_webui.fetch_add(1, Ordering::Relaxed); },
            Ok(TargetType::NovelAI) => { count_novelai.fetch_add(1, Ordering::Relaxed); },
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
    println!("  -> Skipped: {}", count_skip.load(Ordering::Relaxed));
    println!("  -> Errors: {}", count_error.load(Ordering::Relaxed));
    println!("--------------------------------------------------");
    if count_error.load(Ordering::Relaxed) > 0 {
        std::process::exit(1);
    }
}

fn process_image(file_path: &Path, args: &Args, fast_meta_path: &Path) -> Result<TargetType, String> {
    let mut cmd = Command::new(fast_meta_path);
    cmd.arg(file_path);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let output = cmd.output().map_err(|e| format!("Exec failed: {}", e))?;
    
    if !output.status.success() {
        return Err(format!("Metadata extraction failed ({}): {}", output.status, String::from_utf8_lossy(&output.stderr).trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let target = classify_metadata(&stdout);

    if target == TargetType::None {
        return Ok(TargetType::None);
    }

    // 親ディレクトリ取得
    let parent_dir = file_path.parent().ok_or("Cannot get parent dir")?;
    
    let folder_name = match target {
        TargetType::ComfyUI => &args.comfy_dir_name,
        TargetType::WebUI => &args.webui_dir_name,
        TargetType::NovelAI => &args.novelai_dir_name,
        _ => unreachable!(),
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

// The first line contains the input path, not metadata.
fn classify_metadata(output: &str) -> TargetType {
    let metadata = output.lines().skip(1).collect::<Vec<_>>().join("\n");
    let normalized = metadata.to_ascii_lowercase();

    // Prefer format headers over words in free-text prompts or stealth payloads.
    if metadata.contains("--- NovelAI [") {
        TargetType::NovelAI
    } else if metadata.contains("--- workflow ---") ||
              metadata.contains("--- generation_data ---") ||
              metadata.contains("--- prompt ---") {
        TargetType::ComfyUI
    } else if normalized.contains("novelai") {
        TargetType::NovelAI
    } else if metadata.contains("parameters") || metadata.contains("Stable Diffusion") ||
              metadata.contains("Software") || metadata.contains("Steps: ") ||
              metadata.contains("--- Stealth PNG Info (") {
        TargetType::WebUI
    } else {
        TargetType::None
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

    #[test]
    fn classifies_novelai_before_generic_metadata() {
        assert_eq!(classify_metadata("=== PNG File: x.png ===\n--- NovelAI [Settings] ---\n{\"steps\": 28}"), TargetType::NovelAI);
        assert_eq!(classify_metadata("=== PNG File: x.png ===\n--- Stealth PNG Info (Alpha) [stealth_pnginfo] ---\n{\"Software\":\"NovelAI\"}"), TargetType::NovelAI);
        assert_eq!(classify_metadata("=== PNG File: x.png ===\n--- Stable Diffusion (A1111) ---\nSteps: 20"), TargetType::WebUI);
    }

    #[test]
    fn classifies_stealth_and_ignores_the_input_path() {
        assert_eq!(classify_metadata("=== PNG File: NovelAI-workflow.png ===\n(No metadata detected)"), TargetType::None);
        assert_eq!(classify_metadata("=== PNG File: x.png ===\n--- Stealth PNG Info (Alpha) [stealth_pnginfo] ---\nopaque data"), TargetType::WebUI);
        assert_eq!(classify_metadata("=== PNG File: x.png ===\n--- Stealth PNG Info (RGB) [stealth_rgbcomp] ---\nComfyUI workflow"), TargetType::ComfyUI);
        assert_eq!(classify_metadata("=== PNG File: x.png ===\n--- workflow ---\n{}"), TargetType::ComfyUI);
    }
}
