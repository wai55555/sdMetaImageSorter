# sdMetaImageSorter / fast_meta 誤分類改修 — Plan / Design / Specification / Requirements

| 項目 | 内容 |
|---|---|
| 対象リポジトリ A | https://github.com/wai55555/checkImageMetadata （`fast_meta`、現行 v2.1.0） |
| 対象リポジトリ B | https://github.com/wai55555/sdMetaImageSorter （`sd_meta_sorter`、現行 main、fast_meta v2.1.0 をライブラリ内蔵。監査時点の表示 v2.0.1b） |
| 正本となる監査 | `~/workspace/sdMetaImageSorter-metadata-flow-and-false-classification-audit.md`（2026-11-27 確定。独立レビュー全7周、新規推移 7→9→6→2→1→0→0、新規ゼロ2周連続で終了。最終形 F-01〜F-37） |
| 本書の目的 | 上記監査 F-01〜F-37 全件について、処置（修正／判断固定／現状固定）、対象リポジトリ、要件、検証方法を対応付け、追加質問なしに段階実装できる粒度で定義する |
| 本書の範囲外 | 実装コードの変更そのもの。本書は仕様書であり、実装は本書確定後に別途フェーズ単位で着手する |
| 優先順位の付け方 | P0 = 実害が確定しているもの・分類の中核を直すもの（必ずやる）／ P1 = 同型・未発症で頻度が無視できないもの（やる）／ P2 = 頻度が稀・低重大度（やるが後段フェーズでよい。判断固定・現状固定も含む） |

---

## 0. 前提指示（必読）

1. **監査が正本である。** 本書と監査の記述が食い違う場合は監査を正とし、本書側を訂正すること。F 番号の意味・状態（確定／同型・未発症／要実測）を本書で勝手に変えない。
2. **3層分離の原則（監査 §0）を全要件の上位原則とする。**
   1. **抽出**（fast_meta）: 担体から (source, keyword, label, body, detail) のレコードを作る。生成元の判定はしない。
   2. **分類**（sorter）: レコードの **キー＋値のパターン** でだけ判定する。キーの存在だけでは判定しない。
   3. **表示**（fast_meta CLI の TextSink）: レコードを人間向けに整形する。ここで生成元を推定して付け直さない。
   この原則に反する実装（例: 表示ラベル文字列で分類する、抽出層で本文を生成元の都合で合成する）は、個別要件に明記がある場合を除き禁止する。
3. **推測で形式仕様を作らない。** 値検証に使うキー集合・構文パターン（NovelAI 設定 JSON のキー集合、Draw Things JSON のキー集合等）は、Phase 0 で実サンプル／合成フィクスチャから確定し、§14 の未決事項の解消記録として本書へ追記してから該当フェーズに着手する。本書に無いツールの独自形式を新たに発明して対応しない。
4. **壊れた／悪意のある入力で panic しない。** sorter 側のファイル単位 `catch_unwind` は維持し、fast_meta 側も従来通り checked arithmetic と境界チェックを維持すること。
5. **健全として直さない箇所（§3.3）を退行させない。** 監査で健全と確認された設計は、本書の要件を理由に変更しない。
6. **意図した挙動変更は明記したものに限る。** §13.1 の「意図した挙動変更一覧」に無い行き先変更・終了コード変更・CLI 出力変更を混ぜない。fast_meta CLI のゴールデン出力は、本書の要件 ID に追跡できる変更（§9.2 の原則と §13.1 のカテゴリ）以外では変えない。ゴールデンの期待値を変える場合は、対応する要件 ID を §16 のレビューログではなく各フェーズの記録に明記すること。
7. **段階的に実装する。** 各フェーズは独立に `cargo build --release --locked` と `cargo test --locked` が通る状態で終える。コミット規約は §11.3 に従う。
8. **性能の負例コストを守る。** メタデータを持たない画像（多数派）に対する処理コストを増やさない。値検証は借用した本文への部分文字列走査・JSON パース等で行い、本文の全文コピーは作らない（既存仕様書 NFR-PERF-02／03 を継承）。上限や走査範囲に関わる要件（REQ-CLS-08 等）では Phase 0／最終フェーズでベンチマークを取り、悪化が支配的でないことを記録する。

---

## 1. Plan（計画概要）

### 1.1 背景

Photoshop / Celsys Studio Paint 等で保存しただけの非AI画像が `webui_img` へ大量に誤仕分けされていた。実測では 427枚中 341枚（80%）が誤検知で、全て PNG tEXt の `Software = "Celsys Studio Tool"` のみが根拠だった。

監査の結論は、誤検知は1箇所のバグではなく **「汎用キー／汎用担体の存在を、値の検証なしに生成元の証拠として扱う」** という同じ型が、抽出・分類・表示の複数段に埋まっている、というものだった。洗い出しは分類段（F-01〜F-12）に留まらず、レビューを重ねる中で収集・移動・集計段（F-15、F-20〜F-24、F-29、F-30、F-36、F-37）と抽出段の fail-stop／デコード非対称（F-10、F-13、F-18、F-19、F-25〜F-28、F-31〜F-35）まで広がり、最終形は F-01〜F-37 となった。

### 1.2 ゴール

- G1: 非AI画像（編集・変換ツールで保存されただけの画像）が `webui_img` 等へ誤仕分けされない。§10 の負例フィクスチャ全件が「仕分けされない（None）」になる。
- G2: 正例の捕捉を落とさない。Midjourney、Draw Things、AIPromptInformation 付き画像を含む §10 の正例フィクスチャ全件が従来どおり（または §13.1 で意図したとおり）の行き先になる。ただし Midjourney の XMP のみ画像（P-18）は Phase 0 の U-7 で扱いを確定する対象であり、U-7 の結論が出るまでは G2 の判定から除外する。
- G3: 存在シグナル（キーの存在、担体の有無）を分類の確定根拠から全廃し、全シグナルを「キー＋値パターン」化する。優先順位を検証強度順に組み替える。
- G4: 抽出段の fail-stop を、構造上可能な範囲で fail-soft 化し、「壊れた1単位が後ろの本物のメタデータを落とす」経路を塞ぐ。デコード方式の担体間非対称を揃える。
- G5: 収集・移動・集計段の結果を利用者が区別できるようにする。「メタデータ無しでスキップ」「分類されたが dest 衝突で残留」「仕分け済みだが元削除に失敗」「入力が存在しない」等が集計と終了コードで区別できる。
- G6: 分類条件を表示ラベルから切り離し、構造化された印で sorter へ渡す。表示層は生成元を付け直さない。

### 1.3 成功の定義

- §12 の受け入れチェックリストが全て満たされる。
- F-01〜F-37 の全件について、§4〜§7 のいずれかの要件、または判断固定として §3.2（スコープ外）／§7.2（C2PA）／§7.3（16MB 上限）／§13.1（F-04 の見出し行等）のいずれかに処置が対応付いている（対応表 §2.3）。
- §10 の負例・正例フィクスチャが両リポジトリのテストとして固定され、全て通る。条件付きのフィクスチャ（P-18 は U-7 の結論、P-09／P-10／P-16 は U-8 の結論に依存する）は、各未決事項の確定内容に沿った期待値で通ることをもって満たす。

---

## 2. 現行実装の要点と対応表

### 2.1 現行の決定手順（監査 §9 の要約。改修対象の現状）

```
1. keywords.comment                                    → NovelAI   ★検証なしのキーが最優先（F-01）
2. sui 系キー / parameters本文の sui / Model本文の sui   → SwarmUI
3. invokeai_metadata / sd-metadata / Dream             → InvokeAI  ★Dream は検証なし（F-01 と同型）
4. use_stable_diffusion_model                          → WebUI
5. workflow / generation_data / (prompt かつ JSON風)
   / UserComment の "workflow" / stealth の workflow   → ComfyUI
5b. Software本文に "NovelAI" / UserComment の NovelAIパック
   / stealth の novelai / trailing の novelai          → NovelAI   ★Software の値はここでのみ検証される
6. parameters / Steps: 部分一致 / JPEG COM ラベル
   / has_software (値不問) / has_stealth / has_xmp (有無のみ)
   / trailing_webui                                    → WebUI     ★監査で実害が確定した2件（Software 値不問、XMP 有無のみ＝F-02）はここ。F-01 は手順1の Comment の話で、ここではない
7. どれにも当たらない                                  → スキップ
```

構造上の問題（監査 §9）: (a) 優先順位と検証強度が逆転している（最も弱い `Comment` が最上位）。(b) 手順6は強弱の違うシグナルの OR で、弱いシグナルが単独で確定させる。

### 2.2 新しいシグナルの強さの定義（本書全体で使う）

| 強さ | 定義 | 単独での扱い |
|---|---|---|
| 強 | キーが固有、または値が構造検証（JSON パース成功＋固有キー集合との一致、固有文字列の完全なパターン等）を通過したもの | 単独で行き先を確定してよい |
| 中 | 値は見ているが部分一致1個、または固有性の高いフィールドの存在など、誤爆余地が小さいがゼロでないもの | §8.3 手順6に列挙したものは単独で確定してよい。中シグナルは弱の複合の票には数えない（二重計数防止） |
| 弱 | 汎用担体への部分一致1個（`Steps: `、trailing 部分一致、Stealth 本文の部分一致等） | **単独では確定させない。** 原則として、同じ方向の異なる種類の弱シグナル2つ以上の一致でのみ確定する（§8.3 手順 W）。例外: ComfyUI 方向の弱は、§8.3 の注記の理由により複合でも確定させず記録のみとする |
| 無効 | 汎用キー／汎用担体の存在そのもの（Software の存在、XMP の存在、Description 単独等）、および非AIツール名の値 | シグナルとして数えない。強いシグナルへの拒否権（veto）にもしない（AI画像を Photoshop で再保存しても強いシグナルは生きる） |

### 2.3 F 番号 → 要件 対応表（全件カバー表）

処置の種別: **修正** = 挙動を変える／**構造化** = 受け渡し形式を変える／**固定** = テストで現行の望ましい挙動または判断を固定する／**判断** = 本書で「実装しない」と決定し理由を記録する。

| F | 重大度（監査） | 処置 | 主な要件 ID | 主対象 | 検証の主担当（§10） |
|---|---|---|---|---|---|
| F-01 | 高 | 修正 | REQ-CLS-01、REQ-CLS-12（Dream） | sorter（＋fast_meta のラベルは表示として維持可、分類根拠にしない） | 負例 N-07、N-12、正例 P-04、P-07、P-17、P-20、P-21 |
| （確定2件の片方）Software 値不問 | 高 | 修正 | REQ-CLS-11 | sorter（`has_software` 廃止） | 負例 N-01、N-02、N-03、正例 P-11 |
| F-02 | 高 | 修正 | REQ-CLS-02 | sorter（XMP フィールド別シグナル化） | 負例 N-01／N-03／N-05／N-06、正例 P-11／P-12／P-13／P-18／P-19、単体 T-41 |
| F-03 | 中 | 修正 | REQ-DSP-01 | fast_meta（表示層） | ゴールデン G-01、負例 N-05 |
| F-04 | 中 | 部分修正＋判断 | REQ-DSP-02 | fast_meta（CLI 経路） | ゴールデン G-02 |
| F-05 | 中 | 修正 | REQ-CLS-03 | sorter | 負例 N-08、N-13、N-14、正例 P-02、P-22 |
| F-06 | 中 | 構造化 | REQ-API-01、REQ-CLS-04 | fast_meta＋sorter | 負例 N-11、正例 P-14、単体 T-06 |
| F-07 | 中 | 修正 | REQ-CLS-05 | sorter | 負例 N-09、正例 P-09／P-10、P-23 |
| F-08 | 中 | 修正 | REQ-CLS-06 | sorter | 負例 N-10、正例 P-15、P-15b、P-22、P-23 |
| F-09 | 中 | 修正 | REQ-EXT-01 | fast_meta（Stealth Auto ゲート） | 正例 P-16、単体 T-40 |
| F-10 | 中 | 修正（段別） | REQ-EXT-02〜REQ-EXT-05 | fast_meta | 単体 T-10〜T-13、T-37、T-39 |
| F-11 | 低 | 固定 | REQ-INV-01 | 両方（棚卸し表） | 棚卸し表のテスト固定 T-14 |
| F-12 | 低 | 判断 | REQ-INV-02 | fast_meta | 現状固定 T-15 |
| F-13 | 低 | 修正 | REQ-EXT-06 | fast_meta（PNG テキストチャンク） | 単体 T-16、正例 P-13 変種 |
| F-14 | 低 | 修正 | REQ-EXT-07、REQ-CLS-07 | fast_meta＋sorter | 正例 P-12、単体 T-17 |
| F-15 | 中 | 修正 | REQ-COL-01 | sorter（収集） | 統合 T-20 |
| F-16 | 低 | 修正（Software 側）＋判断固定（64KB 側） | REQ-CLS-08 | sorter | 単体 T-07、T-36、正例 P-04 変種 |
| F-17 | 低 | 修正 | REQ-CLS-09 | sorter | 単体 T-08 |
| F-18 | 低 | 修正 | REQ-EXT-08 | fast_meta（WebP） | 単体 T-18 |
| F-19 | 低 | 修正 | REQ-EXT-09 | fast_meta（TIFF） | 単体 T-19 |
| F-20 | 中 | 修正 | REQ-COL-02 | sorter（収集・並列） | 統合 T-21 |
| F-21 | 低 | 修正 | REQ-TRN-01 | sorter（移動・集計） | 統合 T-22 |
| F-22 | 低 | 修正 | REQ-TRN-02 | sorter（移動・集計） | 統合 T-23 |
| F-23 | 低 | 修正 | REQ-COL-03 | sorter（入力検証） | 統合 T-24 |
| F-24 | 低 | 修正 | REQ-COL-04 | sorter（移行の範囲） | 統合 T-25 |
| F-25 | 低 | 修正 | REQ-EXT-03 | fast_meta（PNG Stage C） | 単体 T-11 |
| F-26 | 低 | 修正 | REQ-EXT-10 | fast_meta（TIFF） | 単体 T-26 |
| F-27 | 低 | 修正 | REQ-EXT-04 | fast_meta（JPEG 拡張 XMP） | 単体 T-12 |
| F-28 | 低 | 修正 | REQ-EXT-05 | fast_meta（TIFF IFD 追跡） | 単体 T-13、T-42 |
| F-29 | 低 | 修正 | REQ-COL-05 | sorter（直接ファイル経路） | 統合 T-27 |
| F-30 | 低 | 修正 | REQ-COL-06 | sorter（CLI 引数検証） | 統合 T-28 |
| F-31 | 低 | 修正 | REQ-EXT-11 | fast_meta（XMP） | 単体 T-29 |
| F-32 | 低 | 判断＋固定 | REQ-EXT-12 | fast_meta | 現状固定 T-30 |
| F-33 | 低 | 修正 | REQ-EXT-13 | fast_meta（PNG iTXt） | 単体 T-31 |
| F-34 | 低 | 修正 | REQ-EXT-14、REQ-CLS-09 | fast_meta＋sorter | 単体 T-32、正例 P-03 変種 |
| F-35 | 低 | 修正 | REQ-EXT-15 | fast_meta（XMP） | 単体 T-33 |
| F-36 | 低 | 修正 | REQ-TRN-03 | sorter（移動・集計） | 統合 T-34 |
| F-37 | 低 | 修正 | REQ-COL-07 | sorter（収集） | 統合 T-35 |

監査本文に F 番号なしで「確定」とされた付随事項も扱う: (a) WebP EXIF チャンク先頭の `Exif\0\0` プレフィクスで TIFF パーサがレコード0件になる件（監査 §4）は **REQ-EXT-16** として修正対象に含める（検証は T-38）。(b) 監査 §0 で「確定済みの同型は2件」とされたうち Software 値不問の件は、§10 の F 一覧に独立番号を持たないため、本書では対応表の先頭に「（確定2件の片方）Software 値不問」として独立行を立て、REQ-CLS-11 を割り当てる。F 番号の新設ではない。

§10.3 のテスト番号について: T-01〜T-05 と T-09 は本書では使わない欠番とする（既存仕様書 §10.2 のテスト観点と番号が衝突しないよう予約したもので、欠落ではない）。

---

## 3. スコープ

### 3.1 スコープ内

- sorter の分類シグナル再設計（§4、§8）と集計・収集・移動まわりの是正（§6）
- fast_meta の構造化 API 追加（JPEG COM の印、Stealth ゲートの定義変更、XMP の tEXt/zTXt 委譲、Draw Things の本文合成廃止）（§5）
- fast_meta の fail-soft 化とデコード非対称の是正（§5）
- fast_meta CLI 表示層の是正（再ラベル廃止、診断文の stderr 移行）（§5）
- §10 の負例・正例フィクスチャの整備とテスト固定（両リポジトリ）

### 3.2 スコープ外（実装しない）

- C2PA の実装（F-12。§7.2 の判断を参照）
- 分類に使われていない担体（EXIF Make／Artist／XPComment、IPTC Caption、PNG Title／Source、XMP title）への新規分類対応（F-11。棚卸しまでが本書の範囲）
- 16MB ハード上限そのものの引き上げ（F-32。§7.3 の判断を参照）
- 新しい生成ツール専用フォルダの新設（行き先は従来の5分類のまま。XMP 由来の AI 宣言も行き先は `webui_img` のキャッチオール）
- fast_meta CLI の本文ハッシュによるレコード統合表示（同一本文のレコードを `(also in: …)` として統合する処理。監査 §7 項目3）の変更。監査自身が「軽微」とし F 番号も無いこと、ライブラリ経由の分類には影響しないことから、本書では **扱わない** と明記する。表示上の帰属の混ざりが実害になった場合は別件とする
- sorter の走査・移動の性能最適化（jwalk 化、2フェーズ化等。既存仕様書の Phase 5（P2）の領域であり本書に混ぜない）
- メタデータの書き込み・削除機能、GUI 連携

### 3.3 健全として直さない箇所（監査 §10 末尾の健全リスト）

以下は監査で健全と確認済みであり、本書の改修で変更しない。変更が必要になった場合は本書ではなく別件として扱う。

| # | 箇所 | 内容 |
|---|---|---|
| H-1 | fast_meta PNG キー照合 | 登録テーブルとの完全一致（大文字小文字区別あり）。部分一致にしない |
| H-2 | fast_meta XMP フィールド照合 | 名前空間＋ローカル名による厳密照合。汎用属性（`stEvt:parameters` 等）は拾わない。履歴（xmpMM:History）は抽出対象外のまま |
| H-3 | fast_meta JPEG 歩行 | SOS 以降（走査データ）を読まない |
| H-4 | sorter Trailing の隔離 | Trailing を通常キーと混ぜず専用シグナルに隔離する設計そのもの（シグナルの中身の見直しは REQ-CLS-06 で行うが、隔離は維持する） |
| H-5 | sorter Software 本文の NovelAI 判定 | 値を見て判定する形そのもの（他シグナルもこの形に寄せるのが改修の基本線）。上限による不発動の問題は REQ-CLS-08 で別途直す |
| H-6 | sorter FR-CLS-01（既存） | PNG `Description` 単独で NovelAI にしない設計 |
| H-7 | sorter Model 本文の `sui_image_params`／UserComment の `"Comment":`＋`"Description":` ペア | 固有文字列・複合条件で誤爆が起きにくい形。強シグナルとしてそのまま採用する |

---

## 4. 要件 — 分類（sorter）

### 4.1 中核の置き換え（REQ-CLS）

| ID | 優先度 | 対応 F | 要件 |
|---|---|---|---|
| REQ-CLS-01 | P0 | F-01 | PNG `Comment` キーの存在だけで NovelAI を確定させないこと。NovelAI 確定には値検証を必須とする: 本文が JSON としてパースでき、かつ Phase 0 で確定する NovelAI 設定らしいキー集合（候補: `steps` / `sampler` / `scale` / `seed` / `uc` 等から実サンプルで確定する）のうち **2つ以上** をトップレベルに含むこと。検証を通らない `Comment` は無効シグナルとして数えない（弱シグナルにもしない）。あわせて検証済み `Comment` の優先順位を現行の手順1（最上位）から §8.3 手順5の NovelAI 強グループへ移す（最上位から外す。監査 §11-2 の「少なくとも Comment 単独を最上位から外し」への対応はこの順位変更と値検証の両方で行う） |
| REQ-CLS-11 | P0 | 確定2件の片方（Software 値不問） | `has_software`（Software レコードの存在だけで WebUI 確定）を **廃止** すること。Software は本文が REQ-CLS-10 の AI生成ツール名の表に一致する場合にのみシグナル化する（NovelAI は NovelAI 方向の強、それ以外は WebUI 方向の中。§8.1）。今回の誤検知の発端そのものであり、Phase 1 の中核コミットとして独立させること |
| REQ-CLS-02 | P0 | F-02 | `has_xmp`（XMP レコードの有無のみ）を廃止し、XMP レコードを **keyword 別** に分解して保持すること。最低限、次の別々のシグナルにすること: (a) `AIPromptInformation` が空でない値を持つ → 強（WebUI）、(b) `DigitalSourceType` の値が `trainedAlgorithmicMedia` またはそれを含む複合値 → 強（WebUI）、(c) `CreatorTool` の値が AI生成ツール名パターン（§8.2 のツール名表）に一致 → 中（WebUI）、(d) `UserComment` が Draw Things 検証（REQ-CLS-07）を通過 → 強（WebUI）。ただし (d) は前提（REQ-EXT-07 の本文合成廃止と U-2 の確定）が Phase 2 にあるため、**Phase 1 では未発動のまま置き、Phase 2 で有効化する**（COM の label 暫定と同じ扱いを明示する）、(e) `description` のみ → 原則として無効（単独では何も確定しない）。ただし (f) Midjourney の XMP のみ画像の捕捉条件は Phase 0 で確定する（未決事項 U-7）。確定するまでは (e) の例外を作らない。P-18 の扱いは §11.1 Phase 1 の完了条件のとおりとする（条件が見つかれば本フェーズで実装して含める、見つからなければ期待値 None として含める）。XMP の4経路（PNG iTXt／JPEG APP1／WebP／TIFF XMLPacket）のいずれから来ても同じ扱いにすること |
| REQ-CLS-03 | P1 | F-05 | `UserComment`／`ImageDescription` 本文の A1111 形式判定を次の段階付けにすること: 本文に `Negative prompt:` と `Steps: ` の **両方** を含む → 中シグナル `a1111_text_block`（WebUI。A1111 の JPEG が実際にこの形で parameters を書くための捕捉であり、単独で確定してよい）、`Steps: ` のみ／`Negative prompt:` のみ → 弱シグナル。また `UserComment` 本文への `"workflow"` 部分一致は弱シグナルに格下げする。UserComment の NovelAI パック（`"Comment":`＋`"Description":` の両方、H-7）は強シグナルのまま維持する |
| REQ-CLS-04 | P1 | F-06 | JPEG COM の A1111 判定をラベル文字列から切り離すこと。sorter は REQ-API-01 で fast_meta が渡す構造化された印（source = `JpegComment` かつ detail が A1111 形式を示す値）でのみ判定し、`label` を分類条件に使わない。印の強さは中とし、本文マーカーが `Negative prompt:` と `Steps:` の両方を含む場合に限り中、片方のみの場合は弱とする（印への切り替え時に fast_meta 側で区別して渡す。§5 REQ-API-01） |
| REQ-CLS-05 | P1 | F-07 | Stealth 本文への部分一致（`workflow`／`novelai`）で ComfyUI／NovelAI を確定させないこと。Stealth 本文が JSON としてパースできる場合はトップレベルの構造で判定するが、**どの構造が NovelAI／ComfyUI なのかのキー集合は本書では確定せず、未決事項 U-8 として Phase 0 で実サンプルから確定する**。それまでは構造判定を実装しない。部分一致の弱シグナルは、パースの成否を問わず「構造判定でどちらの集合にも一致しなかった場合」に方向別（NovelAI 方向／ComfyUI 方向）で立てる（構造に一致した場合はそちらが優先し、部分一致の弱は立てない）。Stealth レコードの存在そのものは WebUI 方向の弱シグナルとする。同一の Stealth レコードは、方向ごとに最大1票までしか数えない（存在＋部分一致で2票にしない）。単独では確定させない |
| REQ-CLS-06 | P1 | F-08 | trailing_webui の発動条件から `Software` 部分一致を外すこと。段階付けは次のとおりとする: Trailing は構造のないバイト列検索で拾うため本文の素性が最も弱く（監査 F-08）、本文ブロック（UserComment 等）と同格の中シグナルにはしない。`parameters` のキー形状の出現、`Steps: ` と `Negative prompt:` の両方の併存、いずれか片方のみ、その全てを **弱シグナル `trailing_weak`（WebUI 方向）** とし、種類は分けても複合の票は Trailing 全体で1票までとする。trailing_novelai（`novelai` 部分一致）も弱シグナル（NovelAI 方向）に格下げし、単独では NovelAI を確定させない（従来は手順5bで単独確定していた。§13.1 の意図した変更）。Trailing の隔離（H-4）は維持する |
| REQ-CLS-07 | P1 | F-14 | Draw Things の判定を sorter 側でも値検証すること。fast_meta が REQ-EXT-07 で本文合成をやめた後は、XMP `UserComment` の生 JSON をパースし、Phase 0 で確定する Draw Things 固有キー集合（`"c"` 単独は不可。候補: `c` に加え生成パラメータ系キーの併存を実サンプルで確定する）を満たす場合にのみ強シグナル（WebUI）とする。満たさない UserComment JSON は通常の UserComment として扱う |
| REQ-CLS-08 | P2 | F-16 | 値検証シグナルが上限で打ち切られ、存在シグナルだけが残る非対称について、半分は修正し、半分は判断固定とする: (a) **修正（Software 側）**: Software 本文の AIツール名照合は、本文を複製せず借用した本文に対して直接行い、256バイトの複製上限による不発動を無くす（複製上限は「保持」の上限として残してよいが、判定は保持の有無に依存させない）。(b) **判断固定（UserComment／ImageDescription 側）**: 部分一致の走査範囲は先頭64KB のままとし、範囲外の `Steps: `／`"workflow"` 等は検出しないことを仕様上の制限として明記し、T-36 で現状固定する。存在シグナル側（has 系）は本書で全廃されるため、この制限が残っても「存在だけ残って誤判定する」非対称にはならない。走査範囲の拡大は §0-8 のベンチマーク条件を満たす場合にのみ別途判断する |
| REQ-CLS-09 | P2 | F-17、F-34 | 複合条件は **同一レコード内** で成立させること。(a) `prompt` の JSON 判定は、prompt レコードごとに「そのレコードの本文が JSON か」を保持し、別レコードの結果と合成しない。(b) prompt 本文先頭の UTF-8 BOM（U+FEFF）は JSON 判定の前に取り除く（fast_meta 側の REQ-EXT-14 と二重に防御する） |
| REQ-CLS-12 | P0 | F-01（同型） | InvokeAI の `Dream` キーの存在だけで InvokeAI を確定させないこと。値検証を必須とする: 本文が Phase 0 で確定する Dream 旧式構文のパターン（未決事項 U-3。引用符で囲んだプロンプト＋フラグ列の形を、参照実装の形式定義と実サンプルで固定する）を満たす場合にのみ強シグナル（InvokeAI）とし、満たさない `Dream` は無効シグナルとして数えない（弱シグナルにもしない）。REQ-CLS-01 の `Comment` 検証と同型の処置であり、Phase 1 で同時に実装すること（P-07／P-17 で固定する） |

### 4.2 ツール名パターン（REQ-CLS-02／REQ-CLS-08 の共通部品）

| ID | 優先度 | 要件 |
|---|---|---|
| REQ-CLS-10 | P0 | Software 本文・XMP `CreatorTool` の値照合に使う **AI生成ツール名の表** を sorter 側に1箇所だけ定義すること。初期値は監査 §11-1 の列挙どおり: `Midjourney` / `NovelAI` / `Stable Diffusion` / `AUTOMATIC1111` / `Forge` / `InvokeAI` / `SwarmUI` / `Draw Things`。監査の列挙に無い名前（例: `ComfyUI`）は初期値に入れない。照合は ASCII 大文字小文字を区別しない部分一致とする。非AIツール名（Adobe／Photoshop／Celsys／ImageReady／GIMP 等）の扱いは §13.1 の読み替え判断のとおり: 表に **入れない** ことでシグナル化しないものとし、「非AI名があれば打ち消す」veto ロジックは作らない（§2.2 の無効の定義どおり）。表への追加は、実サンプルでそのツールが当該フィールドに自名を書くことを確認した場合に限る |

---

## 5. 要件 — 抽出・API・表示（fast_meta）

### 5.1 構造化 API と抽出層の是正（REQ-API／REQ-EXT）

| ID | 優先度 | 対応 F | 要件 |
|---|---|---|---|
| REQ-API-01 | P1 | F-06 | JPEG COM レコードに、A1111 形式であることを示す構造化された印を `detail` で渡すこと。値は最低限「両マーカーあり（`Negative prompt:` と `Steps:` の両方を含む）」と「片方のみ」を区別できること。`label` は表示専用のまま変更してよいが、sorter が label に依存しないことが REQ-CLS-04 のテストで固定される。既存の `RecordView` の形（フィールド追加なし、detail の値の新規定義）で収めること |
| REQ-EXT-01 | P1 | F-09 | Stealth PNG Info の Auto ゲートを「通常レコードが1件でもあれば検査しない」から **「実質レコードが1件も無いときだけ検査する」** に変えること。実質レコードは「本文に生成内容／生成パラメータを担うレコード」と定義し、値ではなくレコード種別で決める。全種別について帰属を次のとおり固定する（この表に無い種別が将来増えた場合は、内容を担う側＝閉じる、に倒して本書へ追記する）: **ゲートを閉じる（実質レコード）**: keyword が `parameters`／`prompt`／`workflow`／`generation_data`／`Comment`／`Description`／`sui_image_params`／`sui_extra_data`／`invokeai_metadata`／`sd-metadata`／`Dream`／`use_stable_diffusion_model` の PNG テキスト、EXIF・XMP の `UserComment`、EXIF の `ImageDescription`、JPEG COM、XMP の `description`／`AIPromptInformation`／`DigitalSourceType`。**ゲートを閉じない（汎用レコードのみでは検査を続ける）**: keyword が `Software`／`Title`／`Source` のテキスト、EXIF の `Software`／`Make`／`Model`／`Artist`／`XPComment`、XMP の `CreatorTool`／`title`、IPTC `Caption/Abstract`、source が Trailing のレコード。根拠: `Comment`／`Description` はキー名の汎用性こそ高いが、生成物の本文（設定 JSON／プロンプト文）そのものを担うレコードであり、書き込みツール名や出所のような「付随情報」とは種別が違うため実質側に入れる。この定義は抽出層の「検査要否」であって生成元の判定ではない（§0-2 の例外として明記する）。sorter が使う Auto の意味が変わるため、fast_meta CLI の `--stealth auto` の説明文言も追随させる |
| REQ-EXT-06 | P2 | F-13 | XMP パケット（keyword が xmp_packet）への委譲を、iTXt だけでなく tEXt／zTXt 経路でも行うこと。チャンク種別で委譲先が変わらないようにする |
| REQ-EXT-07 | P1 | F-14 | Draw Things 形式の検出で **抽出層が本文を合成・置換しない** こと。現行の「`"c"` キーだけで発動し、`Steps: ` 行等を含む合成テキストに置換する」処理を廃止し、生の JSON 本文をそのままレコード化する。あわせて同パケットの dc:description を抑制する副作用も廃止する。表示上の整形が必要な場合は TextSink 側（表示層）でのみ行い、ライブラリのレコード本文には影響させない。Draw Things かどうかの判定は REQ-CLS-07 のとおり sorter 側の値検証に移す |
| REQ-EXT-16 | P2 | 監査 §4（F 番号なし） | WebP の EXIF チャンク先頭に `Exif\0\0` プレフィクスがある場合に、それを読み飛ばして TIFF パーサへ渡すこと。プレフィクスなしの従来経路の挙動は変えない |

### 5.2 fail-soft 化とデコード対称化（REQ-EXT 続き）

| ID | 優先度 | 対応 F | 要件 |
|---|---|---|---|
| REQ-EXT-02 | P1 | F-10（TIFF） | TIFF パーサは、エントリ1件が読めない場合に IFD 全体を放棄せず、そのエントリだけを飛ばして残りのエントリの解析を続けること |
| REQ-EXT-03 | P2 | F-10（PNG）／F-25 | (a) PNG Stage A が壊れたチャンクに当たった場合、その時点で得た部分レコードだけで打ち切らず、Stage B／C の回収を試みること。(b) Stage C の最終段（step 4）で IEND 到達や CRC による接続証明なしに得たレコードは、通常レコードとして分類に効かせない。隔離の方法は Trailing 相当（source を分ける、または detail で「未証明」を示し sorter 側で弱シグナル相当に落とす）のいずれかとし、未決事項 U-4 として **Phase 3 着手前** に現行フィクスチャへの影響を確認して決める |
| REQ-EXT-04 | P2 | F-10（XMP）／F-27 | (a) XMP の候補選択（first-wins）を、**全フィールド共通** に次のとおり変えること: フィールドごとに候補を選ぶ際、`rdf:about=""`（主対象）を持つ rdf:Description 要素由来の候補を優先し、主対象の候補が無いフィールドに限り従来どおり先勝ちとすること。(b) JPEG 拡張 XMP の再結合は GUID ごとに組を分け、完全な組があればそれを採用し、GUID／total_length の異なる断片の混在だけで全体を捨てないこと |
| REQ-EXT-05 | P2 | F-10（JPEG／WebP の扱いを含む）／F-28 | (a) TIFF の次IFD 追跡を、ループ防止の上限（深さ4程度）を付けた上で IFD1 以降にも広げ、IFD1 の ExifIFD も IFD0 と同じ条件で辿ること。(b) JPEG セグメント長不正・WebP チャンクサイズ不正のように **構造上再同期できない** 破損については、歩行停止そのものは現状維持とする。これは健全リストの宣言ではなく、長さが壊れている以上は次の境界が分からず、無理に読み進めると画素データ中の偶然の一致を拾う危険があるための、本書としての判断である（F-10 の fail-stop のうち、この2経路だけを判断固定とする）。ただし停止までに得たレコードは捨てずに返すこと（T-39 で固定する） |
| REQ-EXT-08 | P2 | F-18 | WebP 歩行は RIFF サイズを読み、RIFF ペイロードの範囲内でのみチャンクを歩行すること。範囲外に `EXIF`／`XMP ` 形状のバイト列があってもレコード化しない |
| REQ-EXT-09 | P2 | F-19 | BOM なし UNICODE UserComment のエンディアン決定は、EXIF 規格どおり **BE を既定** とし、ヒューリスティクス（utf16_score）による LE 採用は「BE で読むと明確に破綻し、LE で読むと明確に成立する」場合に限ること。ASCII 中心の本文・非ASCII中心の本文の両方のフィクスチャで固定する |
| REQ-EXT-10 | P2 | F-26 | TIFF の ASCII 系タグ（Software／ImageDescription 等）のデコードに、PNG テキストと同等の Latin-1 フォールバックを追加し、UTF-8 でないことを理由にレコードごと落とさないこと。少なくとも ASCII 部分（`Steps: `、`NovelAI` 等の照合対象）は照合可能な形で残ること |
| REQ-EXT-11 | P2 | F-31 | XMP パーサはパケット単位の all-or-nothing をやめ、局所破損（終端の無いコメント／CDATA／タグ等）がある場合でも、破損箇所より前（および再同期可能な範囲）の正常なフィールドをレコード化すること。再同期が不可能な範囲は捨ててよいが、パケット全体を0件にしない |
| REQ-EXT-13 | P2 | F-33 | iTXt のテキストデコードを tEXt／zTXt と同じ経路に揃え、非UTF-8テキストでも Latin-1 フォールバックでレコード化すること（キーワードごと消失させない） |
| REQ-EXT-14 | P2 | F-34 | fast_meta のテキストデコードで先頭の UTF-8 BOM を取り除くこと（PNG テキストレコードの本文先頭に U+FEFF を残さない） |
| REQ-EXT-15 | P2 | F-35 | XMP パケットが UTF-16（BOM 付き BE／LE）でシリアライズされている場合も読み取り、UTF-8 の場合と同じフィールドをレコード化すること。UTF-32 は対象外とする |

### 5.3 表示層（REQ-DSP）

| ID | 優先度 | 対応 F | 要件 |
|---|---|---|---|
| REQ-DSP-01 | P1 | F-03 | TextSink の「XMP Description 単独存在 → `Midjourney (XMP)` への再ラベル」を廃止し、ラベルは常に `XMP Description` とすること。この変更は §13.1 の「意図した変更: fast_meta CLI」の列挙の1項目として記録する（ゴールデン変更はこの変更だけではない。列挙全体は §9.2／§13.1 を参照） |
| REQ-DSP-02 | P1 | F-04 | CLI の診断文（`(Large file or ComfyUI detected. Scanning tail for metadata...)` 等、レコードでも見出しでもない案内文言）を stdout から stderr へ移すこと。**F-04 への処置はこれで全部ではない（部分修正）**: ファイル見出し行（`=== PNG File: <フルパス> ===`）は出力形式の一部として stdout に **意図して残す**（§13.1 の判断）。パス文字列への部分一致で誤判定する消費者は残るが、stdout を分類入力にすること自体が旧構成の誤りであり、正規の消費経路はライブラリである、というのが残す理由である。診断文については誤判定の余地を無くす |

---

## 6. 要件 — 収集・移動・集計（sorter）

| ID | 優先度 | 対応 F | 要件 |
|---|---|---|---|
| REQ-COL-01 | P1 | F-15 | 再帰収集で見つけたシンボリックリンク／ジャンクション経由の画像ファイルとフォルダを対象に含めること。ただしフォルダリンクの循環を防ぐため、実パス（canonicalize 結果）の訪問済み集合を持ち、同じ実フォルダを二度走査しない。リンクを辿った件数と、循環等で素通りした件数を集計に別行で出すこと（無言で落とさない）。直接引数で渡したパスの扱いは現行どおり対象とする |
| REQ-COL-02 | P1 | F-20 | 入力収集の結果を実パスで重複排除してから並列処理に入ること。フォルダとその配下ファイルの同時指定等で同じファイルが2回処理され、結果が実行ごとに変わる非決定性を無くす。重複として除いた件数を集計に別行で出すこと |
| REQ-COL-03 | P2 | F-23 | 存在しない入力パスを無言で無視しないこと。存在しないパスは1件ごとに警告として集計（入力エラー件数）し、他の入力の処理は継続した上で、終了コードは1とすること（現行は exit 0。§13.1 の意図した変更） |
| REQ-COL-04 | P2 | F-24 | 単一ファイル指定時の旧フォルダ移行の範囲を、そのファイルの親フォルダ自身が旧フォルダ（`webui_image`）である場合、または親フォルダ直下の旧フォルダに限定し、親のサブツリー全体の走査・移行を行わないこと。単一ファイル指定では、移行の失敗が指定ファイルの仕分け自体を中止させないこと（警告として集計し、仕分けは継続する）。フォルダ指定時の移行範囲と挙動は現行どおりとする |
| REQ-COL-05 | P2 | F-29 | ファイルを直接渡す経路にも、フォルダ走査と同じ拡張子フィルタと出力フォルダ除外を適用すること。出力フォルダ内にあるファイルを直接渡した場合は再移動せず、除外として集計すること（入れ子先 `comfyui_img/comfyui_img/…` を作らない） |
| REQ-COL-06 | P2 | F-30 | 分類先フォルダ名（5分類分の引数）を処理開始前に検証すること: 空文字、`.`／`..`、パス区切り文字を含むもの、絶対パス、5分類の間での重複（大文字小文字を区別しない一致も含む）をエラーとし、1ファイルも移動せずに使い方エラーとして **終了コード 2**（CLI 引数エラーの慣例値）で終了する。これは処理結果に対する終了コード（0／1）とは別の、開始前エラーの値であり、§13.1 に意図した変更として記録する。既定値では当然通ること |
| REQ-COL-07 | P2 | F-37 | 収集段の読み取りエラー（読めないサブディレクトリ、走査中の削除、file_type 取得失敗等）は、バッチ全体を止めず、当該エントリを収集エラーとして集計して走査を継続すること。収集エラーが1件でもあれば終了コードは1とする（ファイル単位の処理エラーと集計の仕方を揃える） |
| REQ-TRN-01 | P2 | F-21 | 仕分け先に同名ファイルがあるための残留を、「メタデータ無しでスキップ」と区別して集計すること。結果の種別を少なくとも {仕分け済み, スキップ（メタデータ無し／対象外）, スキップ（dest 同名衝突）, エラー} に分け、集計表示に dest 衝突の件数を別行で出すこと |
| REQ-TRN-02 | P2 | F-22 | dest への転送完了後に元ファイルの削除が失敗した場合は、「仕分け済みだが元が残った」ことを警告として別行集計し、通常のエラー（仕分け自体の失敗）と区別すること。ファイルが元と dest の両方に残ること自体は安全側であり維持する。終了コードは未完了事項があるため従来どおり1とする |
| REQ-TRN-03 | P2 | F-36 | 出力フォルダ名と同名の通常ファイルが親フォルダ内にあって出力フォルダを作成できない場合は、その分類先への移動を「dest 作成不可」として別行集計し、通常の処理エラーと区別すること。**件数は影響を受けたファイルの数** で数える（合計が総数と一致する性質を保つため）。繰り返しを抑えるのは表示・ログの側であり、同じ分類先についての失敗メッセージは分類先ごとに1回までとするが、集計件数はファイルごとに数える |

結果種別の集計行を増やす変更は、集計表示の文言追加として §13.1 に記録する。合計が総数と一致する性質（既存の FR-INT-09 相当）は新しい種別でも維持すること。ここでいう合計とは、入力ファイル総数 ＝ {仕分け済み, スキップ（メタデータ無し／対象外）, スキップ（dest 同名衝突）, dest 作成不可, エラー} の5種の和、を指す。警告（元削除失敗）は「仕分け済み」の内訳表示であり、和の項には入れない。重複除去・リンク素通り・入力エラー・収集エラーはファイル単位の処理に入る前の件数であり、合計の項には入れず別枠で表示する。

---

## 7. 要件 — 棚卸しと判断固定（REQ-INV／REQ-EXT-12）

### 7.1 死んだ担体の棚卸し（F-11）

| ID | 優先度 | 対応 F | 要件 |
|---|---|---|---|
| REQ-INV-01 | P2 | F-11 | 分類に使われていない担体の一覧表を sorter 側のコード内コメントまたは doc として固定すること。対象: EXIF `Make`／`Artist`／`XPComment`、IPTC `Caption/Abstract`、XMP `title`、PNG `Title`／`Source`、および REQ-CLS-02 で分解した XMP フィールドのうち分類に使わないもの。各担体について「現行は分類に使わない」ことと、その理由（汎用すぎる／書き手が確認できない等）を1行で記す。これらを将来分類に使う場合は、本書の §2.2 の強さ定義に従って値検証を設計してからにすること（棚卸し表にその旨を明記する） |

### 7.2 C2PA（F-12）

| ID | 優先度 | 対応 F | 要件 |
|---|---|---|---|
| REQ-INV-02 | P2 | F-12 | 本書では C2PA を **実装しない** と決定する。`Source::C2pa` は enum の予約に留まることを README または doc に明記し、対応済みと誤認される表現を置かない。C2PA の生成宣言（trainedAlgorithmicMedia 等）だけを持つ画像が現行はどの分類にも入らないことは、既知の制限としてテストで現状固定する。実装の要否は利用実態が出た時点で別件とする |

### 7.3 16MB ハード上限（F-32）

| ID | 優先度 | 対応 F | 要件 |
|---|---|---|---|
| REQ-EXT-12 | P2 | F-32 | EXIF／XMP パケットの16MB上限は **資源保護として維持** し、上限超過でその担体が0件になる現行挙動を仕様として明記した上でテストで固定すること。上限の引き上げや「切り捨てて部分的に読む」方式は本書では採用しない（切り捨てた XMP／TIFF は構造検証ができず、F-25 と同型の fail-open を招くため）。上限超過が分類結果に影響したかどうかを利用者が知る手段（デバッグ出力等）は将来課題として棚卸し表（REQ-INV-01）に記録するに留める |

---

## 8. Design — 新しい分類（sorter）

### 8.1 シグナル収集（ClassifierSink の変更点）

現行の `KeywordFlags`／`ClassifierSink::on_record` に対し、本書で変わる点だけを記す。変わらない点（Trailing の隔離、本文の全文コピーを作らない、Software 本文の保持上限等）は既存仕様書 §8.2 を継承する。

| シグナル | 収集条件（変更後） | 強さ |
|---|---|---|
| `sui 系`（現行どおり） | `sui_image_params`／`sui_extra_data` キー、parameters 本文・Model 本文の `sui_image_params`（H-7） | 強 |
| `invokeai 系` | `invokeai_metadata`／`sd-metadata` キー → 強。`Dream` キー → 本文が InvokeAI 旧式構文（引用符で囲まれたプロンプト＋ `-s`／`-S`／`-C` 等のフラグ2つ以上。Phase 0 で固定、REQ-CLS-12）なら強、そうでなければ無効 | 強／無効 |
| `use_stable_diffusion_model` | 現行どおり | 強 |
| `comfyui 系` | `workflow`／`generation_data` キー、prompt レコード単独で JSON（REQ-CLS-09）、Stealth の構造判定による ComfyUI（U-8 でキー集合を確定。REQ-CLS-05） | 強 |
| `novelai_strong 系` | 検証済み `Comment`（keyword = `Comment` のレコード本文が JSON パース可能で、Phase 0 確定の NovelAI キー集合を2つ以上含む。同一レコード内、REQ-CLS-01／09）、Software 本文の `NovelAI`（REQ-CLS-10 の表による照合。保持上限に依存しない、REQ-CLS-08）、UserComment の NovelAI パック（H-7）、Stealth の構造判定による NovelAI（REQ-CLS-05） | 強 |
| `parameters_key` | keyword = `parameters`（本文の sui は Swarm 側で先に処理） | 強（WebUI） |
| `xmp_ai_declaration` | XMP `AIPromptInformation` 非空／`DigitalSourceType` が trainedAlgorithmicMedia 系（REQ-CLS-02 a／b） | 強（WebUI） |
| `xmp_draw_things_valid` | XMP UserComment が REQ-CLS-07 の検証を通過 | 強（WebUI） |
| `xmp_creatortool_ai` | XMP `CreatorTool` が REQ-CLS-10 の表に一致 | 中（WebUI） |
| `software_ai_tool` | Software 本文が REQ-CLS-10 の表に一致（NovelAI は novelai_strong 系で先に処理。それ以外は WebUI 側）。Software の存在のみ（現行 `has_software`）は廃止（REQ-CLS-11） | 中（WebUI） |
| `a1111_text_block` | `UserComment`／`ImageDescription` 本文に `Negative prompt:` と `Steps: ` の両方を含む（REQ-CLS-03） | 中（WebUI） |
| `jpeg_com_a1111` | REQ-API-01 の印。両マーカーあり → 中、片方のみ → 弱 | 中／弱（WebUI） |
| 弱シグナル群（種類ごとに bool、方向を明記する） | **WebUI 方向**: `steps_in_text`（本文ブロックの `Steps: ` のみ、REQ-CLS-03）、`negprompt_in_text`（本文ブロックの `Negative prompt:` のみ、REQ-CLS-03）、`stealth_present`（Stealth レコードの存在のみ、REQ-CLS-05）、`trailing_weak`（Trailing 由来は全体で1種類、REQ-CLS-06）、`jpeg_com_weak`（REQ-CLS-04）。**ComfyUI 方向**: `usercomment_workflow_substr`（REQ-CLS-03。現行では ComfyUI 候補だったもの）、`stealth_workflow_substr`（Stealth 本文の `workflow` 部分一致、REQ-CLS-05）。ただし §8.3 の注記のとおり、この2種類は Auto 運用では両立せず、現行では複合の対象にしない（記録のみ）。**NovelAI 方向**: `trailing_novelai`（REQ-CLS-06）、`stealth_novelai_substr`（Stealth 本文の `novelai` 部分一致、REQ-CLS-05） | 弱 |
| 無効（数えない） | Software の存在のみ、XMP の存在のみ、XMP `description` のみ（U-7 の複合条件が確定するまでの間）、検証を通らない `Comment`／`Dream`、非AIツール名の値 | — |

補足:

- XMP レコードについては source（PngXmp 等）ではなく keyword で分解する。source は「XMP 由来である」こと以上の分類根拠に使わない。
- 弱シグナルの複合は **種類** で数え、方向（WebUI／ComfyUI／NovelAI）を混ぜない。同じ種類の弱シグナルが複数レコードから立っても1票とする。同一の Stealth レコードは方向ごとに最大1票まで（存在は WebUI 方向、部分一致はその方向）で、同一方向に存在と部分一致が重なっても1票のままである。
- 中シグナルは弱の複合の票に数えない（§2.2）。中は手順6で単独確定する。

### 8.2 AI生成ツール名の表

REQ-CLS-10 の表を §8.1 の `software_ai_tool`／`xmp_creatortool_ai`／novelai_strong 系の Software 照合で共用する。表は sorter 側の1箇所に定数として置き、T-41 で表の内容自体を固定する（非AI名が混入していないことのテストを含む）。

### 8.3 決定手順（変更後。上が優先）

```
decide():
    // 1. SwarmUI（固有キー。現行どおり）
    if sui 系:
        return SwarmUI
    // 2. InvokeAI（Dream は検証済みの場合のみ、§8.1）
    if invokeai 系（強）:
        return InvokeAI
    // 3. Easy Diffusion（現行どおり）→ WebUI
    if use_stable_diffusion_model:
        return WebUI
    // 4. ComfyUI（強のみ。UserComment の "workflow" 部分一致は弱へ移動）
    if comfyui 系（強）:
        return ComfyUI
    // 5. NovelAI（強のみ。検証済み Comment は現行の手順1からここへ移した）
    if novelai_strong 系（強）:
        return NovelAI
    // 6. WebUI（強・中）
    if parameters_key || xmp_ai_declaration || xmp_draw_things_valid
       || xmp_creatortool_ai || software_ai_tool
       || a1111_text_block || jpeg_com_a1111（中）:
        return WebUI
    // W. 弱の複合（単独では確定しない。方向を混ぜず、方向ごとに2種類以上で確定する）
    // 同点規則: WebUI 方向と NovelAI 方向の複合が同時に成立した場合は NovelAI を優先する
    // （下の評価順がそのまま同点規則である）。根拠は2点: (a) 現行 decide の相対順序
    // （手順5b の NovelAI が手順6の WebUI より上）の維持。(b) 同時成立時に WebUI 方向の票は
    // stealth_present／trailing_weak のような存在系の汎用票である一方、NovelAI 方向の票は
    // NovelAI 固有の部分一致であり、内容の具体性が高い。P-23 で固定する。
    if 弱（NovelAI方向）の異なる種類が2つ以上:
        return NovelAI    // 対象は trailing_novelai と stealth_novelai_substr の2種類のみ。
                          // Trailing は Stealth ゲートを閉じないため両立し得る
    if 弱（WebUI方向）の異なる種類が2つ以上:
        return WebUI
    // ComfyUI 方向の弱（usercomment_workflow_substr、stealth_workflow_substr）は複合で確定させない。
    // 理由: この2種類は Auto 運用では両立しない（UserComment は実質レコードとして Stealth
    // ゲートを閉じるため、UserComment がある画像では Stealth 検査が発動しない）。
    // 到達不能な規則を置く代わりに、ComfyUI 方向の弱は記録だけして確定には使わないことを
    // ここに明記する。将来、両立し得る ComfyUI 方向の弱が増えた場合は本書改訂で複合を復活させる。
    // 7. どれにも当たらない
    return None
```

注記:

- 現行との順序の違いは2点だけである: (1) 検証済み `Comment` を手順1（最上位）から手順5の NovelAI 強グループへ移した（値検証の必須化と合わせ、監査 §11-2 への対応）。(2) 弱シグナルを全て手順 W に分離した。それ以外の相対順序（Swarm → Invoke → Easy → Comfy → NovelAI 本文系 → WebUI）は現行を維持している。順序をさらに変える場合は等価性テスト（§10.4）で差分を説明できること。
- 弱の方向跨ぎは数えない（§8.1 補足）。trailing_novelai と WebUI 方向の弱が同時に立っても、どちらの複合にもならない。
- ただし方向跨ぎの「票の混ぜ合わせ」とは別に、両方向の複合がそれぞれ独立に成立することはあり得る（例: Stealth 1件が stealth_present と stealth_novelai_substr に、Trailing 1件が trailing_weak と trailing_novelai に票を入れる場合）。その同時成立時は手順 W の同点規則により NovelAI を優先する。

### 8.4 パニック隔離・エラー意味論

現行どおり（既存仕様書 §8.4、FR-INT-04／05）。本書で追加する集計種別（§6）はエラー意味論を変えない。終了コードが変わるのは REQ-COL-03（存在しない入力）と REQ-COL-07（収集エラー）で、いずれも「エラーがあれば1」への統一である。唯一の例外は REQ-COL-06 の引数検証エラーで、処理開始前に **終了コード 2** で終了し、移動を一切行わない（引数エラーの慣例値であり、処理結果の 0／1 とは意味の層が違う）。

---

## 9. Specification（入出力仕様の差分）

### 9.1 sorter CLI

- オプション自体は追加しない。変わるのは: (a) フォルダ名引数の検証（REQ-COL-06）、(b) 集計表示への行追加（リンク走査・重複除去・入力エラー・収集エラー・dest 衝突・元削除失敗・dest 作成不可。いずれも該当0件でも行を出すかは現行の集計表示の流儀に合わせる）、(c) 終了コードの条件（§8.4）。
- `--version` の上げ方は §11 のフェーズ完了時に決める（本書では版番号を固定しない。分類の意図した変更を含むため、少なくとも現行の 2.0.x 系からはマイナーを上げることを推奨として記録する）。

### 9.2 fast_meta CLI

- 引数仕様は不変。出力の意図した変更の **原則** は次のとおり: ゴールデン出力の変更は、本書のいずれかの要件 ID に追跡できるものに限る。各フェーズの記録では、実際に生じたゴールデン差分を要件 ID 付きで列挙すること（列挙に無い差分は意図しない変更として扱い、フェーズ完了条件を満たさない）。主な変更カテゴリは §13.1 の列挙のとおり: 再ラベル廃止（REQ-DSP-01）、診断文の stderr 移行（REQ-DSP-02、ただし見出し行は残す）、Stealth ゲート変更に伴う Stealth レコードの出現範囲の変化（REQ-EXT-01）、Draw Things の本文が合成テキストではなく生 JSON になること（REQ-EXT-07）、従来落ちていたレコードが新たに出ること（REQ-EXT-06／11／13／15／16、REQ-EXT-02／04／05 の fail-soft 化に伴うものを含む）、従来誤って出ていたレコードが出なくなること（REQ-EXT-08）、未証明レコードの隔離に伴う source／detail の変化（REQ-EXT-03 (b)）、候補優先順位の変更に伴うフィールド値の変化（REQ-EXT-04 (a)）、デコード是正に伴う本文の変化（REQ-EXT-09／10／14）。
- `--stealth auto` の意味が REQ-EXT-01 で変わるため、ヘルプ文言を実態に合わせて更新する。

### 9.3 ライブラリ API の互換性

- REQ-API-01 は `RecordView.detail` の値の新規定義であり、公開 API の形は変えない。
- **版上げの原則**: ライブラリ利用者から見えるレコード内容が変わるフェーズでは、その都度 fast_meta の版を少なくともマイナー上げし、タグを打つこと。該当するのは Phase 2（REQ-EXT-01 の Stealth ゲート、REQ-EXT-07 の Draw Things 本文、REQ-API-01、REQ-EXT-06）と Phase 3（REQ-EXT-13／15／16 等のレコード復活を含む抽出変更全般）である。sorter がその変更を取り込む場合は、依存タグを新しい版へ更新し、分類テスト一式を再実行してからフェーズ完了とする（Phase 3 で sorter が取り込まない場合でも、次に取り込む際のタグ更新漏れを防ぐため、Phase 3 の記録に「未取り込みの版上げあり」と明記する）。

---

## 10. Test 計画

フィクスチャは合成データを基本とし、実サンプルを使う場合はファイル自体ではなくハッシュと出所・期待結果を記録する（既存仕様書 §10.3 の流儀を継承）。負例＝「仕分けされない（None）」が期待値、正例＝行き先が期待値。

### 10.1 負例フィクスチャ（監査 §11-8 を正とする）

| ID | 内容 | 主に対応する F |
|---|---|---|
| N-01 | Photoshop JPEG: EXIF Software = "Adobe Photoshop…" ＋ XMP CreatorTool = "Adobe Photoshop…" | Software 値不問／F-02 |
| N-02 | Celsys PNG: tEXt Software = "Celsys Studio Tool" のみ（今回の実例そのもの） | Software 値不問（REQ-CLS-11） |
| N-03 | ImageReady PNG: Software ＋ XMP（汎用フィールドのみ） | Software 値不問／F-02 |
| N-04 | JFIF のみ JPEG（メタデータなし） | —（回帰防止） |
| N-05 | Lightroom JPEG: XMP dc:description（キャプション文）のみ。期待は2つに分ける: 分類側＝None（Phase 1 で固定）、表示側＝Midjourney と再ラベルされないこと（REQ-DSP-01 の担当であり、Phase 2 の G-01 で固定する。Phase 1 の完了条件には分類側のみを含める） | F-02／F-03 |
| N-06 | Photoshop WebP: XMP CreatorTool のみ | F-02 |
| N-07 | PNG: tEXt Comment = 非JSONの平文（編集ツールのコメント） | F-01 |
| N-08 | 写真 JPEG: EXIF ImageDescription に "Steps: " を含む説明文のみ（他のシグナルなし） | F-05 |
| N-09 | Stealth PNG: プロンプト文中に "workflow" の語を含む NovelAI 形式でない Stealth（構造判定で ComfyUI にならないこと） | F-07 |
| N-10 | PNG: Trailing に非AIツールの JSON（`Software` フィールドを含む）のみ | F-08 |
| N-11 | 写真 JPEG: COM に "Steps:" を含む写真コメントのみ（片方マーカー＝弱のみ） | F-06 |
| N-12 | PNG: tEXt Comment = JSON だが NovelAI キー集合を満たさない（汎用設定 JSON） | F-01 |
| N-13 | JPEG: EXIF UserComment に `"workflow"` の語を含むだけの本文（他のシグナルなし。ComfyUI 方向の弱は複合対象外のため確定しない） | F-05 |
| N-14 | 写真 JPEG: EXIF ImageDescription に "Negative prompt:" を含む説明文のみ（他のシグナルなし。`negprompt_in_text` は弱単独のため確定しない） | F-05 |

### 10.2 正例フィクスチャ

| ID | 内容 | 期待 | 主に対応する F |
|---|---|---|---|
| P-01 | A1111 PNG: parameters | WebUI | —（回帰防止） |
| P-02 | A1111 JPEG: UserComment／ImageDescription に parameters 全文（プロンプト＋`Negative prompt:`＋`Steps: ` の両マーカーを含む）→ `a1111_text_block`（中）で単独確定する形として固定する | WebUI | F-05 |
| P-03 | ComfyUI PNG: workflow／prompt（JSON）。変種として prompt 本文に UTF-8 BOM 付き | ComfyUI | F-34 |
| P-04 | NovelAI PNG: Comment（設定 JSON）＋ Software = NovelAI。変種として Software 本文が256バイト超の PNG でも NovelAI になること | NovelAI | F-01／F-16 |
| P-05 | NovelAI JPEG／WebP: UserComment の NovelAI パック | NovelAI | — |
| P-06 | SwarmUI PNG: sui キー／parameters 内包／EXIF Model 旧形式 | SwarmUI | — |
| P-07 | InvokeAI PNG: invokeai_metadata／sd-metadata／検証を通る Dream | InvokeAI | F-01 同型（Dream） |
| P-08 | Easy Diffusion PNG: use_stable_diffusion_model | WebUI | — |
| P-09 | Stealth PNG（NovelAI 構造。構造のキー集合は U-8 で確定し、確定後に本行へ具体形を追記する） | NovelAI | F-07 |
| P-10 | Stealth PNG（ComfyUI 構造。同上、U-8） | ComfyUI | F-07 |
| P-11 | Midjourney PNG: Software = Midjourney（＋ XMP description があってもよい） | WebUI | F-02 |
| P-12 | Draw Things: XMP UserComment の Draw Things JSON（固有キー集合を満たす） | WebUI | F-14 |
| P-13 | XMP に AIPromptInformation を持つ画像／DigitalSourceType = trainedAlgorithmicMedia の画像。変種として XMP パケットが tEXt に格納された PNG。本体は REQ-CLS-02 (a)/(b) の検証として **Phase 1** で固定し、tEXt 格納の変種だけは REQ-EXT-06 が前提のため **Phase 2** で固定する | WebUI | F-02／F-13 |
| P-14 | JPEG COM: `Negative prompt:` と `Steps:` の両方を含む A1111 形式 | WebUI | F-06 |
| P-15 | Trailing に A1111 形式テキスト（`Steps: ` と `Negative prompt:` の両方。弱 `trailing_weak` 1票）＋ UserComment に `Steps: ` のみ（弱 `steps_in_text` 1票）→ 弱の複合（WebUI 方向2種類）で確定する形として固定する | WebUI | F-08 |
| P-15b | Trailing に A1111 形式テキスト（両マーカー）のみ（他のシグナルなし） | None（Trailing 単独では確定しない） | F-08 |
| P-16 | Stealth PNG（NovelAI）に Software のみの汎用レコードが併存（Stealth ゲートが閉じないこと） | Stealth 検査が発動すること自体が主目的。行き先は U-8 の結論に依存する: 構造判定が確定すれば NovelAI、確定しなければ（弱1票のみのため）None | F-09 |
| P-17 | 非検証の Dream 風テキスト（引用符＋フラグの形を満たさない）を持つ PNG | None（負例寄りだが Dream 検証の固定として正例表に置く） | F-01 同型 |
| P-18 | Midjourney の XMP のみ画像（Software 等の他担体に MJ の根拠が無い形） | Phase 0（U-7）で捕捉条件と期待値を確定する。確定するまでは本表の期待値を置かず、調査対象として記録する | F-02 |
| P-19 | XMP CreatorTool = AIツール名（例: Midjourney）のみを持つ画像（他にシグナルなし）→ `xmp_creatortool_ai`（中）で単独確定する形として固定する | WebUI | F-02 |
| P-20 | 検証済み Comment（NovelAI）＋ `workflow` キーを併せ持つ PNG（強シグナル同士の競合） | ComfyUI（手順4が手順5に優先。現行の手順1優先からは変更） | F-01 |
| P-21 | 検証済み Comment（NovelAI）＋ sui 系キーを併せ持つ PNG（強シグナル同士の競合） | SwarmUI（手順1が手順5に優先。現行からは変更） | F-01 |
| P-22 | UserComment に `Negative prompt:` のみ（弱 `negprompt_in_text` 1票）＋ Trailing に A1111 形式テキスト（弱 `trailing_weak` 1票）→ 弱の複合（WebUI 方向2種類）で確定する形として固定する | WebUI | F-05／F-08 |
| P-23 | Stealth（`novelai` 部分一致あり、構造集合には一致しない）＋ Trailing（`novelai` 部分一致と `Steps: ` を含む）→ WebUI 方向 {stealth_present, trailing_weak} と NovelAI 方向 {stealth_novelai_substr, trailing_novelai} の複合が同時成立し、手順 W の同点規則で NovelAI を優先する形として固定する | NovelAI | F-07／F-08 |

### 10.3 単体・統合テスト（§2.3 対応表の T 番号）

| ID | 内容 | 対象 |
|---|---|---|
| T-06 | sorter の分類が label 文字列に依存しないこと: COM レコードの label を任意の文字列に変えても（detail の印が同じなら）行き先が変わらない | REQ-CLS-04 |
| T-07 | Software 本文が複製上限（256バイト）を超えても AIツール名照合が働く（P-04 変種の単体版） | REQ-CLS-08 |
| T-08 | prompt レコードが2件（JSON 1件・非JSON 1件）の場合、JSON でない prompt が JSON 扱いにならない | REQ-CLS-09 |
| T-10 | TIFF: 壊れたエントリの後ろにある UserComment が読める | REQ-EXT-02 |
| T-11 | PNG: Stage A が壊れたチャンクに当たっても後方レコードが回収される／step 4 の未証明レコードが通常レコードとして効かない | REQ-EXT-03 |
| T-12 | JPEG 拡張 XMP: GUID の異なる断片が混在しても完全な組が採用される | REQ-EXT-04 |
| T-13 | TIFF: IFD1 の ExifIFD 内 UserComment が読める | REQ-EXT-05 |
| T-14 | 棚卸し表（REQ-INV-01）の担体が分類に効いていないこと（各担体のみの画像が None） | REQ-INV-01 |
| T-15 | C2PA 宣言のみの画像が現行は None であること（既知の制限の固定） | REQ-INV-02 |
| T-16 | tEXt／zTXt に格納された XMP パケットからフィールドが読める | REQ-EXT-06 |
| T-17 | Draw Things 風 JSON（`"c"` のみ）で本文が合成されず、description が抑制されない | REQ-EXT-07 |
| T-18 | WebP: RIFF ペイロード後方の EXIF／XMP 形状バイト列がレコード化されない | REQ-EXT-08 |
| T-19 | BOM なし UNICODE UserComment: ASCII 中心・非ASCII中心の両方で本文が壊れない | REQ-EXT-09 |
| T-20 | 収集: リンク経由の画像・フォルダが対象になり、リンク循環で無限ループしない | REQ-COL-01 |
| T-21 | 収集: フォルダと配下ファイルの同時指定で二重処理されず、結果が決定的 | REQ-COL-02 |
| T-22 | dest 同名衝突が「スキップ（メタデータ無し）」と別集計になる | REQ-TRN-01 |
| T-23 | 転送後の元削除失敗が警告として別集計になり、dest 側は残る | REQ-TRN-02 |
| T-24 | 存在しない入力パスが入力エラーとして集計され、他の入力は処理され、終了コード1 | REQ-COL-03 |
| T-25 | 単一ファイル指定で親のサブツリー全体の移行が走らず、移行失敗でも仕分けが継続する | REQ-COL-04 |
| T-26 | TIFF: 非UTF-8（Latin-1／Shift-JIS バイトを含む）ImageDescription の ASCII 照合部分が効く | REQ-EXT-10 |
| T-27 | 出力フォルダ内のファイルを直接渡しても入れ子移動されず、対象外拡張子の直接指定も走査経路と揃う | REQ-COL-05 |
| T-28 | フォルダ名引数の検証: 空／`.`／`..`／区切り含み／絶対パス／重複が処理開始前にエラーになり、その際の終了コードが 2 であること | REQ-COL-06 |
| T-29 | XMP: 局所破損のあるパケットでも破損前のフィールドが読める | REQ-EXT-11 |
| T-30 | 16MB 超の EXIF／XMP 担体が0件になる現行挙動の固定（仕様明記とセット） | REQ-EXT-12 |
| T-31 | iTXt の非UTF-8テキストが tEXt と同様にレコード化される | REQ-EXT-13 |
| T-32 | UTF-8 BOM 付き prompt で JSON 判定が通る（fast_meta／sorter 両側） | REQ-EXT-14／REQ-CLS-09 |
| T-33 | UTF-16（BE／LE、BOM 付き）XMP パケットからフィールドが読める | REQ-EXT-15 |
| T-34 | 出力フォルダ名と同名の通常ファイルがある場合に「dest 作成不可」として別集計になる | REQ-TRN-03 |
| T-35 | 読めないサブディレクトリがあってもバッチ全体が止まらず、収集エラーとして集計される | REQ-COL-07 |
| T-36 | UserComment の先頭64KB の外に `Steps: `／`"workflow"` がある画像が、それらでは判定されないこと（REQ-CLS-08 (b) の判断固定） | REQ-CLS-08 |
| T-37 | XMP: 複数の rdf:Description がある場合に `rdf:about=""`（主対象）のフィールドが優先される | REQ-EXT-04 (a) |
| T-38 | WebP: `Exif\0\0` プレフィクス付き EXIF チャンクから UserComment 等が読める | REQ-EXT-16 |
| T-39 | JPEG／WebP: 歩行停止（セグメント長／チャンクサイズ不正）より前に得たレコードが返る | REQ-EXT-05 (b) |
| T-40 | Stealth Auto ゲートの種別表（REQ-EXT-01）の固定: 閉じる側の代表（parameters あり／UserComment あり）では Stealth 検査が発動しないこと、閉じない側の代表（Software のみ／XMP CreatorTool のみ／Trailing のみ／Title のみ）では発動すること | REQ-EXT-01 |
| T-41 | AI生成ツール名の表（REQ-CLS-10）の内容自体の固定: 初期値が監査 §11-1 の列挙と一致し、非AI名（Adobe／Photoshop／Celsys／ImageReady／GIMP 等）が混入していないこと | REQ-CLS-10 |
| T-42 | TIFF: IFD2 以降のタグが読めること、および IFD2 の ExifIFD 内 UserComment が読めること（T-13 の IFD1 側と対にする） | REQ-EXT-05 (a) |
| G-01 | fast_meta ゴールデン: XMP Description のラベルが `XMP Description` のまま（再ラベルなし） | REQ-DSP-01 |
| G-02 | fast_meta ゴールデン: 診断文が stdout に出ず stderr に出る。stdout の見出し行とレコード出力は従来どおり | REQ-DSP-02 |

### 10.4 等価性と性能

- 等価性: 既存仕様書 §10.3 のコーパスに §10.1／§10.2 のフィクスチャを加え、改修前後の行き先差分が §13.1 の意図した変更だけであることを最終フェーズで記録する。
- 性能: メタデータなし画像を多数派とする混合コーパスで改修前後の総時間を測り、分類の値検証追加と Stealth ゲート変更（REQ-EXT-01 で Stealth 検査の発動が増える）による悪化が支配的でないことを記録する。特に Stealth 検査は「汎用レコードのみの画像」でも発動するようになるため、その層のコスト増を内訳で示すこと。

---

## 11. 実装計画（フェーズ分割）

### 11.1 フェーズ一覧

| Phase | 主対象 | 内容 | 完了条件 |
|---|---|---|---|
| 0 | 両方（計測・フィクスチャのみ） | §10 の負例・正例フィクスチャの固定、現行挙動の記録、§14 の未決事項 U-1〜U-3（値検証のキー集合）と U-5（弱の方向跨ぎは数えない、で確定済みのため裏付け確認のみ）・U-7（MJ の XMP のみ捕捉条件）・U-8（Stealth 構造判定のキー集合）の確定。**実装変更なし** | フィクスチャと現行結果がリポジトリに記録され、U-1〜U-3 と U-7・U-8 が本書へ追記され、U-5 の裏付け確認の結果（反例の有無。反例があった場合は本書の改訂内容を含む）も記録されている（U-7 が「条件なし＝捕捉しない」で確定した場合は P-18 の期待値を None として記録する。U-8 が「見分け不可」で確定した場合は P-09 を弱の複合で通る形に書き換え（Trailing との併存で到達可能）、P-10 の期待値を None に書き換え（ComfyUI 方向の弱は複合対象外のため、構造判定なしでは ComfyUI Stealth を拾えないことを既知の制限として記録する）、P-16 の期待値を None に書き換える） |
| 1 | B（sorter） | 分類の中核置き換え: REQ-CLS-01／02／03／05／06／08／09／10／11／12、§8 の決定手順。fast_meta 側の変更が要る印（REQ-API-01）が未着手の間は、COM については現行の label 判定を暫定で残し、その旨をコード内コメントと本フェーズの記録に残す。REQ-CLS-02 (d)（Draw Things シグナル）も前提が Phase 2 にあるため本フェーズでは未発動とし、Phase 2 で有効化する | N-01〜N-10、N-12〜N-14、P-01〜P-11、P-13（本体のみ。tEXt 格納 PNG の変種は Phase 2）、P-15／P-15b、P-17、P-19〜P-23、T-07／T-08／T-36、T-41 が通る（N-05 は分類側のみ。表示側は Phase 2 の G-01 で確定させる。**P-16 は前提（REQ-EXT-01 の Stealth ゲート変更）が Phase 2 にあるため本フェーズの完了条件に含めず、Phase 2 で確定させる**。現行ゲートのままでは P-16 は Stealth 検査が発動せず、U-8 の結論によっては期待値と食い違うか、食い違わなくても主目的が検証されないため）。P-18 は U-7 の結論に応じて本フェーズで扱う: 捕捉条件が見つかった場合は REQ-CLS-02 (f) として本フェーズで実装し P-18 を完了条件に含める。見つからなかった場合は P-18（期待値 None）も本フェーズで通す。**N-11（COM 片方マーカー）は暫定の label 判定では WebUI に落ちるため本フェーズの完了条件に含めず、Phase 2 で確定させる**（この除外は暫定の既知事項として本フェーズの記録に残す） |
| 2 | A→B | fast_meta: REQ-API-01、REQ-EXT-01、REQ-EXT-06、REQ-EXT-07、REQ-DSP-01／02。sorter: REQ-CLS-04／07 への切り替え（label 判定の暫定を撤去） | G-01／G-02、T-06／T-16／T-17、T-40、N-11、P-12、P-14、P-13 の変種（XMP パケットが tEXt に格納された PNG。本体は Phase 1 で固定済み）、P-16（U-8 の結論に沿った期待値で）が通る。sorter の分類条件に label への依存が残っていない |
| 3 | A（＋B のタグ更新） | fail-soft 化とデコード対称化: REQ-EXT-02〜05、REQ-EXT-08〜11、REQ-EXT-13〜16 | T-10〜T-13、T-18／T-19、T-26、T-29、T-31〜T-33、T-37〜T-39、T-42 が通る。§9.3 の原則どおり fast_meta の版上げとタグが行われ、sorter が取り込む場合は依存タグ更新と分類テストの再実行が記録されている |
| 4 | B | 収集・移動・集計: REQ-COL-01〜07、REQ-TRN-01〜03 | T-20〜T-25、T-27／T-28、T-34／T-35 が通る |
| 5 | 両方 | 棚卸しと判断固定の仕上げ: REQ-INV-01／02、REQ-EXT-12。等価性・性能の最終確認（§10.4）、README／doc の更新 | T-14／T-15／T-30 が通る。§12 のチェックリストが全て満たされる |

フェーズの順序は「実害のある分類（Phase 1）→ 構造化と表示（Phase 2）→ 抽出の堅牢化（Phase 3）→ 運用まわり（Phase 4）→ 固定と仕上げ（Phase 5）」であり、Phase 3 以降の低重大度項目を理由に Phase 1 を遅らせない。

### 11.2 ブランチ運用

既存仕様書 §11.2 を継承する（フェーズごとのブランチ、PR で main へ）。fast_meta の版上げ（§9.3）は Phase 2 と Phase 3 の各マージ時に行い、sorter 側の依存タグは sorter が取り込む時点で更新する。

### 11.3 コミット規約

既存仕様書 §11.3 を継承し、読み替える: コミットは要件 ID（REQ-CLS-xx、REQ-EXT-xx 等）単位で分割し、対応するテスト（N／P／T／G 番号）を同一コミットに入れる。1フェーズ = 1PR。フェーズをまたぐ変更を混ぜない。各コミット時点で `cargo build --release --locked` と `cargo test --locked` が緑であること。

---

## 12. 受け入れ条件（マスターチェックリスト）

**分類（sorter）**
- [ ] §10.1 の負例 N-01〜N-14 が全て None になる
- [ ] §10.2 の正例 P-01〜P-17（P-15b を含む）と P-19〜P-23 が全て期待どおりの行き先になる。P-18 は U-7 の結論どおりの期待値（捕捉条件ありなら WebUI、なしなら None）になる
- [ ] 分類条件に「キーの存在のみ」「XMP の有無のみ」「label 文字列」が残っていない（コードレビューで §8.1 の表と突き合わせる）
- [ ] 弱シグナル単独で確定する経路が無い（§8.3 手順 W 以外に弱が効く段が無い）

**fast_meta**
- [ ] ゴールデン出力の変更が全て本書の要件 ID に追跡でき、各フェーズの記録に要件 ID 付きで列挙されている（§9.2 の原則。列挙に無い差分が無いこと）
- [ ] Stealth Auto の新ゲート（REQ-EXT-01）で P-16 と T-40 が通り、実質レコードのある画像では Stealth 検査が発動しない
- [ ] 抽出層が本文を合成しない（REQ-EXT-07。T-17）

**収集・移動・集計（sorter）**
- [ ] 集計で {仕分け済み, スキップ（メタデータ無し）, スキップ（dest 衝突）, dest 作成不可, エラー} がファイル単位で区別でき、その和が入力ファイル総数と一致する。警告（元削除失敗）は内訳表示、{入力エラー, 収集エラー, 重複除去, リンク素通り} は処理前件数として別枠で区別できる（§6 末尾の注記の定義どおり）
- [ ] 終了コードの条件が §8.4 のとおり（引数検証エラーは2、それ以外は 0／1）

**全体**
- [ ] F-01〜F-37 の全件に処置が対応付いている（§2.3 の表と実装・テストの突き合わせ）
- [ ] §10.4 の等価性差分が §13.1 の意図した変更のみで、性能記録がある
- [ ] `cargo build --release --locked` / `cargo test --locked` が両リポジトリで緑

---

## 13. 決定事項とリスク

### 13.1 本書で決定済み（再判断しないこと）と意図した挙動変更

| 項目 | 決定 |
|---|---|
| 分類の基本形 | 存在シグナルを全廃し「キー＋値パターン」化する。Software 本文の値判定（H-5）の形に全シグナルを寄せる |
| 弱シグナルの扱い | 単独確定させない。原則として同じ方向の異なる種類2つ以上の複合でのみ確定する（§8.3 手順 W）。例外として ComfyUI 方向の弱は複合でも確定させず記録のみとする（§8.3 の注記の理由による）。WebUI 方向と NovelAI 方向の複合が同時成立した場合は NovelAI を優先する（§8.3 手順 W の同点規則） |
| 非AIツール名の扱い（監査 §11-1 の読み替え判断） | 監査 §11-1 は「非AIツール名は明示的に無効化する側も持つ」と書く。本書ではこれを「ツール名の表に入れず、シグナルとして数えない」ことと読み替え、veto（強いシグナルを打ち消す）ロジックは作らない。理由: AI画像を Photoshop 等で再保存した画像では、強い AI シグナルと非AI名が併存するのが正常であり、veto にするとそちらを取りこぼす。Software 本文に AI名と非AI名が併存する場合は AI名側のシグナルが生きる。この読み替えに反対の場合は本書の改訂事項とする（REQ-CLS-10） |
| F-04 の見出し行 | ファイル見出し行（フルパスを含む）は出力形式の一部として stdout に意図して残す。F-04 への処置は診断文の stderr 移行による部分修正であり、パス行は判断として残す（REQ-DSP-02） |
| XMP の行き先 | フィールド別に分解した上で、AI 宣言系の行き先は従来どおり `webui_img`（専用フォルダは作らない） |
| C2PA | 実装しない（REQ-INV-02） |
| 16MB 上限 | 維持し、超過時の0件を仕様として固定する（REQ-EXT-12） |
| 意図した変更: 分類 | (1) `Comment` 存在のみ／検証を通らない `Dream` では確定しない。(2) XMP の有無のみ、Software の存在のみでは WebUI にならない。(3) 弱シグナル単独（Stealth の存在のみ、trailing 部分一致のみ、`Steps: ` 部分一致のみ等）では確定しない。(4) trailing_novelai 単独では NovelAI にならない。(5) 検証済み `Comment` の順位変更（手順1→手順5）に伴い、強シグナル同士が競合する画像の行き先が変わる: 検証済み Comment ＋ workflow → ComfyUI（現行は NovelAI）、検証済み Comment ＋ sui 系 → SwarmUI（現行は NovelAI）。P-20／P-21 で固定する。(6) 手順 W で WebUI 方向と NovelAI 方向の弱の複合が同時成立した場合は NovelAI を優先する。これは現行 decide の相対順序（手順5b の NovelAI が手順6の WebUI より上）の維持であり、行き先の変更ではない。P-23 で固定する |
| 意図した変更: fast_meta CLI | ゴールデン出力の変更は本書の要件 ID に追跡できるものに限る（原則は §9.2）。カテゴリ: XMP Description の Midjourney 再ラベル廃止。診断文の stderr 移行（見出し行は残す）。`--stealth auto` のゲート定義変更に伴う Stealth レコードの出現範囲の変化。Draw Things 本文の合成廃止（生 JSON を出す）。従来落ちていたレコードの復活（REQ-EXT-06／11／13／15／16、fail-soft 化に伴う REQ-EXT-02／04／05）。従来誤って出ていたレコードの除去（REQ-EXT-08）。未証明レコードの隔離に伴う source／detail の変化（REQ-EXT-03 (b)）。候補優先順位・デコード是正に伴う値・本文の変化（REQ-EXT-04 (a)／09／10／14） |
| 意図した変更: 終了コード | 存在しない入力パスと収集エラーがある場合は終了コード1（現行は前者が0、後者はバッチ全体が異常終了）。フォルダ名引数の検証エラーは処理開始前に終了コード2 |
| 意図した変更: 集計表示 | 結果種別の行追加（§6） |

### 13.2 リスク

| リスク | 影響 | 対策 |
|---|---|---|
| 値検証のキー集合（NovelAI／Draw Things／Dream）が実サンプルとずれる | 正例の取りこぼし（誤検知の裏返し） | Phase 0 で実サンプルから確定し、本書へ追記してから着手する（§0-3、§14）。検証は「2つ以上のキー」という複合条件にし、単一キーへの依存を避ける |
| 弱の複合条件で従来拾えていた画像が落ちる | 実質 A1111 なのに単一の弱根拠しか持たない画像（例: `Steps: ` のみを持つ JPEG）が None になる | §10.4 の等価性確認で差分を列挙し、§13.1 の意図した変更として説明できるものだけを許容する。想定外の差分が出たら本書を改訂する |
| Stealth ゲート変更で検査発動が増える | 汎用レコードのみの画像（多数派の一部）で処理コスト増 | §10.4 で内訳を測る。支配的なら汎用レコードの定義の見直しを本書改訂で扱う |
| fail-soft 化で壊れたファイルから誤ったレコードを拾う（fail-open 化） | 新しい誤分類経路 | REQ-EXT-03 (b) のとおり、証明できない経路のレコードは通常レコードとして効かせない。fail-soft は「読める単位を増やす」ためであり「証明基準を下げる」ためではない |
| Phase 1 の COM 暫定（label 判定）が残留する | F-06 の構造化が未完のまま固定される | Phase 2 の完了条件に「label 依存が残っていない」を含め、Phase 1 の記録に暫定である旨を残す |

---

## 14. 未決事項（Phase 0 で解消し、本書へ追記する）

| ID | 内容 | 解消の期限 |
|---|---|---|
| U-1 | NovelAI 設定 JSON のキー集合の確定（REQ-CLS-01）。実サンプルの Comment JSON から、NovelAI 設定に固有で安定しているキーを選び、「2つ以上含む」の閾値で足りるかを確認する | Phase 1 着手前 |
| U-2 | Draw Things JSON の固有キー集合の確定（REQ-CLS-07）。`"c"` 以外に何が併存すれば Draw Things と言えるか | Phase 2 着手前 |
| U-3 | Dream 旧式構文の検証パターンの確定（§8.1）。receyuki の Dream 形式定義と実サンプルで固定する | Phase 1 着手前 |
| U-4 | PNG Stage C step 4 の未証明レコードの隔離方法（source を分けるか detail で示すか。REQ-EXT-03 (b)） | Phase 3 着手前 |
| U-5 | §8.3 手順 W で弱の方向跨ぎを数えないことの裏付け確認（決定自体は本書で済み。反例の有無を Phase 0 のフィクスチャで確認し、反例があれば本書を改訂する） | Phase 0（Phase 1 着手前） |
| U-6 | 版番号の確定（fast_meta／sorter それぞれ。§9.1／§9.3 の推奨を踏まえる） | 各リリースフェーズ |
| U-7 | Midjourney の XMP のみ画像の捕捉条件の確定（REQ-CLS-02 (f)、P-18）。MJ の XMP に固有のフィールド／値の組み合わせがあるかを実サンプルで確認する。条件が見つからなければ「XMP のみでは捕捉しない」を確定し、P-18 の期待値を None として記録する | Phase 0（Phase 1 着手前） |
| U-8 | Stealth 本文 JSON の構造判定のキー集合の確定（REQ-CLS-05、P-09／P-10）。NovelAI Stealth と ComfyUI Stealth の実サンプルから、トップレベルの構造（キー集合等）の何で見分けるかを確定する。見分けが付かなければ構造判定は実装せず、Stealth は弱シグナル（方向別の部分一致と存在）のみに留めることを確定する | Phase 0（Phase 1 着手前） |

### 14.1 解消記録（2026-11-27、Phase 0）

実サンプルが手元に無いため、参照実装（d3x-at/sd-parsers、receyuki/stable-diffusion-prompt-reader）
の形式定義と公開されている仕様記述から確定した。閾値は単一キー依存を避ける複合条件とした。
詳細な根拠とフィクスチャの現行挙動は `sdMetaImageSorter/doc/phase0/261127-false-classification-baseline.md`
にある。

- **U-1（NovelAI 設定 JSON、REQ-CLS-01）**: キー集合 `uc` / `sampler` / `scale` / `seed` /
  `steps` / `strength` / `noise` / `noise_schedule` / `sm` / `sm_dyn` / `n_samples` /
  `cfg_rescale` のうち **2 つ以上** をトップレベルに含むこと。`prompt` は NovelAI では
  Description 側に出るため集合に含めない。Fooocus の `Comment` JSON とはキーが重ならない。
- **U-2（Draw Things、REQ-CLS-07）**: `c`（文字列）を持ち、かつ `sampler` / `steps` / `seed` /
  `scale` / `model` / `size` のうち **2 つ以上** を持つこと。
- **U-3（Dream、REQ-CLS-12）**: `"` で始まり閉じ `"` の後ろに ` -<英字> <値>` 形式のフラグが
  **2 つ以上** 続くこと（`-s`／`-S`／`-C`／`-W`／`-H`／`-A`）。
- **U-5（弱の方向跨ぎ）**: §10.1／§10.2 の全フィクスチャで反例なし。本書の改訂は不要。
- **U-7（Midjourney の XMP のみ、REQ-CLS-02 (f)／P-18）**: **XMP のみでは捕捉しない**で確定。
  MJ は XMP に固有の生成宣言フィールドを書かず、参照実装にも MJ 用 XMP パーサが無い。
  汎用 `dc:description` を MJ とみなすと N-05 を再び誤検知するため条件を作らない。
  P-18 の期待値は None とし、(f) は実装しない。
- **U-8（Stealth 構造判定、REQ-CLS-05／P-09／P-10）**: NovelAI 構造 = JSON パース成功かつ
  U-1 のキーを 2 つ以上含む、または `Comment` 文字列と `Description` を併せ持つ。
  ComfyUI 構造 = JSON パース成功かつ `nodes` と `links` の両配列を持つ、またはトップレベルの
  全値が `class_type` を持つ object。どちらにも一致しなければ構造判定をせず、方向別の
  部分一致と存在のみを弱シグナルとする。

### 14.2 Phase 0 の記録

- §10.1／§10.2 のフィクスチャを `sdMetaImageSorter/src/classifier.rs` の `fixtures()` に
  レコード単位で固定し、改修前の判定を `doc/phase0/261127-false-classification-baseline.md`
  に記録した（負例14件中11件が誤仕分け、正例側の未達5件）。
- U-4（PNG Stage C の未証明レコードの隔離方法）は Phase 3 着手前に決める。U-6（版番号）は
  各リリースフェーズで決める。


---

## 15. 参考情報

- 監査（正本）: `~/workspace/sdMetaImageSorter-metadata-flow-and-false-classification-audit.md`（F-01〜F-37、§0 の3層分離、§10 の健全リスト、§11 の改修の方向）
- 既存仕様書（本書の体裁・要件 ID 体系・コミット規約の出典。本書と矛盾する分類表・決定手順は本書が優先する）: `~/workspace/sdMetaImageSorter-v2-plan-design-spec-requirements.md`
- fast_meta v2.1.0 の実装: https://github.com/wai55555/checkImageMetadata
- sdMetaImageSorter 現行実装: https://github.com/wai55555/sdMetaImageSorter
- receyuki/stable-diffusion-prompt-reader（MIT、Dream 形式等の出典）: https://github.com/receyuki/stable-diffusion-prompt-reader
- d3x-at/sd-parsers（MIT、形式判別の照合用）: https://github.com/d3x-at/sd-parsers

---

## 16. レビューログ

（独立レビューの周回記録をここへ追記する。新規指摘ゼロが2周連続するまで確定としない。）

### 第1周（2026-11-27、独立レビュー）

- 新規指摘15件。要旨: (1) `has_software` 廃止に要件 ID が無い、(2) ゴールデン変更「2点のみ」が自書の要件群と矛盾、(3) Comment の順位変更が不明確、(4) MJ の XMP のみ捕捉が代替条件なしに消えている、(5) A1111 JPEG の捕捉が弱1票で落ちる／P-02・P-15 が成立条件を固定していない、(6) 弱の複合に二重計数と方向跨ぎの自己矛盾、(7) Stealth ゲートの実質レコード定義が未列挙の種別で未定義、(8) F-04 が部分修正であることが無留保に閉じられている、(9) F-16 の64KB側が判断固定なのに「修正」表記で固定テストも無い、(10) rdf:about 優先／WebP EXIF プレフィクス／歩行停止前のレコード保持にテストが無い、(11) フェーズ完了条件の相互矛盾（N-11、U-5、U-4 の期限）、(12) REQ-COL-06 の終了コード未定義と REQ-TRN-03 の集計整合、(13) ツール名表に監査に無い `ComfyUI`、(14) 非AI名の無効化の読み替えが判断として明示されていない、(15) 参照・根拠の誤り（REQ-FIX-16、§2.1 の注記、H-3 の誤用）。
- 全15件を本書へ反映した: REQ-CLS-11 の新設と対応表への独立行追加、ゴールデン変更の列挙への改訂、検証済み Comment の手順5への移動、U-7／P-18 の新設、`a1111_text_block`／`trailing_a1111`（中）の定義、Stealth 弱の1種類への畳み込みと方向跨ぎの廃止、Stealth ゲートの全種別表、F-04 の部分修正＋判断の明記、F-16 の処置分割と T-36、T-37〜T-39 の追加と欠番の説明、Phase 0／1 完了条件の整合、終了コード2の確定と集計の数え方の定義、ツール名表から `ComfyUI` を除去、読み替え判断の §13.1 への明記、参照・根拠の訂正。

### 第2周（2026-11-27、独立レビュー）

- 新規指摘10件。要旨: (1) Stealth 構造判定のキー集合が未決事項にも無い、(2) 弱シグナルの方向（バケット）が未定義で手順 W が実行できない種類がある、(3) `Negative prompt:` のみの弱に対応するシグナル名が無い、(4) P-18／U-7 の実装先フェーズと受け入れ条件が無い、(5) N-02 の対応 F 表記が §2.3 と §10.1 で食い違う、(6) REQ-DSP-01 に「変更はこの点のみ」が残る、(7) §6 の集計定義の集合表記が自己矛盾、(8) REQ-EXT-04 (a) の範囲が曖昧で T-37 と不一致、(9) `trailing_a1111` の中への格上げが §13.1 と緊張し理由も無い、(10) 版上げ方針が Phase 2 にしか無く Phase 3 に対応していない。
- 全10件を本書へ反映した: U-8（Stealth 構造のキー集合）の新設、弱シグナルへの方向の明記（WebUI／ComfyUI／NovelAI の3方向、手順 W も3方向化）、`negprompt_in_text` の追加、P-18 の Phase 1 実装と §12 への条件付き追加、N-01〜N-03 の対応付け訂正、REQ-DSP-01 の文言改訂、§6 の和の定義の書き直し（5種の和、警告は内訳）、REQ-EXT-04 (a) の全フィールド共通化、`trailing_a1111`（中）の廃止（第1周の定義を覆し、Trailing は全体で弱1票に統一。P-15 を複合形に書き換え P-15b を追加）、§9.3 の版上げ原則のフェーズ一般化と Phase 3 完了条件への追加。

### 第3周（2026-11-27、独立レビュー）

- 新規指摘7件。要旨: (1) REQ-CLS-02 (f) と Phase 1 で P-18 の扱いが矛盾、(2) 手順 W の ComfyUI 方向の複合が Stealth ゲート定義により到達不能、(3) P-16 が U-8 の見分け不可分岐で宙に浮く、(4) パース成功だが構造集合に一致しない Stealth 本文の扱いが未定義、(5) Stealth ゲートの種別表へのテストが P-16 しかない、(6) §9.3 が REQ-EXT-06 を Phase 3 の例に挙げているが Phase 2 所属、(7) 監査 §7 項目3（本文ハッシュによる統合表示）に処置もスコープ外宣言もない。
- 全7件を本書へ反映した: REQ-CLS-02 (f) を Phase 1 の扱いに統一し §1.3 を条件付きに改訂、手順 W から ComfyUI 方向の複合を外し（到達不能な規則を置かず、記録のみとする理由を §8.3 に明記）N-13 を追加、P-16 の期待値を U-8 依存として書き換え Phase 0 の書き換え対象に追加、Stealth の部分一致フォールバックを「構造に一致しない場合」と定義、T-40（ゲート種別表の固定）を追加し Phase 2／§12 へ接続、§9.3 の Phase 3 の例から REQ-EXT-06 を除去、統合表示を §3.2 で「扱わない」と明記。

### 第4周（2026-11-27、独立レビュー）

- 新規指摘8件。要旨: (1) §8.1 の `comfyui 系` に Stealth 構造判定（ComfyUI 側）が無く P-10 が到達不能、(2) Phase 0 の「P-10 を弱の複合で書き換える」が実行不能、(3) CreatorTool が AI名に一致する正例が無く中シグナルが固定されていない、(4) ツール名表を固定するテストに T 番号が無い、(5) T-28 が `.` と終了コード2をカバーしていない、(6) §3.2 の C2PA の参照先が §7.3 で誤り、(7) §2.3 対応表が追加テスト（N-13／P-15b／T-40）に未追随、(8) ゴールデン出力の許容列挙が Phase 3 の正当な出力変化をカバーしきれていない。
- 全8件を本書へ反映した: `comfyui 系` へ Stealth 構造判定（ComfyUI）を追加、Phase 0 の書き換え指示を P-09 と P-10 で分けた（P-10 は期待値 None＋既知の制限として記録）、P-19（CreatorTool の AI名のみ → WebUI）を追加し Phase 1／§12／§2.3 へ接続、T-41（ツール名表の固定）を追加、T-28 に `.` と終了コード2を追加、§3.2 の参照を §7.2 に訂正、§2.3 の F-02／F-05／F-08／F-09 行を更新、ゴールデン変更の扱いを「列挙のみ」から「要件 ID に追跡できる変更のみ可＋フェーズ記録で列挙する」原則に改めた（§0-6／§9.2／§12／§13.1）。

### 第5周（2026-11-27、独立レビュー）

- 新規指摘5件。要旨: (1) §2.2 の弱の定義が ComfyUI 方向の例外を反映していない、(2) REQ-CLS-02 (d)（Draw Things）の Phase 1 での扱いが明示なしに宙に浮いている、(3) Comment の順位変更に伴う強シグナル同士の競合時の行き先変更が §13.1 に無く固定フィクスチャも無い、(4) §1.3 の処置対応付けの参照先が判断固定の実在箇所と合っていない、(5) Phase 0 の完了条件に U-5 の確認結果の記録が含まれていない。
- 全5件を本書へ反映した: §2.2 の弱の定義に ComfyUI 方向の例外を明記、REQ-CLS-02 (d) を Phase 1 では未発動・Phase 2 で有効化と明示（Phase 1 の内容欄にも追記）、§13.1 の意図した変更に (5) 競合時の行き先変更を追加し P-20／P-21 で固定、§1.3 の参照先を §3.2／§7.2／§7.3／§13.1 に訂正、Phase 0 の完了条件に U-5 の結果記録を追加。

### 第6周（2026-11-27、独立レビュー）

- 新規指摘4件。要旨: (1) §2.3 の F-01 行が P-20／P-21 に未追随、(2) §13.1 の「弱シグナルの扱い」の決定文が ComfyUI 方向の例外を反映していない、(3) `negprompt_in_text` に対応するフィクスチャ／テストが無い、(4) 手順 W で WebUI 方向と NovelAI 方向の複合が同時成立した場合の優先が根拠もフィクスチャもなくコード順だけで決まっており、現行 decide（5b の NovelAI が 6 の WebUI より上）からの静かな変更にもなっている。
- 全4件を本書へ反映した: §2.3 の F-01 行へ P-20／P-21 を追加、§13.1 の弱シグナルの扱いに ComfyUI 方向の例外と同点規則を明記、N-14（`Negative prompt:` のみ → None）と P-22（`negprompt_in_text` ＋ `trailing_weak` の複合 → WebUI）を追加し Phase 1／§12／§2.3 へ接続、手順 W に同点規則を新設した（同時成立時は NovelAI 優先。根拠は現行の相対順序の維持と、NovelAI 方向の票が固有部分一致で具体性が高いこと。評価順自体を NovelAI 先行に組み替え、§13.1 の意図した変更に (6) として明記し P-23 で固定）。

### 第7周（2026-11-27、独立レビュー）

- 新規指摘4件。要旨: (1) Dream の値検証に要件 ID が無く、§2.3 にも Phase 1 の要件範囲にも現れないのに Phase 1 の完了条件が P-07 を要求している、(2) §2.3 の検証欄の未追随が残っている（F-01 行に N-12 が無い、F-06 行に N-11 が無い、F-08 行に P-22 が無い）、(3) N-05 が分類期待と表示期待を束ねたまま Phase 1 の完了条件に入っており表示側は Phase 1 では満たせない、(4) REQ-EXT-05 (a) の「IFD2 以降のタグ読み」を固定するテストが無い。
- 全4件を本書へ反映した: REQ-CLS-12（Dream の値検証。REQ-CLS-01 と同型の処置で Phase 1 実装）を新設し §2.3 の F-01 行・§8.1・Phase 1 の内容欄へ接続、§2.3 の F-01 行へ N-12 と P-07／P-17、F-06 行へ N-11、F-08 行へ P-22 を追加、N-05 の期待を分類側（Phase 1）と表示側（Phase 2 の G-01）に分離し Phase 1 の完了条件に注記、T-42（IFD2 以降のタグ／IFD2 の ExifIFD）を追加し §2.3 の F-28 行と Phase 3 の完了条件へ接続。

### 第8周（2026-11-27、独立レビュー）

- 新規指摘2件。要旨: (1) §2.3 と §10 の双方向対応付けに未追随が残っている（F-02 行に P-11 が無い、Software 値不問行に N-01／N-03 が無い、付随事項 (a) の REQ-EXT-16 に検証 T-38 が対応付けられていない）、(2) P-13 のフェーズ帰属が曖昧で、REQ-CLS-02 (a)/(b)（XMP 強宣言）には Phase 1 で対応する正例が割り当てられていない（P-13 本体は Phase 1 で到達可能なのに Phase 2 の完了条件にしか現れない）。
- 全2件を本書へ反映した: §2.3 の F-02 行へ P-11、Software 値不問行へ N-01／N-03 を追加し、付随事項 (a) に T-38 を対応付けた。P-13 を本体（Phase 1 で固定）と変種（tEXt 格納 PNG、REQ-EXT-06 が前提のため Phase 2 で固定）に分け、§10.2 の P-13 行・Phase 1／Phase 2 の完了条件に明記した。

### 第9周（2026-11-27、独立レビュー）

- **新規指摘ゼロ。** §2.3 と §10 の双方向対応付けを第8周で確立した基準で全件（N-01〜N-14、P-01〜P-23、T-06〜T-42、G-01／G-02）突き合わせ、一致を確認。フェーズ帰属（P-13 の本体／変種分割、REQ-CLS-12 の接続）も一貫。その他の観点（監査 §11 の方向8項目、健全リスト、内部整合、事実の発明の有無、渡し先名、番号体系・参照）も問題なし。本書の修正は不要だったため、本文は変更していない。

### 第10周（2026-11-27、独立レビュー）

- 新規指摘1件: P-16 が Phase 1 の完了条件の範囲指定（P-15／P-15b〜P-17）に含まれているが、その主目的（Stealth ゲートが閉じないこと）は前提の REQ-EXT-01 が Phase 2 にあり、Phase 1 では満たせないか、満たせても空虚になる。N-11／N-05／REQ-CLS-02 (d)／P-18 には切り分け注記があるのに P-16 には無かった。
- 本書へ反映した: Phase 1 の完了条件の範囲から P-16 を外し（P-15／P-15b、P-17 と明示）、P-16 は Phase 2 で確定させる旨と理由を注記した。P-16 の本来の帰属は Phase 2 の完了条件と §12 の REQ-EXT-01 のチェック項目であり、そちらは変更していない。これにより新規ゼロの連続は第9周の1周で途切れ、第11周・第12周での2周連続ゼロを改めて目指す。

### 第11周（2026-11-27、独立レビュー）

- **新規指摘ゼロ。** 第10周修正（P-16 の Phase 1 完了条件からの除外）の正しさを確認したうえで、§2.3 と §10 の双方向対応付けと全フィクスチャのフェーズ帰属（Phase 1〜5 の完了条件と前提要件の一致）を全件突き合わせ、一致を確認。その他の観点も問題なし。本書の修正は不要だったため、本文は変更していない。

### 第12周（2026-11-27、独立レビュー）

- **新規指摘ゼロ。** §2.3 と §10 の双方向対応付け（逆方向の一致を含む）と全フィクスチャのフェーズ帰属を全件再確認し、その他の観点も問題なし。本書の修正は不要だったため、本文は変更していない。
- 第11周・第12周で **新規指摘ゼロが2周連続** したため、本書は確定とする。全12周の新規指摘数の推移: 15 → 10 → 7 → 8 → 5 → 4 → 4 → 2 → 0 → 1 → 0 → 0。
