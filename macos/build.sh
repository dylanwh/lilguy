#!/bin/bash
set -euo pipefail
DOMAIN="app.lilguy"
EXE="lilguy"

if ! command -v cargo-get &> /dev/null; then
    cargo install cargo-get
fi

VERSION="$(cargo get package.version)"
cargo build --locked --target x86_64-apple-darwin --release
cargo build --locked --target aarch64-apple-darwin --release

mkdir -p target/universal-apple-darwin/release
lipo -create \
    target/x86_64-apple-darwin/release/$EXE \
    target/aarch64-apple-darwin/release/$EXE \
    -output target/universal-apple-darwin/release/$EXE

xcrun codesign \
    --sign "Developer ID Application: $APPLE_DEVELOPER_NAME ($APPLE_TEAM_ID)" \
    --timestamp \
    --options runtime \
    --entitlements macos/entitlements.plist \
    target/universal-apple-darwin/release/$EXE

pkgbuild --root target/universal-apple-darwin/release \
    --identifier "$DOMAIN.$EXE" \
    --version "$VERSION" \
    --install-location /usr/local/bin \
    --sign "Developer ID Installer: $APPLE_DEVELOPER_NAME ($APPLE_TEAM_ID)" \
    target/$EXE.pkg

pandoc -s -o macos/Resources/welcome.rtf macos/welcome.md
pandoc -s -o macos/Resources/conclusion.rtf macos/conclusion.md

productbuild \
    --distribution macos/Distribution.xml \
    --resources macos/Resources/ --package-path target/ unsigned-$EXE.pkg

productsign --sign "Developer ID Installer: $APPLE_DEVELOPER_NAME ($APPLE_TEAM_ID)" unsigned-$EXE.pkg $EXE.pkg

xcrun notarytool submit $EXE.pkg \
    --apple-id "$APPLE_ID" \
    --password "$APPLE_ID_PASSWORD" \
    --team-id "$APPLE_TEAM_ID" \
    --wait

xcrun stapler staple $EXE.pkg