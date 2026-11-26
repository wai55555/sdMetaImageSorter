# sdMetaImageSorter v2.0.0

画像1枚ごとの `fast_meta.exe` 子プロセス起動を廃止し、`fast_meta` v2.1.0 をライブラリとして
プロセス内で呼び出すようにしました。あわせて仕分け先を3分類から5分類へ拡張しています。

## ハイライト

- **単一 exe 化**: `fast_meta.exe` の同梱・同一フォルダ配置が不要になりました。
- **約12.3倍高速化**: 1,000枚の同一コーパス・同一マシン・移動モードで、中央値
  1.673秒 → 0.136秒（目標の5倍以上を達成）。
- **5分類対応**: ComfyUI / WebUI / NovelAI に加えて **SwarmUI** と **InvokeAI** の
  専用フォルダへ仕分けます。
- **分類の是正**:
  - `Description` 単独では NovelAI と確定しない（Midjourney の誤送防止）
  - `prompt` は本文が JSON のときだけ ComfyUI（Easy Diffusion の平文 prompt 対策）
  - Stealth 本文に `workflow` を含む場合は ComfyUI
  - XMP のみの画像は WebUI のキャッチオール

## 仕分け先

| 生成元 | 既定フォルダ |
|---|---|
| ComfyUI | `comfyui_img` |
| WebUI | `webui_img` |
| NovelAI | `NovelAI_img` |
| SwarmUI | `SwarmUI_img` |
| InvokeAI | `InvokeAI_img` |

メタデータがない画像、および上記に該当しない画像は移動せずスキップします。
AVIF は拡張子の走査対象に残していますが、解析エンジンが未対応のためスキップされます。

## 新しいオプション

```text
-s, --swarm-dir-name <NAME>    SwarmUI用フォルダ名 (デフォルト: SwarmUI_img)
-i, --invoke-dir-name <NAME>   InvokeAI用フォルダ名 (デフォルト: InvokeAI_img)
-V, --version                  バージョンを表示 (2.0.0)
```

カスタム名を指定した場合も、次回実行時の走査からそのフォルダは除外されます。

## 安全性・互換性

- 整理先に同名ファイルがある場合は上書きせずスキップします。
- 移動はハードリンクを優先し、不可の場合は排他作成＋コピーで行います。
- 旧フォルダ `webui_image` の移行は従来どおり維持しています。
- 終了コードは従来どおり、エラー0件なら0、1件以上なら1です。
- 走査の入出力エラーはエラー集計し、未対応形式はエラーではなくスキップします。
- 1ファイルのパニックはそのファイルのエラーとして扱い、残りの処理を継続します。

## ビルド要件の変更

`fast_meta` の既定 feature（`zlib-ng`）を利用するため、**C コンパイラが必要**になりました。

```bash
cargo build --release --locked
```

## 検証

- `cargo build --release --locked` / `cargo test --locked`: 緑
- 等価性コーパス13枚の差分は、意図した変更（SwarmUI / InvokeAI の専用フォルダ化、
  Midjourney の NovelAI 誤送防止、Stealth workflow の ComfyUI 是正、XMP のみ画像の
  WebUI 化）のみ
- 配布サイズ: 旧2 exe合計 1,834,496 bytes → 単一 exe 1,480,192 bytes（約19.3%減）

詳細は `doc/phase0/baseline.md` と `doc/phase4/verification.md` を参照してください。
