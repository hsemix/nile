"""A minimal PNG writer for BGRA (premultiplied) pixels, so the bindings need no imaging dependency."""

import struct
import zlib


def encode(bgra, width, height):
    rgba = bytearray(len(bgra))
    rgba[0::4], rgba[1::4], rgba[2::4], rgba[3::4] = bgra[2::4], bgra[1::4], bgra[0::4], bgra[3::4]
    for i in range(0, len(rgba), 4):  # un-premultiply
        a = rgba[i + 3]
        if 0 < a < 255:
            rgba[i] = min(255, rgba[i] * 255 // a)
            rgba[i + 1] = min(255, rgba[i + 1] * 255 // a)
            rgba[i + 2] = min(255, rgba[i + 2] * 255 // a)
    stride = width * 4
    raw = b"".join(b"\0" + bytes(rgba[y * stride:(y + 1) * stride]) for y in range(height))

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw, 6)) + chunk(b"IEND", b"")
