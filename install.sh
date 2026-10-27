#!/usr/bin/env bash
# Install or upgrade the Klaudometer Plasma widget for the current user.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
id="io.github.ajacks-595.klaudometer"

if kpackagetool6 -t Plasma/Applet -s "$id" >/dev/null 2>&1; then
    kpackagetool6 -t Plasma/Applet -u "$here/package"
else
    kpackagetool6 -t Plasma/Applet -i "$here/package"
fi

echo
echo "Installed. Right-click the desktop or panel > Add or Manage Widgets > \"Klaudometer\"."
echo "If an existing copy doesn't pick up changes, run: systemctl --user restart plasma-plasmashell"
