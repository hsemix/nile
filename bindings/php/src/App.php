<?php

declare(strict_types=1);

namespace Nile;

/** Run Nile as an application: windows and the event loop are the library's. Call from the main thread. */
final class App
{
    /** @var HostFunction[] alive for the whole run */
    private static $functions = [];

    public static function version(): string
    {
        return Native::ffi()->nile_version();
    }

    /** Expose a callable to every window as `host.<name>(...)`. Register before `run`. */
    public static function function(string $name, callable $fn): void
    {
        $f = new HostFunction($name, $fn);
        self::$functions[$name] = $f;
        if (!Native::ffi()->nile_app_register_function($name, $f->callback, null)) {
            throw new \InvalidArgumentException("could not register $name");
        }
    }

    /** Open the HTML file (or folder) in a window and run until the last window closes. Returns the exit code. */
    public static function run(string $path = '.'): int
    {
        return Native::ffi()->nile_run_file($path);
    }

    /** Show HTML text in a window. */
    public static function runHtml(string $html, ?string $baseDir = null, float $width = 0, float $height = 0): int
    {
        return Native::ffi()->nile_run_html($html, $baseDir, $width, $height);
    }

    /** Run the `nile` program with arguments (`page.html`, `--png out.png`, `--pack`, ...). */
    public static function main(array $args): int
    {
        $ffi = Native::ffi();
        $all = array_merge(['nile'], array_map('strval', $args));
        $argv = $ffi->new('char*[' . count($all) . ']');
        $keep = [];
        foreach ($all as $i => $a) {
            $keep[$i] = $ffi->new('char[' . (strlen($a) + 1) . ']');
            \FFI::memcpy($keep[$i], $a, strlen($a));
            $argv[$i] = $ffi->cast('char*', $keep[$i]);
        }
        return $ffi->nile_run(count($all), $argv);
    }
}
