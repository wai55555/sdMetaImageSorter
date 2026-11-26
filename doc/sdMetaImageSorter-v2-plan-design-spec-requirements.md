# sdMetaImageSorter v2.0.0 / fast_meta v2.1.0 — Plan / Design / Specification / Requirements

| 項目 | 内容 |
|---|---|
| 対象リポジトリ A | https://github.com/wai55555/checkImageMetadata （`fast_meta`、現行 version 2.0.0） |
| 対象リポジトリ B | https://github.com/wai55555/sdMetaImageSorter （`sd_meta_sorter`、現行 package version 1.1.0 / `--version` 表示 4.4） |
| 本書の目的 | fast_meta のライブラリ化＋形式追加（v2.1.0）と、それを取り込む sdMetaImageSorter v2.0.0 の製作を、追加質問なしに実装できる粒度で定義する |
| ゴール | 画像1枚ごとの `fast_meta.exe` 子プロセス起動を廃止し、プロセス内ライブラリ呼び出しに置換することで、仕分け処理を **5倍以上** 高速化する（実測で確定させる）。あわせて仕分け先を3分類から5分類へ拡張する |
| 優先順位の付け方 | P0 = 必ずやる / P1 = やる（ただしフィクスチャが用意できる場合） / P2 = 任意（やらなくてもリリース可） |

---

## 0. 前提指示（必読）

1. **2つのリポジトリをまたぐ作業である。** §11 のフェーズ順に進めること。リポジトリ A（fast_meta）の Phase 1〜2 が完了し `v2.1.0` タグが打たれるまで、リポジトリ B（sorter）の Phase 3 に着手しない。
2. **Phase 1（lib 化）は振る舞い不変のリファクタリングである。** fast_meta の CLI 出力・終了コード・エラー挙動を一切変えない。既存の `tests/golden_cli.rs` が**既存ケースの期待値を変更せずに**全て通ることが完了条件（新規ケースの追加は §10.1 で指示した分のみ可）。
3. **推測で形式仕様を作らない。** 本書と §14 の参照実装（receyuki/stable-diffusion-prompt-reader、d3x-at/sd-parsers、いずれも MIT）に書かれたキー名・判定規則だけを実装根拠とする。本書にないツールの独自形式を新たに発明して対応しない。
4. **壊れた / 悪意のある入力で panic しない。** sorter 側ではファイル単位で `catch_unwind` を併用する（§8.4）が、fast_meta ライブラリ側も従来通り checked arithmetic と境界チェックを維持すること。
5. **分類ポリシーは sorter 側の責務である。** fast_meta ライブラリは「レコードを返す」までとし、生成元への分類（どのフォルダに入れるか）の判断を fast_meta 側に持ち込まない。
6. **既存の安全仕様を退行させない。** sorter の「整理先に同名ファイルがあれば上書きせずスキップ」「移動時はハードリンク優先、不可なら排他作成＋コピー」「旧フォルダ `webui_image` の移行」は現行挙動をそのまま維持する。
7. **段階的に実装する。** 各フェーズは独立に `cargo build --release --locked` と `cargo test --locked` が通る状態で終える。コミット規約は §11.3 に従う。

---

## 1. Plan（計画概要）

### 1.1 背景

現行 sorter は画像1枚ごとに `fast_meta.exe` を子プロセスとして起動し、その標準出力テキストをパースして分類している。実測上の支配項はメタデータ解析そのものではなくプロセス起動であり、Windows では概ね **5〜20ms/枚**（プロセス生成＋exe/DLL ロード＋アンチウイルス走査）に対し、fast_meta 本体の走査は **0.1〜0.5ms 級**である。1万枚規模では起動だけで50〜200秒を占める。

fast_meta は大規模改修を終えて v2.0.0 となり、内部は構造化レコード（`RecordView`）ベースに整理された。しかし現行ではまだ **binary crate のみ** で、抽出の入口関数が `src/main.rs` 内の private 関数であるため、sorter からライブラリとして利用できない。本計画は (a) fast_meta の小規模な lib 化と形式追加を v2.1.0 として先行させ、(b) sorter がそれを取り込んで v2.0.0 とする、2段構成で進める。

### 1.2 ゴール

- G1: sorter から子プロセス起動・標準出力パイプ・テキストパースを完全に排除し、fast_meta ライブラリのプロセス内呼び出しに置換する。
- G2: 同一コーパス・同一マシン比較で、sorter 全体の処理時間を現行比 **1/5 以下**（5倍以上高速）にする。Phase 0 でベースラインを、Phase 4 で結果を実測し PR に記録する。
- G3: 仕分け先を5分類に拡張する（ComfyUI / WebUI / NovelAI / **SwarmUI** / **InvokeAI**）。
- G4: 分類の等価性を担保する。旧構成（sorter v1 系＋fast_meta v2.0.0 exe）で仕分けできていた画像が、新構成で行き先を変えるのは §5.2 の分類表で意図した変更（SwarmUI / InvokeAI の専用フォルダ化、XMP のみの画像の WebUI 化、Midjourney の NovelAI 誤送防止、**Stealth 本文の workflow 検出を現行テストの期待へ是正すること（B-09）**）に限る。
- G5: 配布形態を単一 exe 化する（`fast_meta.exe` の同梱・同一フォルダ配置が不要になる）。

### 1.3 成功の定義

- §12 の受け入れチェックリストが全て満たされる。
- fast_meta v2.1.0 の CLI が v2.0.0 と同一のゴールデン出力を持つ（Phase 2 の形式追加による新規出力を除く）。

---

## 2. 現行実装の監査結果（実コード根拠）

### 2.1 fast_meta v2.0.0（リポジトリ A）

| ID | 箇所 | 内容 |
|---|---|---|
| A-01 | `Cargo.toml` | package `fast_meta` version 2.0.0。`[lib]` セクションが無く binary crate のみ。依存は `memmap2`、`serde_json`、`flate2`、`crc32fast`、`memchr`。`[profile.release]` は `lto = true, codegen-units = 1, strip = true` |
| A-02 | `src/main.rs` 冒頭 | `pub mod jpeg; pub mod keywords; pub mod output; pub mod png; pub mod record; pub mod tiff; pub mod xmp;` とモジュールは公開済み。lib 化はモジュール移設が中心で、ロジックの書き換えは最小で済む構造になっている |
| A-03 | `src/main.rs` | 抽出の入口 `extract_universal_metadata`、形式別 `extract_png_metadata` / `extract_webp_metadata` / `extract_exif_metadata`、`StealthPolicy`（enum、private）、`check_stealth_png` とその補助一式が全て `main.rs` 内の private 項目。ライブラリ利用側から到達できない |
| A-04 | `src/main.rs` `extract_universal_metadata` | 形式判定はマジックバイト（PNG 署名8バイト、`RIFF`+`WEBP`、`FF D8 FF`）で行い、`File::open`＋`MmapOptions::new().map()` で全ファイルをマップする。未対応形式は標準エラーに `Unsupported file format` を出して `Ok(())` で終わる。0バイトファイルは mmap エラーとなり伝播する |
| A-05 | `src/record.rs` | `RecordView<'a> { source: Source, label: Cow<'a, str>, body: Cow<'a, str>, keyword: Option<Cow<'a, str>>, detail: Option<Cow<'a, str>> }` と `trait MetadataSink<'a> { fn on_record(&mut self, record: &RecordView<'a>) }` が定義済み。`output::TextSink` がこの trait を実装している。**レコードは入力バイト列からの借用（`Cow::Borrowed`）を含み得る**ため、mmap を関数内部に閉じたまま `Vec<RecordView>` を返す API は寿命が成立しない（§7.1 の API 設計の根拠） |
| A-06 | `src/png.rs` | `pub fn scan(bytes: &[u8], strict: bool) -> PngScan { records, stats }`。Negative-First（Stage A/B/C）で全域走査する |
| A-07 | `src/keywords.rs` | PNG テキストキーワード登録表は14件：`parameters`（label: Stable Diffusion (A1111)）、`prompt`、`workflow`、`generation_data`、`Description`（NovelAI [Prompt]）、`Comment`（NovelAI [Settings]、json_settings）、`Title`、`Software`、`Source`、`sui_image_params`（SwarmUI [Params]）、`sui_extra_data`（SwarmUI [Extra]）、`invokeai_metadata`（InvokeAI）、`sd-metadata`、`XML:com.adobe.xmp`（xmp_packet）。登録表が走査と lookup の単一の出典であることをテストで固定済み |
| A-08 | `src/tiff.rs` | EXIF 取得タグ：UserComment (0x9286)、ImageDescription (0x010E)、Make (0x010F)、Model (0x0110)、Software (0x0131)、Artist (0x013B)、XPComment (0x9C9C)、XMP パケット (0x02BC)。各レコードは keyword にタグ名（`Software`、`Model` 等）を持つ。**SwarmUI 旧形式（EXIF Model 内の `sui_image_params` を含む JSON）も Model レコードとして既に取得できる** |
| A-09 | `src/xmp.rs` | XMP 取得フィールド：Description、Title、UserComment、CreatorTool、HasExtendedXMP、DigitalSourceType、AIPromptInformation。UserComment が Draw Things 形式 JSON（キー `c` を含む）の場合は label が `Draw Things (XMP)` になる |
| A-10 | `src/output.rs` | `TextSink::flush` には「他の SD 系レコードが無い XMP Description を `Midjourney (XMP)` とラベルし直す」処理がある。**このラベル付けは出力層の都合であり、ライブラリのレコード段階では label は `XMP Description` のままである**ことに注意（分類は原則として source と keyword で行い、label で判定するのは JPEG COM のみ。§8.2） |
| A-11 | `src/main.rs` Stealth | `StealthPolicy::{Auto, Always, Never}`。Auto の正確な抑制条件は `has_normal_metadata` が対象とする source の列挙で決まっており、**Trailing / C2PA のみのレコードでは Stealth は抑制されない**。「通常メタデータが無い場合のみ」は概略に過ぎないため、Phase 1 では `has_normal_metadata` のロジックをそのまま移設すること（再解釈して再実装しない）。CLI フラグ `--stealth <auto\|always\|never>` と `--strict` が既にある |
| A-12 | `src/jpeg.rs` `jpeg_comment_record` | JPEG COM セグメントのレコードは **keyword = None** で、A1111 形式かどうかの情報は label にのみ載る（本文に `Negative prompt:` / `Steps:` を含めば label = `Stable Diffusion (A1111)`、他は `JPEG Comment`）。分類で使うには keyword ではなく source と label を見る必要がある（§8.2） |
| A-13 | `src/extract` 相当（現行 `extract_universal_metadata`） | 未対応形式のとき関数内から `eprintln!("Unsupported file format")` を直接出している。lib 化ではこの出力を lib から除去し、bin 側の責務とする（§7.4 の Phase 1 例外） |
| A-14 | `src/jpeg.rs` / `src/png.rs` / WebP 抽出 | 形式別抽出はいずれも sink 駆動ではなく `Vec<RecordView>` を返す形である（`png::scan -> PngScan`、`jpeg::scan -> JpegScan`、`webp_records -> Vec<RecordView>`）。`MetadataSink` を実装するのは `TextSink` のみで、投入も `on_record` ではなく `push_record` 経由である。したがって `scan_file` は「返却されたレコード列を順に `RecordSink` へ渡す」形が実態に合う（FR-LIB-04） |
| A-15 | `src/png.rs` `append_trailing` | IEND 後方の Trailing レコードは、登録キーワードを含む場合は **keyword = Some のまま**生成される（例：`workflow`、`parameters`）。keyword = None なのは Trailing の JSON レコード等のみ。ただし **label は keyword によらず固定で `Trailing Data (after IEND)`** で、keyword は `detail` 側に入るため、旧 sorter の見出しルール（`--- workflow ---` 等）は Trailing 由来では**決して発火しなかった**。旧構成で Trailing が分類に効いたのは本文部分文字列ルール（`parameters`、`Stable Diffusion`、`Software`、`Steps: `、小文字 `novelai`）だけである。分類での扱いは §8.2 / §8.3 でこの範囲に限定する |

### 2.2 sdMetaImageSorter 現行版（リポジトリ B）

| ID | 箇所 | 内容 |
|---|---|---|
| B-01 | `src/main.rs` `process_image` | **画像1枚ごとに `Command::new(fast_meta_path)` で `fast_meta.exe` を起動**し、`output()` で標準出力を全て受け取り、`classify_metadata(&stdout)` で分類する。本計画で排除する支配項 |
| B-02 | `src/main.rs` `classify_metadata` | 判定は出力テキストへの部分文字列一致：`--- NovelAI [` → NovelAI、`--- workflow ---` / `--- generation_data ---` / `--- prompt ---` → ComfyUI、小文字化して `novelai` を含む → NovelAI、`parameters` / `Stable Diffusion` / `Software` / `Steps: ` / `--- Stealth PNG Info (` のいずれか → WebUI、それ以外 → None。全文のコピー（`collect::<Vec<_>>().join("\n")` と `to_ascii_lowercase()`）を作るため ComfyUI の巨大 workflow ではアロケーションも支配的になる |
| B-03 | `src/main.rs` `main` | 起動時に自 exe と同一フォルダの `fast_meta.exe` の存在をチェックし、無ければエラー終了する。v2.0.0 でこの確認自体が不要になる |
| B-04 | `Cargo.toml` | package 名 `sd_meta_tools`、version 1.1.0。ただし `#[command(version = "4.4")]` とハードコードされており package version と食い違う。依存の `memmap2` / `serde` / `serde_json` は `main.rs` から参照されておらず未使用。`[profile.release]` 未設定 |
| B-05 | `src/main.rs` 全体 | 旧フォルダ移行（`webui_image` → `webui_img`、同名時は `__legacy_N` 連番で保持）、WalkDir による走査（出力フォルダは `filter_entry` で除外）、Rayon `par_iter` による並列処理、分類ごとの AtomicUsize 集計、エラーが1件でもあれば終了コード 1 |
| B-06 | `src/main.rs` `transfer_no_replace` | 移動は hard_link＋元ファイル削除を優先し、不可なら `create_new` で排他作成してコピー後に元を削除。コピーは排他作成＋コピー。同名先行ファイルは絶対に上書きしない。既存テストは**関数単位で数える**：transfer 系2件（上書き保護、並行コピー競合）＋分類2関数（複数観点を assert で検証） |
| B-07 | `src/main.rs` `process_image` | 仕分け対象ファイルごとに `fs::create_dir_all(dest_dir)` を呼ぶ（冗長だが v2.0.0 のスコープでは触らない。§3.2） |
| B-08 | 対応拡張子 | `png, jpg, jpeg, webp, avif`。ただし fast_meta は AVIF 未対応で、現行でも AVIF は起動コストだけ払って常にスキップされる |
| B-09 | `src/main.rs` 分類テスト（既知の不一致） | テスト `classifies_stealth_and_ignores_the_input_path` は Stealth 本文 `"ComfyUI workflow"` → ComfyUI を期待するが、現行 `classify_metadata` の実装を辿ると WebUI 段に落ちるため、**現行テストは現行実装に対して確定的に赤である**（仕様書レビュー第3周で実コードを辿って確認）。Phase 0 で現行 `cargo test` の実測でも裏付けて記録すること。本書の §8.3 はテストの期待を正とし、この点は「現行バイナリの実挙動からの是正（意図した変更）」として G4 に含める |

### 2.3 分類に関する既知の事実（外部照合で確認済み）

| ID | 事実 | 出典 |
|---|---|---|
| F-01 | PNG の `Description` キーは **NovelAI と Midjourney の両方** がプロンプト格納に使う。Description 単独で NovelAI と確定させてはならない | Midjourney 公式ダウンロードのメタデータ実態（Description / Software=Midjourney / Author 等）。§2.1 A-07 の通り fast_meta の label は便宜上 `NovelAI [Prompt]` だが、これは表示名に過ぎない |
| F-02 | NovelAI の PNG には `Comment`（設定 JSON）と Software 本文 `NovelAI` が伴う。`Comment` キーは他ツールの使用例が無く NovelAI の確定材料になる | NovelAI 形式仕様、receyuki の NovelAI パーサ |
| F-03 | Midjourney は旧 sorter でも仕分けされていた。行き先は専用フォルダではなく **webui_img** で、旧分類の「`Software` を含めば WebUI」ルールに Software=Midjourney が当たっていたため。Discord 経由で再エンコードされたファイルはメタデータが剥がれて素通りする | B-02 の分類コード、Midjourney ダウンロード形式 |
| F-04 | `sd-metadata` キーを書き込むのは **InvokeAI のみ**（旧形式、v1.15〜2.3.5-post.2。後継は `invokeai_metadata`、最古は `Dream`） | receyuki `format/invokeai.py` の世代表、d3x-at/sd-parsers の InvokeAI パーサ |
| F-05 | InvokeAI 最古形式（〜1.15）のキーは `Dream`。本文は `"プロンプト" -s 30 -S <seed> -C <cfg> -A <sampler> -W <w> -H <h>` 形式の文字列 | receyuki `format/invokeai.py` `_invoke_dream` |
| F-06 | Easy Diffusion は PNG テキストとして設定キー群（`prompt`、`negative_prompt`、`seed`、`use_stable_diffusion_model`、`use_vae_model`、`sampler_name`、`num_inference_steps`、`guidance_scale`、`clip_skip`、`width`、`height`）を書き込む。このうち **`use_stable_diffusion_model` は Easy Diffusion 固有**で他ツールの使用例が無い。また `prompt` キーの本文は ComfyUI と違い JSON ではなく平文プロンプトである | receyuki `format/easydiffusion.py` のマッピング表 |
| F-07 | SwarmUI は現行形式では `sui_image_params` / `sui_extra_data` キー、または `parameters` 本文 JSON 内の `sui_image_params` キーで識別できる。旧形式（0.5.8-alpha 以前）は EXIF Model タグ内の JSON に `sui_image_params` を含む | receyuki の SwarmUI 判定（検出順序表）、A-08 |
| F-08 | Fooocus は A1111 互換の `parameters` を書くため、現行分類でも webui_img に入る。専用フォルダは作らない（§13 決定事項） | d3x-at/sd-parsers の Fooocus パーサ |
| F-09 | Civitai 生成画像（JPEG/PNG）は EXIF UserComment 内の JSON にワークフロー等を含む場合がある。UserComment 本文 JSON に `"workflow"` を含むものは ComfyUI 由来として扱える | receyuki 開発版の ComfyUI JPEG/WebP 対応 |
| F-10 | NovelAI の JPEG / WebP では、メタデータが EXIF UserComment 内の JSON（キー `Description` と `Comment` の両方を持つ梱包形式）として埋め込まれる。fast_meta の TIFF 処理はこの JSON を pretty 整形して返すため、整形後は `"Comment": ` のようにコロンの後に空白が入るが、部分文字列 `"Comment":` / `"Description":` は整形後もそのまま成立する | 現行 sorter が分類対象としてきた形式（§5.2 の NovelAI 行）、fast_meta `tiff.rs` の JSON pretty 整形 |

---

## 3. スコープ

### 3.1 スコープ内

**リポジトリ A（fast_meta v2.1.0）**
- lib 化：`src/lib.rs` の新設、抽出ロジックのライブラリ移設、公開 API（§7）の提供。CLI はライブラリを使う薄い bin にする（振る舞い不変）
- 形式追加：PNG キーワード `Dream` の登録（FR-FMT-01）、Easy Diffusion マーカー `use_stable_diffusion_model` の登録（FR-FMT-02）
- README へのライブラリ利用方法の追記、version 2.1.0 への更新とタグ `v2.1.0`

**リポジトリ B（sdMetaImageSorter v2.0.0）**
- fast_meta v2.1.0 への依存追加と、子プロセス起動経路の全面撤去
- 分類ロジックのレコードベース化（§8）と5分類化、CLI オプション2件の追加
- version 統一（package 2.0.0、`--version` は `CARGO_PKG_VERSION` 参照）、`[profile.release]` の追加、未使用依存の整理
- README の更新（単一 exe 配布、5フォルダ、オプション一覧）
- 等価性テストとベンチマーク（§10）

### 3.2 スコープ外（実装しない）

- sorter の走査・移動まわりの最適化（WalkDir の二重走査解消、`create_dir_all` の集約、jwalk 化、2フェーズ化）。効果の切り分けのため v2.0.0 には混ぜない。§11 Phase 5（P2、任意）としてのみ置く
- sorter への `--strict` / `--stealth` フラグの追加（ライブラリ API としては渡せる形にするが、CLI 露出は v2.0.0 ではしない）
- Fooocus / Easy Diffusion / Draw Things / Midjourney の**専用フォルダ**新設（行き先は §5.2 の通り webui_img のキャッチオール）
- fast_meta への新規パーサ追加（形式追加はキーワード登録レベルの2件のみ。Easy Diffusion の全キー表示対応は P2 として §4.3 に記録のみ）
- AVIF の実対応（拡張子リストには残し、ライブラリが未対応を返したらスキップする現行相当の挙動を維持する）
- メタデータの書き込み・削除機能、GUI 連携、ネットワークアクセス

---

## 4. 要件 — リポジトリ A（fast_meta v2.1.0）

### 4.1 ライブラリ化（FR-LIB）

| ID | 優先度 | 要件 |
|---|---|---|
| FR-LIB-01 | P0 | `src/lib.rs` を新設し、`jpeg / keywords / output / png / record / tiff / xmp` の各モジュールをライブラリ側から `pub` で公開すること。既存のモジュールファイル自体は原則移動しない |
| FR-LIB-02 | P0 | 抽出の入口をライブラリ API として公開すること（§7.2）。現行 `main.rs` private の `extract_universal_metadata` と形式別抽出、Stealth 判定（`check_stealth_png` 一式、`StealthPolicy`）をライブラリ側へ移設し、bin から呼ぶ形にすること。あわせて CLI の出力経路として `extract_to_writer`（見出し・案内文言・TextSink の蓄積と flush を含む現行手順を、mmap と同じフレーム内で行う lib 内に閉じた関数）を公開し、bin はそれを呼ぶこと（§7.2 の設計注意） |
| FR-LIB-03 | P0 | **CLI の振る舞いを一切変えないこと。** 標準出力の見出し・本文・順序、標準エラーへの出力、終了コード、引数仕様（`[--strict] [--stealth <auto\|always\|never>] <image_file>`）は v2.0.0 と同一。`tests/golden_cli.rs` が既存ケースの期待値を変更せずに通ること（新規ケースの追加は §10.1 の指示分のみ可） |
| FR-LIB-04 | P0 | ライブラリ API は §7.1 の理由により **sink（コールバック）方式を主 API** とすること。公開 sink trait は、寿命を trait ではなくメソッド側に置く `RecordSink`（`fn on_record(&mut self, record: &RecordView<'_>)`）として新設すること。現行の `MetadataSink<'a>` は TextSink が「自分の寿命と一致する実装しか持てない」ため `for<'a>` の dyn として外部に渡せず、**bin の TextSink を `scan_file` に直接渡す形は成立しない**（実コード照合で確認済み）。`scan_file` は形式別走査が返したレコード列（A-14）を順に `RecordSink` へ渡す形とし、`MetadataSink` は TextSink（`extract_to_writer` 内）専用の内部 trait として残すこと。`RecordView` の借用が呼び出し中のみ有効であることを API のドキュメントコメントに明記すること |
| FR-LIB-05 | P0 | ライブラリ API は「未対応形式」と「入出力エラー」を区別して返すこと（§7.3）。未対応形式は sorter 側でスキップ扱いになるため、エラーと混同してはならない |
| FR-LIB-06 | P1 | 参考 API として、所有権付きレコードを返す `scan_file_owned`（仮称）も用意してよい。実装は主 API の sink に所有化変換（`xmp::parse_owned` に前例のある `Cow::Owned` 化）を被せる形とし、走査ロジックを二重化しないこと |
| FR-LIB-07 | P0 | `Cargo.toml` に `[lib]`（name = `fast_meta`、path = `src/lib.rs`）と `[[bin]]`（name = `fast_meta`、path = `src/main.rs`）を明記すること。**version の 2.1.0 への更新は Phase 2 で行う**（Phase 1 ではセクション追加のみで、振る舞い不変の原則に従う）。既存の feature（`zlib-ng` 既定 / `portable-zlib`）と `[profile.release]` は維持すること |
| FR-LIB-08 | P1 | README にライブラリとしての利用方法（依存の書き方と sink の最小例）を追記すること |

### 4.2 形式追加（FR-FMT）

| ID | 優先度 | 要件 |
|---|---|---|
| FR-FMT-01 | P0 | PNG キーワード登録表に `Dream` を追加すること。label は `InvokeAI (Legacy)`、json_settings = false、xmp_packet = false とする。登録表の件数を固定している既存テスト（14件）は15件に更新すること。根拠は F-04 / F-05 |
| FR-FMT-02 | P0 | PNG キーワード登録表に `use_stable_diffusion_model` を追加すること。label は `Easy Diffusion`、json_settings = false、xmp_packet = false とする。登録表は16件になる。根拠は F-06。**このキーは分類用マーカーであり、本文の表示整形は行わない**（生テキストのままレコード化する） |
| FR-FMT-03 | P0 | FR-FMT-01 / 02 の追加はキーワード登録のみで行い、走査ロジック（Stage A/B/C）には手を入れないこと。ゴールデンテストには新規2形式のケースを追加すること（§10.1） |

### 4.3 記録のみ（本件では実装しない）

| ID | 内容 |
|---|---|
| FR-FMT-X1 | Easy Diffusion の全設定キー（`negative_prompt`、`use_vae_model`、`num_inference_steps` 等）の登録と表示整形。表示の完全性のための拡張であり、分類には FR-FMT-02 のマーカーで足りるため別件とする |
| FR-FMT-X2 | RuinedFooocus の形式対応（receyuki 開発版のみ対応）。利用実態が限られるため別件とする |

---

## 5. 要件 — リポジトリ B（sdMetaImageSorter v2.0.0）

### 5.1 統合（FR-INT）

| ID | 優先度 | 要件 |
|---|---|---|
| FR-INT-01 | P0 | 依存に `fast_meta = { git = "https://github.com/wai55555/checkImageMetadata", tag = "v2.1.0" }` を追加すること。feature は fast_meta の既定（`zlib-ng`）のまま使う（§13.1 決定事項）。**これにより sorter のビルドに C コンパイラが必要になる**（従来は純 Rust のみでビルドできた点が変わるため、README のビルド要件に明記すること）。`Cargo.lock` をコミットし、ビルドは `--locked` で再現可能にすること |
| FR-INT-02 | P0 | `fast_meta.exe` の存在チェック、`Command` による子プロセス起動、標準出力テキストのパース（現行 `classify_metadata`）を**全て削除**すること。判定条件は「`std::process::{Command, Stdio}` への参照が `main.rs` から消えること」とする（終了コード用の `std::process::exit` は FR-CLI-05 の現行構造を維持するため残る） |
| FR-INT-03 | P0 | 分類は §8 の `ClassifierSink` でレコードを受けて行うこと。走査オプションは既定値（strict = false、stealth = Auto）とし、現行の fast_meta 既定挙動と一致させること |
| FR-INT-04 | P0 | ライブラリ呼び出しはファイル単位で `std::panic::catch_unwind`（`AssertUnwindSafe`）で包み、1ファイルのパニックをそのファイルのエラーとして数え、残りの処理を継続すること（子プロセス時代の隔離性の代替） |
| FR-INT-05 | P0 | エラー意味論を現行と揃えること：走査の入出力エラー等はエラー集計し、1件でもあれば終了コード 1。**未対応形式はエラーではなくスキップ（None）** とする（現行は fast_meta が `Unsupported file format` を出して正常終了し、分類 None でスキップされていた。A-04 / B-08） |
| FR-INT-06 | P0 | package version を 2.0.0 にし、`#[command(version)]` のハードコード `4.4` を廃して `CARGO_PKG_VERSION` 由来にすること。package 名 `sd_meta_tools` は変更しない（差分最小化のため。§13 決定事項） |
| FR-INT-07 | P0 | `[profile.release]` に `lto = true, codegen-units = 1, strip = true` を追加すること。**依存クレート側の profile は適用されず、最上位クレートの設定だけが効く**ため、fast_meta 側にあるだけでは不十分である |
| FR-INT-08 | P0 | 未使用依存 `serde` / `serde_json` / `memmap2` を削除すること（ライブラリ API が mmap を内包するため sorter 側に不要。FR-LIB-04 の sink 方式を前提とする） |
| FR-INT-09 | P1 | 集計表示に SwarmUI / InvokeAI の行を追加し、合計が総数と一致すること |

### 5.2 分類（FR-CLS）— 分類表が正

分類は §8.3 の決定手順（上から順に最初に当たったもの）に従う。要点だけ抜き出した対応表：

| 行き先 | 決め手（レコードの条件） |
|---|---|
| `NovelAI_img` | PNG キーワード `Comment` がある／Software レコードの本文に `NovelAI` を含む／Stealth 本文に `novelai`（大小無視）を含む／WebP・JPEG の UserComment JSON に `"Comment":` と `"Description":` の両方を含む（NovelAI の梱包形式、F-10） |
| `SwarmUI_img` | キーワード `sui_image_params` または `sui_extra_data` がある／`parameters` レコードの本文に `"sui_image_params"` を含む／EXIF `Model` レコードの本文に `sui_image_params` を含む（旧形式） |
| `InvokeAI_img` | キーワード `invokeai_metadata`、`sd-metadata`、`Dream` のいずれかがある |
| `webui_img`（Easy Diffusion） | キーワード `use_stable_diffusion_model` がある |
| `comfyui_img` | キーワード `workflow` または `generation_data` がある／キーワード `prompt` があり本文が `{` で始まる JSON である／UserComment 本文に `"workflow"` を含む／Stealth 本文に `workflow` を含む |
| `webui_img` | キーワード `parameters` がある／UserComment または ImageDescription 本文に `Steps: ` を含む／Software レコードがある（上記で確定しなかったもの）／Stealth がある（生成元不明を含む）／JPEG COM が A1111 形式である（source = `JpegComment` かつ label = `Stable Diffusion (A1111)`、A-12）／**XMP レコードがある**（上記で確定しなかったもの。Midjourney、Draw Things 等のキャッチオール） |
| スキップ（None） | 上記のいずれにも当たらない、またはメタデータ自体が無い |

※ 本文チェックの制限（Software 本文は256バイト以下だけを複製対象とする、UserComment は先頭64KBまで等）は §8.2 が正であり、本表は要点のみを示す。Trailing 由来のレコードは旧本文ルール相当の範囲（§8.3 手順5b・6）でのみ効く。本文由来の NovelAI 条件（Software 本文／梱包形式／Stealth 本文）は、決定手順では ComfyUI の後（手順5b）で評価する（旧順序互換）。

| ID | 優先度 | 要件 |
|---|---|---|
| FR-CLS-01 | P0 | **キーワード `Description` 単独で NovelAI と確定させないこと**（F-01）。NovelAI の確定は上表の条件に限る |
| FR-CLS-02 | P0 | キーワード `prompt` 単独で ComfyUI と確定させないこと。本文が `{` で始まる JSON の場合に限る（F-06：Easy Diffusion の `prompt` は平文）。`workflow` / `generation_data` は JSON 前提のため本文チェック不要 |
| FR-CLS-03 | P0 | 判定はレコードの `keyword` / `source` / `label` と、限られたレコードの本文だけに基づくこと。**本文の全文コピーや小文字化コピーを作らない**（B-02 の廃止理由）。本文を複製してよいのは Software / Model 等の短いレコードに限る（§8.2 の上限規則） |
| FR-CLS-04 | P0 | Stealth レコード（source = `PngStealth`）の扱いは**現行テストの期待を正**とすること：本文に `novelai` を含めば NovelAI、`workflow` を含めば ComfyUI、それ以外（生成元不明）は WebUI。現行テスト2関数をレコードベースで再現すること。なお workflow 検出は現行バイナリの実挙動（WebUI に落ちる）とは異なる是正である（B-09、G4 の意図した変更に含む） |
| FR-CLS-05 | P0 | XMP レコード（source が `PngXmp` / `JpegXmp` / `WebpXmp` / `TiffXmp` のいずれか）しか無い画像は WebUI とすること（F-03 の旧挙動継承を含む、ユーザー決定）。Draw Things（label `Draw Things (XMP)`）も同じ行き先でよい |
| FR-CLS-06 | P1 | SwarmUI 旧形式（EXIF Model 内 JSON）と `parameters` 内包形式（F-07）を SwarmUI と判定すること |
| FR-CLS-07 | P1 | UserComment 本文 JSON に `"workflow"` を含む場合（Civitai 等、F-09）を ComfyUI と判定すること。本文チェックは UserComment レコードに限定し、先頭 64KB までで打ち切ってよい |
| FR-CLS-08 | P0 | 現行の分類単体テスト（2関数。観点は NovelAI 優先、Stealth、入力パス文字列に惑わされない、等）を、テキスト入力からレコード入力へ移植して維持すること。新規分類（SwarmUI / InvokeAI / XMP のみ / Description 単独の非確定）についても単体テストを追加すること（§10.2） |

### 5.3 CLI（FR-CLI）

| ID | 優先度 | 要件 |
|---|---|---|
| FR-CLI-01 | P0 | 既存オプションは全て維持すること：`-c, --comfy-dir-name`（既定 `comfyui_img`）、`-w, --webui-dir-name`（既定 `webui_img`）、`-n, --novelai-dir`（既定 `NovelAI_img`）、`--copy` |
| FR-CLI-02 | P0 | 新規オプションを追加すること：`-s, --swarm-dir-name`（既定 `SwarmUI_img`）、`-i, --invoke-dir-name`（既定 `InvokeAI_img`） |
| FR-CLI-03 | P0 | 走査除外リスト（現行 `is_current_output_name`）に SwarmUI / InvokeAI のフォルダ名（カスタム名を含む）を追加すること。追加しないと再実行時に仕分け済みフォルダを再走査する |
| FR-CLI-04 | P0 | 旧フォルダ移行の対象は現行通り `webui_image` のみとすること。新フォルダには旧名が存在しないため移行は追加しない |
| FR-CLI-05 | P0 | 終了コードの意味を現行と同一にすること：正常終了 0、エラーが1件以上あれば 1 |

---

## 6. 非機能要件

### 6.1 性能（NFR-PERF）

| ID | 要件 |
|---|---|
| NFR-PERF-01 | sorter 全体の処理時間（同一コーパス・同一マシン、移動モード、同ーボリューム）が現行比 1/5 以下であること。Phase 0 / Phase 4 で同一条件の実測値を PR に記録すること |
| NFR-PERF-02 | 分類のためにメタデータ本文全体を複製しないこと。例外は §8.2 で許可した短いレコードのみ。ComfyUI の workflow 本文（数十〜数百KB）は sink 内で参照だけし、確定に不要なら即座に捨てること |
| NFR-PERF-03 | ファイル単位の追加アロケーションは、レコード走査自体に必要なものを除き、分類では定数量（シグナルフラグ＋短い文字列数件）に抑えること |

### 6.2 堅牢性（NFR-ROB）

| ID | 要件 |
|---|---|
| NFR-ROB-01 | 壊れたファイル・0バイトファイル・権限のないファイルで全体が停止しないこと。当該ファイルのエラーとして集計し継続する（0バイトは現行 fast_meta が mmap エラーを返すため、エラー集計が現行互換。§7.3） |
| NFR-ROB-02 | 整理先の既存ファイルを上書きしないこと（現行 `transfer_no_replace` の意味論を無変更で維持） |
| NFR-ROB-03 | 並行実行時の競合でファイルが失われないこと（現行の並行コピーテストが通ること） |

### 6.3 保守性（NFR-MNT）

| ID | 要件 |
|---|---|
| NFR-MNT-01 | 分類表（§5.2）と実装の決定手順（§8.3）が1対1で追えること。手順の各段に分類表の行への参照コメントを付けること |
| NFR-MNT-02 | fast_meta への依存はタグ固定とし、`Cargo.lock` を正とすること。タグを上げた場合は分類テストを必ず再実行すること |

---

## 7. Design — リポジトリ A（fast_meta ライブラリ API）

### 7.1 API 方式の決定：sink 方式を主とする

`RecordView<'a>` は入力バイト列への借用を含む（A-05）。ライブラリが内部で mmap を開いて `Vec<RecordView>` を返すと、mmap の破棄と同時にレコードが宙に浮くため、この形の API は成立しない。取り得る形は次の3つで、本書は (1) を主 API とする：

1. **sink 方式（採用）**：ライブラリが mmap の生存期間内にレコードをコールバックで渡す。借用は呼び出し中のみ有効。大きな本文を複製せずに済み、分類のような「シグナルだけ拾って捨てる」用途に最適。ただし公開 trait は現行の `MetadataSink<'a>` をそのまま使わない。TextSink は自分の寿命と一致する実装しか持てず `for<'a>` を満たせないため、公開用に寿命をメソッド側へ置く `RecordSink` を新設し、形式別走査が返したレコード列を順に `RecordSink` へ渡す形にする（FR-LIB-04、A-14）。CLI の TextSink は sink として外部から渡さず、lib 内の `extract_to_writer` が mmap と同じフレームで使う
2. 所有化方式：全レコードを `Cow::Owned` 化して返す。本文の大きい ComfyUI 画像では複製コストが無視できないため、参考 API（FR-LIB-06）に留める
3. バイト列引数方式：呼び出し側が mmap して `&[u8]` を渡す。形式判定や Stealth の統合入口がライブラリ側にある現行構成とは噛み合わず、呼び出し側に形式知識が漏れるため不採用

### 7.2 公開 API 仕様

```rust
// src/lib.rs（新設）
pub mod jpeg;
pub mod keywords;
pub mod output;
pub mod png;
pub mod record;
pub mod tiff;
pub mod xmp;
pub mod extract; // 新設：現行 main.rs の抽出ロジックの移設先

pub use extract::{extract_to_writer, scan_file, RecordSink, ScanError, ScanOptions, ScanOutcome, Format, StealthPolicy};
pub use record::{RecordView, Source};
// MetadataSink<'a> は lib 内部用として record モジュールから参照する（公開 sink は RecordSink）
```

```rust
// src/extract.rs（骨子。シグネチャが正であり、内部実装は現行ロジックの移設でよい）
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StealthPolicy { #[default] Auto, Always, Never }

#[derive(Clone, Copy, Debug, Default)]
pub struct ScanOptions {
    pub strict: bool,            // 既定 false
    pub stealth: StealthPolicy,  // 既定 Auto
}

/// レコード受け取り用の公開 sink。寿命はメソッド側にだけ現れるため
/// `&mut dyn RecordSink` として渡せる。レコードの借用は呼び出し中のみ有効で、
/// 保持したい場合は on_record 内で所有化すること。
pub trait RecordSink {
    fn on_record(&mut self, record: &RecordView<'_>);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Format { Png, Jpeg, Webp }

#[derive(Clone, Debug, Default)]
pub struct ScanOutcome {
    pub format: Option<Format>,   // 未対応形式では None
    pub record_count: usize,      // sink へ渡したレコード数
    pub saw_exif: bool,           // JPEG: EXIF マーカーの有無（extract_to_writer 内部の文言出し分け用）
    pub comfy_tail_notice: bool,  // PNG: extract_to_writer 内部の案内文言用（現行 extract_png_metadata の条件と同一）
}

#[derive(Debug)]
pub enum ScanError {
    Io(std::io::Error),
    // 未対応形式はエラーにしない。ScanOutcome { format: None, .. } で表す
    // Display と std::error::Error を実装し、From<std::io::Error> を提供すること
}

/// path のファイルを mmap し、形式判定→形式別走査→（PNG のみ）Stealth 判定の順に
/// レコードを sink へ渡す。レコードの借用は本関数の呼び出し中のみ有効。
/// 形式別走査が返したレコード列（A-14）を順に sink へ渡す。
pub fn scan_file(
    path: &std::path::Path,
    options: &ScanOptions,
    sink: &mut dyn RecordSink,
) -> Result<ScanOutcome, ScanError>;

/// CLI 用：現行の形式別抽出関数の構造そのままに、**見出し・案内文言・レコード出力の
/// 全て**を mmap と同じフレーム内で行う。bin は引数パースと終了コードだけを担う。
pub fn extract_to_writer(
    path: &std::path::Path,
    options: &ScanOptions,
    out: &mut dyn std::io::Write,
) -> Result<ScanOutcome, ScanError>;
```

設計上の注意：

- 現行の形式別抽出は「レコード列を作ってから TextSink に積む」流れである。sink 方式では、レコード列が揃ってから順に `sink.on_record` を呼ぶ形でよく、走査ロジック自体は変えない。TextSink の重複排除・ラベル付け（A-10）は出力層（`extract_to_writer` 内）に残る
- PNG の Stealth は「通常レコードの有無」を見てから判定するため、現行通りレコード列の確定後に実行し、Stealth レコードを最後に sink へ渡す
- CLI（bin）は `extract_to_writer` を呼ぶだけとし、見出し（`=== PNG File: ... ===` 等）、`(No metadata detected)` / `(No EXIF marker found)`、PNG の tail 案内文言を含む**標準出力の全てを `extract_to_writer` 側で現行通り再現する**。例外は未対応形式の標準エラー出力 `Unsupported file format` のみで、これは bin が outcome の `format == None` を見て出す（§7.4）。見出しは形式が判明した後にしか書けず、tail 案内文言は見出し直後・レコード出力の前に挟まるため、bin 側で後から再現することはできない（現行の形式別抽出関数が内部で見出しを書く構造をそのまま移す）。**bin が `scan_file` に TextSink を直接渡す形にもしない**（寿命の都合で成立しない。FR-LIB-04）

### 7.3 エラーと未対応形式の扱い

| ケース | ライブラリの返却 | bin（CLI）の挙動 | sorter の扱い |
|---|---|---|---|
| 正常に走査できた | `Ok(outcome)`、format = Some | 現行通り出力 | レコードで分類 |
| 未対応形式（マジック不一致） | `Ok(outcome)`、format = None、record_count = 0 | 標準エラーに `Unsupported file format`、終了コード 0（現行互換） | スキップ（None） |
| ファイルが開けない / mmap 失敗（0バイト含む） | `Err(ScanError::Io)` | 標準エラーに `Error: ...`、終了コード 1（現行互換） | エラー集計（現行互換） |
| 途中で壊れた形式データ | 走査可能な範囲のレコードを返して `Ok`（現行パーサの「無効なチャンクで走査終了」等の方針を維持） | 現行通り | 得られたレコードで分類 |

### 7.4 モジュール移設の範囲

- `src/main.rs` から `src/extract.rs` へ移すもの：`extract_universal_metadata` の中身（形式判定＋mmap）、`extract_png_metadata`、`extract_webp_metadata` と `webp_records`、`extract_exif_metadata`、`StealthPolicy`、`check_stealth_png` とその補助型・定数一式
- **`#[cfg(test)] mod webp_tests` も `webp_records` と一緒に移設先（extract）へ移す**こと。bin 側に残すと `crate::record` 等が解決できずテストビルドが落ちる
- 移設に伴い `crate::` パスの付け替えと `pub` 化のみを行い、**ロジックの「ついで」の変更を混ぜない**こと（Phase 1 の原則）。ただし次の2点は Phase 1 の例外として許可する変更とする：(1) 未対応形式の `eprintln!("Unsupported file format")` を lib から除去し bin 側の責務に移す（A-13。残すと sorter で AVIF 等のたびに標準エラーを汚す）、(2) `RecordSink` と `extract_to_writer` の新設そのもの
- `src/main.rs` の冒頭にある `pub mod` 宣言群は削除し、bin からは `fast_meta::` 経由の参照に置き換えること（二重定義を避ける）
- `src/main.rs` に残すもの：引数パース、`extract_to_writer` の呼び出し、終了コードの決定、および**未対応形式時（戻り値の `format == None` のとき）に標準エラーへ `Unsupported file format` を出すこと**（見出しを含む標準出力は全て lib 側。この標準エラー1文言のみ bin の責務）

---

## 8. Design — リポジトリ B（sorter）

### 8.1 全体フロー（変更後の `process_image`）

```
process_image(path):
    sink = ClassifierSink::new()
    result = catch_unwind(AssertUnwindSafe(|| fast_meta::scan_file(path, &fast_meta::ScanOptions::default(), &mut sink)))
    match result:
        Err(panic)        -> Err("scan panicked")            // FR-INT-04
        Ok(Err(e))        -> Err(e.to_string())              // FR-INT-05。ScanError は Display 実装済み（§7.2）
        Ok(Ok(outcome))   -> target = sink.decide()          // §8.3
    target == None なら Ok(None)
    dest_dir = parent(path) / folder_name(target)            // 現行通り
    create_dir_all(dest_dir)                                  // 現行通り（§3.2）
    transfer_no_replace(path, dest_dir / file_name, copy)     // 現行無変更
```

走査・移行・集計・終了コードの骨格（`main`、`prepare_input_path`、`migrate_legacy_*`、`transfer_no_replace`）は変更しない。変更は `process_image` の分類取得部分と、引数・集計・除外リストの拡張に限る。

### 8.2 ClassifierSink のシグナル収集規則

`ClassifierSink` は `fast_meta::RecordSink` を実装し、`on_record` で以下だけを保持する。**本文を保持するのは (a) の短いレコードに限る**。キーの照合は `&str` 比較で行い、**レコードの参照（keyword / body の借用）は保持しない**（保持すると寿命が成立せず、ファイルごとのヒープ確保も増える）。

**包括規則：source == `Trailing` のレコードは、trailing 系シグナル（`trailing_novelai` / `trailing_webui`）を除く全てのシグナルで無視する。** 以下の各行の条件は、特に断りが無くても Trailing レコードには適用されない：

| シグナル | 収集条件 |
|---|---|
| `keywords` フラグ群（キーごとに bool フィールドを持つ構造体） | record.keyword が判定対象キー（§5.2 の各キー：`Comment`、`sui_image_params`、`sui_extra_data`、`invokeai_metadata`、`sd-metadata`、`Dream`、`use_stable_diffusion_model`、`workflow`、`generation_data`、`prompt`、`parameters`）のいずれかなら該当フラグを立てる。**source == `Trailing` のレコードは対象外**（下記の trailing 系シグナルで別扱いする。A-15） |
| `trailing_novelai: bool` / `trailing_webui: bool` | source == `Trailing` のレコード本文に、大小無視で `novelai` を含むか（前者）／`parameters`、`Stable Diffusion`、`Software`、`Steps: ` のいずれかを含むか（後者）。旧分類の本文部分文字列ルールと等価な範囲に限る（A-15） |
| `has_software: bool` | keyword == `Software` のレコードが1件以上あるか（§8.3 手順6で使用） |
| (a) `software_bodies: Vec<String>` | keyword == `Software` のレコード本文。**本文が256バイト以下の場合のみ**複製する（超過は `has_software` フラグだけ立てる） |
| (b) `model_body_has_sui: bool` | keyword == `Model` のレコード本文に `sui_image_params` を含むか（FR-CLS-06） |
| `parameters_has_sui: bool` | keyword == `parameters` の本文に `"sui_image_params"` を含むか |
| `prompt_is_json: bool` | keyword == `prompt` の本文が空白を除いた先頭が `{` か（FR-CLS-02） |
| `usercomment_has_workflow: bool` | keyword == `UserComment` の本文（先頭64KBまで）に `"workflow"` を含むか（FR-CLS-07） |
| `usercomment_is_novelai_pack: bool` | keyword == `UserComment` の本文（**先頭64KBまで**）に `"Comment":` と `"Description":` の両方を含むか |
| `steps_in_text_fields: bool` | keyword == `UserComment` または `ImageDescription` の本文（**先頭64KBまで**）に `Steps: ` を含むか |
| `jpeg_comment_a1111: bool` | source == `JpegComment` かつ label == `Stable Diffusion (A1111)` であるか（A-12。COM レコードは keyword = None のため label で判定する。**label 判定を正とし、本文チェックで代替しないこと**。label は本文に `Negative prompt:` / `Steps:` のいずれかを含む場合に付くため、`Steps: ` だけの本文チェックでは `Negative prompt:` のみの COM を拾えない） |
| `has_stealth: bool` / `stealth_novelai: bool` / `stealth_comfy: bool` | source == `PngStealth` の有無と、本文に `novelai` / `workflow` を含むか。大小無視比較は **`windows()` による窓単位の `eq_ignore_ascii_case` 照合に一本化**する（本文の小文字化コピーは作らない。FR-CLS-03）。Stealth 本文は上限8MB（fast_meta 側の上限）だが、走査は部分文字列の有無だけとする |
| `has_xmp: bool` | source が `PngXmp` / `JpegXmp` / `WebpXmp` / `TiffXmp` のいずれか |

補足：

- UserComment 系のシグナル（`usercomment_has_workflow` 等）は **source を問わず keyword で判定するのが意図**である。XMP 由来のレコードも keyword `UserComment` を持ち得る（fast_meta `xmp.rs`）が、行き先への影響は無いため source で絞らない
- Trailing レコード（source = `Trailing`）の扱い：**keyword フラグには効かせない**。旧構成では見出しルールが Trailing から発火せず（label が固定のため、A-15）、効いたのは本文部分文字列ルールだけだったため、等価性の範囲は `trailing_novelai` / `trailing_webui` の2シグナルに限定する。keyword = None の Trailing JSON レコードも本文ルールの対象にはなる（旧構成でも本文は出力テキストに含まれていた）。label 判定を使うのは `jpeg_comment_a1111` のみ
- レコード総数は sink で数えず、`ScanOutcome.record_count`（§7.2）を使う

### 8.3 決定手順（§5.2 分類表の実装形。上が優先）

```
decide():
    // 1. NovelAI の見出し相当（FR-CLS-01: Description 単独では確定しない）
    if keywords.Comment:
        return NovelAI
    // 2. SwarmUI（F-07）
    if keywords.sui_image_params || keywords.sui_extra_data
       || parameters_has_sui || model_body_has_sui:
        return SwarmUI
    // 3. InvokeAI（F-04 / F-05）
    if keywords.invokeai_metadata || keywords.sd_metadata || keywords.Dream:
        return InvokeAI
    // 4. Easy Diffusion（F-06）→ WebUI キャッチオール
    if keywords.use_stable_diffusion_model:
        return WebUI
    // 5. ComfyUI（FR-CLS-02 / FR-CLS-07）
    if keywords.workflow || keywords.generation_data
       || (keywords.prompt && prompt_is_json)
       || usercomment_has_workflow || stealth_comfy:
        return ComfyUI
    // 5b. 本文由来の NovelAI（旧順序：Comfy 見出し相当 → novelai 本文 → WebUI 本文、に合わせる）
    if software_bodies.any(contains "NovelAI")
       || usercomment_is_novelai_pack
       || stealth_novelai
       || trailing_novelai:
        return NovelAI
    // 6. WebUI（A1111 / 汎用キャッチオール、FR-CLS-04 / FR-CLS-05）
    if keywords.parameters || steps_in_text_fields || jpeg_comment_a1111
       || has_software || has_stealth || has_xmp || trailing_webui:
        return WebUI
    // 7. どれにも当たらない
    return None
```

優先順位の根拠：

- 手順1は NovelAI の見出し相当（`Comment` キー）だけに限定するのが現行互換（旧順序は NovelAI 見出し → Comfy 見出し → novelai 本文 → WebUI 本文）。本文由来の NovelAI シグナル（Software 本文、梱包形式、Stealth / Trailing 本文）は旧順序に合わせて手順5b（ComfyUI の後）に置く。`Comment` は他ツールと衝突しない
- SwarmUI を ComfyUI / WebUI より上に置くのは、SwarmUI が `parameters`（A1111 互換）や `prompt` を併せ持つ場合があるため。マーカーの特異性が高い順に倒す
- Easy Diffusion の `prompt`（平文）が ComfyUI に誤送されないのは、手順5の JSON チェック（FR-CLS-02）と、手順4が先に WebUI で確定することの二重の防壁による

### 8.4 パニック隔離

- `scan_file` 呼び出しを `catch_unwind` で包む。**パニックフックは差し替えない**（プロセス全域の設定であり、rayon 並列下では他スレッドの出力にも影響して競合する）。デフォルトフックの詳細出力は受け入れる
- パニックしたファイルはエラー集計に含める。他ファイルの処理は継続する

---

## 9. Specification（入出力仕様）

### 9.1 sorter CLI（v2.0.0）

```
Usage: sd_meta_sorter.exe [OPTIONS] <INPUTS>...

Arguments:
  <INPUTS>...    入力ファイルまたはディレクトリへのパス

Options:
  -c, --comfy-dir-name <NAME>     ComfyUI用フォルダ名 (デフォルト: comfyui_img)
  -w, --webui-dir-name <NAME>     WebUI用フォルダ名 (デフォルト: webui_img)
  -n, --novelai-dir <NAME>        NovelAI用フォルダ名 (デフォルト: NovelAI_img)
  -s, --swarm-dir-name <NAME>     SwarmUI用フォルダ名 (デフォルト: SwarmUI_img)   [新規]
  -i, --invoke-dir-name <NAME>    InvokeAI用フォルダ名 (デフォルト: InvokeAI_img) [新規]
      --copy                      ファイルを移動せず、コピーする
  -h, --help
  -V, --version                   2.0.0（CARGO_PKG_VERSION 由来）
```

- 対応拡張子は現行通り `png, jpg, jpeg, webp, avif`（AVIF は走査結果が未対応形式となりスキップされる）
- 集計出力に `SwarmUI:` と `InvokeAI:` の行を追加する。それ以外の出力文言は現行通り
- 終了コード：エラー0件なら 0、1件以上あれば 1

### 9.2 fast_meta CLI（v2.1.0）

- 引数・出力・終了コードは v2.0.0 から不変（FR-LIB-03）。**fast_meta の CLI に `--version` は存在しない**（未知の `--` 引数は usage エラーになる）。変わるのは新規キーワード2件（`Dream`、`use_stable_diffusion_model`）のレコードが出力対象に加わることだけである

### 9.3 依存関係

| 箇所 | 内容 |
|---|---|
| sorter → fast_meta | git 依存、tag `v2.1.0` 固定。`Cargo.lock` でコミットハッシュまで固定する。feature は fast_meta の既定（`zlib-ng`）のまま（FR-INT-01）。**`zlib-ng` は libz-sys のネイティブビルドを伴うため、sorter のビルドには C コンパイラが必要になる** |
| sorter の直接依存（v2.0.0 時点） | `clap`、`rayon`、`walkdir`、`fast_meta` のみ（`memmap2` / `serde` / `serde_json` は削除。fast_meta 経由の間接依存は問わない） |
| ローカル開発時 | 2リポジトリを並べて作業する場合も、コミットする `Cargo.toml` は git 依存の形を正とし、path 依存への書き換えはコミットしないこと |

---

## 10. Test 計画

### 10.1 fast_meta 側

| 種別 | 内容 |
|---|---|
| ゴールデン（既存） | `tests/golden_cli.rs` が Phase 1 終了時点で**既存ケースの期待値を変更せずに**全て通ること（FR-LIB-03 の検証） |
| ゴールデン（追加） | Phase 2 で `Dream` を持つ PNG と `use_stable_diffusion_model` を持つ PNG のケースを合成フィクスチャで追加すること。あわせて未対応形式の入力ケース（標準エラーに `Unsupported file format`、終了コード 0）が既存ゴールデンに無ければ Phase 1 で追加すること（§7.4 の bin 責務の検証） |
| 単体（既存） | キーワード登録表テストの件数を 14 → 16 に更新し、新規2キーの label を検証すること |
| ライブラリ単体 | sink にダミー実装を渡し、`scan_file` が渡すレコード列が**形式別 scan の返却列（`png::scan` / `jpeg::scan` / `webp_records` の返却列の順序）と一致する**ことを検証すること（CLI 出力は TextSink の統合・並び替え後のものなので「CLI と同じ列」にはならない）。入力は PNG / JPEG / WebP の合成バイト列を `std::env::temp_dir()` 配下の一時ファイルに書いてから `scan_file` に渡す形とし、追加依存は入れない。**検証用フィクスチャは Stealth が発火しない構成（通常レコードを含むもの）を使い、Stealth レコードは末尾に追加される分を除いて一致すること**。未対応形式で format = None になること、0バイトで `Err(Io)` になることも含める |

### 10.2 sorter 側

| 種別 | 内容 |
|---|---|
| 分類単体（移植） | 現行の分類テスト2関数（観点：NovelAI の優先、Stealth の WebUI 扱い、Stealth 内 workflow の ComfyUI 検出、入力パス文字列に惑わされない）を、テキスト入力から `RecordView` 列入力へ移植すること |
| 分類単体（新規） | §5.2 の各行について最低1件：SwarmUI（キー形式／parameters 内包／EXIF Model 旧形式）、InvokeAI（`invokeai_metadata`／`sd-metadata`／`Dream`）、Easy Diffusion マーカー → WebUI、Description 単独 → **NovelAI にならない**こと、prompt 平文 → ComfyUI にならないこと、prompt JSON → ComfyUI、XMP のみ → WebUI、Software=Midjourney → WebUI、Software=NovelAI → NovelAI、**COM のみの A1111 JPEG（keyword = None、label 判定）→ WebUI**、**`Negative prompt:` のみを含む COM（`Steps:` を含まない）→ WebUI（label 判定でなければ通らない形で固定する）**、**Trailing 本文に `novelai` を含む → NovelAI（手順5b）、Trailing 本文に `Steps: ` を含む → WebUI、Trailing の keyword（例：`workflow`）だけでは ComfyUI にならないこと**、**`workflow` キーワード＋Software 本文 `NovelAI` → ComfyUI（旧順序互換、手順5b の位置）** |
| 移動まわり（既存） | `transfer_no_replace` の上書き保護と並行コピー競合のテストが無修正で通ること |
| 統合 | 一時ディレクトリに合成 PNG（tEXt チャンクで各分類を再現）を置き、実行後に各フォルダへ正しく振り分けられることを確認すること。SwarmUI / InvokeAI フォルダが再実行時の走査から除外されること（FR-CLI-03）、**カスタム名（`--swarm-dir-name` 等）を指定した場合にその名前のフォルダへ振り分けられ除外も効くこと**（FR-CLI-02 / 03）、**集計表示に SwarmUI / InvokeAI の行があり合計が総数と一致すること**（FR-INT-09）を含める |

### 10.3 等価性（Phase 0 / Phase 4）

- Phase 0 で、分類既知の実サンプルまたは合成サンプルからなるコーパス（最低限：ComfyUI PNG、A1111 PNG、A1111 JPEG、NovelAI PNG、NovelAI WebP、SwarmUI PNG、InvokeAI PNG、Midjourney PNG、Stealth PNG、メタデータなし PNG/JPEG、写真 JPEG（XMP のみ）、**Trailing のみにキーワードテキスト（`Software` または `parameters`）を持つ PNG**）を用意し、旧構成での仕分け結果を記録する
- コーパスの置き場所：**合成サンプルはリポジトリ B の `doc/phase0/corpus/` にコミット**する。実サンプルはファイル自体ではなくハッシュと出所・期待分類を記録し、Phase 4 で同一ファイルを再実行できる状態を保つ
- Phase 4 で新構成の結果と突き合わせ、差分が §1.2 G4 で意図した変更だけであることを PR に明記する

### 10.4 性能（Phase 0 / Phase 4）

- コーパスは実運用相当の混合（最低1,000枚、可能なら10,000枚規模も）を使い、移動モード・同一ボリュームで旧構成と新構成の総時間を各3回測って中央値を記録する
- **移動モードはコーパスを消費するため、マスターコーパスを別に保持し、計測のたびに作業用コピーへ再展開してから測る**こと。複製で枚数を揃える場合はファイル名を一意化する（同名だと整理先で衝突してスキップ扱いになり負荷が変わる）。形式別の内訳を記録する
- **各構成の計測前にウォームアップを1回行い、その結果は捨てる**（初回実行の OS キャッシュ／アンチウイルス走査の影響を除くため）
- 計測条件（マシン、ストレージ、コーパスの内訳）を PR に併記する。NFR-PERF-01（1/5 以下）を満たさない場合は、フェーズを完了とせず原因を切り分けてから次へ進む

---

## 11. 実装計画（フェーズ分割）

### 11.1 フェーズ一覧

| Phase | リポジトリ | 内容 | 完了条件 |
|---|---|---|---|
| 0 | B（計測のみ） | ベースライン計測と等価性コーパスの記録。**実装変更なし** | §10.3 のコーパスと旧構成の仕分け結果、§10.4 の旧構成総時間が PR（または doc ファイル）に記録されている。現行 `cargo test` の赤/緑の記録（B-09 の確認）と、現行2 exe（`sd_meta_sorter.exe`＋`fast_meta.exe`）の合計サイズの記録も含む |
| 1 | A | lib 化（§7）。振る舞い不変 | golden_cli の既存ケースが期待値無変更で全通し（追加は §10.1 の指示分のみ）。`cargo build --release --locked` と `cargo test --locked` が緑。CLI の手動確認（PNG/JPEG/WebP 各1件）が v2.0.0 と同一出力 |
| 2 | A | 形式追加（FR-FMT-01 / 02）と version 2.1.0 化、タグ `v2.1.0` | 新規ゴールデン2件が通る。登録表テストが16件で通る |
| 3 | B | 統合と分類（§5 / §8 / §9）。version 2.0.0 化 | §10.2 のテストが全て通る。`fast_meta.exe` なしで単体動作する。`std::process::{Command, Stdio}` の参照が消えている（FR-INT-02） |
| 4 | B | 等価性確認・ベンチマーク・README 更新・リリース | §10.3 の差分が意図した変更のみ。NFR-PERF-01 を満たす実測が記録されている。単一 exe のサイズを Phase 0 の2 exe 合計と比較して記録している |
| 5（P2・任意） | B | 走査の二重解消、`create_dir_all` 集約等の sorter 単独最適化 | やる場合のみ。ベンチマークで効果を示すこと。やらなくても v2.0.0 のリリースは完了とする |

### 11.2 ブランチ運用

- リポジトリ A：統合ブランチは `main`（v2.0.0 はリリース済み）。Phase 1 は `phase/1-lib`、Phase 2 は `phase/2-formats` のようにフェーズごとのブランチを切り、PR で `main` へ入れる。`v2.1.0` タグは Phase 2 のマージ後に打つ
- リポジトリ B：Phase ごとに `phase/3-integration` 等のブランチを切り、PR で入れる

### 11.3 コミット規約

1. **1フェーズ = 1PR**。フェーズをまたぐ変更を1つの PR に混ぜない
2. コミットは要件 ID（FR-LIB-xx、FR-FMT-xx、FR-INT-xx、FR-CLS-xx 等）単位で分割し、対応するテストを**同一コミット**に入れる
3. 各コミット時点で `cargo build --release --locked` と `cargo test --locked` が緑であること
4. Phase 0 と Phase 1 は実装の振る舞い変更を含まない（Phase 0 は計測記録のみ、Phase 1 は移設のみ。ただし §7.4 に挙げた Phase 1 の例外2点（`eprintln!` の除去と、`RecordSink` / `extract_to_writer` の新設）は振る舞い不変の範囲として許可する）。振る舞い変更（Phase 2 以降）と混ぜない
5. コミットメッセージは先頭に対応する ID を付ける（例：`FR-LIB-02: extract logic を lib の extract モジュールへ移設`）

---

## 12. 受け入れ条件（マスターチェックリスト）

**fast_meta v2.1.0**
- [ ] `cargo build --release --locked` / `cargo test --locked` が緑
- [ ] `tests/golden_cli.rs` が（§10.1 の追加分を除き）既存ケースの期待値無変更で通る
- [ ] ライブラリ単体テスト（§10.1）が通る
- [ ] README にライブラリ利用方法が追記されている（FR-LIB-08）
- [ ] version が 2.1.0 で、タグ `v2.1.0` が打たれている
- [ ] キーワード登録表が16件で、`Dream` / `use_stable_diffusion_model` を含む

**sdMetaImageSorter v2.0.0**
- [ ] `fast_meta.exe` が無い環境で単体で動作する
- [ ] §10.2 の分類単体テスト（移植分＋新規分）が全て通る
- [ ] 既存の移動まわりテストが無修正で通る
- [ ] 等価性（§10.3）の差分が意図した変更のみである
- [ ] ベンチマーク（§10.4）で現行比 1/5 以下を記録している
- [ ] `--version` が 2.0.0 を表示する
- [ ] 集計表示に SwarmUI / InvokeAI の行があり、カスタムフォルダ名オプションが効く（§10.2 統合）
- [ ] 単一 exe のサイズが Phase 0 の2 exe 合計と比較して記録されている
- [ ] README が単一 exe 配布・5フォルダ・新オプションを反映している
- [ ] 旧フォルダ移行、上書き保護、終了コードの挙動が現行と同一である

---

## 13. 決定事項とリスク

### 13.1 本書で決定済み（再判断しないこと）

| 項目 | 決定 |
|---|---|
| ライブラリ API の主方式 | sink 方式（§7.1）。公開 trait は `RecordSink`（メソッド側に寿命）、CLI 出力は lib 内の `extract_to_writer` 経由。所有化 API は参考に留める |
| fast_meta の feature | 既定（`zlib-ng`）を sorter でもそのまま使う（速度優先）。代償として sorter のビルドに C コンパイラが必要になる（FR-INT-01） |
| 分類ポリシーの置き場所 | sorter 側。fast_meta はレコード返しまで |
| SwarmUI / InvokeAI の行き先 | 専用フォルダ `SwarmUI_img` / `InvokeAI_img`（ユーザー決定、2026-11-26） |
| `sd-metadata` の行き先 | InvokeAI_img（書き込むのは InvokeAI のみと外部照合済み、F-04） |
| XMP のみの画像（Midjourney / Draw Things 等） | webui_img（キャッチオール。ユーザー決定、2026-11-26） |
| Easy Diffusion / Fooocus の行き先 | 専用フォルダは作らず webui_img（Fooocus は `parameters` で自動的に入る） |
| Description 単独の扱い | NovelAI と確定しない（F-01） |
| sorter の package 名 | `sd_meta_tools` のまま変更しない |
| fast_meta 依存の固定方法 | git 依存＋tag `v2.1.0`＋`Cargo.lock` |
| sorter 単独最適化（走査・create_dir_all 等） | v2.0.0 のスコープ外。Phase 5（P2）でのみ扱う |
| AVIF | 拡張子リストに残し、未対応形式としてスキップ |

### 13.2 リスク

| リスク | 影響 | 対策 |
|---|---|---|
| lib 化で CLI 出力が微妙に変わる（見出しの出し分け条件の移設漏れ等） | fast_meta 単体利用者の互換性が壊れる | Phase 1 の完了条件を golden_cli の既存ケースの期待値無変更での全通しとし、見出し・文言を含む出力は現行の形式別抽出関数の構造のまま `extract_to_writer` へ移して lib 内で完結させる（§7.2） |
| 分類の取りこぼし・誤送（特に Description / prompt のキー衝突） | 旧構成で仕分けできていた画像の行き先が変わる | §8.3 の優先順位と FR-CLS-01 / 02 の防壁をテスト（§10.2）で固定し、Phase 4 の等価性確認で実コーパスと突き合わせる |
| パニック隔離の喪失（ライブラリ内の panic が sorter 全体を落とす） | 大量処理の途中停止 | FR-INT-04 のファイル単位 `catch_unwind`。fast_meta 側も従来通り panic フリーを維持する |
| XMP キャッチオールの誤送（写真編集ソフト由来の XMP Description 等） | 通常写真が webui_img に入る | 既知の受容事項とする（ユーザー決定）。将来気になる場合は CreatorTool / DigitalSourceType での条件絞り込みを別件で検討する |
| 性能目標未達（コーパスがコピー主体等、分類以外の支配項が大きい場合） | G2 未達 | Phase 0 の計測で支配項の内訳を先に把握する。未達の場合は Phase 5（P2）の前倒しをユーザーと相談する |

---

## 14. 参考情報

- fast_meta v2.0.0 の実装（本書 §2.1 の根拠）：https://github.com/wai55555/checkImageMetadata
- sdMetaImageSorter 現行実装（本書 §2.2 の根拠）：https://github.com/wai55555/sdMetaImageSorter
- receyuki/stable-diffusion-prompt-reader（MIT、形式カタログと判定順序の出典）：https://github.com/receyuki/stable-diffusion-prompt-reader
  - InvokeAI 世代表と Dream 形式：`sd_prompt_reader/format/invokeai.py`
  - Easy Diffusion のキー群：`sd_prompt_reader/format/easydiffusion.py`
  - 検出順序表：`sd_prompt_reader/image_data_reader.py`（DeepWiki の Format Detection System 解説も同内容）
- d3x-at/sd-parsers（MIT、InvokeAI / Fooocus 判別の照合用）：https://github.com/d3x-at/sd-parsers
- 前例の仕様書（本書の体裁とコミット規約の出典）：checkImageMetadata 改修仕様書 `checkImageMetadata-plan-design-spec-requirements.md`
