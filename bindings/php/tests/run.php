<?php

declare(strict_types=1);

// A plain test runner (no PHPUnit to install): exit code 0 when every check passes.
require __DIR__ . '/../src/autoload.php';

use Nile\App;
use Nile\ScriptException;
use Nile\View;

$failed = 0;
function check(string $name, callable $body): void
{
    global $failed;
    try {
        $body();
        echo "ok   $name\n";
    } catch (\Throwable $e) {
        $failed++;
        echo "FAIL $name: {$e->getMessage()}\n";
    }
}
function same($expected, $actual): void
{
    if ($expected !== $actual) {
        throw new \Exception('expected ' . var_export($expected, true) . ', got ' . var_export($actual, true));
    }
}

check('version and title', function () {
    same(true, App::version() !== '');
    $v = View::fromHtml('<title>Hello</title><p>hi</p>');
    same('Hello', $v->title());
});

check('eval and call', function () {
    $v = View::fromHtml('<script>function twice(n){return n*2} var app={name:"x"}</script>');
    same(3, $v->eval('1 + 2'));
    same('text', $v->eval("'text'"));
    same(null, $v->eval('undefined'));
    same(['a' => [1, 2]], $v->eval('({a: [1, 2]})'));
    same(42, $v->call('twice', 21));
    try {
        $v->eval('nope(');
        throw new \Exception('no error');
    } catch (ScriptException $e) {
    }
});

check('host functions', function () {
    $v = View::fromHtml('<p>x</p>');
    $seen = [];
    $v->register('add', function ($a, $b) use (&$seen) {
        $seen[] = [$a, $b];
        return $a + $b;
    });
    $v->register('boom', function () {
        throw new \RuntimeException('boom');
    });
    same(3, $v->eval('host.add(1, 2)'));
    same([[1, 2]], $seen);
    same('undefined', $v->eval('typeof host.boom()'));
});

check('input changes the page', function () {
    $v = View::fromHtml("<button onclick=\"document.title='clicked'\" style='width:100px;height:40px'>Go</button><input id=i style='display:block'>", 300, 200);
    $v->click(20, 20);
    same('clicked', $v->title());
    $v->eval("document.getElementById('i').focus()");
    $v->type('abc');
    same('abc', $v->eval("document.getElementById('i').value"));
});

check('render and png', function () {
    $v = View::fromHtml("<body style='margin:0;background:#f00'>", 20, 10);
    $px = $v->render();
    same(20 * 10 * 4, strlen($px));
    same(255, ord($px[2])); // BGRA: red is byte 2
    $path = sys_get_temp_dir() . '/nile-' . uniqid() . '.png';
    $v->savePng($path);
    $data = file_get_contents($path);
    unlink($path);
    same("\x89PNG\r\n\x1a\n", substr($data, 0, 8));
    same(20, unpack('N', substr($data, 16, 4))[1]);
});

check('a closed view refuses', function () {
    $v = View::fromHtml('<p>x</p>');
    $v->close();
    try {
        $v->eval('1');
        throw new \Exception('no error');
    } catch (\LogicException $e) {
    }
});

check('main renders headless and calls PHP', function () {
    App::function('shout', function ($s) {
        return strtoupper($s) . '!';
    });
    $dir = sys_get_temp_dir() . '/nile-' . uniqid();
    mkdir($dir);
    file_put_contents("$dir/p.html", "<body style='margin:0;background:#00f'><script>document.title = host.shout('hey')</script>");
    same(0, App::main(["$dir/p.html", '--png', "$dir/p.png", '--width', '30', '--height', '20']));
    same("\x89PNG", substr(file_get_contents("$dir/p.png"), 0, 4));
});

exit($failed);
