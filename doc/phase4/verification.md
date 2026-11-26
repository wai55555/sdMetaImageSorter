# Phase 4 検証記録（sdMetaImageSorter v2.0.0 / fast_meta v2.1.0）

## 等価性（§10.3）

Phase 0 と同じ `doc/phase0/corpus/`（13 枚）を移動モードで処理し、旧構成の結果
（`doc/phase0/baseline.md`）と突き合わせた。

| ファイル | 旧構成 | 新構成 | 差分の扱い |
|---|---|---|---|
| `comfyui.png` | `comfyui_img` | `comfyui_img` | 同一 |
| `a1111.png` | `webui_img` | `webui_img` | 同一 |
| `a1111.jpg` | `webui_img` | `webui_img` | 同一 |
| `novelai.png` | `NovelAI_img` | `NovelAI_img` | 同一 |
| `novelai.webp` | `NovelAI_img` | `NovelAI_img` | 同一 |
| `swarmui.png` | スキップ | `SwarmUI_img` | 意図した変更（専用フォルダ新設） |
| `invokeai.png` | スキップ | `InvokeAI_img` | 意図した変更（専用フォルダ新設） |
| `midjourney.png` | `NovelAI_img` | `webui_img` | 意図した変更（FR-CLS-01 / F-01） |
| `stealth.png` | `webui_img` | `comfyui_img` | 意図した変更（B-09、FR-CLS-04） |
| `no_metadata.png` | スキップ | スキップ | 同一 |
| `no_metadata.jpg` | スキップ | スキップ | 同一 |
| `photo_xmp.jpg` | スキップ | `webui_img` | 意図した変更（FR-CLS-05） |
| `trailing.png` | `webui_img` | `webui_img` | 同一 |

新構成の集計:

```
Total Scanned: 13
  -> ComfyUI: 2
  -> WebUI: 5
  -> NovelAI: 2
  -> SwarmUI: 1
  -> InvokeAI: 1
  -> Skipped: 2
  -> Errors: 0
```

差分は §1.2 G4 に列挙された意図した変更（SwarmUI / InvokeAI の専用フォルダ化、
XMP のみの WebUI 化、Midjourney の NovelAI 誤送防止、Stealth 本文の workflow 検出）
だけであり、それ以外の行き先は旧構成と一致した。

## 性能（§10.4 / NFR-PERF-01）

Phase 0 と同一のマシン・同一の 1,000 枚コーパス・移動モード。各構成で
ウォームアップ 1 回（破棄）の後に 3 回計測し、中央値を比較した。

| 構成 | ウォームアップ | 1 回目 | 2 回目 | 3 回目 | 中央値 |
|---|---:|---:|---:|---:|---:|
| 旧構成（sorter 1.1.0 + fast_meta.exe 2.0.0） | 1.795 s | 1.857 s | 1.670 s | 1.673 s | **1.673 s** |
| 新構成（sorter 2.0.0 + fast_meta 2.1.0 ライブラリ） | 0.127 s | 0.131 s | 0.136 s | 0.162 s | **0.136 s** |

- 高速化率: 1.673 / 0.136 = **約 12.3 倍**（目標 5 倍以上を満たす）
- 1 枚あたり: 旧 1.673 ms → 新 0.136 ms
- コーパス内訳: PNG 約 667 枚 / JPEG 約 250 枚 / WebP 約 83 枚（12 種の合成サンプルを
  ファイル名一意で循環）。`python3 doc/phase0/generate_corpus.py --perf-count 1000` で再生成可能。

## 配布サイズ

| 構成 | サイズ |
|---|---:|
| Phase 0: `sd_meta_sorter.exe` + `fast_meta.exe` | 994,816 + 839,680 = **1,834,496 bytes** |
| Phase 4: `sd_meta_sorter.exe` 単体 | **1,480,192 bytes** |

単一 exe 化により **354,304 bytes（約 19.3%）減**。`fast_meta.exe` の同梱・同一フォルダ
配置は不要になった。

## 実行確認

- `cargo build --release --locked` / `cargo test --locked`: 緑
- `sd_meta_sorter.exe --version`: `sd_meta_sorter 2.0.0`
- `fast_meta.exe` を置かない状態で動作（テストは `CARGO_BIN_EXE_sd_meta_sorter` を直接実行）
- `src/main.rs` に `std::process::{Command, Stdio}` の参照なし
