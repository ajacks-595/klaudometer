"""Check every place Klaudometer's version is written agrees, and print it.

  check_versions.py            print "version=X.Y.Z", or fail on a mismatch
  check_versions.py --release  also require docs/release-notes/vX.Y.Z.md

The output suits $GITHUB_OUTPUT.
"""
import json
import pathlib
import re
import sys
import tomllib

root = pathlib.Path(__file__).resolve().parents[2]


def read(path):
    return (root / path).read_text(encoding="utf-8")


versions = {
    "desktop/Cargo.toml": tomllib.loads(read("desktop/Cargo.toml"))["workspace"]["package"]["version"],
    "desktop/src-tauri/tauri.conf.json": json.loads(read("desktop/src-tauri/tauri.conf.json"))["version"],
    "package/metadata.json": json.loads(read("package/metadata.json"))["KPlugin"]["Version"],
    "package/contents/code/fetch_usage.py": re.search(
        r'^USER_AGENT = "klaudometer/([^"]+)"', read("package/contents/code/fetch_usage.py"), re.M
    ).group(1),
}

if len(set(versions.values())) != 1:
    print("Versions don't match:", file=sys.stderr)
    for path, version in versions.items():
        print(f"  {version:10} {path}", file=sys.stderr)
    sys.exit(1)

version = next(iter(versions.values()))
if "--release" in sys.argv[1:]:
    notes = root / "docs" / "release-notes" / f"v{version}.md"
    if not notes.is_file():
        print(f"Write the release notes first: {notes.relative_to(root)}", file=sys.stderr)
        sys.exit(1)

print(f"version={version}")
