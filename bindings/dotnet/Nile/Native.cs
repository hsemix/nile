using System.Reflection;
using System.Runtime.InteropServices;

namespace Nile;

/// <summary>The C functions of libnile_app (nile.h, nile_app.h) and how the library is found.</summary>
internal static unsafe class Native
{
    public const uint AbiVersion = 2;
    const string Lib = "nile_app";

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate IntPtr HostFn(IntPtr argsJson, IntPtr user);

    [StructLayout(LayoutKind.Sequential)]
    public struct Tick
    {
        public uint StructSize;
        [MarshalAs(UnmanagedType.U1)] public bool Redraw;
        public int NextMs;
    }

    static Native() => NativeLibrary.SetDllImportResolver(typeof(Native).Assembly, Resolve);

    static IntPtr Resolve(string name, Assembly assembly, DllImportSearchPath? path)
    {
        if (name != Lib) return IntPtr.Zero;
        var file = OperatingSystem.IsWindows() ? "nile_app.dll" : OperatingSystem.IsMacOS() ? "libnile_app.dylib" : "libnile_app.so";
        var tried = new List<string>();
        foreach (var candidate in Candidates(file))
        {
            tried.Add(candidate);
            if (File.Exists(candidate) && NativeLibrary.TryLoad(candidate, out var handle))
            {
                if (((delegate* unmanaged[Cdecl]<uint>)NativeLibrary.GetExport(handle, "nile_abi_version"))() != AbiVersion)
                    throw new InvalidOperationException($"{candidate} is a different Nile ABI version than these bindings ({AbiVersion})");
                return handle;
            }
        }
        throw new DllNotFoundException($"{file} not found; build it with `cargo build -p nile-app --release` or set NILE_LIBRARY. Looked in: {string.Join(", ", tried.Take(4))}");
    }

    static IEnumerable<string> Candidates(string file)
    {
        var env = Environment.GetEnvironmentVariable("NILE_LIBRARY");
        if (!string.IsNullOrEmpty(env)) yield return env;
        yield return Path.Combine(AppContext.BaseDirectory, file);
        var rid = RuntimeInformation.RuntimeIdentifier;
        yield return Path.Combine(AppContext.BaseDirectory, "runtimes", rid, "native", file);
        for (var dir = new DirectoryInfo(AppContext.BaseDirectory); dir != null; dir = dir.Parent)
            foreach (var profile in new[] { "release", "debug" })
                yield return Path.Combine(dir.FullName, "target", profile, file);
    }

    const CallingConvention C = CallingConvention.Cdecl;
    const UnmanagedType U8 = UnmanagedType.LPUTF8Str;

    [DllImport(Lib, CallingConvention = C)] public static extern uint nile_abi_version();
    [DllImport(Lib, CallingConvention = C)] public static extern IntPtr nile_version();
    [DllImport(Lib, CallingConvention = C)] public static extern IntPtr nile_view_create_html([MarshalAs(U8)] string html, [MarshalAs(U8)] string? baseDir, float w, float h);
    [DllImport(Lib, CallingConvention = C)] public static extern IntPtr nile_view_create_file([MarshalAs(U8)] string path, float w, float h);
    [DllImport(Lib, CallingConvention = C)] public static extern void nile_view_destroy(IntPtr view);
    [DllImport(Lib, CallingConvention = C)] public static extern void nile_view_reload(IntPtr view);
    [DllImport(Lib, CallingConvention = C)] public static extern void nile_string_free(IntPtr s);
    [DllImport(Lib, CallingConvention = C)] public static extern void nile_view_resize(IntPtr view, float w, float h);
    [DllImport(Lib, CallingConvention = C)] public static extern float nile_view_content_height(IntPtr view);
    [DllImport(Lib, CallingConvention = C)] public static extern byte nile_view_render(IntPtr view, byte* pixels, nuint len, uint w, uint h, float scale);
    [DllImport(Lib, CallingConvention = C)] public static extern byte nile_pointer_move(IntPtr view, float x, float y);
    [DllImport(Lib, CallingConvention = C)] public static extern byte nile_pointer_leave(IntPtr view);
    [DllImport(Lib, CallingConvention = C)] public static extern byte nile_pointer_button(IntPtr view, int button, byte pressed, byte modifiers);
    [DllImport(Lib, CallingConvention = C)] public static extern byte nile_wheel(IntPtr view, float dx, float dy);
    [DllImport(Lib, CallingConvention = C)] public static extern byte nile_key(IntPtr view, [MarshalAs(U8)] string key, byte modifiers, byte pressed);
    [DllImport(Lib, CallingConvention = C)] public static extern byte nile_text_input(IntPtr view, [MarshalAs(U8)] string text);
    [DllImport(Lib, CallingConvention = C)] public static extern void nile_view_tick(IntPtr view, ref Tick tick);
    [DllImport(Lib, CallingConvention = C)] public static extern IntPtr nile_view_selected_text(IntPtr view);
    [DllImport(Lib, CallingConvention = C)] public static extern IntPtr nile_view_title(IntPtr view);
    [DllImport(Lib, CallingConvention = C)] public static extern int nile_view_eval(IntPtr view, [MarshalAs(U8)] string code, out IntPtr result);
    [DllImport(Lib, CallingConvention = C)] public static extern int nile_view_call(IntPtr view, [MarshalAs(U8)] string name, [MarshalAs(U8)] string? args, out IntPtr result);
    [DllImport(Lib, CallingConvention = C)] public static extern byte nile_view_register_function(IntPtr view, [MarshalAs(U8)] string name, HostFn callback, IntPtr user);
    [DllImport(Lib, CallingConvention = C)] public static extern IntPtr nile_view_take_console(IntPtr view);
    [DllImport(Lib, CallingConvention = C)] public static extern int nile_view_write_pdf(IntPtr view, [MarshalAs(U8)] string path);
    [DllImport(Lib, CallingConvention = C)] public static extern byte nile_app_register_function([MarshalAs(U8)] string name, HostFn callback, IntPtr user);
    [DllImport(Lib, CallingConvention = C)] public static extern int nile_run(int argc, IntPtr[] argv);
    [DllImport(Lib, CallingConvention = C)] public static extern int nile_run_file([MarshalAs(U8)] string path);
    [DllImport(Lib, CallingConvention = C)] public static extern int nile_run_html([MarshalAs(U8)] string html, [MarshalAs(U8)] string? baseDir, float w, float h);

    /// <summary>Copy a library-owned string into a managed one and free it.</summary>
    public static string TakeString(IntPtr p)
    {
        if (p == IntPtr.Zero) return "";
        var s = Marshal.PtrToStringUTF8(p) ?? "";
        nile_string_free(p);
        return s;
    }
}
