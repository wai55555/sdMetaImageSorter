# Phase 3 記録（261127 誤分類改修）— sorter 側の未証明レコード隔離と依存追随

対象仕様書: `doc/261127_plan-false-classification-fix.md` §5.2／§11.1 Phase 3／§14.3。
対象リポジトリ: sdMetaImageSorter（`sd_meta_tools` 2.1.0 → **2.2.0**）。

## 1. 前提の解消（U-4）

Phase 3 着手時に U-4 を「source を分ける方式」で確定し、仕様書 §14.3 に記録した
（`docs` 側のコミットは本ブランチの先頭 `48e7b29`）。
fast_meta は Stage C step 4 のレコードを `Source::PngUnproven` として返す（v2.3.0）。

## 2. 実装した内容

| 項目 | 内容 | 検証 |
|---|---|---|
| REQ-EXT-03(b)／U-4 | `Source::PngUnproven` を Trailing と同じ隔離に置く。本文から `novelai`／A1111 マーカーを拾って弱 1 票にするが、単独では確定させない | 単体 `t11_unproven_png_records_are_only_weak_evidence`、統合 `unproven_png_records_do_not_sort_on_their_own` |
| 依存更新 | fast_meta を v2.3.0（`{ git = "...", tag = "v2.3.0" }`）へ更新 | `cargo build --release --locked`／`cargo test --locked` |
| 版 | 分類の意図した変更（未証明レコードが弱止まり）を含むため 2.2.0 へ | `--version`（統合テストで固定） |

### 隔離の意味（実装）

未証明レコードは「キー＋値」だけでは本物か偶然かを区別できないため、既存の弱シグナル
（`trailing_weak`／`trailing_novelai`）と同じ枠に載せる。他の弱シグナルと併存した場合のみ
複合で確定に寄与する（§8.3 手順 W）。中・強シグナルとしては数えない。

## 3. テスト結果

- `cargo build --release --locked` → 成功
- `cargo test --locked` → **unit 20 passed / integration 17 passed、失敗 0**
- 追加テスト: 上記 2 件（レコード単位の T-11、実ファイル経由の T-11）

## 4. 未着手

- Phase 5（棚卸し REQ-INV-01／02、16MB 上限 REQ-EXT-12、等価性・性能の最終確認、README 更新）は未着手。
