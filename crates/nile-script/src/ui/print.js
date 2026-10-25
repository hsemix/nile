// @nile/print: find printers and send them work, from script.
//
//   import { printers, printFile, printRaw, sendNetwork } from "@nile/print";
//   const list = await printers();                       // [{ name, default, status }]
//   await printFile("/tmp/invoice.pdf", { printer: "Office", copies: 2 });   // a document (PDF, text, image)
//   await printRaw("EPSON TM-T20", bytes);               // raw bytes to a printer (receipt/label printers)
//   await sendNetwork("192.168.1.50", 9100, bytes);      // raw bytes to a network printer
//
// Everything here goes through the operating system's own print system, so printers installed on the machine (USB
// ones included) just work: nothing is bundled and there is no USB driver in Nile.
//   macOS / Linux: CUPS (`lpstat`, `lp`, `lp -o raw`).
//   Windows:       PowerShell (`Get-CimInstance Win32_Printer`; `Start-Process -Verb PrintTo` for documents, which needs
//                  the program that opens that file type to support printing; the winspool API for raw jobs).
// printers(): `status` is "idle", "printing", "offline", "error" or "unknown". printFile(path, { printer, copies, title }):
// no `printer` means the default one; it resolves once the job was handed to the spooler (not when paper comes out).
// printRaw(printer, bytes): bytes is a Uint8Array/array/ArrayBuffer or a string (UTF-8); the printer must accept raw
// data (receipt printers do; a driver-only printer may not). sendNetwork(host, port, bytes, { timeout }) writes to a socket
// (9100 is the usual "RAW / JetDirect" port), no spooler involved. All of them reject with an Error whose message is
// what the system said.

import { spawn, fs, tmpdir, TCP, uname } from "@sys";
import { PLATFORM } from "@env";

const windows = PLATFORM === "Windows";

function toBytes(data) {
  if (typeof data === "string") return new TextEncoder().encode(data);
  if (data instanceof ArrayBuffer) return new Uint8Array(data);
  if (ArrayBuffer.isView(data)) return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  return Uint8Array.from(data);
}

const decode = (bytes) => (bytes ? new TextDecoder().decode(bytes) : "");

/** Run a program: resolves { code, out, err }. `input` (bytes or text) is written to its standard input. */
export async function run(args, input) {
  let proc;
  try {
    proc = spawn(args, { stdin: input === undefined ? "ignore" : "pipe" });
  } catch (e) {
    throw new Error(args[0] + ": " + (e && e.message ? e.message : String(e)));
  }
  const drain = async (stream) => {
    const parts = [];
    if (!stream) return "";
    for (;;) {
      const chunk = await stream.read();
      if (chunk === undefined) break;
      parts.push(decode(chunk));
    }
    return parts.join("");
  };
  if (input !== undefined) {
    await proc.stdin.write(toBytes(input));
    proc.stdin.close();
  }
  const [out, err, status] = await Promise.all([drain(proc.stdout), drain(proc.stderr), proc.wait()]);
  const code = status && typeof status === "object" ? (status.exit_status !== undefined ? status.exit_status : status.code) : status;
  return { code: code || 0, out: out, err: err };
}

function fail(result, what) {
  return new Error((result.err || result.out || what).trim() || what);
}

// ---- finding printers ----------------------------------------------------------------------------------
/** Parse `lpstat -p -d` output (CUPS). Exported for tests. */
export function parseLpstat(text) {
  const list = [];
  let def = null;
  String(text).split(/\r?\n/).forEach((line) => {
    let m = /^printer (\S+) (?:is |disabled|now printing)(.*)$/.exec(line);
    if (m) {
      const rest = line.toLowerCase();
      const status = /disabled/.test(rest) ? "offline" : /now printing/.test(rest) ? "printing" : /is idle/.test(rest) ? "idle" : "unknown";
      list.push({ name: m[1], default: false, status: status });
      return;
    }
    m = /^system default destination:\s*(.+)$/.exec(line);
    if (m) def = m[1].trim();
  });
  list.forEach((p) => {
    p.default = p.name === def;
  });
  return list;
}

const WIN_STATUS = { 1: "unknown", 2: "unknown", 3: "idle", 4: "printing", 5: "idle", 6: "offline", 7: "offline" };

export async function printers() {
  if (windows) {
    const r = await run(["powershell", "-NoProfile", "-Command", "Get-CimInstance Win32_Printer | Select-Object Name,Default,PrinterStatus,WorkOffline | ConvertTo-Json -Compress"]);
    if (r.code !== 0) throw fail(r, "could not list printers");
    const text = r.out.trim();
    if (!text) return [];
    const parsed = JSON.parse(text);
    return (Array.isArray(parsed) ? parsed : [parsed]).map((p) => ({ name: p.Name, default: !!p.Default, status: p.WorkOffline ? "offline" : WIN_STATUS[p.PrinterStatus] || "unknown" }));
  }
  const r = await run(["lpstat", "-p", "-d"]);
  // lpstat exits non-zero (and complains) when nothing is installed: an empty list, not an error.
  if (r.code !== 0 && !/printer/i.test(r.out)) {
    if (/no destinations|not ready|no default/i.test(r.err + r.out) || r.out.trim() === "") return [];
    throw fail(r, "could not list printers");
  }
  return parseLpstat(r.out);
}

export async function defaultPrinter() {
  const all = await printers();
  const hit = all.find((p) => p.default);
  return hit ? hit.name : all.length ? all[0].name : null;
}

// ---- printing ------------------------------------------------------------------------------------------
/** Send a file to a printer through the system's print path. */
export async function printFile(path, options) {
  const o = options || {};
  const copies = Math.max(1, Math.floor(o.copies || 1));
  if (windows) {
    const printer = o.printer ? "'" + String(o.printer).replace(/'/g, "''") + "'" : "";
    const file = "'" + String(path).replace(/'/g, "''") + "'";
    const verb = printer ? "-Verb PrintTo -ArgumentList ('\"' + " + printer + " + '\"')" : "-Verb Print";
    const script = "for ($i = 0; $i -lt " + copies + "; $i++) { Start-Process -FilePath " + file + " " + verb + " -WindowStyle Hidden }";
    const r = await run(["powershell", "-NoProfile", "-Command", script]);
    if (r.code !== 0) throw fail(r, "printing failed");
    return true;
  }
  const args = ["lp"];
  if (o.printer) args.push("-d", String(o.printer));
  if (copies > 1) args.push("-n", String(copies));
  if (o.title) args.push("-t", String(o.title));
  args.push(String(path));
  const r = await run(args);
  if (r.code !== 0) throw fail(r, "printing failed");
  return true;
}

const WIN_RAW = `param([string]$Printer, [string]$File)
$src = @"
using System;
using System.Runtime.InteropServices;
public class NileRaw {
  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
  public class DocInfo { [MarshalAs(UnmanagedType.LPWStr)] public string Name; [MarshalAs(UnmanagedType.LPWStr)] public string Output; [MarshalAs(UnmanagedType.LPWStr)] public string Type; }
  [DllImport("winspool.drv", EntryPoint = "OpenPrinterW", CharSet = CharSet.Unicode, SetLastError = true)] static extern bool OpenPrinter(string n, out IntPtr h, IntPtr d);
  [DllImport("winspool.drv", SetLastError = true)] static extern bool ClosePrinter(IntPtr h);
  [DllImport("winspool.drv", EntryPoint = "StartDocPrinterW", CharSet = CharSet.Unicode, SetLastError = true)] static extern int StartDocPrinter(IntPtr h, int level, [In] DocInfo di);
  [DllImport("winspool.drv", SetLastError = true)] static extern bool EndDocPrinter(IntPtr h);
  [DllImport("winspool.drv", SetLastError = true)] static extern bool StartPagePrinter(IntPtr h);
  [DllImport("winspool.drv", SetLastError = true)] static extern bool EndPagePrinter(IntPtr h);
  [DllImport("winspool.drv", SetLastError = true)] static extern bool WritePrinter(IntPtr h, byte[] b, int n, out int written);
  public static void Send(string printer, byte[] data) {
    IntPtr h;
    if (!OpenPrinter(printer, out h, IntPtr.Zero)) throw new Exception("Cannot open printer '" + printer + "' (error " + Marshal.GetLastWin32Error() + ")");
    try {
      DocInfo di = new DocInfo(); di.Name = "Nile receipt"; di.Type = "RAW";
      if (StartDocPrinter(h, 1, di) == 0) throw new Exception("StartDocPrinter failed (error " + Marshal.GetLastWin32Error() + ")");
      StartPagePrinter(h);
      int written;
      bool ok = WritePrinter(h, data, data.Length, out written);
      EndPagePrinter(h);
      EndDocPrinter(h);
      if (!ok || written != data.Length) throw new Exception("WritePrinter failed (error " + Marshal.GetLastWin32Error() + ")");
    } finally { ClosePrinter(h); }
  }
}
"@
Add-Type -TypeDefinition $src
[NileRaw]::Send($Printer, [System.IO.File]::ReadAllBytes($File))
`;

let jobs = 0;

/** Send raw bytes (receipt/label commands) to a printer through the spooler. */
export async function printRaw(printer, data) {
  const bytes = toBytes(data);
  if (!printer) throw new Error("printRaw needs a printer name");
  if (windows) {
    const dir = tmpdir();
    const stamp = String(Date.now()) + "-" + jobs++;
    const bin = dir + "\\nile-raw-" + stamp + ".bin";
    const script = dir + "\\nile-raw-" + stamp + ".ps1";
    fs.writeFileSync(bin, bytes);
    fs.writeFileSync(script, WIN_RAW);
    try {
      const r = await run(["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", script, String(printer), bin]);
      if (r.code !== 0) throw fail(r, "raw printing failed");
    } finally {
      try { fs.unlinkSync(bin); fs.unlinkSync(script); } catch (e) { /* the temp folder gets cleaned eventually */ }
    }
    return true;
  }
  const r = await run(["lp", "-d", String(printer), "-o", "raw", "-t", "Nile receipt"], bytes);
  if (r.code !== 0) throw fail(r, "raw printing failed");
  return true;
}

/** Write bytes to a network printer's raw port (9100 by default). */
export async function sendNetwork(host, port, data, options) {
  const o = options || {};
  const bytes = toBytes(data);
  const tcp = new TCP();
  let timer = null;
  const timeout = new Promise((_, reject) => {
    timer = setTimeout(() => {
      try { tcp.close(); } catch (e) { /* already closed */ }
      reject(new Error("printer " + host + ":" + (port || 9100) + " did not answer in time"));
    }, o.timeout || 5000);
  });
  try {
    await Promise.race([
      (async () => {
        await tcp.connect({ ip: String(host), port: port || 9100 });
        await tcp.write(bytes);
        // Give the printer a moment to take the data before the socket closes.
        await tcp.shutdown();
      })(),
      timeout,
    ]);
  } finally {
    clearTimeout(timer);
    try { tcp.close(); } catch (e) { /* closed */ }
  }
  return true;
}

export const platform = windows ? "windows" : uname().sysname === "Darwin" ? "macos" : "linux";
