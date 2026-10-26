# Sending email: `@nile/mail`

An app can send mail through the user's or the company's own mail server with a small SMTP client built on `@sys` sockets.

```js
import { sendMail } from "@nile/mail";
import { keychain } from "@nile/keychain";

const result = await sendMail({
  host: "smtp.example.com", port: 587, secure: "starttls",       // "starttls" (587), "tls" (465), "none" (25)
  user: "shop@example.com", password: await keychain.get("smtp-password"),
  from: { name: "SoftSale", address: "shop@example.com" },
  to: ["ada@example.com", "Alan <alan@example.com>"], cc: [], bcc: [],
  subject: "Receipt 10042",
  text: "Thank you!", html: "<p>Thank you! <img src='cid:logo'></p>",
  attachments: [{ filename: "receipt.pdf", content: pdfBytes }, { filename: "logo.png", path: "/app/logo.png", cid: "logo" }],
});
// { messageId: "<...>", accepted: ["ada@example.com", ...], response: "2.0.0 queued as 42" }
```

What it does: connects (implicit TLS for `secure: "tls"`, or upgrades with STARTTLS), says EHLO, logs in with AUTH PLAIN or
LOGIN when you give `user` and `password`, sends `MAIL FROM`, one `RCPT TO` per recipient (Bcc included, never written into the
headers), the message with dot-stuffing, and QUIT. Non-ASCII subjects and names are encoded (RFC 2047), the body is UTF-8
in base64, text plus html becomes `multipart/alternative`, attachments `multipart/mixed`, and an attachment with a `cid` becomes
an inline image of the HTML. Line breaks in any header value are refused, so user input can't inject headers.

- **Passwords:** keep them in [`@nile/keychain`](credentials.md), never in a JSON file. Sending a password without TLS
  (`secure: "none"`) is refused unless you set `allowInsecureAuth: true`.
- **Errors:** a refusal from the server rejects with `error.responseCode` (for example 550 for an unknown recipient) and
  `error.response`. A silent server times out after `timeout` ms (30,000).
- `buildMessage(options)` returns the raw message text without sending: for "Save as .eml", a preview, or your own transport.
- Not included: OAuth2 login (Gmail and Microsoft 365 are moving to it), DKIM signing, reading mail (IMAP/POP), delivery
  tracking. Certificates are checked against the bundled web roots, so a server with a private or self-signed certificate is
  refused. A mail provider that needs OAuth2 can't be used yet; an app password or an SMTP relay works.
- Tested against a scripted local SMTP server (commands, AUTH, MIME structure, attachments, dot-stuffing, a 550 refusal).
  The TLS and STARTTLS paths reuse the engine's TLS sockets (as `wss://` does) but were not run against a real mail server.
  For one-off messages the user wants to review, `launch("mailto:ada@example.com?subject=Receipt")` opens their mail client.

`examples/mail.html` is a small mail form with a message-source preview and "remember password" through the keychain.
