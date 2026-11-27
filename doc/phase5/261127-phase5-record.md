# Phase 5 記録（261127 誤分類改修）— 棚卸し・判断固定・等価性／性能の最終確認

対象仕様書: `doc/261127_plan-false-classification-fix.md` §7／§10.4／§11.1 Phase 5／§12。
対象リポジトリ: sdMetaImageSorter（`sd_meta_tools` 2.2.0）、checkImageMetadata（`fast_meta` 2.3.0）。

## 1. 実施した要件

| 要件 | 内容 | 検証 |
|---|---|---|
| REQ-INV-01 | 分類に使っていない担体の棚卸し表を `doc/inventory-261127.md` に固定（EXIF `Make`／`Artist`／`XPComment`、IPTC Caption、XMP `title`／`description`、PNG `Title`／`Source`）。各担体の「使わない理由」と、将来使う場合の条件（§2.2 の強さ定義に従う）を明記。`src/classifier.rs` にも同じ趣旨のコメントを置いた | **T-14** |
| REQ-INV-02 | C2PA は実装しない。`Source::C2pa` は enum の予約に留まることを README と棚卸し表に明記し、対応済みと誤認させる表現を置かない | **T-15** |
| REQ-EXT-12 | EXIF／XMP の 16MB 上限を資源保護として維持し、超過は「その担体が 0 件」になることを仕様として明記（コードコメント＋README）。切り捨て部分読みは採用しない理由も記録 | **T-30**（fast_meta） |
| §10.4 等価性 | 既存 Phase 0 コーパス＋混合コーパスで改修前後の行き先差分を取得し、§13.1 の意図した変更だけであることを確認 | 下記 2 |
| §10.4 性能 | メタデータ無し多数派の混合コーパスで改修前／Phase 4 時点／現在を比較し、Stealth ゲート変更のコスト内訳を確認 | 下記 3 |

### T-14（棚卸し表の担体が分類に効かないこと）

`t14_inventoried_carriers_do_not_classify` で 8 担体（EXIF Make／Artist／XPComment、
IPTC Caption/Abstract、XMP title、PNG Title／Source／XMP description）それぞれ単独の
レコード集合が `None` になることを固定した。

### T-15（C2PA の既知の制限）

`t15_c2pa_only_records_do_not_classify` で `Source::C2pa` のレコード単独が `None` に
なることを固定（現行は C2PA を抽出しないため、実ファイルでも常に None）。

### T-30（16MB 上限）

fast_meta 側で `t30_tiff_packet_over_the_16mb_limit_yields_no_records` と
`t30_xmp_packet_over_the_16mb_limit_yields_no_records` を追加し、上限超過が 0 件で
あることを固定した。

## 2. 等価性（改修前 → 改修後）

### 2.1 Phase 0 コーパス（13 ファイル、`doc/phase0/corpus`）

| ファイル | 改修前（main 2.0.1b） | 改修後（2.2.0） | 判定 |
|---|---|---|---|
| a1111.png | webui_img | webui_img | 同一 |
| a1111.jpg | webui_img | webui_img | 同一 |
| comfyui.png | comfyui_img | comfyui_img | 同一 |
| novelai.png | NovelAI_img | NovelAI_img | 同一 |
| novelai.webp | NovelAI_img | NovelAI_img | 同一 |
| swarmui.png | SwarmUI_img | SwarmUI_img | 同一 |
| invokeai.png | InvokeAI_img | InvokeAI_img | 同一 |
| midjourney.png | webui_img | webui_img | 同一（根拠は has_software → CreatorTool/Software の値） |
| photo_xmp.jpg | webui_img | webui_img | 同一（根拠は XMP 有無 → CreatorTool の値） |
| **stealth.png**（本文 `ComfyUI workflow` のみ） | comfyui_img | 仕分けなし | **§13.1 (3)** 弱シグナル単独の禁止 |
| **trailing.png**（末尾データのみ） | webui_img | 仕分けなし | **§13.1 (3)(4)** trailing は弱 1 票 |
| no_metadata.png / .jpg | 仕分けなし | 仕分けなし | 同一 |

### 2.2 混合コーパス（800 ファイル、`doc/phase5/generate_phase5_corpus.py`）

構成: メタデータ無し 560（70%）、汎用レコードのみ 160（20%）、Stealth 40（5%）、
正例 40（5%）。行き先の差分は **80 行（汎用レコードのみ 80 件が webui_img → 仕分けなし）**
だけで、追加・変更は 0 行だった。
- 汎用レコードのみ（`Software` のみ／`Title` のみ）は §13.1 **(2)**「XMP の有無のみ・
  Software の存在のみでは WebUI にならない」による意図した変更。
- Stealth・正例・メタデータ無しは前後で同一。

## 3. 性能（§10.4）

計測条件: Windows MSVC `cargo 1.91.1` の release ビルド、`--copy` で同一コーパスを処理。
3 回実行の範囲を併記（WSL からのプロセス起動オーバーヘッドを含むため、差の読み方は
同一条件内の比較に限る）。

| コーパス | 改修前 2.0.1b | Phase 4 時点 2.1.0 | 現在 2.2.0 |
|---|---|---|---|
| メタデータ無し 500 件（1×1 PNG） | 32–37 ms | 46–53 ms | 42–55 ms |
| 混合 800 件（1×1 PNG） | 39–62 ms | 57–62 ms | 54–58 ms |
| 汎用のみ 100 件（1024×1024 RGBA） | 32–48 ms | — | 37–38 ms |
| 汎用のみ 10 件（4096×4096 RGBA） | 23–25 ms | — | 25–27 ms |

内訳（結論）:
- **Phase 3（Stealth ゲート変更を含む）の追加コストは測定限界以下**。理由は実装側にあり、
  Stealth 検査は IDAT を全解凍せず、シグネチャ判定に必要な先頭行（`STEALTH_HEADER_BITS` 分）
  だけを解凍して不一致なら打ち切る。4096×4096（解凍 64MB/枚）でも差が出ないことで確認した。
- 改修前との差（+10〜20 ms/500 件 ≒ 20〜30 µs/件）は **Phase 4 の実パス canonicalize**
  （REQ-COL-02 の重複排除）によるもので、Phase 3 の Stealth ゲート変更ではない。
- 汎用レコードのみの増減は「Stealth 検査が発動する層」のコストとして上表の 1024×1024／
  4096×4096 で確認し、**支配的でない**（本書 §13.2 のリスク表にある「支配的なら汎用
  レコードの定義を見直す」には該当しない）。

## 4. README／doc の更新

- sorter `readme.md`: 「判定の原則と既知の制限」を追加（キー＋値、弱シグナルの複合、
  棚卸し表への参照、C2PA 未実装、16MB 上限、未証明レコードの隔離）。fast_meta の版を 2.3.0 に更新。
- fast_meta `README.md`: 16MB 上限の仕様（REQ-EXT-12）と C2PA 未実装（REQ-INV-02）、
  Phase 3 の fail-soft／UTF-16 XMP／RIFF 範囲などの挙動を追記。

## 5. §12 受け入れ条件の確認

| チェック項目 | 状態 |
|---|---|
| 負例 N-01〜N-14 が全て None | ✅（`fixures_match_their_expected_destinations`） |
| 正例 P-01〜P-23（P-15b／P-18 含む） | ✅（同上） |
| 分類条件に「キーの存在のみ」「XMP の有無のみ」「label 文字列」が残っていない | ✅（`T-06`、コードレビュー） |
| 弱シグナル単独で確定する経路が無い | ✅（`§8.3` 手順 W のみ、N-08／N-13／N-14／P-15b／T-11） |
| ゴールデン変更が要件 ID に追跡できる | ✅（`doc/phase2-261127-record.md` の一覧、Phase 3 は差分なし） |
| Stealth Auto の新ゲートで P-16／T-40 が通る | ✅（fast_meta `t40_*`、ゲート開閉の実ファイル確認） |
| 抽出層が本文を合成しない | ✅（`t17_*`、T-17） |
| 集計の 5 種の和が総数と一致 | ✅（`Report::accounted_files`＋`debug_assert`、report の単体テスト） |
| 終了コード（2／1／0） | ✅（T-24／T-28／T-34、T-22 は 0） |
| F-01〜F-37 の全件に処置が対応付いている | ✅（本書 §2.3 と各 Phase 記録） |
| 等価性差分が §13.1 のみ・性能記録あり | ✅（本記録 2／3） |
| 両リポジトリで build/test 緑 | ✅（sorter: unit 22 / integration 17、fast_meta: unit 114 / integration 27） |

## 6. 残課題（将来）

- 16MB 上限で担体が 0 件になったことを利用者が知る手段（デバッグ出力等）は、棚卸し表の
  「付随事項」に将来課題として記録した。
- C2PA の実装可否は利用実態が出た時点で別件とする（REQ-INV-02）。
