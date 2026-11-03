using System.Runtime.InteropServices;
using System.Text.Json;

namespace Nile;

/// <summary>Run Nile as an application: windows and the event loop are the library's.</summary>
public static class App
{
    static readonly List<HostFunction> Functions = new(); // alive for the whole run

    public static string Version => Marshal.PtrToStringUTF8(Native.nile_version()) ?? "";

    /// <summary>Expose a function to every window as <c>host.name(...)</c>. Register before <see cref="Run"/>.</summary>
    public static void Function(string name, Func<JsonElement[], object?> fn)
    {
        var f = new HostFunction(name, fn);
        Functions.Add(f);
        if (Native.nile_app_register_function(name, f.Callback, IntPtr.Zero) == 0) throw new ArgumentException($"could not register {name}");
    }

    public static void Function<T1, TR>(string name, Func<T1, TR> fn) => Function(name, HostFunction.Wrap(fn));
    public static void Function<T1, T2, TR>(string name, Func<T1, T2, TR> fn) => Function(name, HostFunction.Wrap(fn));
    public static void Function<T1, T2, T3, TR>(string name, Func<T1, T2, T3, TR> fn) => Function(name, HostFunction.Wrap(fn));

    /// <summary>Open the HTML file (or folder) in a window and run until the last window closes. Returns the exit code.</summary>
    public static int Run(string path = ".") => Native.nile_run_file(path);

    /// <summary>Show HTML text in a window.</summary>
    public static int RunHtml(string html, string? baseDir = null, float width = 0, float height = 0) => Native.nile_run_html(html, baseDir, width, height);

    /// <summary>Run the <c>nile</c> program with arguments (<c>page.html</c>, <c>--png out.png</c>, <c>--pack</c>, ...).</summary>
    public static int Main(params string[] args)
    {
        var all = new[] { "nile" }.Concat(args).ToArray();
        var ptrs = all.Select(Marshal.StringToCoTaskMemUTF8).ToArray();
        try { return Native.nile_run(ptrs.Length, ptrs); }
        finally { foreach (var p in ptrs) Marshal.FreeCoTaskMem(p); }
    }
}
