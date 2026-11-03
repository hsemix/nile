<?php

declare(strict_types=1);

namespace Nile;

use FFI;

/** A page you drive yourself: load it, call into it, send input, and read pixels back (headless or into your own window). */
final class View
{
    public const SHIFT = 1;
    public const CONTROL = 2;
    public const ALT = 4;
    public const META = 8;

    /** @var \FFI\CData|null */
    private $p;
    /** @var HostFunction[] */
    private $callbacks = [];
    /** @var float */
    public $width;
    /** @var float */
    public $height;

    private function __construct($view, float $width, float $height)
    {
        if ($view === null) {
            throw new \InvalidArgumentException('could not create the view');
        }
        $this->p = $view;
        $this->width = $width;
        $this->height = $height;
    }

    /** A page from HTML text; relative paths resolve against `$baseDir`. */
    public static function fromHtml(string $html, float $width = 800, float $height = 600, ?string $baseDir = null): self
    {
        return new self(Native::ffi()->nile_view_create_html($html, $baseDir, $width, $height), $width, $height);
    }

    public static function fromFile(string $path, float $width = 800, float $height = 600): self
    {
        return new self(Native::ffi()->nile_view_create_file($path, $width, $height), $width, $height);
    }

    public function close(): void
    {
        if ($this->p !== null) {
            Native::ffi()->nile_view_destroy($this->p);
            $this->p = null;
        }
    }

    public function __destruct()
    {
        $this->close();
    }

    private function live()
    {
        if ($this->p === null) {
            throw new \LogicException('the view is closed');
        }
        return $this->p;
    }

    public function title(): string
    {
        return Native::ffi()->nile_view_title($this->live()) ?? '';
    }

    public function contentHeight(): float
    {
        return Native::ffi()->nile_view_content_height($this->live());
    }

    public function resize(float $width, float $height): void
    {
        $this->width = $width;
        $this->height = $height;
        Native::ffi()->nile_view_resize($this->live(), $width, $height);
    }

    public function reload(): void
    {
        Native::ffi()->nile_view_reload($this->live());
    }

    // --- scripts ---------------------------------------------------------------------------------------------------

    /** Expose a callable to the page as `host.<name>(...)`; arguments are decoded JSON, the result is encoded as JSON. */
    public function register(string $name, callable $fn): void
    {
        $f = new HostFunction($name, $fn);
        $this->callbacks[$name] = $f; // keeps the C callback alive
        Native::ffi()->nile_view_register_function($this->live(), $name, $f->callback, null);
    }

    /** Run JavaScript. Strings come back as they are, anything else decoded from JSON, `undefined` as null. */
    public function eval(string $code)
    {
        $ffi = Native::ffi();
        $out = $ffi->new('char*');
        $status = $ffi->nile_view_eval($this->live(), $code, FFI::addr($out));
        $text = Native::take($out);
        if ($status !== 0) {
            throw new ScriptException($text);
        }
        return self::decode($text);
    }

    /** Call a page function (`"save"` or dotted `"app.save"`) with JSON-able arguments. */
    public function call(string $name, ...$args)
    {
        $ffi = Native::ffi();
        $out = $ffi->new('char*');
        $status = $ffi->nile_view_call($this->live(), $name, json_encode($args), FFI::addr($out));
        $text = Native::take($out);
        if ($status !== 0) {
            throw new ScriptException($text);
        }
        return $text === '' ? null : json_decode($text, true);
    }

    private static function decode(string $text)
    {
        if ($text === '') {
            return null;
        }
        $value = json_decode($text, true);
        return json_last_error() === JSON_ERROR_NONE ? $value : $text;
    }

    /** @return string[] the `console.log/warn/error` lines since the last call */
    public function console(): array
    {
        $text = Native::take(Native::ffi()->nile_view_take_console($this->live()));
        return $text === '' ? [] : explode("\n", $text);
    }

    // --- input -----------------------------------------------------------------------------------------------------

    public function pointerMove(float $x, float $y): bool
    {
        return Native::ffi()->nile_pointer_move($this->live(), $x, $y);
    }

    public function pointerLeave(): bool
    {
        return Native::ffi()->nile_pointer_leave($this->live());
    }

    public function pointerButton(int $button, bool $pressed, int $modifiers = 0): bool
    {
        return Native::ffi()->nile_pointer_button($this->live(), $button, $pressed, $modifiers);
    }

    public function click(float $x, float $y, int $modifiers = 0): bool
    {
        $this->pointerMove($x, $y);
        $this->pointerButton(0, true, $modifiers);
        return $this->pointerButton(0, false, $modifiers);
    }

    public function wheel(float $dx, float $dy): bool
    {
        return Native::ffi()->nile_wheel($this->live(), $dx, $dy);
    }

    /** Press and release a DOM `KeyboardEvent.key` ("a", "Enter", "ArrowLeft", ...). */
    public function key(string $key, int $modifiers = 0): bool
    {
        Native::ffi()->nile_key($this->live(), $key, $modifiers, true);
        return Native::ffi()->nile_key($this->live(), $key, $modifiers, false);
    }

    public function type(string $text): bool
    {
        return Native::ffi()->nile_text_input($this->live(), $text);
    }

    public function selectedText(): string
    {
        return Native::take(Native::ffi()->nile_view_selected_text($this->live()));
    }

    /** Run timers and animations. @return array{0: bool, 1: ?int} redraw, and the ms until the next tick (null: nothing scheduled) */
    public function tick(): array
    {
        $ffi = Native::ffi();
        $t = $ffi->new('NileTick');
        $t->struct_size = FFI::sizeof($t);
        $ffi->nile_view_tick($this->live(), FFI::addr($t));
        return [$t->redraw, $t->next_ms < 0 ? null : $t->next_ms];
    }

    /** Let the page's timers run for about `$ms` milliseconds. */
    public function runFor(int $ms): void
    {
        $end = microtime(true) + $ms / 1000;
        while (microtime(true) < $end) {
            [, $next] = $this->tick();
            usleep(min($next ?? 10, 10) * 1000);
        }
    }

    // --- pixels ----------------------------------------------------------------------------------------------------

    /** The page as premultiplied BGRA bytes, `width*scale` by `height*scale`. */
    public function render(float $scale = 1.0): string
    {
        $w = (int) round($this->width * $scale);
        $h = (int) round($this->height * $scale);
        $len = $w * $h * 4;
        $buf = Native::ffi()->new("uint8_t[$len]");
        if (!Native::ffi()->nile_view_render($this->live(), $buf, $len, $w, $h, $scale)) {
            throw new \RuntimeException('render failed');
        }
        return FFI::string($buf, $len);
    }

    public function savePng(string $path, float $scale = 1.0): void
    {
        file_put_contents($path, Png::encode($this->render($scale), (int) round($this->width * $scale), (int) round($this->height * $scale)));
    }

    public function writePdf(string $path): void
    {
        if (Native::ffi()->nile_view_write_pdf($this->live(), $path) !== 0) {
            throw new \RuntimeException('could not write the PDF');
        }
    }
}
