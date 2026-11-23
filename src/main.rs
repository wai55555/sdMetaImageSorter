use clap::Parser;
use rayon::prelude::*;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::env;
use walkdir::WalkDir;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Parser)]
#[command(name = "sd_meta_sorter", version = "4.4")]
struct Args {
    #[arg(required = true)]
    inputs: Vec<String>,

    #[arg(short = 'c', long, default_value = "comfyui_img")]
    comfy_dir_name: String,

    #[arg(short = 'w', long, default_value = "webui_image")]
    webui_dir_name: String,

    #[arg(short = 'n', long = "novelai-dir", default_value = "NovelAI")]
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
        return;
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
                    if name == args.comfy_dir_name || name == args.webui_dir_name || name == args.novelai_dir_name {
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

    if let Some(file_name) = file_path.file_name() {
        let dest_path = dest_dir.join(file_name);

        if file_path == dest_path {
            return Ok(target);
        }

        if args.copy {
            fs::copy(file_path, &dest_path).map_err(|e| format!("Copy failed: {}", e))?;
        } else {
            if fs::rename(file_path, &dest_path).is_err() {
                fs::copy(file_path, &dest_path).map_err(|e| format!("Fallback copy failed: {}", e))?;
                let _ = fs::remove_file(file_path);
            }
        }
    }

    Ok(target)
}

// The first line contains the input path, not metadata.
fn classify_metadata(output: &str) -> TargetType {
    let metadata = output.lines().skip(1).collect::<Vec<_>>().join("\n");
    if metadata.contains("--- NovelAI [") {
        TargetType::NovelAI
    } else if metadata.contains("--- workflow ---") || metadata.contains("--- generation_data ---") || metadata.contains("\"workflow\"") || metadata.contains("\"generation_data\"") || metadata.contains("ComfyUI") {
        TargetType::ComfyUI
    } else if metadata.contains("NovelAI") || metadata.contains("novelai") {
        TargetType::NovelAI
    } else if metadata.contains("parameters") || metadata.contains("Stable Diffusion") ||
              metadata.contains("Software") ||
              metadata.contains("Steps: ") || metadata.contains("--- Stealth PNG Info (") {
        TargetType::WebUI
    } else {
        TargetType::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
