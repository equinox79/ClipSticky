#!/usr/bin/env bash
# macOS 用の ClipSticky.app(Apple Silicon と Intel の両対応)を作り、dmg と zip にする。
# Mac 上で実行する: ./packaging/macos/build-app.sh
# 出力: target/macos/ClipSticky.app と、同じ場所の ClipSticky-v<版>-macos-universal.dmg / .zip
#
# 署名は ad-hoc(無料)のみ。Apple の公証には有料の Developer Program が必要なため行わない。
set -euo pipefail
cd "$(dirname "$0")/../.."

VERSION=$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')
OUT=target/macos
APP=$OUT/ClipSticky.app

rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo build --release --locked --target aarch64-apple-darwin
cargo build --release --locked --target x86_64-apple-darwin

rm -rf "$OUT"
mkdir -p "$APP/Contents/MacOS"
lipo -create -output "$APP/Contents/MacOS/clipsticky" \
  target/aarch64-apple-darwin/release/clipsticky \
  target/x86_64-apple-darwin/release/clipsticky
sed "s/__VERSION__/$VERSION/g" packaging/macos/Info.plist > "$APP/Contents/Info.plist"

# Apple Silicon では署名のないバイナリは起動できない。ad-hoc 署名を付ける
codesign --force --deep --sign - "$APP"

ZIP="$OUT/ClipSticky-v$VERSION-macos-universal.zip"
ditto -c -k --keepParent "$APP" "$ZIP"

# インストール用のディスクイメージ(アプリを「アプリケーション」へドラッグする形)
DMG="$OUT/ClipSticky-v$VERSION-macos-universal.dmg"
STAGE=$OUT/dmg
mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
hdiutil create -volname "ClipSticky" -srcfolder "$STAGE" -ov -format UDZO "$DMG"
rm -rf "$STAGE"

(cd "$OUT" && for f in "$(basename "$ZIP")" "$(basename "$DMG")"; do shasum -a 256 "$f" > "$f.sha256"; done)
echo "作成しました: $DMG, $ZIP"
