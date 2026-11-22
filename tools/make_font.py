#!/usr/bin/env python3
"""Bake assets/cp437_8x16.bin: the 256 CP437 glyphs of "Px437 IBM VGA 8x16"
(Ultimate Oldschool PC Font Pack by VileR, CC BY-SA 4.0) as 8x16 1-bit bitmaps,
16 bytes per glyph, MSB = leftmost pixel. 4096 bytes in total."""
import sys
from PIL import Image, ImageDraw, ImageFont

ttf = sys.argv[1] if len(sys.argv) > 1 else "tools/Px437_IBM_VGA_8x16.ttf"
out = sys.argv[2] if len(sys.argv) > 2 else "assets/cp437_8x16.bin"
font = ImageFont.truetype(ttf, 16)
data = bytearray()
for code in range(256):
    img = Image.new("1", (8, 16), 0)
    if code >= 32 and code != 127:  # control codes stay blank
        d = ImageDraw.Draw(img)
        d.fontmode = "1"
        d.text((0, 0), bytes([code]).decode("cp437"), fill=1, font=font)
    for y in range(16):
        row = 0
        for x in range(8):
            if img.getpixel((x, y)):
                row |= 0x80 >> x
        data.append(row)
open(out, "wb").write(data)
print("wrote", out, len(data), "bytes")
