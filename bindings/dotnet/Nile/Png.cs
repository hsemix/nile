using System.Buffers.Binary;
using System.IO.Compression;

namespace Nile;

/// <summary>A minimal PNG writer for premultiplied BGRA pixels.</summary>
internal static class Png
{
    static readonly uint[] Table = Enumerable.Range(0, 256).Select(n =>
    {
        var c = (uint)n;
        for (var k = 0; k < 8; k++) c = (c & 1) != 0 ? 0xEDB88320 ^ (c >> 1) : c >> 1;
        return c;
    }).ToArray();

    static uint Crc(byte[] data)
    {
        var c = 0xFFFFFFFFu;
        foreach (var b in data) c = Table[(c ^ b) & 0xFF] ^ (c >> 8);
        return c ^ 0xFFFFFFFF;
    }

    static void Chunk(Stream o, string kind, byte[] data)
    {
        Span<byte> n = stackalloc byte[4];
        BinaryPrimitives.WriteUInt32BigEndian(n, (uint)data.Length);
        o.Write(n);
        var body = new byte[4 + data.Length];
        for (var i = 0; i < 4; i++) body[i] = (byte)kind[i];
        data.CopyTo(body, 4);
        o.Write(body);
        BinaryPrimitives.WriteUInt32BigEndian(n, Crc(body));
        o.Write(n);
    }

    public static byte[] Encode(byte[] bgra, int width, int height)
    {
        var stride = width * 4;
        var raw = new byte[(stride + 1) * height];
        for (var y = 0; y < height; y++)
        {
            var row = y * (stride + 1);
            for (var x = 0; x < width; x++)
            {
                var s = y * stride + x * 4;
                int b = bgra[s], g = bgra[s + 1], r = bgra[s + 2], a = bgra[s + 3];
                if (a is > 0 and < 255) { r = Math.Min(255, r * 255 / a); g = Math.Min(255, g * 255 / a); b = Math.Min(255, b * 255 / a); }
                var d = row + 1 + x * 4;
                raw[d] = (byte)r; raw[d + 1] = (byte)g; raw[d + 2] = (byte)b; raw[d + 3] = (byte)a;
            }
        }
        using var packed = new MemoryStream();
        using (var z = new ZLibStream(packed, CompressionLevel.Optimal, true)) z.Write(raw);
        var head = new byte[13];
        BinaryPrimitives.WriteUInt32BigEndian(head, (uint)width);
        BinaryPrimitives.WriteUInt32BigEndian(head.AsSpan(4), (uint)height);
        head[8] = 8; head[9] = 6;
        using var o = new MemoryStream();
        o.Write(new byte[] { 0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A });
        Chunk(o, "IHDR", head);
        Chunk(o, "IDAT", packed.ToArray());
        Chunk(o, "IEND", Array.Empty<byte>());
        return o.ToArray();
    }
}
