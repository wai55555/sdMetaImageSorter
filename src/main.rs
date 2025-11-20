use clap::Parser;
use rayon::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::env;
use walkdir::WalkDir;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Parser)]
#[command(name = "sd_meta_sorter", version = "4.0")]
struct Args {
    #[arg(required = true)]
    inputs: Vec<String>,

    /// Folder name for ComfyUI images (created inside input folder)
    #[arg(short = 'c', long, default_value = "comfyui_img")]
    comfy_dir_name: String,

    /// Folder name for WebUI/NovelAI images (created inside input folder)
    #[arg(short = 'w', long, default_value = "webui_image")]
    webui_dir_name: String,

    #[arg(long)]
    copy: bool,
}

// 仕分け先の種別
#[derive(Debug, PartialEq)]
enum TargetType {
    ComfyUI,
    WebUI, // WebUI or NovelAI
    None,  // メタデータなし
}

fn main() {
    let args = Args::parse();

    // 統計用カウンター
    let count_comfy = AtomicUsize::new(0);
    let count_webui = AtomicUsize::new(0);
    let count_skip = AtomicUsize::new(0);
    let count_error = AtomicUsize::new(0);

    // fast_meta.exe の場所特定
    let exe_path = env::current_exe().unwrap_or_default();
    let exe_dir = exe_path.parent().unwrap_or(Path::new("."));
    let fast_meta_path = exe_dir.join("fast_meta.exe");

    if !fast_meta_path.exists() {
        eprintln!("[ERROR] fast_meta.exe not found at {:?}", fast_meta_path);
        return;
    }

    // 1. ファイルリスト作成
    let mut target_files = Vec::new();
    let supported_extensions = ["png", "jpg", "jpeg", "webp", "avif"];

    println!("Scanning inputs...");
    for input in &args.inputs {
        let path = Path::new(input);
        if path.is_dir() {
            for entry in WalkDir::new(path).into_iter().filter_map(|e| e.ok()) {
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

    // 2. 並列処理
    target_files.par_iter().for_each(|file_path| {
        match process_image(file_path, &args, &fast_meta_path) {
            Ok(TargetType::ComfyUI) => { count_comfy.fetch_add(1, Ordering::Relaxed); },
            Ok(TargetType::WebUI) => { count_webui.fetch_add(1, Ordering::Relaxed); },
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
    println!("  -> WebUI/NovelAI: {}", count_webui.load(Ordering::Relaxed));
    println!("  -> Skipped (No Metadata): {}", count_skip.load(Ordering::Relaxed));
    println!("  -> Errors: {}", count_error.load(Ordering::Relaxed));
    println!("--------------------------------------------------");
}

fn process_image(file_path: &Path, args: &Args, fast_meta_path: &Path) -> Result<TargetType, String> {
    // fast_metaを実行して出力を取得
    let mut cmd = Command::new(fast_meta_path);
    cmd.arg(file_path);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let output = cmd.output().map_err(|e| format!("Exec failed: {}", e))?;
    
    // fast_metaがエラーで終了した場合も、メタデータなしとして扱うかエラーにするか。
    // ここでは解析不能＝スキップとして扱うのが安全。
    if !output.status.success() {
        // 念のためスキップ扱い
        return Ok(TargetType::None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    
    // --- 判定ロジック ---
    let target = if stdout.contains("ComfyUI") || stdout.contains("workflow") {
        TargetType::ComfyUI
    } else if stdout.contains("parameters") || stdout.contains("Stable Diffusion") || 
              stdout.contains("NovelAI") || stdout.contains("Software") || 
              stdout.contains("Steps: ") {
        TargetType::WebUI
    } else {
        TargetType::None
    };

    // メタデータがない場合は何もしない
    if target == TargetType::None {
        return Ok(TargetType::None);
    }

    // --- 仕分け先の決定 ---
    // 画像ファイルがある親ディレクトリを取得
    let parent_dir = file_path.parent().ok_or("Cannot get parent dir")?;
    
    let folder_name = match target {
        TargetType::ComfyUI => &args.comfy_dir_name,
        TargetType::WebUI => &args.webui_dir_name,
        _ => unreachable!(),
    };

    // 入力画像と同じ場所にフォルダパスを作成 (例: C:\Input\comfyui_img)
    let dest_dir = parent_dir.join(folder_name);

    // 遅延フォルダ作成 (移動が必要な場合のみ作る)
    // 並列処理なので create_dir_all が重複して呼ばれる可能性があるが、
    // 既に存在するならエラーにならないので問題ない。
    fs::create_dir_all(&dest_dir).map_err(|e| format!("Create dir failed: {}", e))?;

    // --- 移動/コピー実行 ---
    if let Some(file_name) = file_path.file_name() {
        let dest_path = dest_dir.join(file_name);

        // 移動元と移動先が同じならスキップ
        if file_path == dest_path {
            return Ok(target);
        }

        if args.copy {
            fs::copy(file_path, &dest_path).map_err(|e| format!("Copy failed: {}", e))?;
        } else {
            // 移動 (リネーム -> 失敗ならコピー削除)
            if fs::rename(file_path, &dest_path).is_err() {
                fs::copy(file_path, &dest_path).map_err(|e| format!("Fallback copy failed: {}", e))?;
                let _ = fs::remove_file(file_path);
            }
        }
    }

    Ok(target)
}