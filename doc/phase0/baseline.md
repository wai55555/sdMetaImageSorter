# Phase 0 ベースライン記録（sdMetaImageSorter v1.1.0 + fast_meta v2.0.0）

実装変更は行っていない。以下の条件で旧構成（2 exe）の実測と等価性コーパスの記録だけを行った。

## 計測条件

| 項目 | 内容 |
|---|---|
| 日時 | 2026-11-26 |
| マシン | 13th Gen Intel Core i5-13600KF / 64 GiB RAM（Windows 11 + WSL2） |
| ストレージ | `C:` 上の `C:\github\sdMetaImageSorter\target\`（NTFS） |
| OS | WSL2 `6.18.33.2-microsoft-standard-WSL2` |
| ツール | Windows MSVC 版 `cargo 1.91.1` |
| 旧 sorter | `sd_meta_tools` 1.1.0（`--version` は `4.4` 固定） |
| 旧 fast_meta | `fast_meta` 2.0.0 |
| バイナリサイズ | `sd_meta_sorter.exe` 994,816 bytes + `fast_meta.exe` 839,680 bytes = **1,834,496 bytes** |

## 現行テストの状態（B-09 の確認）

`cargo test --locked` は **赤**（4 テスト中 1 失敗）。

```
test tests::classifies_stealth_and_ignores_the_input_path ... FAILED
  left: WebUI
 right: ComfyUI
```

Stealth 本文 `ComfyUI workflow` を ComfyUI と判定する現行テストの期待に対し、現行実装は
`--- Stealth PNG Info (` を WebUI 条件で先に拾うため WebUI を返す。仕様書 §2.2 B-09 の
記載どおりで、v2.0.0 ではテスト期待（ComfyUI）を正として実装を是正する。

## 等価性コーパス（`doc/phase0/corpus/`）

合成サンプルのみ。生成は `python3 doc/phase0/generate_corpus.py`。
実サンプルは使用していない（ファイルをコミットせず、この記録を再実行可能な状態とする）。

| ファイル | 内容 | 旧構成の行き先 | 新構成の期待（§5.2 / §8.3） |
|---|---|---|---|
| `comfyui.png` | `workflow` JSON | `comfyui_img` | `comfyui_img` |
| `a1111.png` | `parameters` | `webui_img` | `webui_img` |
| `a1111.jpg` | EXIF UserComment の `parameters` | `webui_img` | `webui_img` |
| `novelai.png` | `Description` + `Comment` + `Software=NovelAI` | `NovelAI_img` | `NovelAI_img` |
| `novelai.webp` | EXIF UserComment の NovelAI 梱包 JSON | `NovelAI_img` | `NovelAI_img` |
| `swarmui.png` | `sui_image_params` | スキップ | `SwarmUI_img`（新規） |
| `invokeai.png` | `invokeai_metadata` | スキップ | `InvokeAI_img`（新規） |
| `midjourney.png` | `Description` + `Software=Midjourney` | `NovelAI_img` | `webui_img`（意図した変更） |
| `stealth.png` | Stealth 本文 `ComfyUI workflow` | `webui_img` | `comfyui_img`（B-09 の是正） |
| `no_metadata.png` | メタデータなし | スキップ | スキップ |
| `no_metadata.jpg` | EXIF なし | スキップ | スキップ |
| `photo_xmp.jpg` | XMP Description のみ | スキップ | `webui_img`（意図した変更） |
| `trailing.png` | IEND 後の JSON に `Software` / `parameters` | `webui_img` | `webui_img` |

旧構成の実測結果（移動モード、13 枚）:

```
Total Scanned: 13
  -> ComfyUI: 1
  -> WebUI: 4
  -> NovelAI: 3
  -> Skipped: 5
  -> Errors: 0
```

旧構成で `midjourney.png` が NovelAI に入るのは、旧 `classify_metadata` が
`--- NovelAI [` 見出しを最優先するため（fast_meta の Description ラベルが
`NovelAI [Prompt]`）。§1.2 G4 の意図した変更として扱う。

## 性能ベースライン（§10.4）

- コーパス: `target/phase0-perf/` に生成した 1,000 枚（12 種の合成サンプルを
  ファイル名一意で循環）。内訳は PNG 約 667 枚 / JPEG 約 250 枚 / WebP 約 83 枚。
- 移動モード。各計測の前にコーパスを作業ディレクトリへ再展開し、ウォームアップ 1 回
  （結果は破棄）の後に 3 回計測。
- 実行コマンド: `sd_meta_sorter.exe target/phase0-bench-work`
- 生成: `python3 doc/phase0/generate_corpus.py --perf-count 1000`

| 実行 | 時間 |
|---|---:|
| ウォームアップ（破棄） | 1.795 s |
| 1 回目 | 1.857 s |
| 2 回目 | 1.670 s |
| 3 回目 | 1.673 s |
| **中央値** | **1.673 s** |

Phase 4 の目標はこの 1/5 以下（0.335 s 以下）。同一コーパス・同一マシン・同条件で
新構成を再計測し、差分を本ファイルまたは Phase 4 の記録に追記する。
