#!/usr/bin/env bash
set -euo pipefail

out_dir="${OUT_DIR:-dist}"
configuration="${CONFIGURATION:-release}"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
arch="$(uname -m)"
stage="$repo_root/$out_dir/lcp-macos-$arch"
archive="$repo_root/$out_dir/lcp-macos-$arch.tar.gz"

if [[ "${SKIP_BUILD:-0}" != "1" ]]; then
  cargo_profile="$configuration"
  if [[ "$configuration" == "debug" ]]; then
    cargo_profile="dev"
  fi
  (cd "$repo_root" && cargo build --workspace --profile "$cargo_profile")
fi

rm -rf "$stage"
mkdir -p "$stage/scripts"
install -m 0755 "$repo_root/target/$configuration/lcp" "$stage/lcp"
install -m 0755 "$repo_root/target/$configuration/lanclipd" "$stage/lanclipd"
install -m 0644 "$repo_root/README.md" "$stage/README.md"
install -m 0644 "$repo_root/LICENSE" "$stage/LICENSE"
install -m 0755 "$repo_root/scripts/install-macos.sh" "$stage/scripts/install-macos.sh"
install -m 0755 "$repo_root/scripts/uninstall-macos.sh" "$stage/scripts/uninstall-macos.sh"

rm -f "$archive"
(cd "$(dirname "$stage")" && tar -czf "$archive" "$(basename "$stage")")
(cd "$(dirname "$archive")" && shasum -a 256 "$(basename "$archive")" > "$(basename "$archive").sha256")

echo "Wrote $archive"
echo "Wrote $archive.sha256"
