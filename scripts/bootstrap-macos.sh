#!/usr/bin/env bash
set -euo pipefail

repo="${LCP_REPO:-idealkritarat/lclip}"
branch="${LCP_BRANCH:-master}"
install_dir="${INSTALL_DIR:-$HOME/.local/bin}"
tmp="$(mktemp -d)"

cleanup() {
  rm -rf "$tmp"
}
trap cleanup EXIT

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "This installer is for macOS. Use scripts/install-windows.ps1 on Windows." >&2
  exit 1
fi

download() {
  curl -fL --retry 3 --connect-timeout 10 "$1" -o "$2"
}

install_from_release() {
  local arch asset api_json asset_url checksum_url archive checksum bundle_dir
  arch="$(uname -m)"
  asset="lcp-macos-$arch.tar.gz"
  api_json="$tmp/latest-release.json"

  if ! curl -fsSL "https://api.github.com/repos/$repo/releases/latest" -o "$api_json"; then
    return 1
  fi

  asset_url="$(sed -n "s/.*\"browser_download_url\": \"\\([^\"]*${asset}\\)\".*/\\1/p" "$api_json" | head -n 1)"
  if [[ -z "$asset_url" ]]; then
    return 1
  fi

  checksum_url="$asset_url.sha256"
  archive="$tmp/$asset"
  checksum="$tmp/$asset.sha256"

  download "$asset_url" "$archive"
  download "$checksum_url" "$checksum"
  (cd "$tmp" && shasum -a 256 -c "$checksum")

  tar -xzf "$archive" -C "$tmp"
  bundle_dir="$(find "$tmp" -maxdepth 1 -type d -name 'lcp-macos-*' | head -n 1)"
  if [[ -z "$bundle_dir" ]]; then
    echo "Release archive did not contain an lcp-macos-* bundle." >&2
    exit 1
  fi

  INSTALL_DIR="$install_dir" SKIP_BUILD=1 bash "$bundle_dir/scripts/install-macos.sh"
}

ensure_rust() {
  if command -v cargo >/dev/null 2>&1; then
    return
  fi

  echo "Rust toolchain not found; installing rustup with the minimal profile..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs |
    sh -s -- -y --profile minimal

  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
}

install_from_source() {
  if ! command -v git >/dev/null 2>&1; then
    echo "git is required for source install. Install Xcode Command Line Tools, then rerun this command." >&2
    echo "Try: xcode-select --install" >&2
    exit 1
  fi

  ensure_rust
  git clone --depth 1 --branch "$branch" "https://github.com/$repo.git" "$tmp/lclip"
  INSTALL_DIR="$install_dir" bash "$tmp/lclip/scripts/install-macos.sh"
}

if install_from_release; then
  exit 0
fi

echo "No matching binary release found; building LCP from source."
install_from_source
