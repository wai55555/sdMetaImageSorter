use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "sd-sorter-integration-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ if crc & 1 == 1 { 0xedb8_8320 } else { 0 };
        }
    }
    !crc
}

fn png_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut chunk = Vec::new();
    chunk.extend_from_slice(&(data.len() as u32).to_be_bytes());
    chunk.extend_from_slice(kind);
    chunk.extend_from_slice(data);
    chunk.extend_from_slice(&crc32(&chunk[4..]).to_be_bytes());
    chunk
}

fn text_chunk(keyword: &str, text: &str) -> Vec<u8> {
    let mut data = keyword.as_bytes().to_vec();
    data.push(0);
    data.extend_from_slice(text.as_bytes());
    png_chunk(b"tEXt", &data)
}

fn write_png(directory: &Path, name: &str, metadata: &[Vec<u8>]) {
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&1u32.to_be_bytes());
    ihdr.extend_from_slice(&1u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    png.extend_from_slice(&png_chunk(b"IHDR", &ihdr));
    for chunk in metadata {
        png.extend_from_slice(chunk);
    }
    // 1x1 RGBA, filter None, zlib 圧縮済みの固定 IDAT。
    png.extend_from_slice(&png_chunk(
        b"IDAT",
        &[120, 156, 99, 96, 0, 2, 0, 0, 5, 0, 1],
    ));
    png.extend_from_slice(&png_chunk(b"IEND", &[]));
    fs::write(directory.join(name), png).unwrap();
}

fn run(directory: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_sd_meta_sorter"))
        .arg(directory)
        .args([
            "-c",
            "CustomComfy",
            "-w",
            "CustomWebui",
            "-n",
            "CustomNovel",
            "-s",
            "CustomSwarm",
            "-i",
            "CustomInvoke",
        ])
        .output()
        .expect("run sd_meta_sorter")
}

#[test]
fn sorts_into_five_custom_folders_and_excludes_them_on_rerun() {
    let dir = TempDir::new();
    write_png(&dir.path, "comfy.png", &[text_chunk("workflow", "{}")]);
    write_png(
        &dir.path,
        "a1111.png",
        &[text_chunk("parameters", "a cat\nSteps: 20")],
    );
    write_png(
        &dir.path,
        "novelai.png",
        &[text_chunk(
            "Comment",
            "{\"steps\":20,\"sampler\":\"k_euler\",\"scale\":11}",
        )],
    );
    write_png(
        &dir.path,
        "swarm.png",
        &[text_chunk("sui_image_params", "{\"prompt\":\"cat\"}")],
    );
    write_png(
        &dir.path,
        "invoke.png",
        &[text_chunk("invokeai_metadata", "{\"prompt\":\"cat\"}")],
    );
    write_png(&dir.path, "unknown.png", &[]);

    let output = run(&dir.path);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Total Scanned: 6"), "{stdout}");
    assert!(stdout.contains("  -> ComfyUI: 1"), "{stdout}");
    assert!(stdout.contains("  -> WebUI: 1"), "{stdout}");
    assert!(stdout.contains("  -> NovelAI: 1"), "{stdout}");
    assert!(stdout.contains("  -> SwarmUI: 1"), "{stdout}");
    assert!(stdout.contains("  -> InvokeAI: 1"), "{stdout}");
    assert!(stdout.contains("  -> Skipped: 1"), "{stdout}");
    assert!(stdout.contains("  -> Errors: 0"), "{stdout}");

    assert!(dir.path.join("CustomComfy/comfy.png").is_file());
    assert!(dir.path.join("CustomWebui/a1111.png").is_file());
    assert!(dir.path.join("CustomNovel/novelai.png").is_file());
    assert!(dir.path.join("CustomSwarm/swarm.png").is_file());
    assert!(dir.path.join("CustomInvoke/invoke.png").is_file());
    assert!(dir.path.join("unknown.png").is_file());

    fs::remove_file(dir.path.join("unknown.png")).unwrap();
    let rerun = run(&dir.path);
    assert!(rerun.status.success());
    let rerun_stdout = String::from_utf8(rerun.stdout).unwrap();
    assert!(rerun_stdout.contains("Found 0 images"), "{rerun_stdout}");
}

#[test]
fn migrates_legacy_webui_directory_without_rescanning_it() {
    let dir = TempDir::new();
    fs::create_dir_all(dir.path.join("webui_image")).unwrap();
    fs::write(dir.path.join("webui_image/legacy.png"), b"legacy").unwrap();

    let output = run(&dir.path);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(dir.path.join("CustomWebui/legacy.png").is_file());
    assert!(!dir.path.join("webui_image").exists());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Found 0 images"), "{stdout}");
}

#[test]
fn novelai_option_long_name_and_legacy_alias_work() {
    for (option, folder) in [
        ("--novelai-dir-name", "NewNovel"),
        ("--novelai-dir", "OldNovel"),
    ] {
        let dir = TempDir::new();
        write_png(
            &dir.path,
            "novelai.png",
            &[text_chunk(
            "Comment",
            "{\"steps\":20,\"sampler\":\"k_euler\",\"scale\":11}",
        )],
        );
        let output = Command::new(env!("CARGO_BIN_EXE_sd_meta_sorter"))
            .arg(&dir.path)
            .args([option, folder])
            .output()
            .expect("run sd_meta_sorter");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(dir.path.join(folder).join("novelai.png").is_file());
    }
}

#[test]
fn help_and_version_use_the_unified_option_and_2_0_1b() {
    let help = Command::new(env!("CARGO_BIN_EXE_sd_meta_sorter"))
        .arg("--help")
        .output()
        .expect("run sd_meta_sorter --help");
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).unwrap();
    assert!(help.contains("--novelai-dir-name"), "{help}");

    let version = Command::new(env!("CARGO_BIN_EXE_sd_meta_sorter"))
        .arg("--version")
        .output()
        .expect("run sd_meta_sorter --version");
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8(version.stdout).unwrap().trim(),
        "sd_meta_sorter 2.0.1b"
    );
}

/// N-02: 今回の実例（Celsys Studio Tool だけを持つ PNG）が仕分けされないこと。
#[test]
fn celsys_software_only_png_is_not_sorted() {
    let dir = TempDir::new();
    write_png(
        &dir.path,
        "celsys.png",
        &[text_chunk("Software", "Celsys Studio Tool")],
    );
    write_png(
        &dir.path,
        "plain_comment.png",
        &[text_chunk("Comment", "edited in MyPainter 2.0")],
    );

    let output = run(&dir.path);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Total Scanned: 2"), "{stdout}");
    assert!(stdout.contains("  -> WebUI: 0"), "{stdout}");
    assert!(stdout.contains("  -> NovelAI: 0"), "{stdout}");
    assert!(stdout.contains("  -> Skipped: 2"), "{stdout}");
    assert!(stdout.contains("  -> Errors: 0"), "{stdout}");
    // 元の場所に残る（移動されない）。
    assert!(dir.path.join("celsys.png").is_file());
    assert!(dir.path.join("plain_comment.png").is_file());
    assert!(!dir.path.join("CustomWebui").exists());
    assert!(!dir.path.join("CustomNovel").exists());
}

/// P-01／P-04: 正例は従来どおり仕分けされること（回帰防止）。
#[test]
fn positive_samples_still_sort_into_their_folders() {
    let dir = TempDir::new();
    write_png(
        &dir.path,
        "a1111.png",
        &[text_chunk("parameters", "a cat\nNegative prompt: blur\nSteps: 20")],
    );
    write_png(
        &dir.path,
        "novelai.png",
        &[
            text_chunk("Description", "a cat"),
            text_chunk(
                "Comment",
                "{\"uc\":\"blur\",\"steps\":28,\"sampler\":\"k_euler_ancestral\",\"scale\":11,\"seed\":1}",
            ),
        ],
    );

    let output = run(&dir.path);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("  -> WebUI: 1"), "{stdout}");
    assert!(stdout.contains("  -> NovelAI: 1"), "{stdout}");
    assert!(dir.path.join("CustomWebui/a1111.png").is_file());
    assert!(dir.path.join("CustomNovel/novelai.png").is_file());
}
