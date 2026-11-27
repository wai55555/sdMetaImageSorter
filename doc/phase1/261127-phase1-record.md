# Phase 1 記録（261127 誤分類改修）— sorter 分類の中核置き換え

対象仕様書: `doc/261127_plan-false-classification-fix.md` §4／§8／§11.1 Phase 1。
対象リポジトリ: sdMetaImageSorter（`sd_meta_tools` 2.0.1-b）。

## 1. 実装した要件

| 要件 | 実装 | 検証 |
|---|---|---|
| REQ-CLS-01 | PNG `Comment` は値検証（U-1 のキー2つ以上）通過時のみ NovelAI 強。優先順位を手順5へ移動 | N-07／N-12／P-04／P-20／P-21 |
| REQ-CLS-02 | `has_xmp` を廃止し XMP を keyword 別に分解（(a) AIPromptInformation 非空、(b) DigitalSourceType、c CreatorTool、d XMP UserComment の Draw Things 検証、(e) description のみは無効） | N-01／N-03／N-05／N-06、P-11／P-12／P-13／P-13b／P-19 |
| REQ-CLS-03 | `a1111_text_block`（両マーカー＝中）、`steps_in_text`／`negprompt_in_text`（片方＝弱）、UserComment の `"workflow"` は ComfyUI 方向の弱（記録のみ） | N-08／N-13／N-14、P-02／P-15／P-22 |
| REQ-CLS-05 | Stealth は存在＝WebUI 方向の弱、`novelai`／`workflow` 部分一致＝方向別の弱。構造判定（U-8）は強 | N-09、P-09／P-10／P-23 |
| REQ-CLS-06 | trailing_webui から `Software` 部分一致を撤去。Trailing は全体で弱 1 票、`trailing_novelai` も弱へ格下げ | N-10、P-15／P-15b／P-22／P-23 |
| REQ-CLS-08 (a) | Software の AI 名照合から 256 バイト複製上限を撤去 | T-07 |
| REQ-CLS-08 (b) | UserComment／ImageDescription の走査を先頭 64KB に固定（仕様上の制限） | T-36 |
| REQ-CLS-09 | prompt の JSON 判定を同一レコード内で完結。BOM を除去 | T-08、P-03c |
| REQ-CLS-10 | AI 生成ツール名の表を 1 箇所に定義（NovelAI 優先、非AI名は不収録） | T-41 |
| REQ-CLS-11 | `has_software` を廃止。Software は値照合のみ | N-01／N-02／N-03、P-11 |
| REQ-CLS-12 | `Dream` は U-3 の構文検証通過時のみ InvokeAI 強 | P-07b／P-17 |
| §8.3 | 決定手順を Swarm → Invoke → Easy → Comfy → NovelAI → WebUI(強/中) → 手順 W（弱の複合）へ再構成 | 全 N／P |

## 2. 意図した挙動変更（§13.1 への追跡）

すべて §13.1「意図した変更: 分類」の (1)〜(6) のいずれかに追跡できる。列挙外の行き先変更は無い。

- (1) `Comment` 存在のみ／未検証 `Dream` では確定しない: N-07／N-12／P-17。
- (2) XMP の有無のみ・Software の存在のみでは WebUI にならない: N-01／N-02／N-03／N-05／N-06。
- (3) 弱シグナル単独では確定しない: N-08／N-09／N-13／N-14／P-15b。
- (4) `trailing_novelai` 単独では NovelAI にならない: N-10。
- (5) 検証済み `Comment` の順位変更に伴う強シグナル同士の競合: P-20（ComfyUI）／P-21（SwarmUI）。
- (6) 手順 W の同点規則: P-23（NovelAI）。

## 3. フェーズ内の暫定事項（Phase 2 で解消する）

- **JPEG COM の label 判定**: REQ-API-01 の構造化印が fast_meta 側の Phase 2 にあるため、
  Phase 1 では暫定で `label == "Stable Diffusion (A1111)"` を中シグナルとして残した。
  コード内に `Phase 1 暫定` コメントを置き、`phase1_provisional_jpeg_comment_still_uses_label`
  テストで挙動を固定している。この暫定のため **N-11（COM 片方マーカー）は Phase 1 の完了条件から
  除外**し、`n11_is_pending_until_phase2` で「暫定では WebUI に落ちる」ことを記録した。
- **P-16**: Stealth Auto ゲートの変更（REQ-EXT-01）が Phase 2 のため、レコード単位の分類
  （P-16 の期待どおり NovelAI）だけを固定し、ゲートが閉じないことの検証は Phase 2 で行う。
- **REQ-CLS-02 (d) の Draw Things シグナル**: Phase 1 で有効化済み。fast_meta の本文合成廃止
  （REQ-EXT-07）は Phase 2 だが、sorter 側の検証は生 JSON を前提にしているため先行して実装した。

## 4. テスト

`cargo test --locked`（12 unit + 6 integration）が緑。主な追加テスト:

- `fixures_match_their_expected_destinations`: §10.1 の N-01〜N-10／N-12〜N-14 と §10.2 の
  P-01〜P-23（P-15b／P-18 を含む）をレコード単位で一括検証。
- `t07_…`（256 バイト超の Software）、`t08_…`（prompt のレコード内 JSON 判定）、
  `t36_…`（64KB 制限）、`t41_…`（ツール名表）、
  `ai_tool_signal_survives_alongside_non_ai_names`（非AI名の veto を作らない読み替え）。
- 統合: `celsys_software_only_png_is_not_sorted`（N-02 の実例）、
  `positive_samples_still_sort_into_their_folders`（P-01／P-04 の回帰）。

## 5. ゴールデン出力

sorter の CLI 出力（集計行）は本フェーズで変更していない（変更は Phase 4）。fast_meta も
未変更のため、fast_meta のゴールデンに差分は無い。
