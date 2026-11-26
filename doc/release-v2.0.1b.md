# sdMetaImageSorter v2.0.1b

v2.0.1 の走査最適化を含むマイナー更新です。分類・移動の意味論は変更していません。

## 変更内容

- **NovelAI 用オプション名を統一**: `-n, --novelai-dir` を
  `-n, --novelai-dir-name` に変更しました。
  `--comfy-dir-name` / `--webui-dir-name` / `--swarm-dir-name` /
  `--invoke-dir-name` と命名が揃います。
- 旧 `--novelai-dir` は互換用の隠しエイリアスとして引き続き受け付けます。
  `--help` には `--novelai-dir-name` のみ表示されます。
- `--version` の表示を `sd_meta_sorter 2.0.1b` に更新しました。
  Cargo の package version は SemVer に合わせて `2.0.1-b` です。

## 使い方

```text
-n, --novelai-dir-name <NAME>  NovelAI用フォルダ名 (デフォルト: NovelAI_img)
```

```powershell
.\sd_meta_sorter.exe "C:\Images" --novelai-dir-name NovelAI
```

## 変更なし

- 5分類の判定規則
- 旧フォルダ `webui_image` の移行
- 同名ファイルの上書き保護、移動/コピーのフォールバック
- 終了コード
- 走査の二重解消と `create_dir_all` の集約（v2.0.1 の最適化を包含）

## 検証

- `cargo build --release --locked` / `cargo test --locked`: 緑
- ユニットテスト17件・統合テスト4件が成功
- `--help` に `--novelai-dir-name` が表示されること
- `--novelai-dir-name` と旧 `--novelai-dir` の両方で NovelAI フォルダへ仕分けられること
- `sd_meta_sorter.exe --version`: `sd_meta_sorter 2.0.1b`
