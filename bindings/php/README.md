# Nile for PHP

PHP bindings for [Nile](../../README.md) over `libnile_app` with PHP's built-in FFI extension (PHP 8.2 or newer; no Composer packages). Same shape as the
[Python](../python/README.md) and [.NET](../dotnet/README.md) bindings. For desktop apps run from the command line, not for web servers: FFI is
only enabled in the CLI by default (`ffi.enable=preload`), and windows need a display.

```php
require 'bindings/php/src/autoload.php';       // or Composer's autoloader

Nile\App::function('add', fn($a, $b) => $a + $b);   // page scripts call host.add(1, 2)
exit(Nile\App::run('index.html'));                  // a window; returns when the last one closes
```

Or drive a page yourself, headless:

```php
$v = Nile\View::fromHtml('<h1>Hi</h1>', 300, 100);
$v->register('add', fn($a, $b) => $a + $b);
$v->eval('host.add(1, 2)');            // 3
$v->click(20, 20); $v->type('text');
$v->savePng('out.png');
```

`App::main([...])` runs the `nile` program itself (`--png`, `--pack`, ...).

## Setting up

```sh
cargo build -p nile-app --release            # builds libnile_app
php bindings/php/tests/run.php
php bindings/php/examples/hello.php
```

Check that `php -m` lists `FFI`. The library is looked for in `NILE_LIBRARY`, `bindings/php/native/`, then `target/release` and `target/debug` of the
checkout, and its ABI version must match. `App::run` and friends must be called from the main thread, and host functions run on it, with decoded
JSON arguments and a JSON-encodable result; an exception is written to stderr and the script gets `undefined`.

Not on Packagist yet.
