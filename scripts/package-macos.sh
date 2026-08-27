#!/usr/bin/env bash
set -euo pipefail

out_dir="${OUT_DIR:-dist}"
configuration="${CONFIGURATION:-release}"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
arch="${LCP_ASSET_ARCH:-$(uname -m)}"
stage="$repo_root/$out_dir/lcp-macos-$arch"
archive="$repo_root/$out_dir/lcp-macos-$arch.tar.gz"
target_dir="$repo_root/target/$configuration"

if [[ "${SKIP_BUILD:-0}" != "1" ]]; then
  cargo_profile="$configuration"
  if [[ "$configuration" == "debug" ]]; then
    cargo_profile="dev"
  fi
  cargo_args=(build --workspace --profile "$cargo_profile")
  if [[ -n "${CARGO_BUILD_TARGET:-}" ]]; then
    cargo_args+=(--target "$CARGO_BUILD_TARGET")
    target_dir="$repo_root/target/$CARGO_BUILD_TARGET/$configuration"
  fi
  (cd "$repo_root" && cargo "${cargo_args[@]}")
elif [[ -n "${CARGO_BUILD_TARGET:-}" ]]; then
  target_dir="$repo_root/target/$CARGO_BUILD_TARGET/$configuration"
fi

rm -rf "$stage"
mkdir -p "$stage/scripts"
install -m 0755 "$target_dir/lcp" "$stage/lcp"
install -m 0755 "$target_dir/lanclipd" "$stage/lanclipd"
install -m 0644 "$repo_root/README.md" "$stage/README.md"
install -m 0644 "$repo_root/LICENSE" "$stage/LICENSE"
install -m 0755 "$repo_root/scripts/install-macos.sh" "$stage/scripts/install-macos.sh"
install -m 0755 "$repo_root/scripts/uninstall-macos.sh" "$stage/scripts/uninstall-macos.sh"

rm -f "$archive"
(cd "$(dirname "$stage")" && tar -czf "$archive" "$(basename "$stage")")
(cd "$(dirname "$archive")" && shasum -a 256 "$(basename "$archive")" > "$(basename "$archive").sha256")

echo "Wrote $archive"
echo "Wrote $archive.sha256"
