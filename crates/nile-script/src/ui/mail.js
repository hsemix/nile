// @nile/mail: send email over SMTP, with HTML, attachments and TLS.
//
//   import { sendMail } from "@nile/mail";
//   const result = await sendMail({
//     host: "smtp.example.com", port: 587, secure: "starttls",            // "starttls" (587), "tls" (465) or "none" (25)
//     user: "shop@example.com", password: await keychain.get("smtp-password"),
//     from: { name: "SoftSale", address: "shop@example.com" },
//     to: ["ada@example.com", { name: "Alan", address: "alan@example.com" }], cc: [], bcc: [],
//     subject: "Receipt 10042", text: "Thank you!", html: "<p>Thank you!</p>",
//     attachments: [{ filename: "receipt.pdf", content: pdfBytes }, { filename: "logo.png", path: "/app/logo.png", cid: "logo" }],
//   });                                                                    // { messageId, accepted: [...], response }
//
// A small, direct SMTP client (EHLO, STARTTLS or implicit TLS, AUTH PLAIN/LOGIN, MAIL/RCPT/DATA) over `@sys` sockets;
// it is meant for an app sending its own mail through the user's or the company's mail server (put the password in
// `@nile/keychain`, not in a file). Addresses are strings ("a@b.c", "Ada <a@b.c>") or { name, address }. Non-ASCII
// subjects and names are encoded, bodies are UTF-8 (base64), text + html become multipart/alternative and attachments
// multipart/mixed; an attachment with `cid` can be used as `<img src="cid:logo">`. Options: `hostname` (the name given in
// EHLO), `timeout` (ms per server reply, 30000), `headers` ({ "X-Name": "value" }), `replyTo`, `allowInsecureAuth` (send the
// password without TLS: refused by default). `buildMessage(options)` returns the raw message text without sending (for
// "save as .eml" or a preview). Errors carry `responseCode` and `response` when the server said no. Not included: OAuth2
// (XOAUTH2), DKIM signing, delivery status tracking, reading mail (IMAP/POP). Certificates are checked against the
// bundled web roots; a server with a private CA or self-signed certificate is refused.

import { TCP, fs } from "@sys";

const CRLF = "\r\n";
const encoder = new TextEncoder();
const decoder = new TextDecoder();

function toB64(bytes) {
  const u = bytes instanceof Uint8Array ? bytes : encoder.encode(String(bytes));
  let s = "";
  for (let i = 0; i < u.length; i += 0x8000) s += String.fromCharCode.apply(null, u.subarray(i, i + 0x8000));
  return btoa(s);
}
const wrap76 = (b64) => (b64.match(/.{1,76}/g) || []).join(CRLF);
const ascii = (s) => /^[\x20-\x7e]*$/.test(s);

function clean(value, what) {
  const s = String(value === undefined || value === null ? "" : value);
  if (/[\r\n]/.test(s)) throw new Error("line breaks are not allowed in " + what);
  return s;
}
/** A header value with non-ASCII text as RFC 2047 encoded words (UTF-8, base64). */
function encodeWords(text) {
  const s = clean(text, "a header");
  if (ascii(s)) return s;
  // Words of at most ~45 bytes so the encoded line stays under 75 characters.
  const words = [];
  let current = "";
  for (const ch of s) {
    if (encoder.encode(current + ch).length > 45) { words.push(current); current = ""; }
    current += ch;
  }
  if (current) words.push(current);
  return words.map((w) => "=?UTF-8?B?" + toB64(w) + "?=").join(CRLF + " ");
}

/** { name, address } from "Ada <a@b.c>", "a@b.c" or an object. */
export function parseAddress(input) {
  if (input && typeof input === "object") return { name: clean(input.name || "", "a name"), address: clean(input.address, "an address").trim() };
  const s = clean(input, "an address").trim();
  const m = /^(.*?)\s*<([^<>]+)>$/.exec(s);
  const a = m ? { name: m[1].replace(/^"|"$/g, ""), address: m[2].trim() } : { name: "", address: s };
  return a;
}
const list = (v) => (v === undefined || v === null ? [] : Array.isArray(v) ? v : [v]).map(parseAddress);
function validAddress(a) {
  if (!/^[^\s@<>()]+@[^\s@<>()]+\.[^\s@<>()]+$/.test(a.address) && !/^[^\s@<>()]+@(localhost|[^\s@<>()]+)$/.test(a.address)) throw new Error("not an email address: " + a.address);
  return a;
}
function formatAddress(a) {
  if (!a.name) return a.address;
  const name = ascii(a.name) ? (/[()<>@,;:\\".\[\]]/.test(a.name) ? '"' + a.name.replace(/(["\\])/g, "\\$1") + '"' : a.name) : encodeWords(a.name);
  return name + " <" + a.address + ">";
}
const formatList = (l) => l.map(formatAddress).join(", ");

const randomId = () => (globalThis.crypto && crypto.randomUUID ? crypto.randomUUID() : String(Date.now()) + Math.random().toString(16).slice(2));

function bodyPart(contentType, text) {
  return "Content-Type: " + contentType + "; charset=utf-8" + CRLF + "Content-Transfer-Encoding: base64" + CRLF + CRLF + wrap76(toB64(text));
}

function attachmentBytes(a) {
  if (a.content !== undefined) return typeof a.content === "string" ? encoder.encode(a.content) : a.content instanceof ArrayBuffer ? new Uint8Array(a.content) : ArrayBuffer.isView(a.content) ? new Uint8Array(a.content.buffer, a.content.byteOffset, a.content.byteLength) : Uint8Array.from(a.content);
  if (a.path) return new Uint8Array(fs.readFileSync(a.path));
  throw new Error("an attachment needs content or a path");
}
const TYPES = { pdf: "application/pdf", png: "image/png", jpg: "image/jpeg", jpeg: "image/jpeg", gif: "image/gif", txt: "text/plain", csv: "text/csv", html: "text/html", json: "application/json", zip: "application/zip", xml: "application/xml", xlsx: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet", docx: "application/vnd.openxmlformats-officedocument.wordprocessingml.document" };
function attachmentPart(a) {
  const filename = clean(a.filename || (a.path ? String(a.path).split(/[\\/]/).pop() : "attachment"), "a filename");
  const type = clean(a.contentType || TYPES[filename.split(".").pop().toLowerCase()] || "application/octet-stream", "a content type");
  const name = ascii(filename) ? '"' + filename.replace(/(["\\])/g, "\\$1") + '"' : '"' + encodeWords(filename) + '"';
  const disposition = a.cid ? "inline" : "attachment";
  return "Content-Type: " + type + "; name=" + name + CRLF + "Content-Transfer-Encoding: base64" + CRLF + "Content-Disposition: " + disposition + "; filename=" + name + CRLF + (a.cid ? "Content-ID: <" + clean(a.cid, "a content id") + ">" + CRLF : "") + CRLF + wrap76(toB64(attachmentBytes(a)));
}

/** The message as text (headers and body, CRLF line ends, no trailing dot). */
export function buildMessage(options) {
  const o = options || {};
  const from = validAddress(parseAddress(o.from));
  const to = list(o.to).map(validAddress), cc = list(o.cc).map(validAddress), replyTo = list(o.replyTo);
  if (!to.length && !cc.length && !list(o.bcc).length) throw new Error("no recipients");
  const messageId = o.messageId || "<" + randomId() + "@" + (from.address.split("@")[1] || "localhost") + ">";
  const headers = [
    "Date: " + (o.date ? new Date(o.date) : new Date()).toUTCString().replace("GMT", "+0000"),
    "From: " + formatAddress(from),
  ];
  if (to.length) headers.push("To: " + formatList(to));
  if (cc.length) headers.push("Cc: " + formatList(cc));
  if (replyTo.length) headers.push("Reply-To: " + formatList(replyTo));
  headers.push("Subject: " + encodeWords(o.subject || ""), "Message-ID: " + clean(messageId, "a message id"), "MIME-Version: 1.0");
  for (const [k, v] of Object.entries(o.headers || {})) {
    if (!/^[A-Za-z0-9-]+$/.test(k)) throw new Error("invalid header name: " + k);
    headers.push(k + ": " + encodeWords(v));
  }
  const text = o.text !== undefined ? String(o.text) : undefined;
  const html = o.html !== undefined ? String(o.html) : undefined;
  const attachments = o.attachments || [];
  let body;
  const boundary = () => "=_nile_" + randomId().replace(/-/g, "");
  const alternative = () => {
    if (text !== undefined && html !== undefined) {
      const b = boundary();
      return { type: 'multipart/alternative; boundary="' + b + '"', text: "--" + b + CRLF + bodyPart("text/plain", text) + CRLF + "--" + b + CRLF + bodyPart("text/html", html) + CRLF + "--" + b + "--" };
    }
    if (html !== undefined) return { type: null, part: bodyPart("text/html", html) };
    return { type: null, part: bodyPart("text/plain", text === undefined ? "" : text) };
  };
  const alt = alternative();
  const inline = attachments.filter((a) => a.cid), plain = attachments.filter((a) => !a.cid);
  const withRelated = () => {
    // inline images belong with the HTML part in a multipart/related
    if (!inline.length || html === undefined) return alt;
    const b = boundary();
    const first = alt.type ? "Content-Type: " + alt.type + CRLF + CRLF + alt.text : alt.part;
    return { type: 'multipart/related; boundary="' + b + '"', text: "--" + b + CRLF + first + CRLF + inline.map((a) => "--" + b + CRLF + attachmentPart(a)).join(CRLF) + CRLF + "--" + b + "--" };
  };
  const main = withRelated();
  const rest = html === undefined ? attachments : plain;
  if (rest.length) {
    const b = boundary();
    const first = main.type ? "Content-Type: " + main.type + CRLF + CRLF + main.text : main.part;
    headers.push('Content-Type: multipart/mixed; boundary="' + b + '"');
    body = "--" + b + CRLF + first + CRLF + rest.map((a) => "--" + b + CRLF + attachmentPart(a)).join(CRLF) + CRLF + "--" + b + "--";
  } else if (main.type) {
    headers.push("Content-Type: " + main.type);
    body = main.text;
  } else {
    const [ctLine, ...tail] = main.part.split(CRLF);
    headers.push(ctLine, tail[0]);
    body = tail.slice(2).join(CRLF);
  }
  return headers.join(CRLF) + CRLF + CRLF + body + CRLF;
}

class Reply extends Error {
  constructor(code, lines) {
    super(code + " " + lines.join(" "));
    this.responseCode = code;
    this.response = lines.join("\n");
  }
}

class Connection {
  constructor(tcp, timeout) {
    this.tcp = tcp;
    this.timeout = timeout;
    this.buffer = "";
  }
  async _fill() {
    let timer;
    const chunk = await Promise.race([
      this.tcp.read(),
      new Promise((_, reject) => { timer = setTimeout(() => reject(new Error("the mail server did not answer in time")), this.timeout); }),
    ]).finally(() => clearTimeout(timer));
    if (chunk === undefined) throw new Error("the mail server closed the connection");
    this.buffer += decoder.decode(chunk);
  }
  /** A (possibly multi-line) reply: { code, lines }. */
  async reply() {
    const lines = [];
    for (;;) {
      let at;
      while ((at = this.buffer.indexOf("\n")) < 0) await this._fill();
      const line = this.buffer.slice(0, at).replace(/\r$/, "");
      this.buffer = this.buffer.slice(at + 1);
      const m = /^(\d{3})([ -])(.*)$/.exec(line);
      if (!m) throw new Error("unexpected reply from the mail server: " + line);
      lines.push(m[3]);
      if (m[2] === " ") return { code: Number(m[1]), lines };
    }
  }
  async send(text) {
    await this.tcp.write(encoder.encode(text));
  }
  /** Send a command and require a reply code below 400 (or one of `expect`). */
  async command(text, expect) {
    await this.send(text + CRLF);
    return this.expect(expect);
  }
  async expect(expect) {
    const r = await this.reply();
    const ok = expect ? [].concat(expect).includes(r.code) : r.code < 400;
    if (!ok) throw new Reply(r.code, r.lines);
    return r;
  }
}

/** Send a message. Resolves { messageId, accepted, response }. */
export async function sendMail(options) {
  const o = options || {};
  if (!o.host) throw new Error("sendMail needs a host");
  const secure = o.secure === undefined ? (o.port === 465 ? "tls" : o.port === 25 ? "none" : "starttls") : o.secure === true ? "tls" : o.secure === false ? "none" : String(o.secure);
  if (!["starttls", "tls", "none"].includes(secure)) throw new Error('secure must be "starttls", "tls" or "none"');
  const port = o.port || (secure === "tls" ? 465 : secure === "none" ? 25 : 587);
  const user = o.user !== undefined ? o.user : o.auth && o.auth.user, pass = o.password !== undefined ? o.password : o.auth && o.auth.pass;
  if (user && secure === "none" && !o.allowInsecureAuth) throw new Error("refusing to send a password without TLS (secure: \"none\"); set allowInsecureAuth to override");
  const raw = buildMessage(o);
  const from = validAddress(parseAddress(o.from));
  const recipients = [...list(o.to), ...list(o.cc), ...list(o.bcc)].map(validAddress);
  const messageId = /^Message-ID: (.*)$/m.exec(raw)[1].trim();
  const hostname = o.hostname || "localhost";
  const tcp = new TCP();
  const c = new Connection(tcp, o.timeout || 30000);
  try {
    await tcp.connect({ ip: String(o.host), port: port });
    if (secure === "tls") await tcp.tls(String(o.host));
    await c.expect(220);
    let caps = (await c.command("EHLO " + hostname)).lines.map((l) => l.toUpperCase());
    if (secure === "starttls") {
      if (!caps.some((l) => l === "STARTTLS")) throw new Error("the server does not offer STARTTLS; use secure: \"none\" only on a trusted network");
      await c.command("STARTTLS", 220);
      await tcp.tls(String(o.host));
      c.buffer = "";
      caps = (await c.command("EHLO " + hostname)).lines.map((l) => l.toUpperCase());
    }
    if (user) {
      const auth = caps.find((l) => l.startsWith("AUTH")) || "";
      if (/\bPLAIN\b/.test(auth) || !auth) {
        await c.command("AUTH PLAIN " + toB64("\u0000" + user + "\u0000" + (pass === undefined ? "" : pass)), 235);
      } else if (/\bLOGIN\b/.test(auth)) {
        await c.command("AUTH LOGIN", 334);
        await c.command(toB64(String(user)), 334);
        await c.command(toB64(String(pass === undefined ? "" : pass)), 235);
      } else throw new Error("the server offers no supported login method (" + auth + ")");
    }
    await c.command("MAIL FROM:<" + from.address + ">");
    const accepted = [];
    for (const r of recipients) {
      await c.command("RCPT TO:<" + r.address + ">", [250, 251]);
      accepted.push(r.address);
    }
    await c.command("DATA", 354);
    // dot-stuffing: a line that starts with "." gets a second one
    const stuffed = raw.replace(/(^|\r\n)\./g, "$1..");
    const done = await c.command(stuffed + "." , undefined).catch((e) => { throw e; });
    try { await c.command("QUIT", 221); } catch (e) { /* some servers just hang up */ }
    return { messageId: messageId, accepted: accepted, response: done.lines.join("\n") };
  } finally {
    try { tcp.close(); } catch (e) { /* closed */ }
  }
}
