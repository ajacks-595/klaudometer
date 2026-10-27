"""Generate the desktop app's icons from one SVG: two rings (5-hour coral
inside, 7-day blue outside) on a dark rounded square. Needs ImageMagick.

  python3 make_icons.py ICONS_DIR           write app-icon.svg and every icon
  python3 make_icons.py --check ICONS_DIR   check every PNG (also inside the
                                            .ico and .icns) is 8-bit RGBA

Tauri can only decode 8-bit RGBA PNGs: a 16-bit one aborts the app at
startup on macOS, so CI runs --check.
"""
import math
import os
import struct
import subprocess
import sys


def arc(cx, cy, r, fraction):
    angle = 2 * math.pi * fraction - math.pi / 2
    ex, ey = cx + r * math.cos(angle), cy + r * math.sin(angle)
    large = 1 if fraction > 0.5 else 0
    return f"M {cx} {cy - r} A {r} {r} 0 {large} 1 {ex:.1f} {ey:.1f}"


def ring(r, width, fraction, color):
    return (f'<circle cx="512" cy="512" r="{r}" fill="none" stroke="#FFFFFF" stroke-opacity="0.13" stroke-width="{width}"/>'
            f'<path d="{arc(512, 512, r, fraction)}" fill="none" stroke="{color}" stroke-width="{width}" stroke-linecap="round"/>')


def svg():
    # The tray icon's geometry (radii 6.25 and 12.75, width 4.5 on a 32 box), scaled to fit with a margin.
    s = 24.0
    return ('<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">'
            '<defs><linearGradient id="bg" x1="0" y1="0" x2="0" y2="1">'
            '<stop offset="0" stop-color="#2B2F35"/><stop offset="1" stop-color="#1A1C20"/></linearGradient></defs>'
            '<rect x="64" y="64" width="896" height="896" rx="200" fill="url(#bg)"/>'
            + ring(6.25 * s, 4.5 * s, 0.62, "#D97757")
            + ring(12.75 * s, 4.5 * s, 0.40, "#6A9BCC")
            + '</svg>\n')


def png(out, size, name):
    path = os.path.join(out, name)
    # PNG32: forces 8-bit RGBA; ImageMagick would otherwise pick 16-bit.
    subprocess.run(["magick", "-background", "none", "-density", str(max(72, size * 72 // 1024 * 4)),
                    os.path.join(out, "app-icon.svg"), "-resize", f"{size}x{size}", "-depth", "8", "-strip",
                    "PNG32:" + path], check=True)
    return path


def generate(out):
    os.makedirs(out, exist_ok=True)
    with open(os.path.join(out, "app-icon.svg"), "w") as f:
        f.write(svg())
    for size, name in [(32, "32x32.png"), (128, "128x128.png"), (256, "128x128@2x.png"), (512, "icon.png")]:
        png(out, size, name)

    # Windows: one .ico holding the common sizes.
    sizes = (16, 24, 32, 48, 64, 128, 256)
    tmp = [png(out, n, f"ico-{n}.png") for n in sizes]
    subprocess.run(["magick", *tmp, "-depth", "8", os.path.join(out, "icon.ico")], check=True)
    for t in tmp:
        os.remove(t)

    # macOS: an .icns is a header plus PNG entries tagged by size.
    chunks = b""
    for tag, size in [("icp4", 16), ("icp5", 32), ("icp6", 64), ("ic07", 128), ("ic08", 256), ("ic09", 512), ("ic10", 1024)]:
        path = png(out, size, f"icns-{size}.png")
        data = open(path, "rb").read()
        os.remove(path)
        chunks += tag.encode() + struct.pack(">I", 8 + len(data)) + data
    with open(os.path.join(out, "icon.icns"), "wb") as f:
        f.write(b"icns" + struct.pack(">I", 8 + len(chunks)) + chunks)


def png_format(data):
    """(bit depth, colour type) from a PNG's IHDR, or None if it isn't a PNG."""
    if not data.startswith(b"\x89PNG"):
        return None
    return struct.unpack(">BB", data[24:26])


def embedded_pngs(path):
    """PNG images inside an .ico or .icns file, with a label for each."""
    data = open(path, "rb").read()
    if path.endswith(".icns"):
        pos = 8
        while pos < len(data):
            tag, length = data[pos:pos + 4].decode(), struct.unpack(">I", data[pos + 4:pos + 8])[0]
            yield f"{os.path.basename(path)}:{tag}", data[pos + 8:pos + length]
            pos += length
    else:
        count = struct.unpack("<H", data[4:6])[0]
        for i in range(count):
            size, offset = struct.unpack("<II", data[6 + 16 * i + 8:6 + 16 * i + 16])
            yield f"{os.path.basename(path)}[{i}]", data[offset:offset + size]


def check(out):
    bad = []
    for name in sorted(os.listdir(out)):
        path = os.path.join(out, name)
        if name.endswith(".png"):
            images = [(name, open(path, "rb").read())]
        elif name.endswith((".ico", ".icns")):
            images = list(embedded_pngs(path))
        else:
            continue
        for label, data in images:
            fmt = png_format(data)
            # .ico may hold uncompressed bitmaps for small sizes; only PNGs matter here.
            if fmt is not None and fmt != (8, 6):
                bad.append(f"{label}: {fmt[0]}-bit, colour type {fmt[1]}")
    if bad:
        print("Icons Tauri can't load (need 8-bit RGBA):\n  " + "\n  ".join(bad))
        return 1
    print("All icon PNGs are 8-bit RGBA.")
    return 0


if __name__ == "__main__":
    if sys.argv[1] == "--check":
        sys.exit(check(sys.argv[2]))
    generate(sys.argv[1])
    sys.exit(check(sys.argv[1]))
