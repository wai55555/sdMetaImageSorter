# SD Meta Image Sorter

**SD Meta Image Sorter** は、AI生成画像（Stable Diffusion WebUI, ComfyUI, NovelAI, SwarmUI, InvokeAIなど）のメタデータを解析し、自動的にフォルダ分けを行うRust製の高速CLIツールです。

画像ファイル（PNG, JPG, WEBP, AVIF）に含まれるメタデータをスキャンし、ComfyUI、WebUI、NovelAI、SwarmUI、InvokeAI を判別して整理します。AVIF は現行の解析エンジンが未対応のためスキップされます。

## 特徴

*   **高速動作**: Rust言語と並列処理（Rayon）により、大量の画像も一瞬でスキャン・仕分けします。
*   **インテリジェントな仕分け**:
    *   **ComfyUI**: `comfyui_img` フォルダへ移動
    *   **WebUI**: `webui_img` フォルダへ移動
    *   **NovelAI**: `NovelAI_img` フォルダへ移動
    *   **SwarmUI**: `SwarmUI_img` フォルダへ移動
    *   **InvokeAI**: `InvokeAI_img` フォルダへ移動
    *   **メタデータなし**: 移動せず、そのまま無視（スキップ）
*   **無駄のない動作**: 仕分け対象の画像が見つかった場合のみ、その画像があるフォルダ内にサブフォルダを作成します。空のフォルダは作られません。
*   **ステルス情報**: 内蔵の fast_meta ライブラリが Stealth PNG Info を検出します。本文に `workflow` を含めば ComfyUI、`novelai` を含めば NovelAI、生成元が不明な場合は WebUI として扱います。
*   **旧フォルダの移行**: 整理開始前に従来の `webui_image` を `webui_img` へ移します。同名ファイルがある場合は上書きせず、`__legacy_1` などの連番を付けて残します。
*   **重複の保護**: 整理先に同名の項目がある場合は、上書きせずスキップします。
*   **移動/コピー対応**: デフォルトは「移動」ですが、`--copy` オプションで「複製」も可能です。
*   **安全性**: 整理先の既存ファイルを上書きせず、操作失敗を報告します。

## 動作の仕組み

本ツールは単一の `sd_meta_sorter.exe` で動作します。画像の解析には
[fast_meta](https://github.com/wai55555/checkImageMetadata) v2.1.0 をライブラリとして
プロセス内で呼び出します。`fast_meta.exe` を別途ダウンロードしたり、同じフォルダに
置いたりする必要はありません。

## インストール

リリースページから最新の `sd_meta_sorter.exe` をダウンロードして実行します。

## ビルド方法

Rust 環境（Cargo）と C コンパイラが必要です。fast_meta の既定 feature である
`zlib-ng` がネイティブビルドされるためです。

```bash
git clone https://github.com/your-username/sdMetaImageSorter.git
cd sdMetaImageSorter
cargo build --release --locked
```

ビルドが完了すると `target/release/sd_meta_sorter.exe` が生成されます。

## 使い道

### 基本的な使い方

フォルダパス、または画像ファイルをドラッグ＆ドロップ（引数指定）して実行します。

```powershell
# フォルダを指定（中の画像をすべて処理）
.\sd_meta_sorter.exe "C:\Path\To\Images"

# 複数のファイルを指定
.\sd_meta_sorter.exe image1.png image2.png
```

### オプション

```text
Usage: sd_meta_sorter.exe [OPTIONS] <INPUTS>...

Arguments:
  <INPUTS>...    入力ファイルまたはディレクトリへのパス

Options:
  -c, --comfy-dir-name <NAME>    ComfyUI用フォルダ名 (デフォルト: comfyui_img)
  -w, --webui-dir-name <NAME>    WebUI用フォルダ名 (デフォルト: webui_img)
  -n, --novelai-dir-name <NAME>  NovelAI用フォルダ名 (デフォルト: NovelAI_img)
  -s, --swarm-dir-name <NAME>    SwarmUI用フォルダ名 (デフォルト: SwarmUI_img)
  -i, --invoke-dir-name <NAME>   InvokeAI用フォルダ名 (デフォルト: InvokeAI_img)
      --copy                     ファイルを移動せず、コピーする
  -h, --help                     ヘルプを表示
  -V, --version                  バージョンを表示 (2.0.1b)
```

整理先フォルダ名を変更した場合も、次回実行時の走査からそのフォルダは除外されます。

### 実行例

**例：元ファイルを残したまま、コピーして仕分けたい場合**

```powershell
.\sd_meta_sorter.exe "C:\Images" --copy
```

## フォルダ構成の例

実行前：
```text
C:\Images\
  ├── 001.png  (ComfyUI製)
  ├── 002.png  (WebUI製)
  ├── 003.jpg  (写真・メタデータなし)
```

実行後：
```text
C:\Images\
  ├── comfyui_img\
  │     └── 001.png
  ├── webui_img\
  │     └── 002.png
  ├── NovelAI_img\
  │     └── 004.png  (NovelAI製)
  ├── SwarmUI_img\
  │     └── 005.png  (SwarmUI製)
  ├── InvokeAI_img\
  │     └── 006.png  (InvokeAI製)
  ├── 003.jpg  (そのまま)
```

## ライセンス

[MIT License](LICENSE)
