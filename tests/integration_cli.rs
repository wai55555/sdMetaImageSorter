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
        // CARGO_TARGET_TMPDIR は target/ 配下。OS の一時ディレクトリは環境によって
        // ACL（サンドボックス）が異なり、T-23 の削除権限テストが成立しないため使わない。
        let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
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
    assert!(stdout.contains("  -> Skipped (no metadata): 1"), "{stdout}");
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
fn help_and_version_use_the_unified_option_and_2_1_0() {
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
        "sd_meta_sorter 2.1.0"
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
    assert!(stdout.contains("  -> Skipped (no metadata): 2"), "{stdout}");
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

fn jpeg_with_comment(text: &str) -> Vec<u8> {
    let mut jpeg = b"\xff\xd8".to_vec();
    jpeg.extend_from_slice(&[0xff, 0xfe]);
    jpeg.extend_from_slice(&((text.len() + 2) as u16).to_be_bytes());
    jpeg.extend_from_slice(text.as_bytes());
    jpeg.extend_from_slice(b"\xff\xda");
    jpeg
}

/// P-14／N-11（REQ-CLS-04）: COM の A1111 判定は構造化印で行う。
#[test]
fn jpeg_com_marks_decide_a1111_without_using_the_label() {
    let dir = TempDir::new();
    fs::write(
        dir.path.join("both.jpg"),
        jpeg_with_comment("a cat\nNegative prompt: blur\nSteps: 20"),
    )
    .unwrap();
    fs::write(
        dir.path.join("one.jpg"),
        jpeg_with_comment("Steps: 20 photo comment"),
    )
    .unwrap();

    let output = run(&dir.path);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("  -> WebUI: 1"), "{stdout}");
    assert!(stdout.contains("  -> Skipped (no metadata): 1"), "{stdout}");
    assert!(dir.path.join("CustomWebui/both.jpg").is_file());
    assert!(dir.path.join("one.jpg").is_file());
}

/// P-13 変種（REQ-EXT-06）: tEXt に格納された XMP パケットからも分類できる。
#[test]
fn xmp_packet_in_a_text_chunk_is_sorted_as_webui() {
    let dir = TempDir::new();
    let packet = "<x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'><rdf:Description xmlns:i='http://iptc.org/std/Iptc4xmpExt/2008-02-29/'><i:AIPromptInformation>a cat</i:AIPromptInformation></rdf:Description></rdf:RDF></x:xmpmeta>";
    write_png(
        &dir.path,
        "text_xmp.png",
        &[text_chunk("XML:com.adobe.xmp", packet)],
    );

    let output = run(&dir.path);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("  -> WebUI: 1"), "{stdout}");
    assert!(dir.path.join("CustomWebui/text_xmp.png").is_file());
}

fn run_with(directory: &Path, extra: &[&str]) -> std::process::Output {
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
        .args(extra)
        .output()
        .expect("run sd_meta_sorter")
}

/// T-22（REQ-TRN-01）: dest 同名衝突を「メタデータ無し」と別集計にする。
#[test]
fn t22_dest_conflict_has_its_own_tally() {
    let dir = TempDir::new();
    write_png(
        &dir.path,
        "a1111.png",
        &[text_chunk("parameters", "a cat\nSteps: 20")],
    );
    // --copy で元を残し、2 回目は同じ名前の dest が既にある状態にする。
    let first = run_with(&dir.path, &["--copy"]);
    assert!(first.status.success());
    let second = run_with(&dir.path, &["--copy"]);
    // 衝突はスキップ扱い（従来どおり終了コード 0）。
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    let stdout = String::from_utf8(second.stdout).unwrap();
    assert!(stdout.contains("  -> Skipped (dest conflict): 1"), "{stdout}");
    assert!(stdout.contains("  -> Skipped (no metadata): 0"), "{stdout}");
    assert!(stdout.contains("  -> Errors: 0"), "{stdout}");
    assert!(dir.path.join("CustomWebui/a1111.png").is_file());
    assert!(dir.path.join("a1111.png").is_file());
}

/// T-24（REQ-COL-03）: 存在しない入力パスは入力エラーとして集計し、
/// 他の入力は処理したうえで終了コード 1。
#[test]
fn t24_missing_input_is_reported_and_exits_one() {
    let dir = TempDir::new();
    write_png(
        &dir.path,
        "a1111.png",
        &[text_chunk("parameters", "a cat\nSteps: 20")],
    );
    let output = Command::new(env!("CARGO_BIN_EXE_sd_meta_sorter"))
        .arg(dir.path.join("does-not-exist.png"))
        .arg(&dir.path)
        .args(["-c", "C", "-w", "W", "-n", "N", "-s", "S", "-i", "I"])
        .output()
        .expect("run sd_meta_sorter");

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("Input path does not exist"), "{stderr}");
    assert!(stdout.contains("  -> Input errors: 1"), "{stdout}");
    assert!(stdout.contains("  -> WebUI: 1"), "{stdout}");
    assert!(dir.path.join("W/a1111.png").is_file());
}

/// T-25（REQ-COL-04）: 単一ファイル指定では親のサブツリー全体を移行しない。
#[test]
fn t25_single_file_input_does_not_migrate_the_whole_subtree() {
    let dir = TempDir::new();
    fs::create_dir_all(dir.path.join("sub/webui_image")).unwrap();
    fs::write(dir.path.join("sub/webui_image/legacy.png"), b"legacy").unwrap();
    write_png(
        &dir.path,
        "a1111.png",
        &[text_chunk("parameters", "a cat\nSteps: 20")],
    );

    let output = Command::new(env!("CARGO_BIN_EXE_sd_meta_sorter"))
        .arg(dir.path.join("a1111.png"))
        .args(["-c", "C", "-w", "W", "-n", "N", "-s", "S", "-i", "I"])
        .output()
        .expect("run sd_meta_sorter");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    // 親直下ではない旧フォルダは移行されない。
    assert!(dir.path.join("sub/webui_image/legacy.png").is_file());
    assert!(!dir.path.join("sub/W").exists());
    // 指定ファイルは仕分けされる。
    assert!(dir.path.join("W/a1111.png").is_file());
}

/// T-25b（REQ-COL-04）: 親直下の旧フォルダは移行される。
#[test]
fn t25b_single_file_input_migrates_legacy_sibling_folder() {
    let dir = TempDir::new();
    fs::create_dir_all(dir.path.join("webui_image")).unwrap();
    fs::write(dir.path.join("webui_image/legacy.png"), b"legacy").unwrap();
    write_png(
        &dir.path,
        "a1111.png",
        &[text_chunk("parameters", "a cat\nSteps: 20")],
    );

    let output = Command::new(env!("CARGO_BIN_EXE_sd_meta_sorter"))
        .arg(dir.path.join("a1111.png"))
        .args(["-c", "C", "-w", "W", "-n", "N", "-s", "S", "-i", "I"])
        .output()
        .expect("run sd_meta_sorter");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!dir.path.join("webui_image").exists());
    assert!(dir.path.join("W/legacy.png").is_file());
    assert!(dir.path.join("W/a1111.png").is_file());
}

/// T-27（REQ-COL-05）: 直接指定にも拡張子フィルタと出力フォルダ除外を適用する。
#[test]
fn t27_direct_file_input_is_filtered_like_the_scan() {
    let dir = TempDir::new();
    fs::create_dir_all(dir.path.join("CustomWebui")).unwrap();
    write_png(
        &dir.path,
        "CustomWebui/sorted.png",
        &[text_chunk("parameters", "a cat\nSteps: 20")],
    );
    fs::write(dir.path.join("note.txt"), b"not an image").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_sd_meta_sorter"))
        .arg(dir.path.join("CustomWebui/sorted.png"))
        .arg(dir.path.join("note.txt"))
        .args(["-c", "CustomComfy", "-w", "CustomWebui"])
        .output()
        .expect("run sd_meta_sorter");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Found 0 images"), "{stdout}");
    assert!(
        stdout.contains("  -> Excluded (output folder / unsupported): 2"),
        "{stdout}"
    );
    // 入れ子の出力フォルダを作らない。
    assert!(!dir.path.join("CustomWebui/CustomWebui").exists());
    assert!(dir.path.join("CustomWebui/sorted.png").is_file());
}

/// T-28（REQ-COL-06）: フォルダ名の開始前検証は移動せずに終了コード 2。
#[test]
fn t28_invalid_folder_names_exit_with_two_and_move_nothing() {
    let cases: &[&[&str]] = &[
        &["-c", ""],
        &["-c", "."],
        &["-c", ".."],
        &["-c", "a/b"],
        &["-c", "a\\b"],
        &["-c", "C:\\abs"],
        &["-c", "same", "-w", "SAME"],
    ];
    for case in cases {
        let dir = TempDir::new();
        write_png(
            &dir.path,
            "a1111.png",
            &[text_chunk("parameters", "a cat\nSteps: 20")],
        );
        let output = Command::new(env!("CARGO_BIN_EXE_sd_meta_sorter"))
            .arg(&dir.path)
            .args(*case)
            .output()
            .expect("run sd_meta_sorter");
        assert_eq!(output.status.code(), Some(2), "case {:?}", case);
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains("Invalid output folder name"), "{stderr}");
        // 1 ファイルも移動していない。
        assert!(dir.path.join("a1111.png").is_file());
        assert!(!dir.path.join("webui_img").exists());
    }
}

/// T-34（REQ-TRN-03）: 出力フォルダ名と同名の通常ファイルがある場合は
/// 「dest 作成不可」としてファイル単位で集計し、メッセージは分類先ごとに 1 回。
#[test]
fn t34_dest_unavailable_is_tallied_per_file() {
    let dir = TempDir::new();
    // `CustomWebui` という名前の通常ファイルが出力フォルダ作成を妨げる。
    fs::write(dir.path.join("CustomWebui"), b"blocker").unwrap();
    write_png(
        &dir.path,
        "one.png",
        &[text_chunk("parameters", "a cat\nSteps: 20")],
    );
    write_png(
        &dir.path,
        "two.png",
        &[text_chunk("parameters", "a dog\nSteps: 20")],
    );

    let output = run(&dir.path);
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Total Scanned: 2"), "{stdout}");
    assert!(stdout.contains("  -> Dest unavailable: 2"), "{stdout}");
    assert!(stdout.contains("  -> Errors: 0"), "{stdout}");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(
        stderr.matches("Cannot create destination folder").count(),
        1,
        "{stderr}"
    );
    assert!(dir.path.join("one.png").is_file());
    assert!(dir.path.join("two.png").is_file());
}

/// T-23（REQ-TRN-02）: 転送後の元削除失敗は警告として別集計し、終了コードは 1。
/// Windows では、ファイル単位で削除権限を拒否（icacls (D)）して再現する。
/// 権限設定に失敗する環境ではスキップする。
#[cfg(windows)]
#[test]
fn t23_source_removal_failure_is_a_warning() {
    let dir = TempDir::new();
    write_png(
        &dir.path,
        "a1111.png",
        &[text_chunk("parameters", "a cat\nSteps: 20")],
    );
    let source = dir.path.join("a1111.png");
    let user = std::env::var("USERNAME").unwrap_or_default();
    let deny = Command::new("icacls")
        .arg(&source)
        .args(["/deny", &format!("{}:(DE)", user)])
        .output();
    let denied = deny.as_ref().map(|o| o.status.success()).unwrap_or(false);
    if !denied {
        eprintln!("skipped T-23: cannot deny delete permission on this environment");
        return;
    }

    let output = run(&dir.path);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();

    let _ = Command::new("icacls")
        .arg(&source)
        .args(["/remove:d", &user])
        .output();

    assert!(stdout.contains("  -> WebUI: 1"), "{stdout} {stderr}");
    assert!(
        stdout.contains("  -> (warning) Source not removed: 1"),
        "{stdout} {stderr}"
    );
    assert!(stdout.contains("  -> Errors: 0"), "{stdout} {stderr}");
    assert_eq!(output.status.code(), Some(1));
    assert!(dir.path.join("CustomWebui/a1111.png").is_file());
    assert!(source.is_file());
}
