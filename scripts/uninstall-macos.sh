#!/usr/bin/env bash
set -euo pipefail

install_dir="${INSTALL_DIR:-$HOME/.local/bin}"
lcp="$install_dir/lcp"

if [[ -x "$lcp" ]]; then
  "$lcp" daemon uninstall || true
  "$lcp" daemon stop || true
else
  launchctl unload -w "$HOME/Library/LaunchAgents/com.lcp.lanclipd.plist" 2>/dev/null || true
  rm -f "$HOME/Library/LaunchAgents/com.lcp.lanclipd.plist"
fi

if [[ "${KEEP_FILES:-0}" != "1" ]]; then
  rm -f "$install_dir/lcp" "$install_dir/lanclipd"
fi

echo "LCP autostart removed. Identity/config are left intact."
