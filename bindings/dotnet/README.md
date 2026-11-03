# Nile for .NET

.NET bindings for [Nile](../../README.md) (`net8.0`, P/Invoke over `libnile_app`, no dependencies). Same shape as the [Python bindings](../python/README.md).

```csharp
using Nile;

App.Function<int, int, int>("add", (a, b) => a + b);   // page scripts call host.add(1, 2)
return App.Run("index.html");                          // a window; returns when the last one closes
```

Or drive a page yourself, headless:

```csharp
using var v = NileView.FromHtml("<h1>Hi</h1>", 300, 100);
v.Register<int, int, int>("add", (a, b) => a + b);
v.Eval<int>("host.add(1, 2)");          // 3
v.Click(20, 20); v.Type("text");
v.SavePng("out.png");
```

`App.Main(args)` runs the `nile` program itself (`--png`, `--pack`, ...).

## Setting up

```sh
cargo build -p nile-app --release            # builds libnile_app
dotnet run --project bindings/dotnet/Nile.Tests
dotnet run --project bindings/dotnet/examples/Hello
```

The library is looked for in `NILE_LIBRARY`, next to the app, `runtimes/<rid>/native/`, then `target/release` and `target/debug` of the
checkout, and its ABI version must match. `App.Run` and friends must be called on the main thread (macOS requires it), and host
functions run on that thread, with JSON arguments and a JSON-serialisable result; an exception is written to stderr and the script gets
`undefined`.

Not on NuGet yet (the package layout is ready: put each platform's library in `Nile/runtimes/<rid>/native`).
