# App lifecycle: one instance, links, files and login

A desktop app is more than its window. This page covers the parts the operating system sees:
running only one copy, being opened with a file or a `myapp://` link, and starting at login.

## Arguments

`application.arguments` is the program's own arguments, an array of strings. In a packed app that is
everything after the executable (the system passes a double-clicked file or a link this way on
Windows and Linux). When you run a page with `nile page.html -- one two`, it is what follows `--`
(or a second file name).

## One running copy

Declare it on the page:

```html
<html app-single-instance="com.example.notes">
```

or in the app folder's `nile.json`:

```json
{ "singleInstance": "com.example.notes" }
```

(`true` and an empty attribute use the program's name.) The check happens before any script runs. A
second launch sends its arguments and working folder to the first one, which comes to the front, and
exits with code 0:

```js
document.addEventListener("second-instance", (e) => {
  openFiles(e.arguments);   // what the second launch was given
  console.log(e.cwd);       // where it was started
});
```

The two instances talk over a private local socket (a Unix socket in the user's runtime folder, or a
loopback port recorded in `%LOCALAPPDATA%\nile` on Windows). A leftover from a crash is replaced.

## Links and files

Declare what the app opens when you pack it (or list them in `nile.json` as `"protocols"` and
`"fileTypes"`):

```sh
nile --pack notes -o Notes.app --protocol notes --file-type md --file-type txt
```

- **macOS:** written into the bundle's `Info.plist` (`CFBundleURLTypes`, `CFBundleDocumentTypes`).
  The system sends the open request as an Apple event, which Nile turns into events.
- **Linux:** written into the `.desktop` entry (`MimeType`, `Exec ... %U`). Install it with `--install`.
- **Windows:** nothing is written at pack time; call `application.registerProtocol()` and
  `application.registerFileType()` from your installer step or first run (below).

Whatever the system opens with the app reaches the page as events (and, on Windows and Linux, also as
`application.arguments` when it starts the program):

```js
document.addEventListener("open-url",  (e) => route(e.url));    // notes://note/42
document.addEventListener("open-file", (e) => openFile(e.path));
```

On macOS the events arrive both when the app is already running and when the link or file launches
it. A running app with `app-single-instance` gets a `second-instance` event on Windows and Linux
instead (the second launch is the one carrying the link).

## Registering at run time (Windows, Linux)

```js
application.registerProtocol("notes");     // -> true when the system took it
application.registerFileType("md");        // known extensions only on Linux (text, images, audio, json, ...)
```

These write per-user settings (the registry under `HKCU\Software\Classes`, or a `.desktop` entry plus
`xdg-mime`), so no administrator rights are needed. On macOS they return `false`: declare the scheme
and the file types in the bundle instead.

## Start at login

```js
application.loginItem = true;     // a Run key (Windows), a LaunchAgent (macOS), an autostart entry (Linux)
console.log(application.loginItem);
```

## Global hotkeys

A shortcut that works while another program is in front (a quick-capture window, start/stop for a recorder):

```js
const key = application.registerHotkey("CmdOrCtrl+Shift+K", () => Window.this.focus());
if (!(await key.ready)) console.warn("another program owns that shortcut");
key.close();                       // give it back
```

The accelerator is modifiers (`Ctrl`, `Shift`, `Alt`/`Option`, `Cmd`/`Win`/`Super`, `CmdOrCtrl`) and one key (a letter, digit, `F1`-`F24`, `Space`, `Enter`, arrows...) joined by `+`. `ready` resolves with whether the system accepted it; a misspelling or a shortcut another program already owns is refused. The keys belong to the window that registered them and go when the program ends. Linux works under X11 only (Wayland has no global shortcuts). `examples/hotkeys.html`. Registration is checked on macOS; the keypress itself, Windows and Linux haven't been run here.

## What isn't here

Auto-update, an installer (MSI/DMG) and code signing are not part of Nile yet, and a C host that embeds
`libnile` has to find single-instance and link handling itself (the pieces are in `nile-shell`'s
`instance.rs`).

`examples/app-lifecycle.html` shows all of it.
