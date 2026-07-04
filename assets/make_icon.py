#!/usr/bin/env python3
"""Generate icon.ico (16/32/48/256 PNG entries) and icon_64.rgba (runtime
window icon) into this directory.

Draws the NotepadMD+ mark: dark slate rounded square, white "MD", blue "+".
Pure stdlib (zlib/struct), supersampled for smooth edges.

Usage: python3 assets/make_icon.py
"""
import struct, zlib, os

OUT_DIR = os.path.dirname(os.path.abspath(__file__))

BG = (38, 50, 66)        # dark slate
FG = (240, 244, 248)     # near-white MD
ACCENT = (86, 156, 214)  # blue +

def dist_seg(px, py, ax, ay, bx, by):
    vx, vy = bx - ax, by - ay
    wx, wy = px - ax, py - ay
    L2 = vx * vx + vy * vy
    t = 0.0 if L2 == 0 else max(0.0, min(1.0, (wx * vx + wy * vy) / L2))
    dx, dy = px - (ax + t * vx), py - (ay + t * vy)
    return (dx * dx + dy * dy) ** 0.5

def render(size):
    """Return RGBA bytes for size x size icon, 3x supersampled."""
    ss = 3
    S = size * ss
    r = S * 0.22           # corner radius
    stroke = S * 0.058     # letter stroke width
    # "M": left stem, middle V, right stem
    mx0, mx1 = 0.13 * S, 0.42 * S
    y_top, y_bot = 0.30 * S, 0.70 * S
    m_segs = [
        (mx0, y_bot, mx0, y_top),
        (mx0, y_top, (mx0 + mx1) / 2, 0.55 * S),
        ((mx0 + mx1) / 2, 0.55 * S, mx1, y_top),
        (mx1, y_top, mx1, y_bot),
    ]
    # "D": stem + half-circle bowl meeting the stem tips
    dx = 0.52 * S
    d_stem = (dx, y_top, dx, y_bot)
    d_cy, d_r = 0.50 * S, 0.20 * S
    # "+": top-right
    pcx, pcy, parm = 0.84 * S, 0.35 * S, 0.075 * S
    p_segs = [
        (pcx - parm, pcy, pcx + parm, pcy),
        (pcx, pcy - parm, pcx, pcy + parm),
    ]
    half = stroke / 2
    buf = bytearray(size * size * 4)
    for y in range(size):
        for x in range(size):
            acc = [0, 0, 0, 0]
            for sy in range(ss):
                for sx in range(ss):
                    px, py = x * ss + sx + 0.5, y * ss + sy + 0.5
                    # rounded-rect coverage
                    qx = max(abs(px - S / 2) - (S / 2 - r), 0)
                    qy = max(abs(py - S / 2) - (S / 2 - r), 0)
                    if (qx * qx + qy * qy) ** 0.5 > r:
                        continue
                    c = BG
                    hit_md = min(dist_seg(px, py, *s) for s in m_segs) <= half \
                        or dist_seg(px, py, *d_stem) <= half
                    if not hit_md and px >= dx:
                        # D bowl: annulus band, right half only
                        d = ((px - dx) ** 2 + (py - d_cy) ** 2) ** 0.5
                        hit_md = abs(d - d_r) <= half
                    if hit_md:
                        c = FG
                    elif min(dist_seg(px, py, *s) for s in p_segs) <= half * 1.5:
                        c = ACCENT
                    acc[0] += c[0]; acc[1] += c[1]; acc[2] += c[2]; acc[3] += 255
            n = ss * ss
            i = (y * size + x) * 4
            if acc[3]:
                cov = acc[3] / (255 * n)
                buf[i] = round(acc[0] / (acc[3] / 255))
                buf[i + 1] = round(acc[1] / (acc[3] / 255))
                buf[i + 2] = round(acc[2] / (acc[3] / 255))
                buf[i + 3] = round(255 * cov)
    return bytes(buf)

def to_png(rgba, size):
    def chunk(tag, data):
        c = tag + data
        return struct.pack(">I", len(data)) + c + struct.pack(">I", zlib.crc32(c))
    raw = b"".join(b"\x00" + rgba[y * size * 4:(y + 1) * size * 4] for y in range(size))
    return (b"\x89PNG\r\n\x1a\n"
            + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(raw, 9))
            + chunk(b"IEND", b""))

sizes = [16, 32, 48, 256]
pngs = [to_png(render(s), s) for s in sizes]
with open(os.path.join(OUT_DIR, "icon_64.rgba"), "wb") as f:
    f.write(render(64))

# ICO container with PNG entries
ico = struct.pack("<HHH", 0, 1, len(sizes))
offset = 6 + 16 * len(sizes)
entries = b""
for s, png in zip(sizes, pngs):
    b = s if s < 256 else 0
    entries += struct.pack("<BBBBHHII", b, b, 0, 0, 1, 32, len(png), offset)
    offset += len(png)
with open(os.path.join(OUT_DIR, "icon.ico"), "wb") as f:
    f.write(ico + entries + b"".join(pngs))
print("wrote icon.ico and icon_64.rgba")
