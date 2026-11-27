//! 結果の集計と表示（§6 REQ-TRN-01〜03、§12）。
//!
//! 入力ファイル総数 ＝ {仕分け済み, スキップ（メタデータ無し）, スキップ（dest 衝突）,
//! dest 作成不可, エラー} の 5 種の和、を維持する。警告（元削除失敗）は「仕分け済み」の
//! 内訳であり和には入れない。重複除去・リンク素通り・入力エラー・収集エラーは処理前の
//! 件数として別枠で表示する。

use std::sync::atomic::{AtomicUsize, Ordering};

use crate::classifier::TargetType;

/// 1 ファイルの処理結果。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProcessResult {
    /// 仕分け済み。
    Sorted(TargetType),
    /// 仕分け済みだが、元ファイルの削除に失敗して両方に残った（警告・内訳）。
    SortedWithWarning(TargetType, String),
    /// メタデータ無し／対象外でスキップ。
    SkippedNoMetadata,
    /// 仕分け先に同名ファイルがあり残留（REQ-TRN-01）。
    DestConflict,
    /// 仕分け先フォルダを作成できない（REQ-TRN-03）。
    DestUnavailable,
}

#[derive(Default)]
pub struct Report {
    pub comfy: AtomicUsize,
    pub webui: AtomicUsize,
    pub novelai: AtomicUsize,
    pub swarm: AtomicUsize,
    pub invoke: AtomicUsize,
    pub skipped_no_metadata: AtomicUsize,
    pub dest_conflict: AtomicUsize,
    pub dest_unavailable: AtomicUsize,
    pub errors: AtomicUsize,
    pub source_remains: AtomicUsize,
    // ---- 処理前の件数（和には入れない） ----
    pub input_errors: usize,
    pub collection_errors: usize,
    pub excluded: usize,
    pub duplicates: usize,
    pub links_followed: usize,
    pub links_skipped: usize,
}

impl Report {
    pub fn record(&self, result: &ProcessResult) {
        match result {
            ProcessResult::Sorted(TargetType::None) => {}
            ProcessResult::Sorted(target) | ProcessResult::SortedWithWarning(target, _) => {
                self.count_target(*target);
                if matches!(result, ProcessResult::SortedWithWarning(..)) {
                    self.source_remains.fetch_add(1, Ordering::Relaxed);
                }
            }
            ProcessResult::SkippedNoMetadata => {
                self.skipped_no_metadata.fetch_add(1, Ordering::Relaxed);
            }
            ProcessResult::DestConflict => {
                self.dest_conflict.fetch_add(1, Ordering::Relaxed);
            }
            ProcessResult::DestUnavailable => {
                self.dest_unavailable.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    pub fn record_error(&self) {
        self.errors.fetch_add(1, Ordering::Relaxed);
    }

    fn count_target(&self, target: TargetType) {
        let counter = match target {
            TargetType::ComfyUI => &self.comfy,
            TargetType::WebUI => &self.webui,
            TargetType::NovelAI => &self.novelai,
            TargetType::SwarmUI => &self.swarm,
            TargetType::InvokeAI => &self.invoke,
            TargetType::None => return,
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    fn target(&self, counter: &AtomicUsize) -> usize {
        counter.load(Ordering::Relaxed)
    }

    /// §12: 5 種の和が入力ファイル総数と一致すること。
    pub fn accounted_files(&self) -> usize {
        self.target(&self.comfy)
            + self.target(&self.webui)
            + self.target(&self.novelai)
            + self.target(&self.swarm)
            + self.target(&self.invoke)
            + self.target(&self.skipped_no_metadata)
            + self.target(&self.dest_conflict)
            + self.target(&self.dest_unavailable)
            + self.target(&self.errors)
    }

    /// 終了コード 1 の条件（§8.4）。引数検証エラー（2）は main 側で扱う。
    pub fn failed(&self) -> bool {
        self.target(&self.errors) > 0
            || self.target(&self.dest_unavailable) > 0
            || self.target(&self.source_remains) > 0
            || self.input_errors > 0
            || self.collection_errors > 0
    }

    pub fn print(&self, total: usize) {
        println!("--------------------------------------------------");
        println!("Total Scanned: {}", total);
        println!("  -> ComfyUI: {}", self.target(&self.comfy));
        println!("  -> WebUI: {}", self.target(&self.webui));
        println!("  -> NovelAI: {}", self.target(&self.novelai));
        println!("  -> SwarmUI: {}", self.target(&self.swarm));
        println!("  -> InvokeAI: {}", self.target(&self.invoke));
        println!(
            "  -> Skipped (no metadata): {}",
            self.target(&self.skipped_no_metadata)
        );
        println!(
            "  -> Skipped (dest conflict): {}",
            self.target(&self.dest_conflict)
        );
        println!(
            "  -> Dest unavailable: {}",
            self.target(&self.dest_unavailable)
        );
        println!("  -> Errors: {}", self.target(&self.errors));
        println!(
            "  -> (warning) Source not removed: {}",
            self.target(&self.source_remains)
        );
        println!("Pre-processing:");
        println!("  -> Input errors: {}", self.input_errors);
        println!("  -> Collection errors: {}", self.collection_errors);
        println!("  -> Excluded (output folder / unsupported): {}", self.excluded);
        println!("  -> Duplicate files removed: {}", self.duplicates);
        println!("  -> Links followed: {}", self.links_followed);
        println!("  -> Links skipped (cycle/visited): {}", self.links_skipped);
        println!("--------------------------------------------------");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T-22（REQ-TRN-01）: dest 衝突を「メタデータ無し」と別に数える。
    #[test]
    fn t22_dest_conflict_is_counted_separately() {
        let report = Report::default();
        report.record(&ProcessResult::SkippedNoMetadata);
        report.record(&ProcessResult::DestConflict);
        report.record(&ProcessResult::DestConflict);

        assert_eq!(report.target(&report.skipped_no_metadata), 1);
        assert_eq!(report.target(&report.dest_conflict), 2);
        assert_eq!(report.accounted_files(), 3);
        assert!(!report.failed());
    }

    /// T-23（REQ-TRN-02）: 元削除失敗は「仕分け済み」の内訳警告として数え、
    /// 和（accounted_files）には入れない。終了コードは 1。
    #[test]
    fn t23_source_remains_is_a_warning_inside_sorted() {
        let report = Report::default();
        report.record(&ProcessResult::SortedWithWarning(
            TargetType::WebUI,
            "remove failed".to_string(),
        ));

        assert_eq!(report.target(&report.webui), 1);
        assert_eq!(report.target(&report.source_remains), 1);
        assert_eq!(report.accounted_files(), 1);
        assert!(report.failed());
    }

    /// T-34（REQ-TRN-03）: dest 作成不可はファイル単位で数え、和に含める。
    #[test]
    fn t34_dest_unavailable_counts_each_file() {
        let report = Report::default();
        report.record(&ProcessResult::Sorted(TargetType::ComfyUI));
        report.record(&ProcessResult::DestUnavailable);
        report.record(&ProcessResult::DestUnavailable);

        assert_eq!(report.target(&report.dest_unavailable), 2);
        assert_eq!(report.accounted_files(), 3);
        assert!(report.failed());
    }
}
