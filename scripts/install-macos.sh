#!/usr/bin/env bash
set -euo pipefail

install_dir="${INSTALL_DIR:-$HOME/.local/bin}"
configuration="${CONFIGURATION:-release}"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
bundled_lcp="$repo_root/lcp"
bundled_daemon="$repo_root/lanclipd"

if [[ -x "$bundled_lcp" && -x "$bundled_daemon" ]]; then
  lcp_source="$bundled_lcp"
  daemon_source="$bundled_daemon"
else
  if [[ "${SKIP_BUILD:-0}" != "1" ]]; then
    cargo_profile="$configuration"
    if [[ "$configuration" == "debug" ]]; then
      cargo_profile="dev"
    fi
    (cd "$repo_root" && cargo build --workspace --profile "$cargo_profile")
  fi

  target_dir="$repo_root/target/$configuration"
  lcp_source="$target_dir/lcp"
  daemon_source="$target_dir/lanclipd"
fi

if [[ ! -x "$lcp_source" || ! -x "$daemon_source" ]]; then
  echo "Missing LCP binaries. Re-run without SKIP_BUILD=1, or run this script from an extracted release bundle." >&2
  exit 1
fi

mkdir -p "$install_dir"
install -m 0755 "$lcp_source" "$install_dir/lcp"
install -m 0755 "$daemon_source" "$install_dir/lanclipd"

"$install_dir/lcp" daemon install
"$install_dir/lcp" daemon start

echo "LCP installed to $install_dir"
echo "Add it to PATH if needed: export PATH=\"$install_dir:\$PATH\""
