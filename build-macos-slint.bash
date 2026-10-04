#!/bin/bash
#
# Build a self-contained macOS installer for gephgui-slint.
#
# This is a single-script replacement for the official build-common.bash +
# build-macos.bash flow. It:
#   1. stages gephgui-slint + geph5 into a machine-local build directory
#   2. builds the Slint GUI and geph5 binaries with Cargo
#   3. assembles Geph.app
#   4. optionally code-signs the app
#   5. builds the component pkg and distribution pkg
#   6. optionally notarizes + staples the pkg
#   7. publishes the final pkg to ./output/
#
# Run from anywhere:
#   ./build-macos-slint.bash
#
# Useful overrides:
#   ARCHS="aarch64-apple-darwin"
#   ARCHS="x86_64-apple-darwin aarch64-apple-darwin"
#   GEPHGUI_PKG_BUILD_ROOT="$HOME/.cache/gephgui-pkg"
#   APP_SIGN_ID="Developer ID Application: ..."
#   INSTALLER_SIGN_ID="Developer ID Installer: ..."
#   NOTARY_PROFILE="geph-notary"
#   CARGO_OFFLINE=0
#
# The script assumes the repository has:
#   gephgui-slint/
#   geph5/
#   macos/template.app/
#   macos/component.plist
#   macos/pkg-scripts/
#   macos/resources/
#   macos/uninstall.sh
#

set -euo pipefail

cd "$(dirname "$0")"
REPO_ROOT="$PWD"

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------

MACOS_DIR="$REPO_ROOT/macos"

LOCAL_BUILD_ROOT="${GEPHGUI_PKG_BUILD_ROOT:-${XDG_CACHE_HOME:-$HOME/.cache}/gephgui-slint-pkg}"
LOCAL_SRC="$LOCAL_BUILD_ROOT/src"
LOCAL_MACOS="$LOCAL_BUILD_ROOT/macos"

export CARGO_TARGET_DIR="$LOCAL_BUILD_ROOT/target"

OUTPUT="$REPO_ROOT/output"

BUNDLE_ID="io.geph.GephGui"
APP_NAME="Geph.app"
GUI_BINARY="gephgui-slint"

# Default: Apple Silicon.
# For universal:
#   ARCHS="x86_64-apple-darwin aarch64-apple-darwin" ./build-macos-slint.bash
# For x86_64
#   ARCHS="x86_64-apple-darwin" ./build-macos-slint.bash
ARCHS="${ARCHS:-aarch64-apple-darwin}"

# Use offline Cargo builds by default, matching the official script.
CARGO_OFFLINE="${CARGO_OFFLINE:-0}"

NORMAL_VERSION="${VERSION:-$(git -C "$REPO_ROOT" describe --always 2>/dev/null || echo 0.1.0)}"
VERSION="${NORMAL_VERSION#v}"

ARTIFACT="$OUTPUT/geph-macos-slint-${VERSION}.pkg"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

min_os_for() {
    case "$1" in
        aarch64-apple-darwin)
            echo "11.0"
            ;;
        x86_64-apple-darwin)
            echo "10.15"
            ;;
        *)
            echo "10.15"
            ;;
    esac
}

lipo_or_copy() {
    local out="$1"
    shift

    if [ "$#" -eq 1 ]; then
        cp "$1" "$out"
    else
        lipo -create -output "$out" "$@"
    fi
}

publish() {
    local src="$1"
    local dst="$2"
    local tmp

    mkdir -p "$(dirname "$dst")"

    tmp="$(dirname "$dst")/.${dst##*/}.partial"
    cp "$src" "$tmp"
    mv -f "$tmp" "$dst"
}

copy_tree() {
    local src="$1"
    local dst="$2"
    shift 2

    mkdir -p "$dst"

    if command -v rsync >/dev/null 2>&1; then
        local args=(-a --delete)
        local ex

        for ex in "$@"; do
            args+=(--exclude "/$ex")
        done

        rsync "${args[@]}" "$src/" "$dst/"
    else
        # macOS normally has rsync installed on the Geph build machines.
        # Keep a simple fallback for environments where it is unavailable.
        rm -rf "$dst"
        mkdir -p "$dst"

        local tar_args=()
        local ex

        for ex in "$@"; do
            tar_args+=(--exclude="./$ex")
        done

        (
            cd "$src"
            tar -cf - "${tar_args[@]}" .
        ) | (
            cd "$dst"
            tar -xf -
        )
    fi
}

set_plist_string() {
    local plist="$1"
    local key="$2"
    local value="$3"

    if plutil -extract "$key" raw -o /dev/null "$plist" >/dev/null 2>&1; then
        plutil -replace "$key" -string "$value" "$plist"
    else
        plutil -insert "$key" -string "$value" "$plist"
    fi
}

find_identity() {
    security find-identity -v -p basic 2>/dev/null \
        | grep -o "\"$1: [^\"]*\"" \
        | head -1 \
        | tr -d '"' || true
}

# ---------------------------------------------------------------------------
# Signing configuration
# ---------------------------------------------------------------------------

NOTARY_PROFILE_NAME="geph-notary"

APP_SIGN_ID="${APP_SIGN_ID:-$(find_identity 'Developer ID Application')}"
INSTALLER_SIGN_ID="${INSTALLER_SIGN_ID:-$(find_identity 'Developer ID Installer')}"

if [ -z "${NOTARY_PROFILE:-}" ] && [ -n "$INSTALLER_SIGN_ID" ]; then
    NOTARY_PROFILE="$NOTARY_PROFILE_NAME"
fi

NOTARY_PROFILE="${NOTARY_PROFILE:-}"

echo ">> signing config:"
echo "   app      = ${APP_SIGN_ID:-<none>}"
echo "   installer= ${INSTALLER_SIGN_ID:-<none>}"
echo "   notary   = ${NOTARY_PROFILE:-<none>}"

echo ">> building Geph $VERSION for:"
for t in $ARCHS; do
    echo "   $t (macOS >= $(min_os_for "$t"))"
done

# ---------------------------------------------------------------------------
# 0. Initialize missing submodules
# ---------------------------------------------------------------------------

# git -C "$REPO_ROOT" submodule status --recursive 2>/dev/null \
#     | awk '/^-/ {print $2}' \
#     | while read -r sm; do
#         echo ">> initializing missing submodule: $sm"
#         git -C "$REPO_ROOT" submodule update --init --recursive "$sm"
#     done

# for t in $ARCHS; do
#     rustup target add "$t" >/dev/null 2>&1 || true
# done

# ---------------------------------------------------------------------------
# 1. Stage source trees into a machine-local directory
#
# gephgui-slint and geph5 are kept as siblings because the GUI project may
# contain Cargo [patch]/path dependencies pointing at ../geph5.
# ---------------------------------------------------------------------------

echo ">> staging sources into $LOCAL_SRC"

copy_tree \
    "$REPO_ROOT/gephgui-slint" \
    "$LOCAL_SRC/gephgui-slint" \
    .git target

copy_tree \
    "$REPO_ROOT/geph5" \
    "$LOCAL_SRC/geph5" \
    .git target

# ---------------------------------------------------------------------------
# 2. Clean local build area
# ---------------------------------------------------------------------------

BUILD_APP="$LOCAL_MACOS/build.app"
STAGE="$LOCAL_MACOS/pkgroot"
CARGO_OUT="$LOCAL_MACOS/cargo-out"

rm -rf \
    "$BUILD_APP" \
    "$STAGE" \
    "$CARGO_OUT" \
    "$LOCAL_MACOS/geph-component.pkg" \
    "$LOCAL_MACOS/distribution.xml"

mkdir -p \
    "$OUTPUT" \
    "$LOCAL_MACOS" \
    "$CARGO_OUT"

# Only remove old macOS packages. Do not touch other OS artifacts.
rm -f "$OUTPUT"/geph-macos-*.pkg

# ---------------------------------------------------------------------------
# 3. Assemble application bundle
# ---------------------------------------------------------------------------

echo ">> assembling $APP_NAME"

rsync -aW --delete \
    "$MACOS_DIR/template.app/" \
    "$BUILD_APP/"

# Ensure the template points at the Slint executable.
set_plist_string \
    "$BUILD_APP/Contents/Info.plist" \
    "CFBundleExecutable" \
    "$GUI_BINARY"

set_plist_string \
    "$BUILD_APP/Contents/Info.plist" \
    "CFBundleShortVersionString" \
    "$VERSION"

set_plist_string \
    "$BUILD_APP/Contents/Info.plist" \
    "CFBundleVersion" \
    "$VERSION"

# ---------------------------------------------------------------------------
# 4. Build binaries
# ---------------------------------------------------------------------------

gui_inputs=()
mgr_inputs=()
engine_inputs=()

CARGO_OFFLINE_ARG=""
if [ "$CARGO_OFFLINE" = "1" ]; then
    CARGO_OFFLINE_ARG="--offline"
fi

for t in $ARCHS; do
    export MACOSX_DEPLOYMENT_TARGET="$(min_os_for "$t")"

    echo ">> building Slint GUI for $t"

    cargo install \
        $CARGO_OFFLINE_ARG \
        --force \
        --locked \
        --target "$t" \
        --path "$LOCAL_SRC/gephgui-slint" \
        --root "$CARGO_OUT/gui-$t"

    echo ">> building geph5 manager for $t"

    cargo install \
        $CARGO_OFFLINE_ARG \
        --force \
        --locked \
        --target "$t" \
        --path "$LOCAL_SRC/geph5/binaries/geph5-app" \
        --root "$CARGO_OUT/manager-$t"

    echo ">> building geph5-client for $t"

    cargo install \
        $CARGO_OFFLINE_ARG \
        --force \
        --locked \
        --target "$t" \
        --path "$LOCAL_SRC/geph5/binaries/geph5-client" \
        --root "$CARGO_OUT/engine-$t" \
        --features aws_lambda

    gui_inputs+=("$CARGO_OUT/gui-$t/bin/gephgui-slint")
    mgr_inputs+=("$CARGO_OUT/manager-$t/bin/geph5")
    engine_inputs+=("$CARGO_OUT/engine-$t/bin/geph5-client")
done

mkdir -p "$BUILD_APP/Contents/MacOS"

lipo_or_copy \
    "$BUILD_APP/Contents/MacOS/gephgui-slint" \
    "${gui_inputs[@]}"

lipo_or_copy \
    "$BUILD_APP/Contents/Resources/geph" \
    "${mgr_inputs[@]}"

lipo_or_copy \
    "$BUILD_APP/Contents/Resources/geph5-client" \
    "${engine_inputs[@]}"

# gui_inputs=()
# mgr_inputs=()
# engine_inputs=()

# for t in $ARCHS; do
#     export MACOSX_DEPLOYMENT_TARGET="$(min_os_for "$t")"

#     echo ">> building Slint GUI for $t"

#     CARGO_ARGS=()
#     if [ "$CARGO_OFFLINE" = "1" ]; then
#         CARGO_ARGS+=(--offline)
#     fi

#     cargo install \
#         "${CARGO_ARGS[@]}" \
#         --force \
#         --locked \
#         --target "$t" \
#         --path "$LOCAL_SRC/gephgui-slint" \
#         --root "$CARGO_OUT/gui-$t"

#     echo ">> building geph5 manager for $t"

#     cargo install \
#         "${CARGO_ARGS[@]}" \
#         --force \
#         --locked \
#         --target "$t" \
#         --path "$LOCAL_SRC/geph5/binaries/geph5-app" \
#         --root "$CARGO_OUT/manager-$t"

#     echo ">> building geph5-client for $t"

#     cargo install \
#         "${CARGO_ARGS[@]}" \
#         --force \
#         --locked \
#         --target "$t" \
#         --path "$LOCAL_SRC/geph5/binaries/geph5-client" \
#         --root "$CARGO_OUT/engine-$t" \
#         --features aws_lambda

#     gui_inputs+=(
#         "$CARGO_OUT/gui-$t/bin/gephgui-slint"
#     )

#     mgr_inputs+=(
#         "$CARGO_OUT/manager-$t/bin/geph5"
#     )

#     engine_inputs+=(
#         "$CARGO_OUT/engine-$t/bin/geph5-client"
#     )
# done

# mkdir -p "$BUILD_APP/Contents/MacOS"

# lipo_or_copy \
#     "$BUILD_APP/Contents/MacOS/gephgui-slint" \
#     "${gui_inputs[@]}"

# lipo_or_copy \
#     "$BUILD_APP/Contents/Resources/geph" \
#     "${mgr_inputs[@]}"

# lipo_or_copy \
#     "$BUILD_APP/Contents/Resources/geph5-client" \
#     "${engine_inputs[@]}"

# # ---------------------------------------------------------------------------
# # 5. Add uninstaller before signing
# # ---------------------------------------------------------------------------

install -m 755 \
    "$MACOS_DIR/uninstall.sh" \
    "$BUILD_APP/Contents/Resources/uninstall.sh"

# ---------------------------------------------------------------------------
# 6. Optional code signing
#
# Sign nested executables first, then the application bundle.
# ---------------------------------------------------------------------------

if [ -n "$APP_SIGN_ID" ]; then
    echo ">> codesigning app with '$APP_SIGN_ID'"

    for f in \
        "Contents/Resources/geph" \
        "Contents/Resources/geph5-client" \
        "Contents/MacOS/$GUI_BINARY"
    do
        codesign \
            --force \
            --options runtime \
            --timestamp \
            -s "$APP_SIGN_ID" \
            "$BUILD_APP/$f"
    done

    codesign \
        --force \
        --options runtime \
        --timestamp \
        -s "$APP_SIGN_ID" \
        "$BUILD_APP"
fi

# ---------------------------------------------------------------------------
# 7. Build component pkg
# ---------------------------------------------------------------------------

echo ">> building component pkg"

mkdir -p "$STAGE/Applications"

mv \
    "$BUILD_APP" \
    "$STAGE/Applications/$APP_NAME"

pkgbuild \
    --root "$STAGE" \
    --component-plist "$MACOS_DIR/component.plist" \
    --scripts "$MACOS_DIR/pkg-scripts" \
    --identifier "$BUNDLE_ID" \
    --version "$VERSION" \
    --install-location "/" \
    --ownership recommended \
    "$LOCAL_MACOS/geph-component.pkg"

# ---------------------------------------------------------------------------
# 8. Build distribution pkg
# ---------------------------------------------------------------------------

DIST_XML="$LOCAL_MACOS/distribution.xml"

cat > "$DIST_XML" <<XML_EOF
<?xml version="1.0" encoding="utf-8"?>
<installer-gui-script minSpecVersion="1">
    <title>Geph</title>
    <organization>io.geph</organization>

    <options
        customize="never"
        require-scripts="true"
        hostArchitectures="x86_64,arm64"
    />

    <domains
        enable_localSystem="true"
        enable_anywhere="false"
        enable_currentUserHome="false"
    />

    <welcome file="welcome.html"/>
    <conclusion file="conclusion.html"/>

    <volume-check>
        <allowed-os-versions>
            <os-version min="10.15"/>
        </allowed-os-versions>
    </volume-check>

    <choices-outline>
        <line choice="default">
            <line choice="$BUNDLE_ID"/>
        </line>
    </choices-outline>

    <choice id="default"/>

    <choice id="$BUNDLE_ID" visible="false">
        <pkg-ref id="$BUNDLE_ID"/>
    </choice>

    <pkg-ref
        id="$BUNDLE_ID"
        version="$VERSION"
        onConclusion="none"
    >geph-component.pkg</pkg-ref>
</installer-gui-script>
XML_EOF

LOCAL_ARTIFACT="$LOCAL_MACOS/$(basename "$ARTIFACT")"

PRODUCTBUILD_ARGS=(
    --distribution "$DIST_XML"
    --package-path "$LOCAL_MACOS"
    --resources "$MACOS_DIR/resources"
)

if [ -n "$INSTALLER_SIGN_ID" ]; then
    PRODUCTBUILD_ARGS+=(
        --sign "$INSTALLER_SIGN_ID"
    )
fi

echo ">> building distribution pkg"

productbuild \
    "${PRODUCTBUILD_ARGS[@]}" \
    "$LOCAL_ARTIFACT"

# ---------------------------------------------------------------------------
# 9. Optional notarization + stapling
# ---------------------------------------------------------------------------

if [ -n "$NOTARY_PROFILE" ]; then
    echo ">> notarizing"

    xcrun notarytool submit \
        "$LOCAL_ARTIFACT" \
        --keychain-profile "$NOTARY_PROFILE" \
        --wait

    echo ">> stapling"

    xcrun stapler staple \
        "$LOCAL_ARTIFACT"
fi

# ---------------------------------------------------------------------------
# 10. Publish final artifact
# ---------------------------------------------------------------------------

publish \
    "$LOCAL_ARTIFACT" \
    "$ARTIFACT"

# ---------------------------------------------------------------------------
# 11. Cleanup local intermediate packaging files
#
# Keep Cargo's local target cache so subsequent builds remain incremental.
# ---------------------------------------------------------------------------

rm -rf \
    "$STAGE" \
    "$CARGO_OUT" \
    "$LOCAL_MACOS/geph-component.pkg" \
    "$LOCAL_ARTIFACT" \
    "$DIST_XML"

echo
echo ">> done:"
echo "   $ARTIFACT"
