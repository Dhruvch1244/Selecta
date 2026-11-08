#!/usr/bin/env bash
# Build the current checkout and install/update Selecta for this user (Linux).
#   ./scripts/install-local.sh            build + install
#   ./scripts/install-local.sh --uninstall
#
# Installs the native binary rather than the AppImage: on Arch-based systems
# it uses the system WebKitGTK + GStreamer, so audio works without bundling.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$HOME/.local/bin/selecta"
ICON_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/512x512/apps"
APPS="${XDG_DATA_HOME:-$HOME/.local/share}/applications"

if [ "${1:-}" = "--uninstall" ]; then
  rm -f "$BIN" "$ICON_DIR/selecta.png" "$APPS/selecta.desktop"
  update-desktop-database "$APPS" 2>/dev/null || true
  echo "Selecta removed. Your library and settings are kept in ~/.local/share/com.dhruvch1244.selecta and ~/.config/com.dhruvch1244.selecta."
  exit 0
fi

cd "$ROOT"
echo "• Building $(git rev-parse --abbrev-ref HEAD) @ $(git rev-parse --short HEAD)"
npm install --silent
(cd apps/desktop && npx tauri build --no-bundle)

if pgrep -x selecta >/dev/null; then
  echo "• Selecta is running; it will use the new build next time you open it"
fi
mkdir -p "$(dirname "$BIN")" "$ICON_DIR" "$APPS"
install -m 755 target/release/selecta-desktop "$BIN"
install -m 644 apps/desktop/src-tauri/icons/icon.png "$ICON_DIR/selecta.png"
cat > "$APPS/selecta.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Selecta
GenericName=AI Auto-DJ
Comment=Describe a set and Selecta builds and performs it from your own library
Exec=$BIN
Icon=selecta
Categories=AudioVideo;Audio;Music;
Terminal=false
EOF
update-desktop-database "$APPS" 2>/dev/null || true
echo "• Installed $(du -h "$BIN" | cut -f1) to $BIN. Open \"Selecta\" from your app menu."
