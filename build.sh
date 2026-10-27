#!/usr/bin/env bash
# Build release files into dist/. For now that's the Plasma widget as a
# .plasmoid: a zip of package/ that "Install Widget From Local File",
# Discover and kpackagetool6 all accept. The tray icon ships as source.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
version="$(python3 -c 'import json, sys; print(json.load(open(sys.argv[1]))["KPlugin"]["Version"])' "$here/package/metadata.json")"
out="$here/dist/klaudometer-$version.plasmoid"
mkdir -p "$here/dist"
rm -f "$out"

python3 - "$here/package" "$out" <<'EOF'
import os, sys, zipfile
src, out = sys.argv[1:]
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
    for root, dirs, files in os.walk(src):
        dirs[:] = sorted(d for d in dirs if d != "__pycache__")
        for name in sorted(files):
            if not name.endswith(".pyc"):
                path = os.path.join(root, name)
                z.write(path, os.path.relpath(path, src))
EOF

echo "Built $out"
