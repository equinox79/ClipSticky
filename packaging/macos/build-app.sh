#!/usr/bin/env bash
# macOS 用の ClipSticky.app(Apple Silicon と Intel の両対応)を作り、zip にする。
# Mac 上で実行する: ./packaging/macos/build-app.sh
# 出力: target/macos/ClipSticky.app と target/macos/ClipSticky-v<版>-macos-universal.zip
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
(cd "$OUT" && shasum -a 256 "$(basename "$ZIP")" > "$(basename "$ZIP").sha256")
echo "作成しました: $ZIP"
