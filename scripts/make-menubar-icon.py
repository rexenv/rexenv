#!/usr/bin/env python3
"""Derive the menu-bar template icon from the app icon — never hand-drawn.

macOS wants a TEMPLATE image for a status item: alpha only, colour ignored, so
the system can tint it black on a light menu bar and white on a dark one. Our
app icon (`icons/icon.png`) is already the crowned R on transparency, so the
template IS its alpha channel — anything hand-drawn beside it is a second mark
that drifts from the first the day the brand changes.

    ./scripts/make-menubar-icon.py        writes icons/menubar.png

Pure stdlib on purpose: this machine has no PIL, no ImageMagick and no
rsvg-convert, and a generator that needs a toolchain nobody has is a generator
that gets replaced by a hand-drawn PNG the first time it is run.
"""
import struct, sys, zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "src-tauri" / "icons" / "icon.png"

# ONE file, at 2x density, because `tray-icon` normalises whatever it is given to
# an 18pt HEIGHT (tray-icon-0.24.1 macos/mod.rs: `let icon_height: f64 = 18.0`)
# and hands it to NSImage. So the pixel size is free — 2x is what makes it crisp
# on a retina bar — and the only thing the canvas decides is the MARGIN: a 20pt
# canvas holding an 18pt mark draws the glyph at ~16pt, the size Apple's own
# status items use. A mark filling the canvas edge to edge would be drawn at the
# full 18 and read louder than every other icon in the bar.
SCALE = 2
CANVAS, MARK = 20 * SCALE, 18 * SCALE


def read_png(path):
    """Minimal PNG reader: 8-bit RGBA or RGB, non-interlaced. Returns (w, h, rgba)."""
    data = path.read_bytes()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        sys.exit(f"{path}: not a PNG")
    pos, idat, w = 8, bytearray(), None
    while pos < len(data):
        (length,) = struct.unpack(">I", data[pos : pos + 4])
        kind = data[pos + 4 : pos + 8]
        body = data[pos + 8 : pos + 8 + length]
        if kind == b"IHDR":
            w, h, depth, colour, _, _, interlace = struct.unpack(">IIBBBBB", body)
            if depth != 8 or colour not in (2, 6) or interlace:
                sys.exit(f"{path}: need 8-bit RGB/RGBA, non-interlaced (got {depth}/{colour}/{interlace})")
        elif kind == b"IDAT":
            idat += body
        elif kind == b"IEND":
            break
        pos += 12 + length
    if w is None:
        sys.exit(f"{path}: no IHDR")
    chan = 4 if colour == 6 else 3
    raw = zlib.decompress(bytes(idat))
    stride = w * chan
    out = bytearray(w * h * 4)
    prev = bytearray(stride)
    p = 0
    for y in range(h):
        f = raw[p]
        p += 1
        line = bytearray(raw[p : p + stride])
        p += stride
        # PNG filters (§9.2) — each byte reconstructed from its left/up neighbours.
        for i in range(stride):
            a = line[i - chan] if i >= chan else 0
            b = prev[i]
            c = prev[i - chan] if i >= chan else 0
            if f == 1:
                line[i] = (line[i] + a) & 0xFF
            elif f == 2:
                line[i] = (line[i] + b) & 0xFF
            elif f == 3:
                line[i] = (line[i] + (a + b) // 2) & 0xFF
            elif f == 4:
                pa, pb, pc = abs(b - c), abs(a - c), abs(a + b - 2 * c)
                pred = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                line[i] = (line[i] + pred) & 0xFF
            elif f != 0:
                sys.exit(f"{path}: unknown filter {f}")
        if chan == 4:
            out[y * w * 4 : (y + 1) * w * 4] = line
        else:
            for x in range(w):
                out[(y * w + x) * 4 : (y * w + x) * 4 + 3] = line[x * 3 : x * 3 + 3]
                out[(y * w + x) * 4 + 3] = 255
        prev = line
    return w, h, out


def write_png(path, size, alpha):
    """Write a `size`x`size` BLACK image whose only content is `alpha`."""
    rows = bytearray()
    for y in range(size):
        rows.append(0)  # filter: none — the image is 2 KB, not worth predicting
        for x in range(size):
            rows += bytes((0, 0, 0, alpha[y * size + x]))
    def chunk(kind, body):
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body))
    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(bytes(rows), 9))
    png += chunk(b"IEND", b"")
    path.write_bytes(png)


def box_alpha(src_w, src_h, rgba, size, mark):
    """Box-filter the source alpha into a `size` canvas holding a `mark`-tall glyph.

    Box filtering, not nearest: at 22px the crown is three strokes a few source
    pixels wide, and nearest-neighbour drops whichever ones miss a sample point —
    the crown loses a spike and the mark stops being the mark.
    """
    # Trim to the source's own ink, so the padding below is OUR margin and not
    # whatever transparent border the app icon happens to carry.
    xs = [x for x in range(src_w) for y in range(src_h) if rgba[(y * src_w + x) * 4 + 3] > 8]
    ys = [y for y in range(src_h) for x in range(src_w) if rgba[(y * src_w + x) * 4 + 3] > 8]
    if not xs or not ys:
        sys.exit("source icon has no opaque pixels")
    x0, x1, y0, y1 = min(xs), max(xs) + 1, min(ys), max(ys) + 1
    side = max(x1 - x0, y1 - y0)  # square box: never distort the mark
    cx, cy = (x0 + x1) / 2, (y0 + y1) / 2
    x0, y0 = cx - side / 2, cy - side / 2
    pad = (size - mark) / 2
    out = bytearray(size * size)
    for oy in range(size):
        for ox in range(size):
            # Sample the source box this destination pixel covers.
            sx0 = x0 + (ox - pad) * side / mark
            sy0 = y0 + (oy - pad) * side / mark
            sx1, sy1 = sx0 + side / mark, sy0 + side / mark
            total = count = 0
            for sy in range(max(0, int(sy0)), min(src_h, max(int(sy1) + 1, int(sy0) + 1))):
                for sx in range(max(0, int(sx0)), min(src_w, max(int(sx1) + 1, int(sx0) + 1))):
                    total += rgba[(sy * src_w + sx) * 4 + 3]
                    count += 1
            out[oy * size + ox] = total // count if count else 0
    return out


def main():
    w, h, rgba = read_png(SRC)
    alpha = box_alpha(w, h, rgba, CANVAS, MARK)
    dst = SRC.parent / "menubar.png"
    write_png(dst, CANVAS, alpha)
    print(f"wrote {dst.relative_to(ROOT)}  {CANVAS}x{CANVAS} ({SCALE}x of a {CANVAS // SCALE}pt canvas)")


if __name__ == "__main__":
    main()
