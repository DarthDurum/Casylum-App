#!/bin/sh
# Installs the latest Casylum release from GitHub on macOS or Linux.
#   curl -fsSL https://raw.githubusercontent.com/DarthDurum/Casylum-App/main/scripts/install.sh | sh
set -eu

REPO="DarthDurum/Casylum-App"
API="https://api.github.com/repos/$REPO/releases/latest"

say() { printf '\033[1m%s\033[0m\n' "$*"; }
die() { printf 'Error: %s\n' "$*" >&2; exit 1; }
command -v curl >/dev/null 2>&1 || die "curl is required"

# Prints the download URL of the first asset in the latest release whose name matches $1.
asset_url() {
  curl -fsSL -H "Accept: application/vnd.github+json" "$API" \
    | grep -o '"browser_download_url": *"[^"]*"' \
    | cut -d'"' -f4 \
    | grep -E "$1" \
    | head -n1
}

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

case "$(uname -s)" in
  Darwin)
    case "$(uname -m)" in
      arm64) arch="aarch64" ;;
      *) arch="x64" ;;
    esac
    url=$(asset_url "_${arch}\\.dmg$") || true
    [ -n "${url:-}" ] || die "no macOS ($arch) download found in the latest release"
    say "Downloading $(basename "$url")"
    curl -fL --progress-bar "$url" -o "$tmp/Casylum.dmg"
    mnt=$(hdiutil attach -nobrowse -readonly "$tmp/Casylum.dmg" | tail -n1 | sed 's/.*\(\/Volumes\/.*\)/\1/')
    app=$(find "$mnt" -maxdepth 1 -name '*.app' | head -n1)
    dest="/Applications"
    [ -w "$dest" ] || dest="$HOME/Applications"
    mkdir -p "$dest"
    say "Installing to $dest"
    rm -rf "$dest/$(basename "$app")"
    cp -R "$app" "$dest/"
    hdiutil detach "$mnt" -quiet || true
    # The build isn't notarized yet, so clear the download flag that makes macOS refuse to open it.
    xattr -dr com.apple.quarantine "$dest/$(basename "$app")" 2>/dev/null || true
    say "Done. Open Casylum from $dest."
    ;;
  Linux)
    [ "$(uname -m)" = "x86_64" ] || die "only x86_64 Linux builds are published right now"
    url=$(asset_url "_amd64\\.AppImage$") || true
    [ -n "${url:-}" ] || die "no Linux AppImage found in the latest release"
    bin="$HOME/.local/bin"
    apps="$HOME/.local/share/applications"
    icons="$HOME/.local/share/icons/hicolor/128x128/apps"
    mkdir -p "$bin" "$apps" "$icons"
    say "Downloading $(basename "$url")"
    curl -fL --progress-bar "$url" -o "$bin/casylum"
    chmod +x "$bin/casylum"
    curl -fsSL "https://raw.githubusercontent.com/$REPO/main/src-tauri/icons/128x128.png" -o "$icons/casylum.png" || true
    cat > "$apps/casylum.desktop" <<DESKTOP
[Desktop Entry]
Name=Casylum
Comment=A calm, fast desktop AI coding agent
Exec=$bin/casylum
Icon=casylum
Type=Application
Categories=Development;
DESKTOP
    say "Done. Run 'casylum' or find Casylum in your app menu."
    case ":$PATH:" in
      *":$bin:"*) ;;
      *) echo "Note: add $bin to your PATH to run 'casylum' from a terminal." ;;
    esac
    ;;
  *)
    die "unsupported system; on Windows run: irm https://raw.githubusercontent.com/$REPO/main/scripts/install.ps1 | iex"
    ;;
esac
