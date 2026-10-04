#!/bin/bash
#
# Build the Windows Slint GUI installer on a local Windows build machine
# (git-bash).
#

set -e

cd "$(dirname "$(readlink -f "$0")")"
. ./build-common.bash

# export VERSION="${VERSION:-$(git describe --always)}"
export VERSION="0.1.0"

ARCH="${1:-64}"

case "$ARCH" in
    32|ia32|x86)
        TARGET="i686-pc-windows-msvc"
        WIN_ARCH="ia32"
        ;;
    64|x64|amd64)
        TARGET="x86_64-pc-windows-msvc"
        WIN_ARCH="x64"
        ;;
    *)
        echo "Usage: $0 [32|64]"
        exit 1
        ;;
esac

export WIN_ARCH

ARTIFACT="output/geph-windows-${WIN_ARCH}-${VERSION#v}.exe"

OUTPUT="output"
mkdir -p "$OUTPUT"

# Drop stale artifacts for this architecture.
rm -f "$OUTPUT"/geph-windows-"$WIN_ARCH"-*.exe

# Scrub build junk that older versions of this script left in the
# shared checkout.
rm -rf windows/iscc windows/Output

# Stage the sources plus the installer inputs locally.
stage_sources

WIN="$LOCAL_BUILD_ROOT/win-$WIN_ARCH"

copy_tree windows "$WIN/windows" iscc Output

# Architecture-specific Windows runtime/driver files.
copy_tree "blobs/win-$WIN_ARCH" "$WIN/blobs/win-$WIN_ARCH"

STAGE="$WIN/blobs/win-$WIN_ARCH"

# Make sure the requested Rust target is installed.
rustup target add "$TARGET"

# --- Code-signing hook -------------------------------------------------------

TSCT="/c/Program Files/Trusted Signing Client Tools"

if [ -z "${SIGNTOOL:-}" ]; then
    SIGNTOOL="$(find "$TSCT" -name signtool.exe 2>/dev/null | head -1)"
    [ -n "$SIGNTOOL" ] || \
        SIGNTOOL="$(ls "/c/Program Files (x86)/Windows Kits/10/bin"/10.*/x64/signtool.exe 2>/dev/null | sort -V | tail -1)"
fi

if [ -z "${AZURE_SIGN_DLIB:-}" ]; then
    AZURE_SIGN_DLIB="$(find "$TSCT" -path '*x64*' -name Azure.CodeSigning.Dlib.dll 2>/dev/null | head -1)"
    [ -n "$AZURE_SIGN_DLIB" ] || \
        AZURE_SIGN_DLIB="$(ls ~/.nuget/packages/microsoft.trusted.signing.client/*/bin/x64/Azure.CodeSigning.Dlib.dll 2>/dev/null | sort -V | tail -1)"
fi

AZURE_SIGN_METADATA="${AZURE_SIGN_METADATA:-$WIN/windows/trusted-signing.json}"

if [ ! -x "${SIGNTOOL:-/nonexistent}" ] \
    || [ ! -f "${AZURE_SIGN_DLIB:-/nonexistent}" ] \
    || [ ! -f "$AZURE_SIGN_METADATA" ] \
    || grep -qs REPLACE_ME "$AZURE_SIGN_METADATA"; then

    echo ">> WARNING: Azure Trusted Signing tooling not configured; building UNSIGNED"
    sign() { :; }

else

    echo ">> signing with $SIGNTOOL"

    sign() {
        "$SIGNTOOL" sign \
            /fd SHA256 \
            /td SHA256 \
            /tr http://timestamp.acs.microsoft.com \
            /dlib "$(cygpath -w "$AZURE_SIGN_DLIB")" \
            /dmdf "$(cygpath -w "$AZURE_SIGN_METADATA")" \
            "$(cygpath -w "$1")"
    }

fi

# --- Inno Setup compiler -----------------------------------------------------

rm -rf "$WIN/windows/iscc"
mkdir -p "$WIN/windows/iscc"

unzip -o "$WIN/windows/IS6.zip" -d "$WIN/windows/iscc"

# --- Slint GUI ---------------------------------------------------------------
#
# No WebUI/npm/frontend build is required.
#
cargo install \
    --locked \
    --force \
    --target "$TARGET" \
    --path "$LOCAL_SRC/gephgui-slint"

cp "$(which gephgui-slint)" "$STAGE/"
sign "$STAGE/gephgui-slint.exe"

# --- Manager + engine --------------------------------------------------------

(cd "$LOCAL_SRC/geph5" && \
    cargo build \
        --locked \
        --release \
        --target "$TARGET" \
        -p geph5-app \
        -p geph5-client \
        --features geph5-client/aws_lambda)

GEPH5_BIN="$CARGO_TARGET_DIR/$TARGET/release"

cp "$GEPH5_BIN/geph5.exe" \
    "$STAGE/"

cp "$GEPH5_BIN/geph5-client.exe" \
    "$STAGE/"

sign "$STAGE/geph5.exe"
sign "$STAGE/geph5-client.exe"

# --- Compile the installer ---------------------------------------------------

VNUM="${VERSION#v}"
VNUM="${VNUM%%[-+]*}"

export VERSION_NUM="$VNUM.0"

(cd "$WIN/windows" && sh -c "./iscc/ISCC.exe setup.iss")

sign "$WIN/windows/Output/geph-windows-x64-setup.exe"

publish \
    "$WIN/windows/Output/geph-windows-x64-setup.exe" \
    "$ARTIFACT"

echo ">> done: $ARTIFACT"
