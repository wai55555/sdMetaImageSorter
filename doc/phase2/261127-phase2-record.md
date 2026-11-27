# Phase 2 記録（261127 誤分類改修）— sorter 側の構造化印への切り替え

対象仕様書: `doc/261127_plan-false-classification-fix.md` §5／§11.1 Phase 2。
対象リポジトリ: sdMetaImageSorter（`sd_meta_tools`）。

## 1. 実装した要件

| 要件 | 実装 | 検証 |
|---|---|---|
| REQ-CLS-04 | JPEG COM の A1111 判定を `label` から `RecordView.detail` の構造化印へ切り替え（REQ-API-01）。両マーカー＝中、片方＝弱。label 依存を撤去 | T-06、N-11、P-14、`jpeg_com_marks_decide_a1111_without_using_the_label` |
| REQ-CLS-07 | Phase 1 で有効化済み（XMP UserComment の生 JSON を値検証）。REQ-EXT-07 の本文合成廃止と整合することを確認 | P-12 |
| 依存更新 | fast_meta を v2.2.0（`{ path = "../checkImageMetadata" }`）へ更新 | `cargo build --locked`／`cargo test --locked` |

Phase 1 の暫定（`label == "Stable Diffusion (A1111)"` で確定）は撤去した。分類条件に
`record.label` を参照する箇所はテスト以外に存在しない（`grep -n "label" src/classifier.rs`）。

## 2. フィクスチャの更新

- **N-11**（COM 片方マーカー）を Phase 1 の除外から戻し、期待値 None で固定した
  （`detail = a1111-one-marker` → 弱1票のみ）。
- **P-14** に `detail = a1111-both-markers` を付け、中シグナル単独で WebUI になることを固定した。
- **P-13 変種**（XMP パケットが tEXt に格納された PNG）を統合テスト
  `xmp_packet_in_a_text_chunk_is_sorted_as_webui` で固定した（REQ-EXT-06 が前提）。

## 3. 完了条件の確認

| Phase 2 完了条件 | 状態 |
|---|---|
| G-01／G-02 | fast_meta 側で達成（`doc/phase2-261127-record.md`） |
| T-06／T-16／T-17 | T-06 は sorter、T-16／T-17 は fast_meta のテストで固定 |
| T-40 | fast_meta の `t40_*` |
| N-11／P-12／P-14／P-13 変種 | すべて緑 |
| P-16 | レコード単位の分類（NovelAI）と fast_meta の T-40 で担保。実ファイルの Stealth 合成器が
  sorter 側に無いため、LSB 経路の E2E は fast_meta の phase4_stealth に委ねる |
| label 依存が残っていない | `classifier.rs` はテスト以外で label を参照しない |

## 4. 申し送り

- 依存は開発用に path 指定へ切り替えている。`v2.2.0` タグ公開後に
  `{ git = "https://github.com/wai55555/checkImageMetadata", tag = "v2.2.0" }` へ戻すこと
  （Cargo.toml にコメント済み）。
- Phase 3（fail-soft 化）以降は未着手。
