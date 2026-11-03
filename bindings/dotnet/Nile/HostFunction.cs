using System.Runtime.InteropServices;
using System.Text.Json;

namespace Nile;

/// <summary>A .NET delegate exposed to page scripts as <c>host.name(...)</c>: JSON arguments in, JSON result out.</summary>
internal sealed class HostFunction
{
    readonly string name;
    readonly Func<JsonElement[], object?> fn;
    IntPtr result; // the library copies the string at once, so one buffer per function is enough
    public readonly Native.HostFn Callback;

    public HostFunction(string name, Func<JsonElement[], object?> fn)
    {
        this.name = name;
        this.fn = fn;
        Callback = Call;
    }

    IntPtr Call(IntPtr argsJson, IntPtr user)
    {
        try
        {
            var text = argsJson == IntPtr.Zero ? "[]" : Marshal.PtrToStringUTF8(argsJson) ?? "[]";
            var args = JsonSerializer.Deserialize<JsonElement[]>(text) ?? Array.Empty<JsonElement>();
            var value = fn(args);
            if (value is null) return IntPtr.Zero;
            Marshal.FreeCoTaskMem(result);
            result = Marshal.StringToCoTaskMemUTF8(JsonSerializer.Serialize(value));
            return result;
        }
        catch (Exception e)
        {
            Console.Error.WriteLine($"nile: host.{name} threw: {e}");
            return IntPtr.Zero;
        }
    }

    public static Func<JsonElement[], object?> Wrap<T1, TR>(Func<T1, TR> f) => a => f(Arg<T1>(a, 0));
    public static Func<JsonElement[], object?> Wrap<T1, T2, TR>(Func<T1, T2, TR> f) => a => f(Arg<T1>(a, 0), Arg<T2>(a, 1));
    public static Func<JsonElement[], object?> Wrap<T1, T2, T3, TR>(Func<T1, T2, T3, TR> f) => a => f(Arg<T1>(a, 0), Arg<T2>(a, 1), Arg<T3>(a, 2));

    static T Arg<T>(JsonElement[] a, int i) => i < a.Length ? a[i].Deserialize<T>()! : default!;
}
