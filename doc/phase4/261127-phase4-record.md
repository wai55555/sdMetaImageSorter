# Phase 4 記録（261127 誤分類改修）— 収集・移動・集計の是正

対象仕様書: `doc/261127_plan-false-classification-fix.md` §6／§8.4／§11.1 Phase 4／§13.1。
対象リポジトリ: sdMetaImageSorter（`sd_meta_tools` 2.0.1-b → **2.1.0**）。

## 1. 実装した要件

| 要件 | 実装 | 検証 |
|---|---|---|
| REQ-COL-01 | シンボリックリンク／ジャンクション経由のファイルとフォルダを対象化。実パス（canonicalize）の訪問済み集合で循環を防止。リンクを辿った件数と素通り件数を別行集計 | T-20（ジャンクション循環）、集計行 `Links followed`／`Links skipped` |
| REQ-COL-02 | 収集結果を実パスで重複排除し、実パス順に整列して決定的に処理。除去件数を別行集計 | T-21、`Duplicate files removed` |
| REQ-COL-03 | 存在しない入力は入力エラーとして集計し、他の入力の処理を継続。終了コード 1 | T-24 |
| REQ-COL-04 | 単一ファイル指定の移行範囲を「親自身が旧フォルダ」「親直下の旧フォルダ」に限定。移行失敗は警告（収集エラー）に留め、仕分けは継続 | T-25／T-25b |
| REQ-COL-05 | 直接指定ファイルにも拡張子フィルタと出力フォルダ除外を適用（入れ子出力フォルダを作らない） | T-27、単体 `t27_direct_file_filters_output_folders_and_extensions` |
| REQ-COL-06 | 5 分類のフォルダ名を処理開始前に検証（空・`.`・`..`・区切り含み・絶対パス・ドライブ表記・重複）。違反は移動せず終了コード **2** | T-28 |
| REQ-COL-07 | 走査中の読み取りエラー（read_dir／file_type／リンク先解決／canonicalize）を収集エラーとして集計し継続。1件でもあれば終了コード 1 | T-35 |
| REQ-TRN-01 | dest 同名衝突を「メタデータ無し」と別集計（スキップ扱い、終了コードは従来どおり 0） | T-22 |
| REQ-TRN-02 | 転送後の元削除失敗を「仕分け済み」の内訳警告として別行集計し、終了コード 1 | T-23（icacls で削除権限を拒否）、report の単体テスト |
| REQ-TRN-03 | 出力フォルダ名と同名の通常ファイルで作成できない場合を「dest 作成不可」としてファイル単位で集計。メッセージは分類先ごとに1回 | T-34、report の単体テスト |

## 2. 集計表示の変更（§13.1「意図した変更: 集計表示」）

```
Total Scanned: N                       ← 入力ファイル総数（下の5種の和と一致）
  -> ComfyUI / WebUI / NovelAI / SwarmUI / InvokeAI
  -> Skipped (no metadata)             ← 旧 "Skipped"
  -> Skipped (dest conflict)           ← 新規（REQ-TRN-01）
  -> Dest unavailable                  ← 新規（REQ-TRN-03）
  -> Errors
  -> (warning) Source not removed      ← 新規（REQ-TRN-02、仕分け済みの内訳）
Pre-processing:                        ← 処理前件数（和には入れない）
  -> Input errors / Collection errors / Excluded / Duplicate files removed
  -> Links followed / Links skipped (cycle/visited)
```

和の不変条件（§6 末尾）は `Report::accounted_files()` に実装し、デバッグビルドで
`debug_assert_eq!(accounted_files(), total)` を取る。`report::tests` で単体固定もした。

## 3. 終了コード（§8.4／§13.1）

| 条件 | 値 |
|---|---|
| フォルダ名の開始前検証エラー（REQ-COL-06） | 2 |
| ファイル単位のエラー、dest 作成不可、元削除失敗、入力エラー、収集エラー | 1 |
| それ以外（dest 同名衝突はスキップ扱い） | 0 |

Phase 1 の分類変更で追加された終了コード変更は無い。Phase 4 で変わったのは
REQ-COL-03（存在しない入力で 1）と REQ-COL-07（収集エラーで 1、従来はバッチ全体が異常終了）
および REQ-COL-06（2）で、いずれも §13.1 に列挙済み。

## 4. 実装上の判断

- **dest 作成不可と終了コード**: 従来は `create_dir_all` 失敗が処理エラー（終了コード 1）だった。
  Phase 4 では集計種別だけを分け、終了コード 1 は維持した（未完了の移動が残るため）。
  §13.1 の「意図した変更: 終了コード」に対する追加変更ではない。
- **移行失敗（単一ファイル指定）の集計先**: 専用行を増やさず「収集エラー」に含めた
  （警告文は `[WARN] Could not migrate existing output folders (continuing)`）。
- **重複排除のコスト**: 処理対象ファイル 1 件につき `canonicalize` 1 回。メタデータを持たない
  画像でも 1 ファイル 1 回の syscall であり、走査自体の read_dir に対して支配的ではない。
- **決定性**: 収集結果は実パス文字列で整列してから並列処理へ渡す（REQ-COL-02 の
  「結果が実行ごとに変わらない」を満たすため）。
- **単体テストの一時ディレクトリ**: OS の一時ディレクトリは環境によって ACL・リンク作成の
  可否が異なるため、`target/`（単体は `target/unit-tmp`、統合は `CARGO_TARGET_TMPDIR`）を使う。
  T-23 は icacls の `(DE)` 拒否で再現し、設定できない環境ではスキップする。

## 5. 版（U-6）

分類の意図した変更と CLI の出力・終了コード変更を含むため、sorter を **2.1.0** へマイナー上げした
（`Cargo.toml` と `--version`）。fast_meta は Phase 2 で 2.2.0。

## 6. 未対応

- Phase 3（fail-soft 化・デコード対称化: REQ-EXT-02〜05、08〜16）は未着手。
- Phase 5（棚卸し REQ-INV-01／02、16MB 上限 REQ-EXT-12、等価性・性能の最終確認）は未着手。
