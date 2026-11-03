<?php

// For use without Composer: `require 'path/to/bindings/php/src/autoload.php';`
spl_autoload_register(static function (string $class): void {
    if (strncmp($class, 'Nile\\', 5) === 0) {
        $file = __DIR__ . '/' . str_replace('\\', '/', substr($class, 5)) . '.php';
        if (is_file($file)) {
            require $file;
        }
    }
});
