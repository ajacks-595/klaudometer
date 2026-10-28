#!/usr/bin/env bash
# Create or update the draft GitHub release for a version, attaching every
# download found under a folder plus their SHA-256 checksums. It never
# publishes. Once the version's release is published it changes nothing,
# and says to bump the version.
#
#   draft_release.sh VERSION FOLDER
#
# Needs gh with GH_TOKEN set. The draft points at $GITHUB_SHA, where
# publishing it creates the tag.
set -euo pipefail

version="$1"
dir="$2"
tag="v$version"
notes="docs/release-notes/$tag.md"
target="${GITHUB_SHA:?}"

mapfile -t files < <(find "$dir" -type f \( -name '*.dmg' -o -name '*.msi' -o -name '*.exe' -o -name '*.plasmoid' \) | sort)
for ext in dmg msi exe plasmoid; do
    if ! printf '%s\n' "${files[@]}" | grep -q "\.$ext\$"; then
        echo "No .$ext file in $dir to attach." >&2
        exit 1
    fi
done

sums="$dir/SHA256SUMS.txt"
: > "$sums"
for file in "${files[@]}"; do
    (cd "$(dirname "$file")" && sha256sum "$(basename "$file")") >> "$sums"
done
files+=("$sums")

# Drafts have no tag yet, so look the release up by its tag name in the list.
draft="$(gh release list --limit 100 --json tagName,isDraft --jq "map(select(.tagName == \"$tag\"))[0].isDraft | tostring")"
case "$draft" in
    true)
        gh release edit "$tag" --draft --title "Klaudometer $version" --notes-file "$notes" --target "$target"
        gh release upload "$tag" "${files[@]}" --clobber
        echo "Updated the draft release $tag."
        ;;
    false)
        echo "::notice::$tag is already published, so it was left alone. Bump the version to draft the next release."
        ;;
    *)
        gh release create "$tag" --draft --title "Klaudometer $version" --notes-file "$notes" --target "$target" "${files[@]}"
        echo "Created the draft release $tag."
        ;;
esac
