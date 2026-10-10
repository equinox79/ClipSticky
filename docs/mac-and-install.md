# Mac 対応と配布(インストール)の検討

状態: **検討メモ(未実装)**。実装前に方針を決めるための文書です。

## 1. 現状

- 実装は Windows 専用(Win32 + GDI + DWM、`src/main.rs` に約 1,200 行)。
- 配布物は `cargo build --release` で作る単体 exe のみ。インストーラー・自動起動・署名は無い。

## 2. Mac 版の方針

### 2.1 構成案: 共通部分と OS 依存部分を分ける

```
src/
  core/        # OS 非依存(そのまま流用できる)
    content.rs   Content / show_controls / code_rows / display_text / detail
    settings.rs  設定の読み書き(保存先だけ OS ごと)
    theme.rs     パレット
  ui/          # 共通 UI(egui/eframe) …下記の選択肢 A の場合
  platform/
    windows.rs   クリップボード監視(WM_CLIPBOARDUPDATE)、NOACTIVATE、トレイ、DWM 角丸
    macos.rs     クリップボード監視(ポーリング)、nonactivating パネル、メニューバー
```

流用できる量が多いのは、文字の分析・表示用テキストの生成・設定・配色です。
Win32 の描画(`paint`)と、ウィンドウ・クリップボード・トレイの部分は書き直しになります。

### 2.2 UI の選択肢

| | A. 共通 GUI(egui/eframe) | B. Mac だけ別実装(objc2 + AppKit) |
| --- | --- | --- |
| UI コード | 1 本に統合 | Windows 用(GDI)と Mac 用の 2 本 |
| Windows 版への影響 | **描画が変わる**(見た目の再現と再確認が必要) | 無し |
| 日本語の表示 | フォントの読み込み設定が必要(egui は既定で日本語を持たない) | OS のフォントをそのまま使える |
| ネイティブ感 | 低い(自前描画) | 高い(NSVisualEffectView のぼかしなど) |
| 保守コスト | 低い | 高い |
| 向いている場合 | 機能追加を両 OS で揃えたい | Mac の見た目と挙動にこだわる |

**推奨は A**。機能追加のたびに 2 回書くのは続きません。ただし、日本語フォントの扱いと、
ウィンドウを非アクティブにする処理(後述)は egui の外で書く必要があります。
代替として、描画を GPU 任せにせず、ウィンドウは `winit`/`tao` で作り、描画は `tiny-skia` + `cosmic-text` で自前、という手もあります。
この場合はテキスト描画が自前になるので、手間は増えます。

### 2.3 機能ごとの Mac での実現方法

| 機能 | Mac での方法 | 注意点 |
| --- | --- | --- |
| クリップボード監視 | `NSPasteboard.general.changeCount` を 0.2〜0.5 秒ごとに確認し、変わったら読む | 変更通知の API が無いため**ポーリングが必須**。CPU への負担はごく小さい |
| テキスト・画像・ファイルの読み取り | `NSPasteboard` の `string` / `png` / `tiff` / `fileURL` を `objc2-app-kit` で読む | `arboard` クレートでも文字と画像は読めるが、ファイル一覧は読めない |
| 最前面でフォーカスを奪わない | `NSPanel` + `.nonactivatingPanel` + `level = .floating`、`collectionBehavior` に `canJoinAllSpaces` と `fullScreenAuxiliary` | `winit` の標準 API だけでは足りないので、`objc2` で NSWindow を直接設定する |
| Dock・Cmd+Tab に出さない | `Info.plist` の `LSUIElement = true`(アクセサリアプリ) | `.app` として動かす必要がある(`cargo run` では Dock に出る) |
| メニューバーのアイコン | `tray-icon` クレート(+ `muda` でメニュー) | 左クリックでメニューが出るのが Mac の作法。Windows と操作が変わる |
| 右クリックメニュー | ウィンドウ上の右クリックで同じメニューを出す | `muda` のポップアップを使う |
| 半透明・角丸 | 透明ウィンドウ + 自前の角丸(共通 GUI の場合)、またはぼかしは `NSVisualEffectView` | ぼかしを使うと「背景画像」「透明度」の挙動が Windows と変わる |
| ファイル選択 | `rfd` クレート | |
| フォント選択 | Mac には Windows のような標準ダイアログが無い(`NSFontPanel` は特殊) | インストール済みフォントの一覧から選ぶ自前の画面を作る(`font-kit` で列挙) |
| 設定ファイルの場所 | `~/Library/Application Support/ClipSticky/settings.txt`(`dirs` クレート) | 形式は共通 |
| 背景画像のドロップ | `winit` のファイルドロップイベント | |
| DPI・Retina | `winit` が倍率を扱う | 描画サイズは物理ピクセルで計算する |

### 2.4 実機が必要な確認事項

この開発環境は Windows で、Mac 上の動作は確認できません。次の点は Mac 実機で確認が要ります。

- コピーしても他のアプリのフォーカスが奪われないこと(入力中の文字が途切れない)
- 別の Space・フルスクリーンアプリの上にも付箋が出ること
- メニューバーのアイコンとメニューの操作
- Retina での表示、ダークモード
- 初回起動時の権限(クリップボード読み取りの許可。macOS 15 以降で他アプリのクリップボード読み取りに確認が出る場合がある)

CI(GitHub Actions の macOS ランナー)でビルドが通ることまでは確認できますが、挙動の確認にはなりません。

### 2.5 進め方(段階案)

1. `core/` の切り出し(Windows 版は動作を変えずにリファクタリング)
2. Mac で最小構成: メニューバー常駐 + ポーリング + テキストを出す付箋パネル
3. 画像・ファイル、テーマ、制御文字・文字コード表示
4. 透明度、背景画像、フォント選択
5. `.app` 化・署名・公証・配布(次章)

## 3. インストール方法の検討

### 3.1 共通の要件

- 自動起動(ログイン時に常駐)を選べること
- アンインストール時にアプリを消せること(設定ファイルは残すか選べること)
- 更新手段があること
- 署名されていて、OS の警告が出にくいこと

### 3.2 Windows

| 方式 | 内容 | 長所 | 短所 |
| --- | --- | --- | --- |
| **zip 配布(ポータブル)** | exe を zip で配る | 最も簡単。今すぐできる | 自動起動や更新は手動 |
| **winget** | `winget install ClipSticky`(マニフェストを `microsoft/winget-pkgs` に提出) | コマンド 1 つで入る。更新も `winget upgrade` | インストーラー(MSI/EXE/zip)が先に要る。審査あり |
| **Scoop** | 自前の bucket、または `extras` に登録 | 開発者に人気。zip だけで済む | 一般ユーザーには馴染みが薄い |
| **MSI**(`cargo-wix` / WiX) | 標準のインストーラー | 企業の配布に向く。アンインストールが正しく動く | 作成と保守の手間 |
| **MSIX** | ストア形式。署名必須 | クリーンなインストール・アンインストール | 署名の証明書が必要。ストア外配布は手間が大きい |
| **Microsoft Store** | ストア経由 | 署名と更新を任せられる | 申請と審査。アプリ形式の制約 |

- **署名**: 署名が無いと SmartScreen が警告を出す。OSS 向けには SignPath Foundation(無償)や、Azure の署名サービスがある。実績が貯まるまで警告が出る場合がある。
- **自動起動**: `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` にレジストリを書くのが簡単。設定画面(トレイメニュー)に「ログイン時に起動」を足すのがよい。
- **推奨**: まず **zip + winget**(zip を GitHub Releases に置き、winget のマニフェストから参照する)。需要があれば MSI を足す。

### 3.3 macOS

| 方式 | 内容 | 長所 | 短所 |
| --- | --- | --- | --- |
| **.dmg / .zip(`.app`)** | GitHub Releases で配る | 標準的。ドラッグして Applications へ | 署名と公証が無いと Gatekeeper が止める |
| **Homebrew Cask** | `brew install --cask clipsticky` | コマンド 1 つ。更新は `brew upgrade` | 自前の tap か、`homebrew-cask` への登録(要件あり) |
| **`cargo install`** | Rust 利用者向け | 簡単 | `.app` にならず、`LSUIElement` が効かない。一般向けではない |
| **Mac App Store** | ストア | 署名・更新を任せられる | サンドボックス必須。クリップボード常時監視は審査で問題になりうる |

- **署名と公証**: Apple Developer Program(年額 99 USD)が必要。`codesign` → `notarytool` → `stapler` の流れ。
  公証されていないと「開発元を検証できません」の警告で起動できない(右クリック→開くで回避はできるが、一般向けではない)。
- **Universal Binary**: `aarch64-apple-darwin` と `x86_64-apple-darwin` をビルドして `lipo` で束ねる。
- **自動起動**: `SMAppService`(macOS 13 以降)でログイン項目に登録する。古い OS 向けには LaunchAgent の plist。
- **`.app` の作成**: `cargo-bundle` か `cargo-packager`。`Info.plist` に `LSUIElement`、バンドル ID、アイコン(`.icns`)を設定する。
- **推奨**: **署名・公証済みの `.dmg` を GitHub Releases に置き、Homebrew Cask でも配る**。

### 3.4 共通: リリースの自動化

- GitHub Actions で、タグ(`v1.0.0` など)を付けたら Windows と macOS のランナーでビルドし、成果物を GitHub Releases に載せる。
- Rust 向けのリリース自動化ツールには `cargo-dist` や `cargo-packager` がある。
  `cargo-dist` は zip・MSI・Homebrew の配布物まで自動生成できる。
  ただし、macOS の署名と公証、`.app` 化は追加の設定が必要。
- バージョン番号は `Cargo.toml` を単一の情報源にし、アプリ内表示とアイコンのメタデータにも反映する。

### 3.5 配布に先立って必要なアプリ側の対応

| 項目 | 内容 |
| --- | --- |
| アイコン | 今は実行時に生成した 32×32 のみ。exe のアイコン(`.ico`)と Mac の `.icns` を用意する |
| exe のメタデータ | 名前・版数・著作権(Windows はリソース、`winresource` クレート) |
| 「ログイン時に起動」 | トレイ/メニューバーのメニューに追加 |
| 「バージョン情報」 | メニューに追加(版数と GitHub へのリンク) |
| ライセンス | `LICENSE` ファイル(配布前に必須。未決定) |
| 更新通知 | 最初は無し。GitHub Releases の最新版を確認する簡易チェックは後から追加できる |

## 4. 決定事項

| 項目 | 決定 |
| --- | --- |
| UI の方針 | **共通 GUI**(Windows と Mac で同じ UI コード) |
| Mac 実機 | あり(動作確認に使える) |
| 署名 | 有料の署名はしない。**無料でできる署名は行う**(下記) |
| 配布の範囲 | **当面は GitHub Releases のみ**(winget・Homebrew は後回し) |
| ライセンス | **MIT** |

### 無料でできる署名について

- **Windows**: [SignPath Foundation](https://signpath.org/) がオープンソース向けに無償でコード署名を提供している。
  申請と審査があり、署名は GitHub Actions からの連携で行う。承認されるまでは未署名で配布し、
  README に SmartScreen の回避方法を書く。
- **macOS**: Gatekeeper の警告を完全に消す「公証」は Apple Developer Program(有料)が必須で、無料の代替は無い。
  無料でできるのは **ad-hoc 署名**(`codesign --sign -`)まで。Apple Silicon では署名の無いバイナリは起動できないため、
  ad-hoc 署名は付ける。ただし初回は警告が出るので、README に「右クリック→開く」または
  `xattr -dr com.apple.quarantine ClipSticky.app` の手順を載せる。
  将来、利用者が増えたら公証のために登録を再検討する。
## 5. おすすめの順序

1. ~~ライセンスを決める~~(MIT に決定。`LICENSE` を追加済み)
2. ~~Windows 版を GitHub Releases の zip で配る~~(`.github/workflows/release.yml` を追加済み。`v0.1.0` のようなタグを push すると zip が公開される)
3. 「ログイン時に起動」、アイコン、版数表示を追加し、winget に提出する
4. `core/` を切り出し、Mac 版をプロトタイプする(Mac 実機で確認)
5. Mac の署名・公証・`.dmg`・Homebrew Cask
6. CI でタグから両 OS のリリース物を自動生成する
