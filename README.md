# ClipSticky

クリップボードの最新の内容を、画面の隅に付箋のように表示する小さな常駐アプリです。
コピーするたびに表示が更新され、他のアプリのフォーカスは奪いません。Windows と macOS に対応しています(Rust 製)。

<p align="center">
  <img src="docs/images/demo.gif" alt="ClipSticky の動作: 文章・複数行のコマンド・画像・ファイル・文字コードを付箋に表示" width="576">
</p>

- テキスト・画像・ファイル(コピーしたファイルのパス一覧)に対応
- 最前面・タスクバー/Dock に出ない・フォーカスを奪わない
- 角丸のモダンなデザイン。テーマ(イエロー / ライト / ダーク)を切り替え可能
- 制御文字・不可視文字の表示、文字コード一覧(Unicode / UTF-8)、文字数・行数・バイト数の表示
- 透明度、背景画像、フォント(書体とサイズ)の指定
- タスクトレイ / メニューバーに常駐

> **macOS 版について**: Apple M2 の Mac で動作を確認しています。Intel の Mac や、ほかの macOS のバージョンでは未確認です。
> 不具合があれば Issue で教えてください。

## スクリーンショット

<table>
  <tr>
    <td align="center"><img src="docs/images/scene-1.png" alt="文章(イエローのテーマ)" width="320"><br>文章をそのまま表示</td>
    <td align="center"><img src="docs/images/scene-2.png" alt="複数行のコマンド(ライトのテーマ)" width="320"><br>複数行のコマンドを確認</td>
    <td align="center"><img src="docs/images/scene-3.png" alt="制御文字の表示(ダークのテーマ)" width="320"><br>制御文字を表示して、末尾の改行に気づく</td>
  </tr>
  <tr>
    <td align="center"><img src="docs/images/scene-4.png" alt="画像" width="320"><br>画像もそのまま見られる</td>
    <td align="center"><img src="docs/images/scene-5.png" alt="ファイルの一覧" width="320"><br>コピーしたファイルはパスの一覧</td>
    <td align="center"><img src="docs/images/scene-6.png" alt="文字コード一覧" width="320"><br>文字コード(Unicode / UTF-8)の一覧</td>
  </tr>
</table>

「制御文字を表示」にすると、コピーした複数行のコマンドの末尾に改行(`↓`)が含まれているかどうかが一目で分かります。
ターミナルに貼り付けたときに、うっかり実行されるのを防げます。

サンプルの文章は、夏目漱石『吾輩は猫である』(1905 年)の冒頭です。著作権の保護期間が満了しており、自由に利用できます。
画像は、このために描いたものです。

## インストール

[Releases](https://github.com/equinox79/ClipSticky/releases) から、お使いの OS 用の zip をダウンロードします。
どちらも `.sha256` ファイルで、ダウンロードしたファイルを確認できます。

### Windows

`ClipSticky-vX.Y.Z-windows-x64.zip` を展開し、`clipsticky.exe` を好きな場所に置いて実行します。インストーラーはありません。
削除するときは exe を消すだけです(設定は `%APPDATA%\ClipSticky` に残ります)。

署名していないため、初回起動時に SmartScreen が警告を出すことがあります。
その場合は「詳細情報」→「実行」を選んでください。

### macOS

`ClipSticky-vX.Y.Z-macos-universal.zip`(Apple Silicon・Intel 両対応)を展開し、`ClipSticky.app` を「アプリケーション」に移動します。

Apple の公証(有料)を受けていないため、初回は「開発元を検証できません」と表示されます。次のどちらかで開いてください。

- `ClipSticky.app` を**右クリック → 開く**
- ターミナルで検疫の属性を外す: `xattr -dr com.apple.quarantine /Applications/ClipSticky.app`

Dock には出ず、メニューバーにアイコンが出ます。

## ビルドと実行

[Rust](https://www.rust-lang.org/tools/install)(stable)が必要です。

```sh
cargo run --release
```

- Windows: `target\release\clipsticky.exe` が単体で動きます。
- macOS: `./packaging/macos/build-app.sh` で、`target/macos/ClipSticky.app` と zip を作れます(ad-hoc 署名)。

## 使い方

| 操作 | 内容 |
| --- | --- |
| クリップボードにコピー | 内容を表示し、ウィンドウを前面に出す(フォーカスは奪わない) |
| ウィンドウをドラッグ | 移動 |
| 端・右下のつまみをドラッグ | リサイズ |
| マウスホイール | 長いテキストのスクロール |
| 右上の × | ウィンドウを隠す(アプリは終了しない) |
| ウィンドウを右クリック | メニューを開く |
| トレイ / メニューバーのアイコン | Windows は左クリックで表示・右クリックでメニュー。macOS はクリックでメニュー |

### メニュー

- **表示**: 隠れているウィンドウを出す
- **監視を一時停止 / 再開**: クリップボードの更新に反応するかどうか
- **テキスト表示**: 通常 / 制御文字を表示 / 文字コード一覧(下記)
- **テーマ**: イエロー / ライト / ダーク
- **透明度**: 100% 〜 50%(標準は 96%)。背景だけが透け、文字は透けません
- **背景画像**: 画像を選択 / 解除(画像ファイルをウィンドウにドロップしても設定できます)
- **フォント**: 書体とサイズを選ぶ / 既定に戻す(標準は Windows: Yu Gothic UI、macOS: Hiragino Sans、15px)
- **終了**: アプリを終了する

### テキストの表示形式

下部には、文字数・行数・UTF-8 でのバイト数を表示します(例: `28 文字 · 3 行 · UTF-8 58B`)。

- **通常**: そのまま表示します(タブは 4 桁ごとに展開します)。
- **制御文字を表示**: エディタの「制御文字を表示」と同じように、記号だけを薄い色で重ねて表示します。

  | 文字 | 表示 |
  | --- | --- |
  | 改行 CRLF / LF / CR | `↵` / `↓` / `←`(記号の後ろで実際にも改行します) |
  | タブ | `→`(次のタブ位置まで空白で埋めます。4 桁ごと) |
  | 半角スペース / 全角スペース / NBSP | `·` / `□` / `°` |
  | テキストの終わり | `[EOF]`(末尾に改行があるかが分かります) |
  | そのほかの制御文字・不可視文字 | `⟦NUL⟧`、`⟦ZWSP⟧`、`⟦BOM⟧` のように名前で表示 |
- **文字コード一覧**: 1 文字 1 行で、文字 / Unicode コードポイント / UTF-8 のバイト列 / 名前(制御文字など)を並べます。
  先頭の 1,000 文字までです。

どの形式でも、表示は先頭の 20,000 文字(文字コード一覧は 1,000 文字)までで、超えた分は省略します。

### フォント

「フォント」→「フォントを選択…」で、インストール済みのフォントの一覧と、本文のサイズを選べます
(選ぶとすぐに反映され、OK で確定、キャンセルで元に戻ります)。
アプリ名やフッターなどの小さな文字は固定サイズです。

### 背景画像

PNG / JPEG / BMP / GIF / WebP に対応しています。縦横比を保ったまま全面に敷き(はみ出す部分は中央でトリミング)、
文字が読めるように上からテーマの色を重ねます。重ねる濃さは `src/app.rs` の `SCRIM_ALPHA` で変えられます。
大きな画像は読み込み時に長辺 1280px まで縮小します。

## 設定ファイル

位置・サイズ・透明度・テーマ・表示形式・背景画像・フォントは、次の場所に保存されます。

- Windows: `%APPDATA%\ClipSticky\settings.txt`
- macOS: `~/Library/Application Support/ClipSticky/settings.txt`

`key=value` の行形式で、手で編集することもできます。位置とサイズの単位は論理ポイント(画面の拡大率に依存しない値)です。

```
left=1500.0
top=24.0
width=380.0
height=220.0
opacity=96
theme=yellow
view=normal
background=C:\Users\me\Pictures\wallpaper.png
font=Meiryo
font_size=16
```

`theme` は `yellow` / `light` / `dark`、`view` は `normal` / `controls` / `codes` です。
`background`、`font`、`font_size` は省略できます。ファイルを消すと初期状態に戻ります。

## 仕組み

GUI は [egui](https://github.com/emilk/egui)(eframe)で、Windows と macOS で同じ描画コードを使います。OS ごとに違う部分だけを `platform/` に分けています。

```
src/
  main.rs        起動(ウィンドウの設定)
  app.rs         描画と操作(egui)
  clipboard.rs   クリップボードの監視と読み取り(arboard)
  menu.rs        メニュー(muda)とトレイアイコン(tray-icon)
  fonts.rs       システムのフォントの読み込み(fontdb)
  core/          OS に依存しない部分(内容の型、テキスト加工、設定、配色)。単体テストあり
  platform/
    windows.rs   フォーカスを奪わないスタイル、表示/非表示、枠線、メニューの表示
    macos.rs     アクセサリアプリ化、全 Space で最前面、表示/非表示、メニューの表示
```

- クリップボードは、別スレッドで変更番号(Windows: `GetClipboardSequenceNumber`、macOS: `NSPasteboard.changeCount`)を
  150ms ごとに見て、変わったら読み取ります。他のプロセスが使用中のときは、50ms 間隔で最大 5 回リトライします。
- ウィンドウは透明な枠なしウィンドウで、角丸・背景・透明度は自前で描画しています。
- Windows では、winit がウィンドウの拡張スタイルを書き換えるため、フォーカスを奪わないスタイル
  (`WS_EX_NOACTIVATE`)を毎フレーム確認して付け直し、表示/非表示も Win32 を直接呼んでいます。
  また、DWM が非クライアント領域の最外周 1px を不透明な線として描くため、描画範囲をクライアント領域に切り詰めて
  (`SetWindowRgn`)、角丸の外側(四隅)に線が残らないようにしています。

## ライセンス

[MIT License](LICENSE)
