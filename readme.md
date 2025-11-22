# SD Meta Image Sorter

**SD Meta Image Sorter** は、AI生成画像（Stable Diffusion WebUI, ComfyUI, NovelAIなど）のメタデータを解析し、自動的にフォルダ分けを行うRust製の高速CLIツールです。

画像ファイル（PNG, JPG, WEBP, AVIF）に含まれるメタデータをスキャンし、ComfyUIで作られた画像と、それ以外（A1111 WebUI, NovelAIなど）を判別して整理します。

## 特徴

*   **高速動作**: Rust言語と並列処理（Rayon）により、大量の画像も一瞬でスキャン・仕分けします。
*   **インテリジェントな仕分け**:
    *   **ComfyUI**: `comfyui_img` フォルダへ移動
    *   **WebUI / NovelAI**: `webui_image` フォルダへ移動
    *   **メタデータなし**: 移動せず、そのまま無視（スキップ）
*   **無駄のない動作**: 仕分け対象の画像が見つかった場合のみ、その画像があるフォルダ内にサブフォルダを作成します。空のフォルダは作られません。
*   **移動/コピー対応**: デフォルトは「移動」ですが、`--copy` オプションで「複製」も可能です。
*   **安全性**: メモリマップドファイル（mmap）を使用し、ファイルの読み込みを最小限に抑えています。

## 動作の仕組み

本ツールは以下の2つのバイナリで構成します。

1.  **`sd_meta_sorter.exe`**: ユーザーが実行するメインプログラム。フォルダの探索、並列処理の管理、ファイル操作（移動/コピー）を行います。
2.  **`fast_meta.exe`**: 内部で使用される解析エンジン。単一ファイルのバイナリを高速スキャンし、AI生成メタデータの痕跡を探します。

## インストール
リリースページから最新の `sd_meta_sorter.exe` をダウンロードします。
[fast_meta.exe](https://github.com/wai55555/checkImageMetadata/releases)をダウンロードします。


## ビルド方法
Rust環境（Cargo）が必要です。

```bash
git clone https://github.com/your-username/sdMetaImageSorter.git
cd sdMetaImageSorter
cargo build --release
```

ビルドが完了すると、`target/release/` フォルダ内に `sd_meta_sorter.exe` と `fast_meta.exe` が生成されます。

> [!IMPORTANT]
> **重要:** 実行時は必ず `sd_meta_sorter.exe` と `fast_meta.exe` を**同じフォルダ**に置いてください。

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
  -c, --comfy-dir <NAME>    ComfyUI用フォルダ名 (デフォルト: comfyui_img)
  -w, --webui-dir <NAME>    WebUI/NovelAI用フォルダ名 (デフォルト: webui_image)
      --copy                ファイルを移動せず、コピーする
  -h, --help                ヘルプを表示
```

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
  ├── webui_image\
  │     └── 002.png
  ├── 003.jpg  (そのまま)
```

## ライセンス

[MIT License](LICENSE)
