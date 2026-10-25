// @nile/escpos: build receipts for ESC/POS thermal printers (Epson and the many compatible 58/80 mm models).
//
//   import { Receipt, printReceipt } from "@nile/escpos";
//   const r = new Receipt({ width: 48 })                // characters per line: 48 for 80 mm paper, 32 for 58 mm
//     .align("center").bold().size(2, 2).line("SOFTSALE").size(1, 1).bold(false)
//     .line("12 Market St, Kampala").rule()
//     .align("left")
//     .row("Milk 1 l  x2", "5.00").row("Bread", "3.80")
//     .rule("=").bold().row("TOTAL", "8.80").bold(false)
//     .feed(1).align("center").qr("https://shop.example/r/10042", { size: 5 }).barcode("code128", "10042")
//     .feed(2).cut().drawer();
//   await printReceipt(r, { host: "192.168.1.50" });            // network printer, port 9100
//   await printReceipt(r, { printer: "EPSON TM-T20" });          // installed printer (USB included), via the spooler
//   await printReceipt(r, { file: "/dev/usb/lp0" });             // a device or file (Linux USB / serial paths)
//
// Builder methods (all chain): init(), text(s) (no line break), line(s) (with one), feed(n), align("left"|"center"|"right"),
// bold(on = true), underline(on = true | 2), invert(on), font("A"|"B"), size(width 1-8, height 1-8), rule(char = "-"),
// row(left, right) (right aligned, padded to the width), columns([{ text, width, align }]) (widths in characters, one
// may omit `width` to take the rest), barcode(type, data, { height, width, text: "below"|"above"|"both"|"none" }) with types
// code128 code39 ean13 ean8 upca upce itf codabar, qr(data, { size 1-16, ecc "L"|"M"|"Q"|"H" }), image(imageData,
// { threshold, width }) for a logo (a canvas ImageData: RGBA), cut({ partial }), drawer(pin = 0) (opens the cash drawer),
// beep(times), raw(bytes), codepage(name). Results: bytes() (a Uint8Array to send), preview() (the text lines, for an on-screen
// preview), hex() (for debugging). Text is encoded for the printer's code page (`codepage`: "cp437" the default, "cp858"
// for the euro and Western Europe, "cp1252", "ascii"); characters it doesn't have print as "?". Match the number the
// printer expects with `codepageId` if your model numbers its pages differently.
//
// A receipt prints the way the printer draws it: fonts, widths and cuts depend on the model. Test on yours.

import { printRaw, sendNetwork } from "@nile/print";
import { fs } from "@sys";

const ESC = 0x1b, GS = 0x1d, LF = 0x0a;

// The upper half (0x80-0xFF) of each code page, as text. Index 0 of the string is byte 0x80.
const HIGH = {
  cp437:
    "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜ¢£¥₧ƒáíóúñÑªº¿⌐¬½¼¡«»░▒▓│┤╡╢╖╕╣║╗╝╜╛┐└┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀αßΓπΣσµτΦΘΩδ∞φε∩≡±≥≤⌠⌡÷≈°∙·√ⁿ²■ ",
  cp858:
    "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜø£Ø×ƒáíóúñÑªº¿®¬½¼¡«»░▒▓│┤ÁÂÀ©╣║╗╝¢¥┐└┴┬├─┼ãÃ╚╔╩╦╠═╬¤ðÐÊËÈ€ÍÎÏ┘┌█▄¦Ì▀ÓßÔÒõÕµþÞÚÛÙýÝ¯´­±‗¾¶§÷¸°¨·¹³²■ ",
  cp1252:
    "€\u0081‚ƒ„…†‡ˆ‰Š‹Œ\u008dŽ\u008f\u0090‘’“”•–—˜™š›œ\u009džŸ ¡¢£¤¥¦§¨©ª«¬­®¯°±²³´µ¶·¸¹º»¼½¾¿ÀÁÂÃÄÅÆÇÈÉÊËÌÍÎÏÐÑÒÓÔÕÖ×ØÙÚÛÜÝÞßàáâãäåæçèéêëìíîïðñòóôõö÷øùúûüýþÿ",
};
// ESC t n values on Epson printers.
const PAGE_ID = { cp437: 0, cp858: 19, cp1252: 16, ascii: 0 };

const tables = {};
function table(name) {
  if (tables[name]) return tables[name];
  const map = new Map();
  const high = HIGH[name];
  if (high) Array.from(high).forEach((ch, i) => map.set(ch, 0x80 + i));
  tables[name] = map;
  return map;
}

/** Text as bytes for a code page; characters it lacks become "?". Exported for tests. */
export function encodeText(text, codepage) {
  const map = table(codepage || "cp437");
  const out = [];
  for (const ch of String(text)) {
    const code = ch.codePointAt(0);
    if (code < 0x80) out.push(code);
    else if (map.has(ch)) out.push(map.get(ch));
    else if (ch === "’" || ch === "‘") out.push(0x27);
    else if (ch === "“" || ch === "”") out.push(0x22);
    else out.push(0x3f);
  }
  return out;
}

const BARCODES = { upca: 65, upce: 66, ean13: 67, ean8: 68, code39: 69, itf: 70, codabar: 71, code128: 73 };
const ECC = { L: 48, M: 49, Q: 50, H: 51 };

export class Receipt {
  constructor(options) {
    const o = options || {};
    this.width = o.width || 48;
    this.codepageName = HIGH[o.codepage] || o.codepage === "ascii" ? o.codepage : "cp437";
    this.codepageId = o.codepageId !== undefined ? o.codepageId : PAGE_ID[this.codepageName];
    this._bytes = [];
    this._lines = [];
    this._cur = "";
    this._align = "left";
    this._scale = 1;
    this._bold = false;
    if (o.init !== false) this.init();
  }
  _push(...b) {
    for (const x of b) {
      if (Array.isArray(x) || ArrayBuffer.isView(x)) for (const y of x) this._bytes.push(y & 0xff);
      else this._bytes.push(x & 0xff);
    }
    return this;
  }
  init() {
    this._push(ESC, 0x40, ESC, 0x74, this.codepageId & 0xff);
    this._align = "left";
    this._scale = 1;
    this._bold = false;
    return this;
  }
  codepage(name, id) {
    this.codepageName = HIGH[name] || name === "ascii" ? name : "cp437";
    this.codepageId = id !== undefined ? id : PAGE_ID[this.codepageName];
    return this._push(ESC, 0x74, this.codepageId & 0xff);
  }
  /** Text without a line break. */
  text(s) {
    s = String(s === undefined || s === null ? "" : s);
    // Line breaks inside the text are real ones.
    const parts = s.split("\n");
    parts.forEach((p, i) => {
      if (i > 0) this._endLine();
      this._push(encodeText(p, this.codepageName));
      this._cur += p;
    });
    return this;
  }
  _endLine() {
    this._push(LF);
    this._lines.push({ text: this._cur, align: this._align, bold: this._bold, scale: this._scale });
    this._cur = "";
    return this;
  }
  line(s) {
    if (s !== undefined) this.text(s);
    return this._endLine();
  }
  feed(n) {
    const count = n === undefined ? 1 : Math.max(0, Math.min(255, Math.floor(n)));
    if (this._cur) this._endLine();
    for (let i = 0; i < count; i++) this._lines.push({ text: "", align: this._align, bold: false, scale: 1 });
    return count ? this._push(ESC, 0x64, count) : this;
  }
  align(where) {
    const n = where === "center" ? 1 : where === "right" ? 2 : 0;
    this._align = n === 1 ? "center" : n === 2 ? "right" : "left";
    return this._push(ESC, 0x61, n);
  }
  bold(on) {
    this._bold = on !== false;
    return this._push(ESC, 0x45, this._bold ? 1 : 0);
  }
  underline(on) {
    return this._push(ESC, 0x2d, on === false ? 0 : on === 2 ? 2 : 1);
  }
  invert(on) {
    return this._push(GS, 0x42, on === false ? 0 : 1);
  }
  font(which) {
    return this._push(ESC, 0x4d, String(which).toUpperCase() === "B" ? 1 : 0);
  }
  /** Character size as a multiple of the normal one (1 to 8 each way). */
  size(w, h) {
    const cw = Math.max(1, Math.min(8, Math.floor(w || 1))), ch = Math.max(1, Math.min(8, Math.floor(h || w || 1)));
    this._scale = cw;
    return this._push(GS, 0x21, ((cw - 1) << 4) | (ch - 1));
  }
  _cols() {
    return Math.max(1, Math.floor(this.width / this._scale));
  }
  /** A line of one repeated character across the width. */
  rule(ch) {
    return this.line(String(ch || "-").charAt(0).repeat(this._cols()));
  }
  /** `left` at the left edge and `right` at the right edge of the line (the left text is cut if they collide). */
  row(left, right) {
    const cols = this._cols();
    const l = String(left === undefined ? "" : left), r = String(right === undefined ? "" : right);
    const gap = cols - Array.from(l).length - Array.from(r).length;
    if (gap >= 1) return this.line(l + " ".repeat(gap) + r);
    const room = Math.max(0, cols - Array.from(r).length - 1);
    return this.line(Array.from(l).slice(0, room).join("") + " " + r);
  }
  /** Several cells on one line: [{ text, width, align: "left"|"right"|"center" }]; one cell may leave out `width`. */
  columns(cells) {
    const cols = this._cols();
    const fixed = cells.reduce((n, c) => n + (c.width || 0), 0);
    const open = cells.filter((c) => !c.width).length;
    const share = open ? Math.max(1, Math.floor((cols - fixed) / open)) : 0;
    const out = cells
      .map((c) => {
        const w = c.width || share;
        let t = Array.from(String(c.text === undefined ? "" : c.text));
        if (t.length > w) t = t.slice(0, w);
        const padTotal = w - t.length;
        const left = c.align === "right" ? padTotal : c.align === "center" ? Math.floor(padTotal / 2) : 0;
        return " ".repeat(left) + t.join("") + " ".repeat(padTotal - left);
      })
      .join("");
    return this.line(out);
  }
  barcode(type, data, options) {
    const o = options || {};
    const m = BARCODES[String(type).toLowerCase()];
    if (!m) throw new Error("unknown barcode type: " + type);
    let body = encodeText(String(data), "ascii");
    if (m === 73) body = [0x7b, 0x42].concat(body);   // code set B
    const hri = { none: 0, above: 1, below: 2, both: 3 }[o.text === undefined ? "below" : o.text] || 0;
    this._push(GS, 0x68, Math.max(1, Math.min(255, o.height || 80)), GS, 0x77, Math.max(2, Math.min(6, o.width || 3)), GS, 0x48, hri, GS, 0x6b, m, body.length, body);
    this._lines.push({ text: "[" + type + " " + data + "]", align: this._align, bold: false, scale: 1 });
    return this;
  }
  qr(data, options) {
    const o = options || {};
    const body = Array.from(new TextEncoder().encode(String(data)));
    const size = Math.max(1, Math.min(16, Math.floor(o.size || 6)));
    const len = body.length + 3;
    this._push(
      GS, 0x28, 0x6b, 4, 0, 0x31, 0x41, 0x32, 0,           // model 2
      GS, 0x28, 0x6b, 3, 0, 0x31, 0x43, size,              // module size
      GS, 0x28, 0x6b, 3, 0, 0x31, 0x45, ECC[o.ecc || "M"], // error correction
      GS, 0x28, 0x6b, len & 0xff, len >> 8, 0x31, 0x50, 0x30, body,
      GS, 0x28, 0x6b, 3, 0, 0x31, 0x51, 0x30,              // print
    );
    this._lines.push({ text: "[QR " + data + "]", align: this._align, bold: false, scale: 1 });
    return this;
  }
  /** A picture as a monochrome raster. `img` is `{ width, height, data }` with RGBA bytes (e.g. canvas getImageData). */
  image(img, options) {
    const o = options || {};
    const threshold = o.threshold === undefined ? 128 : o.threshold;
    let w = img.width, h = img.height;
    const rows = [];
    // The printer wants whole bytes per row: pad the width to a multiple of 8 with white.
    const bytesPerRow = Math.ceil(w / 8);
    for (let y = 0; y < h; y++) {
      for (let bx = 0; bx < bytesPerRow; bx++) {
        let b = 0;
        for (let bit = 0; bit < 8; bit++) {
          const x = bx * 8 + bit;
          let on = 0;
          if (x < w) {
            const i = (y * w + x) * 4;
            const a = img.data[i + 3] / 255;
            // Transparent counts as white; otherwise brightness decides.
            const lum = (0.299 * img.data[i] + 0.587 * img.data[i + 1] + 0.114 * img.data[i + 2]) * a + 255 * (1 - a);
            on = lum < threshold ? 1 : 0;
          }
          b |= on << (7 - bit);
        }
        rows.push(b);
      }
    }
    this._push(GS, 0x76, 0x30, 0, bytesPerRow & 0xff, bytesPerRow >> 8, h & 0xff, h >> 8, rows);
    this._lines.push({ text: "[image " + w + "x" + h + "]", align: this._align, bold: false, scale: 1 });
    return this;
  }
  /** Cut the paper (after feeding past the cutter). */
  cut(options) {
    if (this._cur) this._endLine();
    return this._push(GS, 0x56, options && options.partial ? 66 : 65, 3);
  }
  /** Open the cash drawer connected to the printer. */
  drawer(pin) {
    return this._push(ESC, 0x70, pin === 1 ? 1 : 0, 25, 250);
  }
  beep(times) {
    return this._push(ESC, 0x42, Math.max(1, Math.min(9, times || 1)), 2);
  }
  raw(bytes) {
    return this._push(Array.from(bytes));
  }
  bytes() {
    return Uint8Array.from(this._bytes);
  }
  hex() {
    return this._bytes.map((b) => (b < 16 ? "0" : "") + b.toString(16)).join(" ");
  }
  /** The printed lines as plain text (alignment applied), for showing a preview on screen. */
  preview() {
    const lines = this._lines.slice();
    if (this._cur) lines.push({ text: this._cur, align: this._align, scale: this._scale });
    return lines
      .map((l) => {
        const cols = Math.max(1, Math.floor(this.width / (l.scale || 1)));
        const t = l.text;
        const pad = Math.max(0, cols - Array.from(t).length);
        return l.align === "center" ? " ".repeat(Math.floor(pad / 2)) + t : l.align === "right" ? " ".repeat(pad) + t : t;
      })
      .join("\n");
  }
}

/**
 * Send a receipt (or bytes) to a printer: { host, port } (network), { printer } (installed printer, through the
 * spooler) or { file } (a device path such as /dev/usb/lp0, or any file).
 */
export async function printReceipt(receipt, target) {
  const bytes = receipt instanceof Receipt ? receipt.bytes() : receipt;
  const t = target || {};
  if (t.host) return sendNetwork(t.host, t.port || 9100, bytes, { timeout: t.timeout });
  if (t.printer) return printRaw(t.printer, bytes);
  if (t.file) {
    fs.writeFileSync(t.file, bytes);
    return true;
  }
  throw new Error("printReceipt needs a target: { host }, { printer } or { file }");
}
