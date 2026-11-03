using Nile;

// A plain console test runner (no test framework to download): exit code 0 when every check passes.
var failed = 0;
void Check(string name, Action body)
{
    try { body(); Console.WriteLine($"ok   {name}"); }
    catch (Exception e) { failed++; Console.WriteLine($"FAIL {name}: {e.Message}"); }
}
void Equal<T>(T expected, T actual)
{
    if (!EqualityComparer<T>.Default.Equals(expected, actual)) throw new Exception($"expected {expected}, got {actual}");
}

Check("version and title", () =>
{
    if (App.Version.Length == 0) throw new Exception("no version");
    using var v = NileView.FromHtml("<title>Hello</title><p>hi</p>");
    Equal("Hello", v.Title);
});

Check("eval and call", () =>
{
    using var v = NileView.FromHtml("<script>function twice(n){return n*2}</script>");
    Equal(3, v.Eval<int>("1 + 2"));
    Equal("text", v.Eval("'text'"));
    Equal<string?>(null, v.Eval("undefined"));
    Equal(42, v.Call<int>("twice", 21));
    try { v.Eval("nope("); throw new Exception("no error"); } catch (ScriptException) { }
});

Check("host functions", () =>
{
    using var v = NileView.FromHtml("<p>x</p>");
    var seen = new List<(int, int)>();
    v.Register<int, int, int>("add", (a, b) => { seen.Add((a, b)); return a + b; });
    v.Register("boom", _ => throw new InvalidOperationException("boom"));
    Equal(3, v.Eval<int>("host.add(1, 2)"));
    Equal(1, seen.Count);
    Equal("undefined", v.Eval("typeof host.boom()"));
});

Check("input changes the page", () =>
{
    using var v = NileView.FromHtml("<button onclick=\"document.title='clicked'\" style='width:100px;height:40px'>Go</button><input id=i style='display:block'>", 300, 200);
    v.Click(20, 20);
    Equal("clicked", v.Title);
    v.Eval("document.getElementById('i').focus()");
    v.Type("abc");
    Equal("abc", v.Eval("document.getElementById('i').value"));
});

Check("render and png", () =>
{
    using var v = NileView.FromHtml("<body style='margin:0;background:#f00'>", 20, 10);
    var px = v.Render();
    Equal(20 * 10 * 4, px.Length);
    Equal<byte>(255, px[2]); // BGRA: red is byte 2
    var path = Path.Combine(Path.GetTempPath(), $"nile-{Guid.NewGuid():N}.png");
    v.SavePng(path);
    var data = File.ReadAllBytes(path);
    File.Delete(path);
    Equal<byte>(0x89, data[0]);
    Equal(20, (data[16] << 24) | (data[17] << 16) | (data[18] << 8) | data[19]);
});

Check("a disposed view refuses", () =>
{
    var v = NileView.FromHtml("<p>x</p>");
    v.Dispose();
    try { v.Eval("1"); throw new Exception("no error"); } catch (ObjectDisposedException) { }
});

Check("main renders headless and calls .NET", () =>
{
    App.Function<string, string>("shout", s => s.ToUpperInvariant() + "!");
    var dir = Directory.CreateTempSubdirectory().FullName;
    var page = Path.Combine(dir, "p.html");
    var png = Path.Combine(dir, "p.png");
    File.WriteAllText(page, "<body style='margin:0;background:#00f'><script>document.title = host.shout('hey')</script>");
    Equal(0, App.Main(page, "--png", png, "--width", "30", "--height", "20"));
    Equal<byte>(0x89, File.ReadAllBytes(png)[0]);
});

return failed;
