<p align="center"><img src="docs/social-preview.png" alt="likegetuploader" width="640"></p>

> [!Warning]
 AI Slop!  
 model: opencode GLM-5.3-Flash Free

# likeget-uploader (Rust 版)

uploader.jp のようなリスト式アップローダー。

「ゲームのMOD・シナリオ置き場」「サークルの素材置き場」のように、**1つの目的のためのファイルを番号付きの一覧で並べる**アップローダーを、セルフホストのサーバーに設置できます。PHP の [likegetuploader](https://github.com/PYU224/likegetuploader) を Rust でフル移植した実装で、本体は1つのバイナリのみです。

## 特徴

**uploader.jp と同じ使い心地**
- 番号・ファイル名・コメント・容量・日時・DL数の一覧、並べ替え、検索、ページ送り
- ファイルページ経由のダウンロード（直リンク不可）、MD5表示
- 削除キー（投稿者が自分で消すため）、DLパスワード、アップロードパスワード
- `/download/123` 形式のURL、統計（ディスク使用量・ファイル数・総DL数）、リンク欄
- お問い合わせフォーム、通報フォーム
- **uploader.jp からの引っ越しツール付き**（番号・日時・DL数・コメントをそのまま移行！）

**運営をラクにする機能**
- 管理画面（ファイルの修正・非公開・削除、投稿者、アップロード禁止、通報・お問い合わせの確認）
- **管理メールへのメール通知**（SMTP）
- **Cloudflare Turnstile**（人間確認）をアップロード・ダウンロード・お問い合わせ・管理ログインに個別に設定
- 連続投稿の制限、二重拡張子などの危険なファイルの拒否
- IPアドレスは生で保存せず、ハッシュで保存

**サーバーに優しい**
- **S3互換ストレージに保存可能**（Backblaze B2 / Cloudflare R2 / iDrive e2 / Wasabi など）。VPSのディスクも回線もほとんど使いません
- 8MBずつの分割アップロードなので、Cloudflare経由でも大きなファイルを送れます
- 外部サービスはすべて自前実装（S3・SMTP・Turnstileも自前実装）

**収益化**
- 広告枠7か所＋`<head>`共通タグ（AdSense・Googleアナリティクス・サイト認証）＋`</body>`直前タグ欄
- ads.txt を管理画面から編集

**見た目**
- スマホではカード型に切り替わるレスポンシブデザイン、ダークモード対応
- ファビコン・OGP（SNSにURLを貼った時のカード表示）対応

## 必要なもの

- Rust 1.70 以上（cargo）

## 設置

1. ビルドします:

```sh
cargo build --release
```

2. `config-sample.toml` を **`config.toml`** という名前でコピーし、最低限次を変更
   - `title` / `description`（サイト名と説明）
   - `admin_password`（管理パスワード）
   - `secret`（長いランダム文字列。例: `openssl rand -hex 32` の出力）
   - Cloudflare 経由なら `behind_cloudflare = true`
3. `data/` フォルダに書き込み権限を付けます（無ければ自動生成されますが、権限は運営側で確認してください）
4. `./target/release/likeget-uploader serve` で起動します。既定は `0.0.0.0:8080`（`listen` で変更可）。例:

```toml
listen = "0.0.0.0:8080"
```

5. リバースプロキシでも直接でもOK。`https://あなたのサイト/data/uploader.sqlite` への直接アクセスができないよう、サーバー設定で必ず遮断してください。

`/download/123` 形式のURLが使えない環境では、`config.toml` の `pretty_urls` を `false` にしてください。

## 設定

`config.toml` で行います（各項目にコメントがあります）。現在の設定は管理画面の「設定状況」で確認できます。

### S3 に保存する

`storage = "s3"` にして、`[s3]` セクション（エンドポイント・バケット・キー）を書きます。管理画面の「設定状況」の「S3接続テスト」で確認できます。

- バケットは**非公開**のままでOK（ダウンロード時に、数十秒有効な署名付きURLへ転送します）
- 公開バケットやCDN経由で配りたい場合は `public_url` を設定
- 途中で切り替えても既存ファイルはそのまま配信されます。サーバーのファイルをS3へ移すには `likeget-uploader move-to-s3`
- キーには、そのバケットだけの読み書き・削除権限を付けるのがおすすめ
- バケットのライフサイクル設定で「未完了のマルチパートアップロードを数日で削除」も入れておくと安全

### Turnstile（人間確認）

Cloudflare ダッシュボードの Turnstile でサイトを追加し、サイトキーとシークレットキーを`[turnstile]` セクションに書きます。`upload` / `download` / `contact` / `admin` を個別に true / false で切り替えられます。

### メール通知（SMTP）

`[mail]` セクションを設定します。お問い合わせ・通報・新規アップロードの通知を、それぞれ有効/無効にできます。

### アクセス解析・AdSense

管理画面の「解析・共通タグ」に貼り付けます。

- Googleアナリティクス、AdSense（審査用コード、自動広告設定）、サイト認証の meta タグ（1つで複数可）
- ads.txt（同じ画面の ads.txt 欄）。/ads.txt で配信
- 位置を決めて出すバナー広告（「広告枠」タブ）

| 広告枠 | 場所 |
|---|---|
| top | 全ページ上部 |
| list_top / list_middle / list_bottom | 一覧の上/ 10件目の後/ 下 |
| detail_top / detail_bottom | ファイルページの上/ ダウンロードボタンの下 |
| side | 右サイドバー（スマホでは一覧の下） |

### ファビコン・OGP

最初からサンプルのアイコンとOGP画像が入っています。独自のものに替えるときは、次のファイルを同じ名前で差し替えてください。

| ファイル | 用途 | サイズ |
|---|---|---|
| `favicon.ico` | ブラウザのタブ（古いブラウザ用） | 16/32/48px |
| `assets/favicon.svg` | ブラウザのタブ | 正方形のSVG |
| `assets/apple-touch-icon.png` | スマホのホーム画面 | 180×180 |
| `assets/ogp.png` | SNSのカード画像 | 1200×630 |

### デモモード

サンプルとして公開する場合、`[demo] enabled = true` にすると、新しくアップロードされたファイルが一定時間（既定: 4時間）で自動削除され、サイズ上限も下がります。

## uploader.jp からの引っ越し

### 1. バックアップ（手元のPCで実行）

Python 3.8 以上があれば、追加のインストールなしで動きます。

```sh
python tools/getuploader_backup.py アップローダー名 ./保存先フォルダ
```

例: `https://ux.getuploader.com/vahren/` なら `python tools/getuploader_backup.py vahren ./vahren_backup`

- まず `--limit 5` で数件だけ試すのがおすすめ
- 相手のサーバーに負荷をかけないよう1秒間隔で取得します（500件ほどで1時間前後）
- 止めても、同じコマンドで続きから再開できます
- 詳細ページのMD5と照合して、一致したものだけ保存します
- パスワード付きのファイルなど、取得できなかったものは `failed.jsonl` に記録されます

### 2. 取り込み（サーバーで実行）

```sh
likeget-uploader import ./保存先フォルダ
```

番号・ファイル名・コメント（一覧で省略されている部分も全部）・日時・DL数がそのまま入ります。新しいアップロードは最大番号の次から始まります。S3保存に設定してあれば、ファイルはS3へ送られます。

> **ファイルの権利について**: バックアップしたファイルの著作権は、それぞれの投稿者にあります。新しいアップローダーへの移行や、コミュニティでの保存・再公開には投稿者の意向に配慮してください。

## バックアップ

`data/` フォルダを丸ごとコピーすればすべてです（DB・ファイル・設定テキスト）。S3保存の場合は、ファイル本体があるバケットも合わせてバックアップしてください。

## コマンド一覧

```sh
likeget-uploader serve              # サーバー起動（既定）
likeget-uploader import <フォルダ> [--move]   # getuploader_backupの取り込み
likeget-uploader move-to-s3 [--keep]          # ローカル保存のファイルをS3へ移行
```

## ファイル構成

```
likeget-uploader/  Rust本体（cargo project）
assets/            CSS・JavaScript
data/              DB・ファイル・設定テキスト（外部から見えない）
tools/             uploader.jp バックアップスクリプト（Python）
config.toml        設定（配置時に config-sample.toml からコピー）
```

## ライセンス

[MIT License](LICENSE)
