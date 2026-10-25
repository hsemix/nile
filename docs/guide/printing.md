# Printing: pages, PDF and receipt printers

Nile prints through the operating system, so any printer installed on the machine works, USB ones included. There are
three pieces, all optional and built into the program:

| What | How |
|---|---|
| The page itself as paper (invoices, reports, labels) | `Window.this.print()` / `Window.this.exportPdf(path)` |
| Finding printers, sending a file or raw bytes | `@nile/print` |
| Thermal receipt printers (ESC/POS) | `@nile/escpos` |

`examples/printing.html` shows all of them.

## The page as paper

```js
await Window.this.print({ printer: "Office", copies: 2, title: "Invoice 7" });   // silent: no dialog
const file = await Window.this.exportPdf("/Users/me/invoice-7.pdf");              // just the file
```

The page is laid out on paper the way `--pdf` does it: `@page { size, margin }`, `page-break-*`, repeated table headers
and footers, `counter(page)` in the margin boxes. Use `@media print` to hide what shouldn't print. The window's own size
and what is on screen are untouched. `print()` writes the PDF to a temporary file, hands it to the system and deletes
it afterwards; leave out `printer` for the default one. Both calls resolve when the job was *handed over*, not when paper
comes out, and reject with the system's message if it was refused.

There is no system print dialog yet: pick the printer from your own list (below) and call `print`.

## Printers and jobs: `@nile/print`

```js
import { printers, printFile, printRaw, sendNetwork } from "@nile/print";
const list = await printers();                       // [{ name, default, status }]  status: idle, printing, offline, unknown
await printFile("/tmp/label.pdf", { printer: list[0].name, copies: 1 });
await printRaw("EPSON TM-T20", bytes);               // raw bytes through the spooler
await sendNetwork("192.168.1.50", 9100, bytes);      // raw bytes to a network printer's port
```

How it reaches the printer:

| | `printers`, `printFile` | `printRaw` |
|---|---|---|
| macOS, Linux | CUPS: `lpstat`, `lp` | `lp -o raw` |
| Windows | PowerShell: `Get-CimInstance Win32_Printer`, `Start-Process -Verb PrintTo` | the winspool API (`WritePrinter`) through PowerShell |

On Windows, `printFile` relies on the program registered for that file type to support printing (a PDF reader that
handles `PrintTo`); raw jobs do not. A printer must accept raw data for `printRaw` (receipt and label printers do).
`sendNetwork` is a plain socket, no spooler: 9100 is the usual port, `timeout` (5 s) stops it hanging on a dead address.

## Receipts: `@nile/escpos`

```js
import { Receipt, printReceipt } from "@nile/escpos";
const r = new Receipt({ width: 48, codepage: "cp858" })       // 48 characters for 80 mm paper, 32 for 58 mm
  .align("center").bold().size(2, 2).line("SOFTSALE").size(1, 1).bold(false)
  .rule()
  .align("left").row("Milk 1 l  x2", "5.00").row("Bread", "3.80")
  .rule("=").bold().row("TOTAL", "8.80").bold(false)
  .align("center").qr("https://shop.example/r/10042", { size: 5 }).barcode("code128", "10042")
  .feed(2).cut().drawer();
await printReceipt(r, { printer: "EPSON TM-T20" });   // installed printer, USB or not: through the spooler
await printReceipt(r, { host: "192.168.1.50" });      // network printer on port 9100
await printReceipt(r, { file: "/dev/usb/lp0" });      // a device path (Linux), or any file
```

`Receipt` writes the ESC/POS command bytes: `text`, `line`, `feed`, `align`, `bold`, `underline`, `invert`, `font`,
`size(width, height)`, `rule`, `row(left, right)`, `columns([{ text, width, align }])`, `barcode(type, data, options)`
(`code128`, `code39`, `ean13`, `ean8`, `upca`, `upce`, `itf`, `codabar`), `qr(data, { size, ecc })`,
`image(imageData, { threshold })` for a logo (RGBA data, e.g. from a canvas), `cut({ partial })`, `drawer(pin)`,
`beep`, `raw(bytes)`. `bytes()` is what to send, `preview()` is the text for an on-screen preview and `hex()` helps
debugging.

Text is encoded for the printer's code page: `codepage: "cp437"` (default), `"cp858"` (Western Europe with the euro sign),
`"cp1252"` or `"ascii"`; anything a page lacks prints as `?`. The number sent to the printer (`ESC t n`) follows Epson's
numbering; pass `codepageId` if your model differs. Printers draw fonts, widths and cuts their own way, so try a receipt on
the model you will ship with.

## What is not here

- **Direct USB and serial access.** Printers installed in the OS work through the spooler; a printer *not* installed
  needs a device path (`{ file }`, e.g. `/dev/usb/lp0` or a serial port) or the network. A native USB/serial extension
  would be the next step if a device can't be installed.
- **A print dialog and print preview window.** Use your own list and the `@media print` view.
- **Printing a different page than the one shown**, label designers and status polling (paper out, cover open). A
  receipt printer's status bytes can be read from a socket you open yourself.
- **Windows and Linux paths are checked by CI and by the unit tests, not on real printers.**
