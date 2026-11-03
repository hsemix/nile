<?php

declare(strict_types=1);

namespace Nile;

/** A minimal PNG writer for premultiplied BGRA pixels. */
final class Png
{
    public static function encode(string $bgra, int $width, int $height): string
    {
        $stride = $width * 4;
        $raw = '';
        for ($y = 0; $y < $height; $y++) {
            $row = '';
            for ($x = 0; $x < $width; $x++) {
                [$b, $g, $r, $a] = array_values(unpack('C4', $bgra, $y * $stride + $x * 4));
                if ($a > 0 && $a < 255) {
                    $r = min(255, intdiv($r * 255, $a));
                    $g = min(255, intdiv($g * 255, $a));
                    $b = min(255, intdiv($b * 255, $a));
                }
                $row .= chr($r) . chr($g) . chr($b) . chr($a);
            }
            $raw .= "\0" . $row;
        }
        $chunk = static function (string $kind, string $data): string {
            return pack('N', strlen($data)) . $kind . $data . pack('N', crc32($kind . $data));
        };
        return "\x89PNG\r\n\x1a\n" . $chunk('IHDR', pack('NNCCCCC', $width, $height, 8, 6, 0, 0, 0))
            . $chunk('IDAT', gzcompress($raw, 6)) . $chunk('IEND', '');
    }
}
