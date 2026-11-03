<?php

declare(strict_types=1);

namespace Nile;

use FFI;

/** Loads libnile_app through PHP's FFI and declares the C functions (nile.h, nile_app.h). */
final class Native
{
    public const ABI_VERSION = 2;

    private const CDEF = <<<'C'
        typedef struct NileView NileView;
        typedef struct NileTick { uint32_t struct_size; bool redraw; int32_t next_ms; } NileTick;
        typedef const char *(*NileHostFn)(const char *args_json, void *user);
        typedef struct NileHostHolder { NileHostFn f; } NileHostHolder;
        uint32_t nile_abi_version(void);
        const char *nile_version(void);
        NileView *nile_view_create_html(const char *html, const char *base_dir, float width, float height);
        NileView *nile_view_create_file(const char *path, float width, float height);
        void nile_view_destroy(NileView *view);
        void nile_view_reload(NileView *view);
        void nile_string_free(char *s);
        void nile_view_resize(NileView *view, float width, float height);
        float nile_view_content_height(NileView *view);
        bool nile_view_render(NileView *view, uint8_t *pixels, size_t len, uint32_t width, uint32_t height, float scale);
        bool nile_pointer_move(NileView *view, float x, float y);
        bool nile_pointer_leave(NileView *view);
        bool nile_pointer_button(NileView *view, int32_t button, bool pressed, uint8_t modifiers);
        bool nile_wheel(NileView *view, float dx, float dy);
        bool nile_key(NileView *view, const char *key, uint8_t modifiers, bool pressed);
        bool nile_text_input(NileView *view, const char *utf8);
        void nile_view_tick(NileView *view, NileTick *out);
        char *nile_view_selected_text(NileView *view);
        const char *nile_view_title(NileView *view);
        int32_t nile_view_eval(NileView *view, const char *code, char **out);
        int32_t nile_view_call(NileView *view, const char *name, const char *args_json, char **out);
        bool nile_view_register_function(NileView *view, const char *name, NileHostFn callback, void *user);
        char *nile_view_take_console(NileView *view);
        int32_t nile_view_write_pdf(NileView *view, const char *path);
        bool nile_app_register_function(const char *name, NileHostFn callback, void *user);
        int32_t nile_run(int32_t argc, const char *const *argv);
        int32_t nile_run_file(const char *path);
        int32_t nile_run_html(const char *html, const char *base_dir, float width, float height);
        C;

    private static ?FFI $ffi = null;

    public static function ffi(): FFI
    {
        return self::$ffi ??= self::load();
    }

    private static function load(): FFI
    {
        if (!extension_loaded('ffi')) {
            throw new \RuntimeException('The PHP ffi extension is required (and ffi.enable must not be "false")');
        }
        $file = PHP_OS_FAMILY === 'Windows' ? 'nile_app.dll' : (PHP_OS_FAMILY === 'Darwin' ? 'libnile_app.dylib' : 'libnile_app.so');
        $tried = [];
        foreach (self::candidates($file) as $path) {
            $tried[] = $path;
            if (!is_file($path)) {
                continue;
            }
            $ffi = FFI::cdef(self::CDEF, $path);
            if ($ffi->nile_abi_version() !== self::ABI_VERSION) {
                throw new \RuntimeException("$path is Nile ABI {$ffi->nile_abi_version()}, these bindings are for " . self::ABI_VERSION);
            }
            return $ffi;
        }
        throw new \RuntimeException("$file not found; build it with `cargo build -p nile-app --release` or set NILE_LIBRARY. Looked in: " . implode(', ', array_slice($tried, 0, 4)));
    }

    /** @return iterable<string> */
    private static function candidates(string $file): iterable
    {
        $env = getenv('NILE_LIBRARY');
        if ($env !== false && $env !== '') {
            yield $env;
        }
        yield __DIR__ . '/../native/' . $file;
        for ($dir = __DIR__; ; $dir = dirname($dir)) {
            foreach (['release', 'debug'] as $profile) {
                yield "$dir/target/$profile/$file";
            }
            if (dirname($dir) === $dir) {
                break;
            }
        }
    }

    /** Copy a library-owned `char *` into a PHP string and free it. */
    public static function take(?\FFI\CData $ptr): string
    {
        if ($ptr === null) {
            return '';
        }
        $s = FFI::string($ptr);
        self::ffi()->nile_string_free($ptr);
        return $s;
    }
}
