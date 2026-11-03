<?php

declare(strict_types=1);

namespace Nile;

use FFI;

/** A PHP callable exposed to page scripts as `host.name(...)`: JSON arguments in, JSON result out. */
final class HostFunction
{
    /** @var \FFI\CData holds the C function pointer (`->f`); keep this object alive as long as the library may call it */
    private $holder;
    /** @var \FFI\CData the C function pointer */
    public $callback;
    /** @var \FFI\CData|null the library copies the result at once, so one buffer per function is enough */
    private $result = null;
    private $name;
    private $fn;

    public function __construct(string $name, callable $fn)
    {
        $this->name = $name;
        $this->fn = $fn;
        // A PHP closure becomes a C function pointer by being assigned to a field of a function-pointer type.
        $this->holder = Native::ffi()->new('NileHostHolder');
        $this->holder->f = function ($argsJson, $user) {
            return $this->call($argsJson);
        };
        $this->callback = $this->holder->f;
    }

    private function call($argsJson)
    {
        try {
            $args = $argsJson === null ? [] : (json_decode($argsJson, true, 512, JSON_THROW_ON_ERROR) ?? []);
            $value = ($this->fn)(...$args);
            if ($value === null) {
                return null;
            }
            $json = json_encode($value, JSON_THROW_ON_ERROR);
            $this->result = Native::ffi()->new('char[' . (strlen($json) + 1) . ']');
            FFI::memcpy($this->result, $json, strlen($json));
            return Native::ffi()->cast('char*', $this->result);
        } catch (\Throwable $e) {
            fwrite(STDERR, "nile: host.{$this->name} threw: $e\n");
            return null;
        }
    }
}
