<?php

// A window whose page calls into PHP.  Run:  php examples/hello.php   (headless: php examples/hello.php --png out.png)
require __DIR__ . '/../src/autoload.php';

Nile\App::function('greet', function ($name) {
    return "Hello, $name, from PHP " . PHP_MAJOR_VERSION . '.' . PHP_MINOR_VERSION;
});

exit(Nile\App::main(array_merge([__DIR__ . '/hello.html'], array_slice($argv, 1))));
