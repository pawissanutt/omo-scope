#!/bin/sh
set -eu

repo="${OMO_SCOPE_REPO:-pawissanutt/omo-scope}"
version="${OMO_SCOPE_VERSION:-latest}"
base="${OMO_SCOPE_BASE_URL:-https://github.com/$repo/releases}"
bin_dir="${OMO_SCOPE_INSTALL_DIR:-$HOME/.local/bin}"

fail() {
    echo "omo-scope: $*" >&2
    exit 1
}

case "$(uname -s)" in
Linux) os=unknown-linux-musl ;;
Darwin) os=apple-darwin ;;
*) fail "unsupported OS $(uname -s); build from source with: cargo install --git https://github.com/$repo" ;;
esac
case "$(uname -m)" in
x86_64 | amd64) arch=x86_64 ;;
aarch64 | arm64) arch=aarch64 ;;
*) fail "unsupported CPU $(uname -m)" ;;
esac
asset="omo-scope-$arch-$os.tar.gz"
if [ "$version" = latest ]; then
    url="$base/latest/download/$asset"
else
    url="$base/download/$version/$asset"
fi

if [ "${1:-}" = "--dry-run" ]; then
    echo "would download $url into $bin_dir"
    exit 0
fi

fetch() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$1" -o "$2"
    elif command -v wget >/dev/null 2>&1; then
        wget -qO "$2" "$1"
    else
        fail "need curl or wget"
    fi
}

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fetch "$url" "$tmp/$asset" || fail "download failed: $url"
fetch "$url.sha256" "$tmp/$asset.sha256" || fail "checksum download failed: $url.sha256"
if command -v sha256sum >/dev/null 2>&1; then
    (cd "$tmp" && sha256sum -c "$asset.sha256" >/dev/null) || fail "checksum mismatch for $asset"
elif command -v shasum >/dev/null 2>&1; then
    (cd "$tmp" && shasum -a 256 -c "$asset.sha256" >/dev/null) || fail "checksum mismatch for $asset"
else
    fail "need sha256sum or shasum to verify the download"
fi
tar -xzf "$tmp/$asset" -C "$tmp"
mkdir -p "$bin_dir"
cp "$tmp/omo-scope" "$bin_dir/omo-scope.new"
chmod 755 "$bin_dir/omo-scope.new"
mv "$bin_dir/omo-scope.new" "$bin_dir/omo-scope"
echo "installed $("$bin_dir/omo-scope" --version) to $bin_dir/omo-scope"
case ":$PATH:" in
*":$bin_dir:"*) ;;
*) echo "note: $bin_dir is not on your PATH; add it to use 'omo-scope'" ;;
esac
