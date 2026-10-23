// @nile/ui/barcode: barcodes and QR codes, generated in plain JavaScript (no engine support needed).
//
//   import { svg, encode, dataUrl, types } from "@nile/ui/barcode";
//   el.innerHTML = svg("code128", "ORDER-1042", { height: 50 });
//   <nile-barcode type="ean13" value="5901234123457"></nile-barcode>
//
// Types: code128, code39, ean13, ean8, upca, qr. `encode` returns the modules (a string of 1/0 bars for the
// linear types, `{ size, rows }` for qr); `svg` draws them (bars land on whole pixels, which is what scanners
// need, so give `moduleWidth` as a whole number).

const CODE128 = "11011001100110011011001100110011010010011000100100011001000100110010011001000100110001001000110010011001001000110010001001100010010010110011100100110111001001100111010111001100100111011001001110011011001110010110010111001100100111011011100100110011101001110110111011101001100111001011001110010011011101100100111001101001110011001011011011000110110001101100011011010100011000100010110001000100011010110001000100011010001000110001011010001000110001010001100010001010110111000101100011101000110111010111011000101110001101000111011011101110110110100011101100010111011011101000110111000101101110111011101011000111010001101110001011011101101000111011000101110001101011101111010110010000101111000101010100110000101000011001001011000010010000110100001011001000010011010110010000101100001001001101000010011000010100001101001000011001011000010010110010100001111011101011000010100100011110101010011110010010111100100100111101011110010010011110100100111100101111010010011110010100111100100101101101111011011110110111101101101010111100010100011110100010111101011110100010111100010111101010001111010001010111011110101111011101110101111011110101110110100001001101001000011010011100";
const CODE39_CHARS = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ-. $/+%";
const CODE39_PATTERNS = "101000111011101 111010001010111 101110001010111 111011100010101 101000111010111 111010001110101 101110001110101 101000101110111 111010001011101 101110001011101 111010100010111 101110100010111 111011101000101 101011100010111 111010111000101 101110111000101 101010001110111 111010100011101 101110100011101 101011100011101 111010101000111 101110101000111 111011101010001 101011101000111 111010111010001 101110111010001 101010111000111 111010101110001 101110101110001 101011101110001 111000101010111 100011101010111 111000111010101 100010111010111 111000101110101 100011101110101 100010101110111 111000101011101 100011101011101 100010001000101 100010001010001 100010100010001 101000100010001".split(" ");
const CODE39_EDGE = "100010111011101";
const STOP128 = "1100011101011";

const EAN_L = ["0001101", "0011001", "0010011", "0111101", "0100011", "0110001", "0101111", "0111011", "0110111", "0001011"];
const EAN_G = ["0100111", "0110011", "0011011", "0100001", "0011101", "0111001", "0000101", "0010001", "0001001", "0010111"];
const EAN_R = EAN_L.map((c) => [...c].map((b) => (b === "1" ? "0" : "1")).join(""));
const EAN_PARITY = ["LLLLLL", "LLGLGG", "LLGGLG", "LLGGGL", "LGLLGG", "LGGLLG", "LGGGLL", "LGLGLG", "LGLGGL", "LGGLGL"];

export const types = ["code128", "code39", "ean13", "ean8", "upca", "qr"];

function fail(message) {
  throw new Error("barcode: " + message);
}

// ---- Code 128 ---------------------------------------------------------------------------------------------

function code128(value) {
  const s = String(value);
  if (!s.length) fail("Code 128 needs a value");
  const n = s.length;
  for (let i = 0; i < n; i++) if (s.charCodeAt(i) > 126) fail("Code 128 can't encode " + JSON.stringify(s[i]));
  const digit = (i) => s.charCodeAt(i) >= 48 && s.charCodeAt(i) <= 57;
  const run = (i) => {
    let k = 0;
    while (i + k < n && digit(i + k)) k++;
    return k;
  };
  const values = [];
  let set = "";
  let i = 0;
  while (i < n) {
    const digits = run(i);
    const wantC = set === "C" ? digits >= 2 : digits >= (i === 0 ? 4 : 6) || (digits >= 4 && i + digits === n) || (i === 0 && n === 2 && digits === 2);
    if (wantC) {
      if (set === "") values.push(105);
      else if (set !== "C") values.push(99);
      set = "C";
      values.push(Number(s.substr(i, 2)));
      i += 2;
      continue;
    }
    const c = s.charCodeAt(i);
    const target = set === "A" ? (c <= 95 ? "A" : "B") : set === "B" ? (c >= 32 ? "B" : "A") : c < 32 ? "A" : "B";
    if (set === "") values.push(target === "A" ? 103 : 104);
    else if (set !== target) values.push(target === "A" ? 101 : 100);
    set = target;
    values.push(target === "A" ? (c < 32 ? c + 64 : c - 32) : c - 32);
    i++;
  }
  let sum = values[0];
  for (let k = 1; k < values.length; k++) sum += k * values[k];
  values.push(sum % 103);
  let bars = "";
  for (const v of values) bars += CODE128.substr(v * 11, 11);
  return { type: "code128", bars: bars + STOP128, text: s };
}

// ---- Code 39 ----------------------------------------------------------------------------------------------

function code39(value) {
  const s = String(value).toUpperCase();
  if (!s.length) fail("Code 39 needs a value");
  let bars = CODE39_EDGE;
  for (const ch of s) {
    const at = CODE39_CHARS.indexOf(ch);
    if (at < 0) fail("Code 39 can't encode " + JSON.stringify(ch) + " (digits, A-Z, - . space $ / + %)");
    bars += "0" + CODE39_PATTERNS[at];
  }
  return { type: "code39", bars: bars + "0" + CODE39_EDGE, text: s };
}

// ---- EAN / UPC --------------------------------------------------------------------------------------------

/** The check digit for `digits` (without it): weights 3, 1, 3... from the right. */
function eanCheck(digits) {
  let sum = 0;
  for (let i = 0; i < digits.length; i++) sum += Number(digits[digits.length - 1 - i]) * (i % 2 === 0 ? 3 : 1);
  return (10 - (sum % 10)) % 10;
}

function digitsOf(name, value, without, withCheck) {
  const s = String(value).trim();
  if (!/^\d+$/.test(s)) fail(name + " is digits only");
  if (s.length === without) return s + eanCheck(s);
  if (s.length === withCheck) {
    if (eanCheck(s.slice(0, -1)) !== Number(s[s.length - 1])) fail(name + " check digit is wrong (expected " + eanCheck(s.slice(0, -1)) + ")");
    return s;
  }
  return fail(name + " needs " + without + " digits (check digit added) or " + withCheck);
}

function ean13Bars(d) {
  const parity = EAN_PARITY[Number(d[0])];
  let bars = "101";
  for (let i = 1; i <= 6; i++) bars += (parity[i - 1] === "L" ? EAN_L : EAN_G)[Number(d[i])];
  bars += "01010";
  for (let i = 7; i <= 12; i++) bars += EAN_R[Number(d[i])];
  return bars + "101";
}

function ean13(value) {
  const d = digitsOf("EAN-13", value, 12, 13);
  // Text: the first digit sits left of the bars, then a group under each half.
  return { type: "ean13", bars: ean13Bars(d), text: d, labels: [{ at: -5, text: d[0] }, { at: 24, text: d.slice(1, 7) }, { at: 71, text: d.slice(7) }], guard: 3 };
}

function ean8(value) {
  const d = digitsOf("EAN-8", value, 7, 8);
  let bars = "101";
  for (let i = 0; i < 4; i++) bars += EAN_L[Number(d[i])];
  bars += "01010";
  for (let i = 4; i < 8; i++) bars += EAN_R[Number(d[i])];
  return { type: "ean8", bars: bars + "101", text: d, labels: [{ at: 16.5, text: d.slice(0, 4) }, { at: 49.5, text: d.slice(4) }], guard: 3 };
}

function upca(value) {
  const d = digitsOf("UPC-A", value, 11, 12);
  return { type: "upca", bars: ean13Bars("0" + d), text: d, labels: [{ at: -5, text: d[0] }, { at: 24, text: d.slice(1, 6) }, { at: 71, text: d.slice(6, 11) }, { at: 100, text: d[11] }], guard: 3 };
}

// ---- QR code (byte mode, versions 1-40, error correction L/M/Q/H) --------------------------------------------

const ECC_PER_BLOCK = [
  [-1, 7, 10, 15, 20, 26, 18, 20, 24, 30, 18, 20, 24, 26, 30, 22, 24, 28, 30, 28, 28, 28, 28, 30, 30, 26, 28, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30],
  [-1, 10, 16, 26, 18, 24, 16, 18, 22, 22, 26, 30, 22, 22, 24, 24, 28, 28, 26, 26, 26, 26, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28],
  [-1, 13, 22, 18, 26, 18, 24, 18, 22, 20, 24, 28, 26, 24, 20, 30, 24, 28, 28, 26, 30, 28, 30, 30, 30, 30, 28, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30],
  [-1, 17, 28, 22, 16, 22, 28, 26, 26, 24, 28, 24, 28, 22, 24, 24, 30, 28, 28, 26, 28, 30, 24, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30],
];
const BLOCKS = [
  [-1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 4, 4, 4, 4, 4, 6, 6, 6, 6, 7, 8, 8, 9, 9, 10, 12, 12, 12, 13, 14, 15, 16, 17, 18, 19, 19, 20, 21, 22, 24, 25],
  [-1, 1, 1, 1, 2, 2, 4, 4, 4, 5, 5, 5, 8, 9, 9, 10, 10, 11, 13, 14, 16, 17, 17, 18, 20, 21, 23, 25, 26, 28, 29, 31, 33, 35, 37, 38, 40, 43, 45, 47, 49],
  [-1, 1, 1, 2, 2, 4, 4, 6, 6, 8, 8, 8, 10, 12, 16, 12, 17, 16, 18, 21, 20, 23, 23, 25, 27, 29, 34, 34, 35, 38, 40, 43, 45, 48, 51, 53, 56, 59, 62, 65, 68],
  [-1, 1, 1, 2, 4, 4, 4, 5, 6, 8, 8, 11, 11, 16, 16, 18, 16, 19, 21, 25, 25, 25, 34, 30, 32, 35, 37, 40, 42, 45, 48, 51, 54, 57, 60, 63, 66, 70, 74, 77, 81],
];
const ECC_LEVELS = { L: 0, M: 1, Q: 2, H: 3 };
const FORMAT_BITS = [1, 0, 3, 2]; // L, M, Q, H

function utf8(text) {
  const out = [];
  for (const ch of text) {
    const c = ch.codePointAt(0);
    if (c < 0x80) out.push(c);
    else if (c < 0x800) out.push(0xc0 | (c >> 6), 0x80 | (c & 63));
    else if (c < 0x10000) out.push(0xe0 | (c >> 12), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
    else out.push(0xf0 | (c >> 18), 0x80 | ((c >> 12) & 63), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
  }
  return out;
}

function rawModules(version) {
  let n = (16 * version + 128) * version + 64;
  if (version >= 2) {
    const align = Math.floor(version / 7) + 2;
    n -= (25 * align - 10) * align - 55;
    if (version >= 7) n -= 36;
  }
  return n;
}

const dataCodewords = (version, ecc) => Math.floor(rawModules(version) / 8) - ECC_PER_BLOCK[ecc][version] * BLOCKS[ecc][version];

function gfMultiply(x, y) {
  let z = 0;
  for (let i = 7; i >= 0; i--) {
    z = (z << 1) ^ ((z >>> 7) * 0x11d);
    z ^= ((y >>> i) & 1) * x;
  }
  return z;
}

function rsDivisor(degree) {
  const result = new Array(degree).fill(0);
  result[degree - 1] = 1;
  let root = 1;
  for (let i = 0; i < degree; i++) {
    for (let j = 0; j < degree; j++) {
      result[j] = gfMultiply(result[j], root);
      if (j + 1 < degree) result[j] ^= result[j + 1];
    }
    root = gfMultiply(root, 2);
  }
  return result;
}

function rsRemainder(data, divisor) {
  const result = new Array(divisor.length).fill(0);
  for (const b of data) {
    const factor = b ^ result.shift();
    result.push(0);
    divisor.forEach((coef, i) => (result[i] ^= gfMultiply(coef, factor)));
  }
  return result;
}

function interleave(data, version, ecc) {
  const blocks = BLOCKS[ecc][version];
  const eccLen = ECC_PER_BLOCK[ecc][version];
  const raw = Math.floor(rawModules(version) / 8);
  const shortBlocks = blocks - (raw % blocks);
  const shortLen = Math.floor(raw / blocks);
  const divisor = rsDivisor(eccLen);
  const all = [];
  for (let i = 0, k = 0; i < blocks; i++) {
    const part = data.slice(k, k + shortLen - eccLen + (i < shortBlocks ? 0 : 1));
    k += part.length;
    const check = rsRemainder(part, divisor);
    if (i < shortBlocks) part.push(0);
    all.push(part.concat(check));
  }
  const out = [];
  for (let i = 0; i < all[0].length; i++) {
    all.forEach((block, j) => {
      if (i !== shortLen - eccLen || j >= shortBlocks) out.push(block[i]);
    });
  }
  return out;
}

const MASKS = [
  (x, y) => (x + y) % 2 === 0,
  (x, y) => y % 2 === 0,
  (x, y) => x % 3 === 0,
  (x, y) => (x + y) % 3 === 0,
  (x, y) => (Math.floor(x / 3) + Math.floor(y / 2)) % 2 === 0,
  (x, y) => ((x * y) % 2) + ((x * y) % 3) === 0,
  (x, y) => (((x * y) % 2) + ((x * y) % 3)) % 2 === 0,
  (x, y) => (((x + y) % 2) + ((x * y) % 3)) % 2 === 0,
];

function penalty(m, size) {
  let score = 0;
  const line = (get) => {
    let runColor = false, runLen = 0, history = 0;
    for (let i = 0; i < size; i++) {
      const c = get(i);
      if (i > 0 && c === runColor) {
        runLen++;
        if (runLen === 5) score += 3;
        else if (runLen > 5) score++;
      } else {
        runColor = c;
        runLen = 1;
      }
      // 1:1:3:1:1 finder-like patterns with four light modules on a side
      history = ((history << 1) | (c ? 1 : 0)) & 0x7ff;
      if (i >= 10 && (history === 0x05d || history === 0x5d0)) score += 40;
    }
  };
  for (let y = 0; y < size; y++) line((x) => m[y][x]);
  for (let x = 0; x < size; x++) line((y) => m[y][x]);
  for (let y = 0; y < size - 1; y++) {
    for (let x = 0; x < size - 1; x++) if (m[y][x] === m[y][x + 1] && m[y][x] === m[y + 1][x] && m[y][x] === m[y + 1][x + 1]) score += 3;
  }
  let dark = 0;
  for (const row of m) for (const c of row) if (c) dark++;
  const total = size * size;
  score += (Math.ceil(Math.abs(dark * 20 - total * 10) / total) - 1) * 10;
  return score;
}

function qr(value, o) {
  const level = String(o.ecc || "M").toUpperCase();
  if (!(level in ECC_LEVELS)) fail("ecc is L, M, Q or H");
  const ecc = ECC_LEVELS[level];
  const bytes = utf8(String(value));
  let version = Math.max(1, Math.min(40, Number(o.version) || 1));
  for (;; version++) {
    if (version > 40) fail("too much data for a QR code at level " + level + " (" + bytes.length + " bytes)");
    if (4 + (version <= 9 ? 8 : 16) + bytes.length * 8 <= dataCodewords(version, ecc) * 8) break;
  }
  const bits = [];
  const put = (v, n) => {
    for (let i = n - 1; i >= 0; i--) bits.push((v >>> i) & 1);
  };
  put(4, 4);
  put(bytes.length, version <= 9 ? 8 : 16);
  bytes.forEach((b) => put(b, 8));
  const capacity = dataCodewords(version, ecc) * 8;
  put(0, Math.min(4, capacity - bits.length));
  put(0, (8 - (bits.length % 8)) % 8);
  for (let pad = 0xec; bits.length < capacity; pad ^= 0xec ^ 0x11) put(pad, 8);
  const data = [];
  for (let i = 0; i < bits.length; i += 8) data.push(parseInt(bits.slice(i, i + 8).join(""), 2));
  const codewords = interleave(data, version, ecc);

  const size = version * 4 + 17;
  const m = Array.from({ length: size }, () => new Array(size).fill(false));
  const fixed = Array.from({ length: size }, () => new Array(size).fill(false));
  const set = (x, y, dark) => {
    m[y][x] = dark;
    fixed[y][x] = true;
  };
  for (let i = 0; i < size; i++) {
    set(6, i, i % 2 === 0);
    set(i, 6, i % 2 === 0);
  }
  for (const [cx, cy] of [[3, 3], [size - 4, 3], [3, size - 4]]) {
    for (let dy = -4; dy <= 4; dy++) {
      for (let dx = -4; dx <= 4; dx++) {
        const d = Math.max(Math.abs(dx), Math.abs(dy));
        const x = cx + dx, y = cy + dy;
        if (x >= 0 && x < size && y >= 0 && y < size) set(x, y, d !== 2 && d !== 4);
      }
    }
  }
  if (version > 1) {
    const count = Math.floor(version / 7) + 2;
    const step = version === 32 ? 26 : Math.ceil((version * 4 + 4) / (count * 2 - 2)) * 2;
    const at = [6];
    for (let p = size - 7; at.length < count; p -= step) at.splice(1, 0, p);
    for (let i = 0; i < count; i++) {
      for (let j = 0; j < count; j++) {
        if ((i === 0 && j === 0) || (i === 0 && j === count - 1) || (i === count - 1 && j === 0)) continue;
        for (let dy = -2; dy <= 2; dy++) for (let dx = -2; dx <= 2; dx++) set(at[i] + dx, at[j] + dy, Math.max(Math.abs(dx), Math.abs(dy)) !== 1);
      }
    }
  }
  const format = (mask) => {
    const d = (FORMAT_BITS[ecc] << 3) | mask;
    let rem = d;
    for (let i = 0; i < 10; i++) rem = (rem << 1) ^ ((rem >>> 9) * 0x537);
    const f = ((d << 10) | rem) ^ 0x5412;
    const bit = (i) => ((f >>> i) & 1) !== 0;
    for (let i = 0; i <= 5; i++) set(8, i, bit(i));
    set(8, 7, bit(6));
    set(8, 8, bit(7));
    set(7, 8, bit(8));
    for (let i = 9; i < 15; i++) set(14 - i, 8, bit(i));
    for (let i = 0; i < 8; i++) set(size - 1 - i, 8, bit(i));
    for (let i = 8; i < 15; i++) set(8, size - 15 + i, bit(i));
    set(8, size - 8, true);
  };
  format(0);
  if (version >= 7) {
    let rem = version;
    for (let i = 0; i < 12; i++) rem = (rem << 1) ^ ((rem >>> 11) * 0x1f25);
    const v = (version << 12) | rem;
    for (let i = 0; i < 18; i++) {
      const dark = ((v >>> i) & 1) !== 0;
      const a = size - 11 + (i % 3), b = Math.floor(i / 3);
      set(a, b, dark);
      set(b, a, dark);
    }
  }
  let k = 0;
  for (let right = size - 1; right >= 1; right -= 2) {
    if (right === 6) right = 5;
    for (let vert = 0; vert < size; vert++) {
      for (let j = 0; j < 2; j++) {
        const x = right - j;
        const y = ((right + 1) & 2) === 0 ? size - 1 - vert : vert;
        if (!fixed[y][x] && k < codewords.length * 8) {
          m[y][x] = ((codewords[k >>> 3] >>> (7 - (k & 7))) & 1) !== 0;
          k++;
        }
      }
    }
  }
  const apply = (mask) => {
    for (let y = 0; y < size; y++) for (let x = 0; x < size; x++) if (!fixed[y][x] && MASKS[mask](x, y)) m[y][x] = !m[y][x];
  };
  let best = 0, bestScore = Infinity;
  const forced = Number.isInteger(o.mask) && o.mask >= 0 && o.mask < 8 ? o.mask : -1;
  for (let mask = 0; mask < 8; mask++) {
    if (forced >= 0 && mask !== forced) continue;
    apply(mask);
    format(mask);
    const score = penalty(m, size);
    if (score < bestScore) {
      best = mask;
      bestScore = score;
    }
    apply(mask);
  }
  apply(best);
  format(best);
  return { type: "qr", size, version, ecc: level, mask: best, rows: m.map((r) => r.map((c) => (c ? "1" : "0")).join("")), text: String(value) };
}

// ---- public API -------------------------------------------------------------------------------------------

/** The modules of a barcode: `{ type, bars, text, ... }` for the linear types, `{ type: "qr", size, rows, ... }` for qr. */
export function encode(type, value, options) {
  const o = options || {};
  switch (String(type).toLowerCase()) {
    case "code128": return code128(value);
    case "code39": return code39(value);
    case "ean13": return ean13(value);
    case "ean8": return ean8(value);
    case "upca": return upca(value);
    case "qr": return qr(value, o);
    default: return fail("unknown type " + JSON.stringify(type) + " (" + types.join(", ") + ")");
  }
}

const escapeXml = (s) => String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
const num = (v, fallback) => (Number.isFinite(Number(v)) && v !== "" && v !== null && v !== undefined ? Number(v) : fallback);

/**
 * An `<svg>` of the barcode. Options: `moduleWidth` (px per narrow bar, default 2, QR 4), `height` (bar height
 * in px, default 60), `margin` (quiet zone in modules: default 10, QR 4), `color`, `background` (default white,
 * "none" for transparent), `text` (print the value under linear codes, default true), `fontSize`, `ecc` and
 * `version` for qr.
 */
export function svg(type, value, options) {
  const o = options || {};
  const code = encode(type, value, o);
  const color = o.color || "#000";
  const background = o.background === undefined ? "#fff" : o.background;
  const mw = Math.max(1, num(o.moduleWidth, code.type === "qr" ? 4 : 2));
  const margin = Math.max(0, num(o.margin, code.type === "qr" ? 4 : 10));
  const rect = (w, h) => (background && background !== "none" ? `<rect width="${w}" height="${h}" fill="${escapeXml(background)}"/>` : "");
  let path = "";
  let width, height, extra = "";
  if (code.type === "qr") {
    width = height = (code.size + margin * 2) * mw;
    code.rows.forEach((row, y) => {
      for (let x = 0; x < code.size; ) {
        if (row[x] !== "1") { x++; continue; }
        let end = x;
        while (end < code.size && row[end] === "1") end++;
        path += `M${(x + margin) * mw} ${(y + margin) * mw}h${(end - x) * mw}v${mw}h${-(end - x) * mw}z`;
        x = end;
      }
    });
  } else {
    const bars = code.bars;
    const barHeight = Math.max(1, num(o.height, 60));
    const showText = o.text !== false;
    const fontSize = num(o.fontSize, Math.max(8, Math.min(14, mw * 6)));
    const textBlock = showText ? Math.ceil(fontSize * 1.25) : 0;
    width = (bars.length + margin * 2) * mw;
    height = barHeight + textBlock;
    for (let x = 0; x < bars.length; ) {
      if (bars[x] !== "1") { x++; continue; }
      let end = x;
      while (end < bars.length && bars[end] === "1") end++;
      path += `M${(x + margin) * mw} 0h${(end - x) * mw}v${barHeight}h${-(end - x) * mw}z`;
      x = end;
    }
    if (showText) {
      const labels = code.labels || [{ at: bars.length / 2, text: code.text }];
      const y = barHeight + fontSize;
      for (const label of labels) extra += `<text x="${(margin + label.at) * mw}" y="${y}" font-family="sans-serif" font-size="${fontSize}" text-anchor="middle" fill="${escapeXml(color)}">${escapeXml(label.text)}</text>`;
    }
  }
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}" shape-rendering="crispEdges">${rect(width, height)}<path d="${path}" fill="${escapeXml(color)}"/>${extra}</svg>`;
}

/** The same picture as a `data:` URL, for `<img src>`, CSS backgrounds or printing. */
export function dataUrl(type, value, options) {
  return "data:image/svg+xml;charset=utf-8," + encodeURIComponent(svg(type, value, options));
}

// ---- <nile-barcode> -----------------------------------------------------------------------------------------

const OPTION_ATTRS = ["type", "value", "height", "module-width", "margin", "color", "background", "text", "ecc", "version", "font-size"];

/** `<nile-barcode type="code128" value="..." height="50">`: draws the barcode inside itself. */
export function defineBarcode(tag) {
  if (typeof customElements === "undefined" || typeof HTMLElement === "undefined") return;
  const name = tag || "nile-barcode";
  if (customElements.get && customElements.get(name)) return;
  class NileBarcode extends HTMLElement {
    static get observedAttributes() {
      return OPTION_ATTRS;
    }
    connectedCallback() {
      this.render();
    }
    attributeChangedCallback() {
      if (this.isConnected !== false) this.render();
    }
    render() {
      const attr = (n) => this.getAttribute(n);
      try {
        this.innerHTML = svg(attr("type") || "code128", attr("value") || "", {
          height: attr("height"),
          moduleWidth: attr("module-width"),
          margin: attr("margin"),
          color: attr("color") || undefined,
          background: attr("background") === null ? undefined : attr("background"),
          text: attr("text") === "false" || attr("text") === "off" ? false : undefined,
          ecc: attr("ecc") || undefined,
          version: attr("version"),
          fontSize: attr("font-size"),
        });
        this.removeAttribute("invalid");
        this.title = this.getAttribute("title") || "";
      } catch (e) {
        this.innerHTML = '<span class="nile-barcode-error">' + escapeXml(e.message) + "</span>";
        this.setAttribute("invalid", "");
      }
    }
  }
  customElements.define(name, NileBarcode);
  if (typeof document !== "undefined" && document.head && !document.querySelector("style[data-nile-ui=barcode]")) {
    const style = document.createElement("style");
    style.setAttribute("data-nile-ui", "barcode");
    style.textContent = ":where(nile-barcode) { display: inline-block; line-height: 0; } :where(nile-barcode) > .nile-barcode-error { font: 12px sans-serif; line-height: 1.3; color: #b91c1c; }";
    document.head.appendChild(style);
  }
}

defineBarcode();
