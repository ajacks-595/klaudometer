#!/usr/bin/env bash
# Install or upgrade the Klaudometer Plasma widget for the current user.
#
# Checks for KDE Plasma 6 and everything the widget needs first, and names the
# package to install on Fedora, Debian/Ubuntu, Arch and openSUSE when
# something is missing. Run with --check to only run the checks.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
id="io.github.ajacks-595.klaudometer"
dest="${XDG_DATA_HOME:-$HOME/.local/share}/plasma/plasmoids/$id"

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

# pkg <need>: the package that provides it on this distro family.
pkg() {
    case "$family:$1" in
        debian:python3 | fedora:python3 | suse:python3) echo python3 ;;
        arch:python3) echo python ;;
        debian:plasma5support) echo qml6-module-org-kde-plasma-plasma5support ;;
        fedora:plasma5support | arch:plasma5support) echo plasma5support ;;
        suse:plasma5support) echo plasma5support6 ;;
        debian:shapes) echo qml6-module-qtquick-shapes ;;
        fedora:shapes) echo qt6-qtdeclarative ;;
        arch:shapes) echo qt6-declarative ;;
        suse:shapes) echo qt6-declarative-imports ;;
        debian:kcmutils) echo qml6-module-org-kde-kcmutils ;;
        fedora:kcmutils) echo kf6-kcmutils ;;
        arch:kcmutils) echo kcmutils ;;
        suse:kcmutils) echo kf6-kcmutils-imports ;;
        debian:notifications) echo qml6-module-org-kde-notifications ;;
        fedora:notifications) echo kf6-knotifications ;;
        arch:notifications) echo knotifications ;;
        suse:notifications) echo kf6-knotifications-imports ;;
        debian:kquickcontrols) echo qml6-module-org-kde-kquickcontrols ;;
        fedora:kquickcontrols) echo kf6-kdeclarative ;;
        arch:kquickcontrols) echo kdeclarative ;;
        suse:kquickcontrols) echo kf6-kdeclarative-imports ;;
        debian:secret-tool) echo libsecret-tools ;;
        fedora:secret-tool | arch:secret-tool) echo libsecret ;;
        suse:secret-tool) echo secret-tool ;;
        *) echo "the package that provides $1" ;;
    esac
}

install_hint() {
    case "$family" in
        debian) echo "sudo apt install $*" ;;
        fedora) echo "sudo dnf install $*" ;;
        arch) echo "sudo pacman -S $*" ;;
        suse) echo "sudo zypper install $*" ;;
        *) echo "install $*" ;;
    esac
}

# Directories Qt 6 looks in for QML modules on the common distro layouts.
qml_dirs=()
if command -v qtpaths6 >/dev/null 2>&1; then
    qml_dirs+=("$(qtpaths6 --query QT_INSTALL_QML 2>/dev/null || true)")
fi
IFS=: read -r -a env_dirs <<< "${QML_IMPORT_PATH:-}:${QML2_IMPORT_PATH:-}"
qml_dirs+=("${env_dirs[@]}" /usr/lib64/qt6/qml /usr/lib/qt6/qml /usr/lib/*-linux-gnu/qt6/qml)

has_qml() {
    local dir
    for dir in "${qml_dirs[@]}"; do
        [ -n "$dir" ] && [ -f "$dir/$1/qmldir" ] && return 0
    done
    return 1
}

missing=()   # packages the widget can't run without
optional=()  # packages that only turn features on

if ! command -v plasmashell >/dev/null 2>&1; then
    echo "KDE Plasma isn't installed. Klaudometer is a Plasma 6 widget and only runs on the Plasma desktop." >&2
    exit 1
fi
plasma_version="$(plasmashell --version 2>/dev/null | grep -oE '[0-9]+(\.[0-9]+)+' | head -1 || true)"
if [ "${plasma_version%%.*}" != "6" ]; then
    echo "Klaudometer needs KDE Plasma 6, but this system has Plasma ${plasma_version:-of an unknown version}." >&2
    echo "(Debian 12 and Kubuntu 24.04 ship Plasma 5. Debian 13 and Kubuntu 25.04 or newer have Plasma 6.)" >&2
    exit 1
fi

command -v python3 >/dev/null 2>&1 || missing+=("$(pkg python3)")
has_qml org/kde/plasma/plasma5support || missing+=("$(pkg plasma5support)")
has_qml QtQuick/Shapes || missing+=("$(pkg shapes)")
has_qml org/kde/kcmutils || missing+=("$(pkg kcmutils)")
has_qml org/kde/kquickcontrols || missing+=("$(pkg kquickcontrols)")
has_qml org/kde/notification || optional+=("$(pkg notifications)  (desktop notifications)")
command -v secret-tool >/dev/null 2>&1 || optional+=("$(pkg secret-tool)  (keeps the Admin API key for API spend in your wallet)")

echo "KDE Plasma $plasma_version found."
if [ ${#optional[@]} -gt 0 ]; then
    echo
    echo "Optional, for extra features:"
    printf '  %s\n' "${optional[@]}"
fi
if [ ${#missing[@]} -gt 0 ]; then
    echo
    echo "Klaudometer can't run until these are installed:" >&2
    printf '  %s\n' "${missing[@]}" >&2
    echo "  $(install_hint "${missing[@]}")" >&2
    exit 1
fi
if [ "${1:-}" = "--check" ]; then
    echo "Everything Klaudometer needs is installed."
    exit 0
fi

# kpackagetool6 isn't installed everywhere; a plain copy is all it does for a
# per-user widget anyway.
if command -v kpackagetool6 >/dev/null 2>&1; then
    if kpackagetool6 -t Plasma/Applet -s "$id" >/dev/null 2>&1; then
        kpackagetool6 -t Plasma/Applet -u "$here/package"
    else
        kpackagetool6 -t Plasma/Applet -i "$here/package"
    fi
else
    rm -rf "$dest"
    mkdir -p "$dest"
    cp -r "$here/package/." "$dest/"
    echo "Installed to $dest"
fi

echo
echo "Installed. Right-click the desktop or panel > Add or Manage Widgets > \"Klaudometer\"."
echo "If an existing copy doesn't pick up changes, run: systemctl --user restart plasma-plasmashell"
