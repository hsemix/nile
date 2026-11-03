// A window whose page calls into .NET.  Run:  dotnet run --project bindings/dotnet/examples/Hello   (headless: -- --png out.png)
using Nile;

App.Function<string, string>("greet", name => $"Hello, {name}, from .NET {Environment.Version.Major}");
return App.Main(args.Prepend(Path.Combine(AppContext.BaseDirectory, "hello.html")).ToArray());
