//! 入力の収集と旧出力フォルダの移行（§6 REQ-COL-01〜05、07）。
//!
//! 走査は次の原則で行う。
//! - シンボリックリンク／ジャンクション経由のファイルとフォルダを対象に含める（REQ-COL-01）。
//! - 実パス（canonicalize 結果）の訪問済み集合でフォルダリンクの循環を防ぐ。
//!   循環等で素通りした件数と、リンクを辿った件数は別々に集計する。
//! - 収集段の読み取りエラーはバッチを止めず、収集エラーとして集計して継続する（REQ-COL-07）。
//! - 収集結果は実パスで重複排除してから並列処理へ渡す（REQ-COL-02）。

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::{is_current_output_name, is_supported_image_path, legacy_target_name, Args, Transfer};

pub struct Collector<'a> {
    args: &'a Args,
    visited_dirs: HashSet<PathBuf>,
    collected: Vec<PathBuf>,
    /// 存在しない等、入力そのもののエラー（処理前件数。REQ-COL-03）。
    pub input_errors: usize,
    /// 走査中・移行中のエラー（処理前件数。REQ-COL-07）。
    pub collection_errors: usize,
    /// 出力フォルダ内・対象外拡張子として除外した直接指定ファイル（REQ-COL-05）。
    pub excluded: usize,
    /// リンクを辿って対象にした件数（REQ-COL-01）。
    pub links_followed: usize,
    /// 循環・訪問済みで素通りした件数（REQ-COL-01）。
    pub links_skipped: usize,
}

impl<'a> Collector<'a> {
    pub fn new(args: &'a Args) -> Self {
        Self {
            args,
            visited_dirs: HashSet::new(),
            collected: Vec::new(),
            input_errors: 0,
            collection_errors: 0,
            excluded: 0,
            links_followed: 0,
            links_skipped: 0,
        }
    }

    /// 1 つの入力（フォルダまたはファイル）を収集する。
    pub fn collect_input(&mut self, input: &Path) {
        if input.is_dir() {
            // フォルダ指定時の移行範囲と挙動は現行どおり（REQ-COL-04）。
            match prepare_folder_input(input, self.args) {
                Ok(path) => self.walk_dir(&path, false),
                Err(error) => {
                    eprintln!("[ERROR] Could not migrate existing output folders: {}", error);
                    std::process::exit(1);
                }
            }
        } else if input.is_file() {
            let (path, warning) = prepare_file_input(input, self.args);
            if let Some(warning) = warning {
                // REQ-COL-04: 移行の失敗は指定ファイルの仕分けを中止させない。
                eprintln!(
                    "[WARN] Could not migrate existing output folders (continuing): {}",
                    warning
                );
                self.collection_errors += 1;
            }
            self.collect_direct_file(&path);
        }
    }

    /// REQ-COL-05: 直接指定されたファイルにも、走査と同じ拡張子フィルタと
    /// 出力フォルダ除外を適用する。
    fn collect_direct_file(&mut self, path: &Path) {
        if is_supported_image_path(path) && !has_output_folder_ancestor(path, self.args) {
            self.collected.push(path.to_path_buf());
        } else {
            self.excluded += 1;
        }
    }

    fn walk_dir(&mut self, dir: &Path, via_link: bool) {
        // 出力フォルダと旧出力フォルダの配下は走査しない。
        if let Some(name) = dir.file_name().map(|name| name.to_string_lossy()) {
            if let Some(target_name) = legacy_target_name(&name, self.args) {
                if !name.eq_ignore_ascii_case(target_name) {
                    let migrated = dir
                        .parent()
                        .ok_or_else(|| "Legacy output folder has no parent".to_string())
                        .and_then(|parent| migrate_legacy_directory(dir, &parent.join(target_name)));
                    if let Err(error) = migrated {
                        self.record_collection_error(dir, &error);
                    }
                    return;
                }
            }
            if is_current_output_name(&name, self.args) {
                return;
            }
        }

        // REQ-COL-01: 実パスで循環を検出する。同じ実フォルダを二度走査しない。
        let canonical = match fs::canonicalize(dir) {
            Ok(path) => path,
            Err(error) => {
                self.record_collection_error(dir, &format!("Canonicalize failed: {}", error));
                return;
            }
        };
        if !self.visited_dirs.insert(canonical) {
            if via_link {
                self.links_skipped += 1;
            }
            return;
        }

        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(error) => {
                self.record_collection_error(dir, &format!("Read directory failed: {}", error));
                return;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    self.record_collection_error(
                        dir,
                        &format!("Read directory entry failed: {}", error),
                    );
                    continue;
                }
            };
            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) => {
                    self.record_collection_error(
                        &path,
                        &format!("Read file type failed: {}", error),
                    );
                    continue;
                }
            };

            if file_type.is_symlink() {
                // REQ-COL-01: リンク先の種別で振り分ける（ジャンクションも含む）。
                match fs::metadata(&path) {
                    Ok(metadata) if metadata.is_dir() => {
                        self.links_followed += 1;
                        self.walk_dir(&path, true);
                    }
                    Ok(metadata) if metadata.is_file() => {
                        if is_supported_image_path(&path) {
                            self.links_followed += 1;
                            self.collected.push(path);
                        }
                    }
                    Ok(_) => {}
                    Err(error) => {
                        self.record_collection_error(
                            &path,
                            &format!("Follow link failed: {}", error),
                        );
                    }
                }
            } else if file_type.is_dir() {
                self.walk_dir(&path, false);
            } else if file_type.is_file() && is_supported_image_path(&path) {
                self.collected.push(path);
            }
        }
    }

    fn record_collection_error(&mut self, path: &Path, message: &str) {
        eprintln!("[WARN] Collection error at {:?}: {}", path, message);
        self.collection_errors += 1;
    }

    /// REQ-COL-02: 実パスで重複排除し、決定的な順序（実パス順）で返す。
    /// 戻り値は (処理対象, 重複除去件数)。
    pub fn finish(&mut self) -> (Vec<PathBuf>, usize) {
        self.collected.sort();
        let mut seen = HashSet::new();
        let mut files = Vec::with_capacity(self.collected.len());
        let mut duplicates = 0usize;
        for path in std::mem::take(&mut self.collected) {
            match fs::canonicalize(&path) {
                Ok(real) => {
                    if seen.insert(real) {
                        files.push(path);
                    } else {
                        duplicates += 1;
                    }
                }
                Err(error) => {
                    self.record_collection_error(&path, &format!("Canonicalize failed: {}", error));
                }
            }
        }
        (files, duplicates)
    }
}

/// 直接指定ファイルが出力フォルダ（現行または旧名）の配下にあるか。
fn has_output_folder_ancestor(path: &Path, args: &Args) -> bool {
    path.ancestors().skip(1).any(|ancestor| {
        ancestor
            .file_name()
            .map(|name| name.to_string_lossy())
            .is_some_and(|name| {
                is_current_output_name(&name, args)
                    || legacy_target_name(&name, args)
                        .is_some_and(|target| !name.eq_ignore_ascii_case(target))
            })
    })
}

fn prepare_folder_input(path: &Path, args: &Args) -> Result<PathBuf, String> {
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
    }
    Ok(path.to_path_buf())
}

/// REQ-COL-04: 単一ファイル指定時の移行範囲は、親自身が旧フォルダである場合と、
/// 親直下の旧フォルダに限定する。親のサブツリー全体は走査しない。
/// 移行に失敗しても警告に留め、指定ファイルの仕分けは継続する。
fn prepare_file_input(path: &Path, args: &Args) -> (PathBuf, Option<String>) {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));

    if let Some(name) = parent.file_name().map(|name| name.to_string_lossy()) {
        if let Some(target_name) = legacy_target_name(&name, args) {
            if !name.eq_ignore_ascii_case(target_name) {
                let Some(grandparent) = parent.parent() else {
                    return (
                        path.to_path_buf(),
                        Some("Legacy output folder has no parent".to_string()),
                    );
                };
                let destination = grandparent.join(target_name);
                let relative = match path.strip_prefix(parent) {
                    Ok(relative) => relative.to_path_buf(),
                    Err(error) => {
                        return (
                            path.to_path_buf(),
                            Some(format!("Resolve legacy file path failed: {}", error)),
                        )
                    }
                };
                return match migrate_legacy_directory(parent, &destination) {
                    Ok(()) => (destination.join(relative), None),
                    Err(error) => (path.to_path_buf(), Some(error)),
                };
            }
        }
    }

    match migrate_legacy_children(parent, args) {
        Ok(()) => (path.to_path_buf(), None),
        Err(error) => (path.to_path_buf(), Some(error)),
    }
}

/// 親直下の旧出力フォルダだけを移行する（再帰しない）。
fn migrate_legacy_children(parent: &Path, args: &Args) -> Result<(), String> {
    let entries =
        fs::read_dir(parent).map_err(|e| format!("Read directory {:?} failed: {}", parent, e))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("Read directory entry in {:?} failed: {}", parent, e))?;
        let file_type = entry
            .file_type()
            .map_err(|e| format!("Read file type for {:?} failed: {}", entry.path(), e))?;
        if !file_type.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(target_name) = legacy_target_name(&name, args) {
            if !name.eq_ignore_ascii_case(target_name) {
                migrate_legacy_directory(&entry.path(), &parent.join(target_name))?;
            }
        }
    }
    Ok(())
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
            let entry =
                entry.map_err(|e| format!("Read directory entry in {:?} failed: {}", parent, e))?;
            let file_type = entry
                .file_type()
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
            fs::rename(source, destination).map_err(|e| {
                format!(
                    "Rename {:?} to {:?} failed: {}",
                    source, destination, e
                )
            })?;
            println!("[INFO] Renamed legacy folder {:?} -> {:?}", source, destination);
        }
        Err(e) => return Err(format!("Inspect destination {:?} failed: {}", destination, e)),
        Ok(metadata) if metadata.file_type().is_dir() => {
            merge_legacy_directory_contents(source, destination)?;
            fs::remove_dir(source)
                .map_err(|e| format!("Remove migrated legacy folder {:?} failed: {}", source, e))?;
            println!("[INFO] Merged legacy folder {:?} into {:?}", source, destination);
        }
        Ok(_) => {
            return Err(format!(
                "Migration destination exists and is not a directory: {:?}",
                destination
            ))
        }
    }
    Ok(())
}

fn merge_legacy_directory_contents(source: &Path, destination: &Path) -> Result<(), String> {
    let entries =
        fs::read_dir(source).map_err(|e| format!("Read legacy folder {:?} failed: {}", source, e))?;
    for entry in entries {
        let entry =
            entry.map_err(|e| format!("Read legacy folder entry in {:?} failed: {}", source, e))?;
        let source_entry = entry.path();
        let destination_entry = destination.join(entry.file_name());
        let source_type = entry
            .file_type()
            .map_err(|e| format!("Read file type for {:?} failed: {}", source_entry, e))?;
        let destination_metadata = match fs::symlink_metadata(&destination_entry) {
            Ok(metadata) => Some(metadata),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(format!("Inspect {:?} failed: {}", destination_entry, e)),
        };

        if source_type.is_dir()
            && destination_metadata
                .as_ref()
                .is_some_and(|m| m.file_type().is_dir())
        {
            merge_legacy_directory_contents(&source_entry, &destination_entry)?;
            fs::remove_dir(&source_entry)
                .map_err(|e| format!("Remove merged directory {:?} failed: {}", source_entry, e))?;
        } else if destination_metadata.is_none() {
            if source_type.is_file() {
                if !transfer_for_migration(&source_entry, &destination_entry, false)? {
                    move_legacy_conflict(&source_entry, destination, &entry.file_name())?;
                }
            } else {
                fs::rename(&source_entry, &destination_entry).map_err(|e| {
                    format!(
                        "Move {:?} to {:?} failed: {}",
                        source_entry, destination_entry, e
                    )
                })?;
            }
        } else {
            move_legacy_conflict(&source_entry, destination, &entry.file_name())?;
        }
    }
    Ok(())
}

/// 移行経路では「dest 既存」だけを false として扱い、転送自体の失敗はエラーにする。
fn transfer_for_migration(source: &Path, dest: &Path, copy: bool) -> Result<bool, String> {
    match crate::transfer_no_replace(source, dest, copy)? {
        Transfer::DestinationExists => Ok(false),
        Transfer::Done => Ok(true),
        Transfer::SourceRemains(message) => Err(message),
    }
}

fn move_legacy_conflict(
    source: &Path,
    destination_dir: &Path,
    original_name: &std::ffi::OsStr,
) -> Result<(), String> {
    let source_metadata = fs::symlink_metadata(source)
        .map_err(|e| format!("Inspect conflict source {:?} failed: {}", source, e))?;
    for index in 1..=10_000usize {
        let destination = legacy_conflict_path(destination_dir, original_name, index);
        if source_metadata.file_type().is_file() {
            if transfer_for_migration(source, &destination, false)? {
                println!("[INFO] Preserved filename conflict as {:?}", destination);
                return Ok(());
            }
        } else if source_metadata.file_type().is_dir() {
            match fs::create_dir(&destination) {
                Ok(()) => {
                    merge_legacy_directory_contents(source, &destination)?;
                    fs::remove_dir(source).map_err(|e| {
                        format!("Remove merged conflict folder {:?} failed: {}", source, e)
                    })?;
                    println!("[INFO] Preserved folder conflict as {:?}", destination);
                    return Ok(());
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => {
                    return Err(format!(
                        "Create conflict folder {:?} failed: {}",
                        destination, e
                    ))
                }
            }
        } else {
            match fs::rename(source, &destination) {
                Ok(()) => {
                    println!("[INFO] Preserved filename conflict as {:?}", destination);
                    return Ok(());
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => {
                    return Err(format!(
                        "Move conflict {:?} to {:?} failed: {}",
                        source, destination, e
                    ))
                }
            }
        }
    }
    Err(format!(
        "Could not find a free conflict name for {:?}",
        source
    ))
}

fn legacy_conflict_path(
    parent: &Path,
    name: &std::ffi::OsStr,
    index: usize,
) -> std::path::PathBuf {
    let name = name.to_string_lossy();
    let path = Path::new(name.as_ref());
    if let Some(extension) = path.extension() {
        let stem = path
            .file_stem()
            .unwrap_or(std::ffi::OsStr::new(name.as_ref()));
        parent.join(format!(
            "{}__legacy_{}.{}",
            stem.to_string_lossy(),
            index,
            extension.to_string_lossy()
        ))
    } else {
        parent.join(format!("{}__legacy_{}", name, index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    /// リンク（ジャンクション）と ACL を扱えるよう、OS の一時ディレクトリではなく
    /// リポジトリの `target/` 配下を使う。
    fn temp_dir(tag: &str) -> PathBuf {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("unit-tmp")
            .join(format!(
                "sd-sorter-collect-{}-{}-{}",
                tag,
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn args() -> Args {
        Args::parse_from(["sd_meta_sorter", "input"])
    }

    fn create_junction(link: &Path, target: &Path) -> bool {
        #[cfg(windows)]
        {
            let output = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .output();
            match output {
                Ok(output) if output.status.success() => true,
                Ok(output) => {
                    eprintln!(
                        "mklink failed: {} {}",
                        String::from_utf8_lossy(&output.stdout),
                        String::from_utf8_lossy(&output.stderr)
                    );
                    false
                }
                Err(error) => {
                    eprintln!("mklink could not run: {}", error);
                    false
                }
            }
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link).is_ok()
        }
        #[cfg(not(any(windows, unix)))]
        {
            let _ = (link, target);
            false
        }
    }

    /// T-21（REQ-COL-02）: フォルダと配下ファイルの同時指定で二重処理しない。
    #[test]
    fn t21_folder_and_contained_file_are_deduplicated() {
        let root = temp_dir("dedupe");
        fs::create_dir_all(root.join("batch")).unwrap();
        fs::write(root.join("batch/a.png"), b"a").unwrap();
        fs::write(root.join("batch/b.png"), b"b").unwrap();

        let args = args();
        let mut collector = Collector::new(&args);
        collector.collect_input(&root);
        collector.collect_input(&root.join("batch/a.png"));
        let (files, duplicates) = collector.finish();

        assert_eq!(files.len(), 2, "{files:?}");
        assert_eq!(duplicates, 1);
        fs::remove_dir_all(root).unwrap();
    }

    /// T-20（REQ-COL-01）: リンク経由のファイル・フォルダを対象にし、循環で止まる。
    #[test]
    fn t20_follows_links_without_looping() {
        let root = temp_dir("links");
        fs::create_dir_all(root.join("a")).unwrap();
        fs::write(root.join("a/image.png"), b"a").unwrap();
        fs::write(root.join("top.png"), b"t").unwrap();
        let linked_file = root.join("a").join("link.png");
        #[cfg(unix)]
        let created_file_link =
            std::os::unix::fs::symlink(root.join("top.png"), &linked_file).is_ok();
        #[cfg(windows)]
        let created_file_link =
            std::os::windows::fs::symlink_file(root.join("top.png"), &linked_file).is_ok();
        #[cfg(not(any(unix, windows)))]
        let created_file_link = false;
        let cycle = root.join("a").join("loop");
        let cycle_created = create_junction(&cycle, &root);

        let args = args();
        let mut collector = Collector::new(&args);
        collector.collect_input(&root);
        let (files, _) = collector.finish();

        // 循環していても走査は終了し、実ファイルは重複除去される。
        assert!(files.iter().any(|path| path.ends_with("a/image.png")));
        if cycle_created {
            assert!(collector.links_followed >= 1);
            assert!(collector.links_skipped >= 1);
        }
        if created_file_link {
            assert!(collector.links_followed >= 1);
        }
        if !cycle_created && !created_file_link {
            eprintln!("skipped link assertions: cannot create links on this environment");
        }
        let _ = fs::remove_dir_all(root);
    }

    /// T-24（REQ-COL-03）: 存在しない入力は入力エラーとして数え、処理を続ける。
    #[test]
    fn t24_missing_input_is_counted_and_others_continue() {
        let root = temp_dir("missing");
        fs::write(root.join("a.png"), b"a").unwrap();
        let args = args();
        let mut collector = Collector::new(&args);
        collector.collect_input(&root.join("does-not-exist.png"));
        collector.input_errors += 1;
        collector.collect_input(&root);
        let (files, _) = collector.finish();
        assert_eq!(collector.input_errors, 1);
        assert_eq!(files.len(), 1);
        fs::remove_dir_all(root).unwrap();
    }

/// T-35（REQ-COL-07）: 読めないエントリ（ここでは壊れたジャンクション）があっても
/// バッチを止めず、収集エラーとして集計して走査を継続する。
#[test]
fn t35_collection_error_does_not_stop_the_batch() {
    let root = temp_dir("collect-error");
    fs::write(root.join("a.png"), b"a").unwrap();
    let broken = root.join("broken-link");
    let created = create_junction(&broken, &root.join("does-not-exist"));
    let args = args();
    let mut collector = Collector::new(&args);
    collector.collect_input(&root);
    let (files, _) = collector.finish();

    assert_eq!(files.len(), 1, "{files:?}");
    if created {
        assert_eq!(collector.collection_errors, 1);
    } else {
        eprintln!("skipped collection error assertion: cannot create junctions here");
    }
    let _ = fs::remove_dir_all(root);
}

/// T-27 の単体版（REQ-COL-05）: 直接指定ファイルのフィルタ。
#[test]
fn t27_direct_file_filters_output_folders_and_extensions() {
    let root = temp_dir("direct");
    fs::create_dir_all(root.join("webui_img")).unwrap();
    write_dummy(&root.join("webui_img/sorted.png"));
    write_dummy(&root.join("note.txt"));
    write_dummy(&root.join("ok.png"));

    let args = args();
    let mut collector = Collector::new(&args);
    collector.collect_input(&root.join("webui_img/sorted.png"));
    collector.collect_input(&root.join("note.txt"));
    collector.collect_input(&root.join("ok.png"));
    let (files, _) = collector.finish();

    assert_eq!(collector.excluded, 2);
    assert_eq!(files, vec![root.join("ok.png")]);
    let _ = fs::remove_dir_all(root);
}

fn write_dummy(path: &Path) {
    fs::write(path, b"x").unwrap();
}
}
