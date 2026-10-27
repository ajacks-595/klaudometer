#!/usr/bin/env bash
# Install, check or remove Klaudometer's tray icon for the current user.
#
# The tray icon works on GNOME (with the AppIndicator extension, built into
# Ubuntu), XFCE, Cinnamon, MATE, Budgie, LXQt and KDE Plasma 5 or 6.
#
#   tray/install.sh                 check, install, start it and start it at login
#   tray/install.sh --check         only check what's needed
#   tray/install.sh --no-autostart  install without starting it at login
#   tray/install.sh --uninstall     stop it and remove it (settings are kept)
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(dirname "$here")"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
dest="$data/klaudometer-tray"
entry="klaudometer-tray.desktop"
autostart_dir="${XDG_CONFIG_HOME:-$HOME/.config}/autostart"

mode=install
autostart=yes
for arg in "$@"; do
    case "$arg" in
        --check) mode=check ;;
        --uninstall) mode=uninstall ;;
        --no-autostart) autostart=no ;;
        *) echo "Unknown option: $arg" >&2; exit 2 ;;
    esac
done

if [ "$mode" = uninstall ]; then
    pkill -f "^python3 $dest/klaudometer_tray.py" 2>/dev/null || true
    rm -rf "$dest" "$data/applications/$entry" "$autostart_dir/$entry"
    echo "Removed Klaudometer's tray icon. Your settings in ~/.config/klaudometer/ were kept."
    exit 0
fi

family=other
if [ -r /etc/os-release ]; then
    . /etc/os-release
    case " ${ID:-} ${ID_LIKE:-} " in
        *" debian "* | *" ubuntu "*) family=debian ;;
        *" fedora "* | *" rhel "*) family=fedora ;;
        *" arch "*) family=arch ;;
        *" suse "* | *" opensuse "*) family=suse ;;
    esac
fi

# pkg <need>: the package(s) that provide it on this distro family.
pkg() {
    case "$family:$1" in
        arch:python3) echo python ;;
        *:python3) echo python3 ;;
        debian:gtk) echo python3-gi gir1.2-gtk-3.0 ;;
        fedora:gtk) echo python3-gobject gtk3 ;;
        arch:gtk) echo python-gobject gtk3 ;;
        suse:gtk) echo python3-gobject python3-gobject-Gdk typelib-1_0-Gtk-3_0 ;;
        debian:indicator) echo gir1.2-ayatanaappindicator3-0.1 ;;
        fedora:indicator) echo libayatana-appindicator-gtk3 ;;
        arch:indicator) echo libayatana-appindicator ;;
        suse:indicator) echo typelib-1_0-AyatanaAppIndicator3-0_1 ;;
        *:gtk) echo "PyGObject and GTK 3" ;;
        *:indicator) echo "Ayatana AppIndicator (GTK 3) and its GObject introspection data" ;;
    esac
}

install_hint() {
    case "$family" in
        debian) echo "sudo apt install $*" ;;
        fedora) echo "sudo dnf install $*" ;;
        arch) echo "sudo pacman -S $*" ;;
        suse) echo "sudo zypper install $*" ;;
        *) echo "install: $*" ;;
    esac
}

missing=()
if ! command -v python3 >/dev/null 2>&1; then
    missing+=($(pkg python3) $(pkg gtk) $(pkg indicator))
else
    python3 - 2>/dev/null <<'EOF' || missing+=($(pkg gtk))
import gi
gi.require_version("Gtk", "3.0")
from gi.repository import Gtk
EOF
    python3 - 2>/dev/null <<'EOF' || missing+=($(pkg indicator))
import gi
for ns in ("AyatanaAppIndicator3", "AppIndicator3"):
    try:
        gi.require_version(ns, "0.1")
        __import__("gi.repository." + ns)
        break
    except (ImportError, ValueError):
        pass
else:
    raise SystemExit(1)
EOF
fi

if [ ${#missing[@]} -gt 0 ]; then
    echo "Klaudometer's tray icon can't run until these are installed:" >&2
    printf '  %s\n' "${missing[@]}" >&2
    echo "  $(install_hint "${missing[@]}")" >&2
    exit 1
fi

desktop="${XDG_CURRENT_DESKTOP:-}"
case ":$desktop:" in
    *:GNOME:* | *:ubuntu:*)
        if command -v gnome-extensions >/dev/null 2>&1 \
            && ! gnome-extensions list --enabled 2>/dev/null | grep -qi appindicator; then
            echo "Note: GNOME only shows tray icons with the AppIndicator extension turned on."
            case "$family" in
                debian) echo "  Install gnome-shell-extension-appindicator (Ubuntu ships it as part of its desktop), then turn it on in the Extensions app." ;;
                fedora | arch) echo "  $(install_hint gnome-shell-extension-appindicator), log out and back in, then turn it on in the Extensions app." ;;
                *) echo "  Get \"AppIndicator and KStatusNotifierItem Support\" from extensions.gnome.org." ;;
            esac
        fi
        ;;
    *:KDE:*)
        if plasmashell --version 2>/dev/null | grep -q ' 6\.'; then
            echo "Note: on Plasma 6 the Klaudometer widget ($root/install.sh) gives you rings on the desktop too. The tray icon works as well."
        fi
        ;;
esac

if [ "$mode" = check ]; then
    echo "Everything Klaudometer's tray icon needs is installed."
    exit 0
fi

mkdir -p "$dest" "$data/applications"
cp "$here/klaudometer_tray.py" "$here/klaudometer.svg" "$dest/"
cp "$root/package/contents/code/fetch_usage.py" "$dest/"

cat > "$data/applications/$entry" <<EOF
[Desktop Entry]
Type=Application
Name=Klaudometer
GenericName=Claude usage
Comment=Claude subscription usage and API spend in your system tray
Exec=python3 "$dest/klaudometer_tray.py"
Icon=$dest/klaudometer.svg
Terminal=false
Categories=Utility;
StartupNotify=false
X-GNOME-Autostart-enabled=true
EOF

if [ "$autostart" = yes ]; then
    mkdir -p "$autostart_dir"
    cp "$data/applications/$entry" "$autostart_dir/$entry"
else
    rm -f "$autostart_dir/$entry"
fi

# Restart a running copy so it picks up the new code.
pkill -f "^python3 $dest/klaudometer_tray.py" 2>/dev/null && sleep 1 || true
setsid python3 "$dest/klaudometer_tray.py" >/dev/null 2>&1 < /dev/null &

echo "Installed to $dest and started."
[ "$autostart" = yes ] && echo "It will start automatically when you log in."
echo "Settings: ~/.config/klaudometer/tray.ini (or Edit Settings… in the tray menu)."
