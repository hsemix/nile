using System.Text.Json;

namespace Nile;

/// <summary>A script error raised in the page.</summary>
public sealed class ScriptException : Exception
{
    public ScriptException(string message) : base(message) { }
}

[Flags]
public enum Modifiers : byte { None = 0, Shift = 1, Control = 2, Alt = 4, Meta = 8 }

/// <summary>A page you drive yourself: load it, call into it, send input, and read pixels back (headless or into your own window).</summary>
public sealed class NileView : IDisposable
{
    IntPtr p;
    readonly Dictionary<string, HostFunction> callbacks = new();

    public float Width { get; private set; }
    public float Height { get; private set; }

    NileView(IntPtr view, float width, float height)
    {
        if (view == IntPtr.Zero) throw new ArgumentException("could not create the view");
        (p, Width, Height) = (view, width, height);
    }

    /// <summary>A page from HTML text; relative paths resolve against <paramref name="baseDir"/>.</summary>
    public static NileView FromHtml(string html, float width = 800, float height = 600, string? baseDir = null) =>
        new(Native.nile_view_create_html(html, baseDir, width, height), width, height);

    public static NileView FromFile(string path, float width = 800, float height = 600) =>
        new(Native.nile_view_create_file(path, width, height), width, height);

    IntPtr Live => p != IntPtr.Zero ? p : throw new ObjectDisposedException(nameof(NileView));

    public void Dispose()
    {
        if (p != IntPtr.Zero) Native.nile_view_destroy(p);
        p = IntPtr.Zero;
        GC.SuppressFinalize(this);
    }

    ~NileView() => Dispose();

    public string Title => System.Runtime.InteropServices.Marshal.PtrToStringUTF8(Native.nile_view_title(Live)) ?? "";
    public float ContentHeight => Native.nile_view_content_height(Live);

    public void Resize(float width, float height)
    {
        (Width, Height) = (width, height);
        Native.nile_view_resize(Live, width, height);
    }

    public void Reload() => Native.nile_view_reload(Live);

    // --- scripts ---------------------------------------------------------------------------------------------------

    /// <summary>Expose a function to the page as <c>host.name(...)</c>; arguments arrive as JSON values.</summary>
    public void Register(string name, Func<JsonElement[], object?> fn)
    {
        var f = new HostFunction(name, fn);
        callbacks[name] = f; // keeps the delegate alive
        Native.nile_view_register_function(Live, name, f.Callback, IntPtr.Zero);
    }

    public void Register<T1, TR>(string name, Func<T1, TR> fn) => Register(name, HostFunction.Wrap(fn));
    public void Register<T1, T2, TR>(string name, Func<T1, T2, TR> fn) => Register(name, HostFunction.Wrap(fn));
    public void Register<T1, T2, T3, TR>(string name, Func<T1, T2, T3, TR> fn) => Register(name, HostFunction.Wrap(fn));

    /// <summary>Run JavaScript. A string result comes back as is, anything else as JSON text, <c>undefined</c> as null.</summary>
    public string? Eval(string code)
    {
        var status = Native.nile_view_eval(Live, code, out var result);
        var text = Native.TakeString(result);
        if (status != 0) throw new ScriptException(text);
        return text.Length == 0 ? null : text;
    }

    /// <summary>Run JavaScript and decode the JSON result.</summary>
    public T? Eval<T>(string code)
    {
        var text = Eval(code);
        if (text is null) return default;
        try { return JsonSerializer.Deserialize<T>(text); }
        catch (JsonException) when (typeof(T) == typeof(string)) { return (T)(object)text; }
    }

    /// <summary>Call a page function (<c>"save"</c> or dotted <c>"app.save"</c>) with JSON-able arguments.</summary>
    public T? Call<T>(string name, params object?[] args)
    {
        var status = Native.nile_view_call(Live, name, JsonSerializer.Serialize(args), out var result);
        var text = Native.TakeString(result);
        if (status != 0) throw new ScriptException(text);
        return text.Length == 0 ? default : JsonSerializer.Deserialize<T>(text);
    }

    /// <summary>The <c>console.log/warn/error</c> lines since the last call.</summary>
    public string[] Console()
    {
        var text = Native.TakeString(Native.nile_view_take_console(Live));
        return text.Length == 0 ? Array.Empty<string>() : text.Split('\n');
    }

    // --- input -----------------------------------------------------------------------------------------------------

    public bool PointerMove(float x, float y) => Native.nile_pointer_move(Live, x, y) != 0;
    public bool PointerLeave() => Native.nile_pointer_leave(Live) != 0;
    public bool PointerButton(int button, bool pressed, Modifiers mods = 0) => Native.nile_pointer_button(Live, button, (byte)(pressed ? 1 : 0), (byte)mods) != 0;

    public bool Click(float x, float y, Modifiers mods = 0)
    {
        PointerMove(x, y);
        PointerButton(0, true, mods);
        return PointerButton(0, false, mods);
    }

    public bool Wheel(float dx, float dy) => Native.nile_wheel(Live, dx, dy) != 0;

    /// <summary>Press and release a DOM <c>KeyboardEvent.key</c> ("a", "Enter", "ArrowLeft", ...).</summary>
    public bool Key(string key, Modifiers mods = 0)
    {
        Native.nile_key(Live, key, (byte)mods, 1);
        return Native.nile_key(Live, key, (byte)mods, 0) != 0;
    }

    public bool Type(string text) => Native.nile_text_input(Live, text) != 0;
    public string SelectedText() => Native.TakeString(Native.nile_view_selected_text(Live));

    /// <summary>Run timers and animations. <c>NextMs</c> is null when nothing is scheduled.</summary>
    public (bool Redraw, int? NextMs) Tick()
    {
        var t = new Native.Tick { StructSize = (uint)System.Runtime.InteropServices.Marshal.SizeOf<Native.Tick>() };
        Native.nile_view_tick(Live, ref t);
        return (t.Redraw, t.NextMs < 0 ? null : t.NextMs);
    }

    /// <summary>Let the page's timers run for about <paramref name="ms"/> milliseconds.</summary>
    public void RunFor(int ms)
    {
        var end = DateTime.UtcNow.AddMilliseconds(ms);
        while (DateTime.UtcNow < end)
        {
            var (_, next) = Tick();
            Thread.Sleep(Math.Min(next ?? 10, 10));
        }
    }

    // --- pixels ----------------------------------------------------------------------------------------------------

    /// <summary>The page as premultiplied BGRA bytes, <c>Width*scale</c> by <c>Height*scale</c>.</summary>
    public unsafe byte[] Render(float scale = 1f)
    {
        var (w, h) = ((uint)MathF.Round(Width * scale), (uint)MathF.Round(Height * scale));
        var buf = new byte[w * h * 4];
        fixed (byte* px = buf)
            if (Native.nile_view_render(Live, px, (nuint)buf.Length, w, h, scale) == 0) throw new InvalidOperationException("render failed");
        return buf;
    }

    public void SavePng(string path, float scale = 1f) =>
        File.WriteAllBytes(path, Png.Encode(Render(scale), (int)MathF.Round(Width * scale), (int)MathF.Round(Height * scale)));

    public void WritePdf(string path)
    {
        if (Native.nile_view_write_pdf(Live, path) != 0) throw new IOException("could not write the PDF");
    }
}
