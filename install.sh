#!/bin/sh
# Install an official RefreshAgent release without Rust or root privileges.
set -eu
repository=https://github.com/RefreshAgent/refreshagent
say() { printf '%s\n' "$*"; }
fail() { say "RefreshAgent: $*" >&2; exit 1; }
command -v curl >/dev/null 2>&1 || fail 'curl is required.'
case "$(uname -s):$(uname -m)" in
  Darwin:arm64|Darwin:aarch64) target=aarch64-apple-darwin ;;
  Darwin:x86_64) target=x86_64-apple-darwin ;;
  Linux:x86_64|Linux:amd64) target=x86_64-unknown-linux-gnu ;;
  Linux:aarch64|Linux:arm64) target=aarch64-unknown-linux-gnu ;;
  *) fail 'Supported platforms: macOS and Linux, Intel/x86_64 or ARM64.' ;;
esac
fetch() { curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 --connect-timeout 10 --max-time 120 "$@"; }
version=${REFRESHAGENT_VERSION:-}
if [ -z "$version" ]; then
  latest=$(fetch --output /dev/null --write-out '%{url_effective}' "$repository/releases/latest")
  case "$latest" in "$repository/releases/tag/"*) version=${latest##*/} ;; *) fail 'Cannot resolve the latest official release.' ;; esac
fi
printf '%s\n' "$version" | LC_ALL=C grep -Eq '^v[0-9]+\.[0-9]+\.[0-9]+$' || fail 'REFRESHAGENT_VERSION must be a stable vX.Y.Z tag.'
install_dir=${REFRESHAGENT_INSTALL_DIR:-${HOME:?HOME must be set}/.local/bin}
case "$install_dir" in /*) ;; *) fail 'REFRESHAGENT_INSTALL_DIR must be an absolute path.' ;; esac
if printf '%s' "$install_dir" | LC_ALL=C grep -q '[[:cntrl:]]'; then
  fail 'Unsupported control character in installation path.'
fi
mkdir -p "$install_dir"
[ ! -L "$install_dir/refreshagent" ] || fail 'Existing binary is a symlink; use its package manager or another install directory.'
staging=$(mktemp -d "$install_dir/.refreshagent-install.XXXXXX")
trap 'rm -rf "$staging"' EXIT
trap 'exit 1' HUP INT TERM
asset=refreshagent-$target
say "Downloading RefreshAgent $version ($target)..."
fetch --output "$staging/$asset" "$repository/releases/download/$version/$asset"
fetch --output "$staging/checksum" "$repository/releases/download/$version/$asset.sha256"
expected=$(awk 'NR == 1 {print $1}' "$staging/checksum")
printf '%s\n' "$expected" | LC_ALL=C grep -Eq '^[0-9a-f]{64}$' || fail 'Invalid release checksum.'
if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$staging/$asset" | awk '{print $1}')
elif command -v shasum >/dev/null 2>&1; then
  actual=$(shasum -a 256 "$staging/$asset" | awk '{print $1}')
else
  fail 'sha256sum or shasum is required for verification.'
fi
[ "$expected" = "$actual" ] || fail 'Checksum mismatch; existing installation untouched.'
chmod 755 "$staging/$asset"
"$staging/$asset" --version > "$staging/version" || fail 'Binary cannot run on this system; existing installation untouched.'
[ "$(cat "$staging/version")" = "refreshagent ${version#v}" ] || fail 'Binary version mismatch; existing installation untouched.'
if [ -f "$install_dir/refreshagent" ]; then
  cp -p "$install_dir/refreshagent" "$staging/previous"
  mv -f "$staging/previous" "$install_dir/refreshagent.previous"
fi
mv -f "$staging/$asset" "$install_dir/refreshagent"
say "Installed: $install_dir/refreshagent"
case ":${PATH:-}:" in
  *":$install_dir:"*) say 'Run: refreshagent' ;;
  *)
    escaped_dir=$(printf '%s' "$install_dir" | sed 's/[\\$`"]/\\&/g')
    path_line=$(printf 'export PATH="%s:$PATH"' "$escaped_dir")
    if [ "${REFRESHAGENT_NO_MODIFY_PATH:-0}" != 1 ]; then
      # Handle HOME paths containing spaces without splitting profile filenames.
      case "${SHELL:-/bin/sh}" in
        */zsh) set -- "$HOME/.zprofile" "$HOME/.zshrc" ;;
        */bash) set -- "$HOME/.bashrc" "$HOME/.bash_profile" ;;
        */sh|*/dash) set -- "$HOME/.profile" ;;
        *) set -- ;;
      esac
      for profile in "$@"; do
        if ! grep -Fqx "$path_line" "$profile" 2>/dev/null; then
          printf '\n# RefreshAgent user installation\n%s\n' "$path_line" >> "$profile"
        fi
      done
    fi
    say 'Open a new terminal, then run: refreshagent'
    say "For this terminal, run: $path_line"
    ;;
esac
if ! command -v git >/dev/null 2>&1; then say 'Install Git before configuring a repository.'; fi
if ! command -v codex >/dev/null 2>&1 && ! command -v claude >/dev/null 2>&1; then
  say 'Install and authenticate Codex or Claude Code to execute improvements. Local scanning does not require an agent.'
fi
say 'Automatic release updates are built in. Use refreshagent update disable to opt out.'
