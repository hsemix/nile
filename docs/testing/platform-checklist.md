# Platform checklist (Windows and Linux)

CI builds Nile and runs the automated tests on macOS, Windows and Linux, but most of the native shell code (menus, tray,
printing, credential stores, hotkeys, devices) can only be proved by running it. Development happens on macOS, so
this list is for a person sitting at a **Windows** or **Linux** machine. Run it before a release and after changing
shell code, and record what you saw in the last column (`pass`, `fail: what happened`, or `n/a`).

Run each page with:

```sh
cargo run -p nile-shell -- examples/<page>.html          # add --release for a faster build
```

Setup. Windows: Rust stable, Visual Studio Build Tools (C++), Git. Linux: Rust stable and
`pkg-config libfontconfig1-dev libfreetype6-dev libxkbcommon-dev libwayland-dev libx11-dev libxcursor-dev libxi-dev libxrandr-dev libgl1-mesa-dev libasound2-dev`
(and `zenity` or `kdialog` for file dialogs, `xdotool` is not needed). Note the Linux session type (X11 or Wayland), the
desktop (GNOME, KDE, ...) and the Windows build number; several results depend on them.

Result: machine / OS version / session: ______________________  date: __________  tester: __________

## 1. Basics (both)

| # | Page | Do | Expect | Result |
|---|---|---|---|---|
| 1.1 | `hello.html` | Open it; press F5; resize the window | Page draws sharply (check a HiDPI screen too); reload works; layout follows the size | |
| 1.2 | `forms.html` | Type in inputs, textarea, select; Tab through; Ctrl+Z; Ctrl+C/V | Caret, selection, drop-downs and undo behave; clipboard works with another app | |
| 1.3 | `editor.html` | Type, bold with the toolbar, make a list, undo | Rich-text editing works; selection survives toolbar clicks | |
| 1.4 | `word-ribbon.html` | Use a few ribbon commands | No missing glyphs (check icons and symbols); fonts look right | |
| 1.5 | `symbols.html` | Look for empty boxes | Every symbol row has glyphs; note any gaps | |
| 1.6 | `popups.html`, `dialogs.html` | Open popups, then message boxes and file open/save | Popups position correctly; the system file dialogs appear, belong to the window and return the chosen path | |
| 1.7 | `clipboard.html` | Copy text, HTML and an image from here and from another app | Round trips both ways | |
| 1.8 | Any page | IME: type with an input method (Pinyin, Kana, Gboard...) | Pre-edit text shows in place; commit inserts it | |
| 1.9 | `stress.html` | Scroll and hover quickly | Stays smooth (no multi-second stalls) | |

## 2. Windows and chrome

| # | Page | Do | Expect | Result |
|---|---|---|---|---|
| 2.1 | `window.html` | Try each `window-*` setting it offers | Size, min/max, state, alignment and icon apply | |
| 2.2 | `solid-shadow.html` | Open; drag by the caption; resize by the edges | Frameless window drags and resizes; shadow on Windows (Linux: compositor's) | |
| 2.3 | `transparent.html`, `material.html` | Open | Windows: translucency or Mica/Acrylic where supported (Mica needs build 22523+). Linux: **expected gap on X11** (corners stay opaque); Wayland shows alpha | |
| 2.4 | `screens.html` | Open on a multi-monitor setup if you have one; move the window between screens | Lists every monitor, marks the current one, fires `resolutionchange`; battery, online and idle values are plausible (Linux idle needs `xprintidle`) | |
| 2.5 | Windows only | Frameless window with a maximize button: hover it | Snap Layouts flyout appears (Windows 11) | |
| 2.6 | Windows only | Frameless window with rounded `html` corners | Window is clipped to the radius | |

## 3. Menus, tray and desktop integration

| # | Page | Do | Expect | Result |
|---|---|---|---|---|
| 3.1 | `native-menu.html` | Use the menu bar and its shortcuts | Windows: a real menu bar; Linux: the in-window bar. Items, checks, submenus, accelerators and `role` items (copy, paste, quit) work | |
| 3.2 | `tray.html` | Show the tray icon; click; double-click; right-click | Windows: icon in the notification area (maybe behind the `^` arrow), clicks reach the page, menu opens. Linux: **expected: no tray** | |
| 3.3 | `tray.html` | Badge and progress buttons | Windows: taskbar overlay and progress. Linux: nothing (expected) | |
| 3.4 | `hotkeys.html` | Register `CmdOrCtrl+Shift+K`, switch to another app, press it | The log line appears and the window raises. Linux: X11 only; Wayland refuses (expected) | |
| 3.5 | `hotkeys.html` | Register `Nonsense+Q` | Refused (`ready` false), no crash | |
| 3.6 | `app-lifecycle.html` | Start a second copy | Windows: the first copy gets `second-instance`. Linux: same | |
| 3.7 | `drag-out.html` | Drag the file item to the desktop or a folder | The file lands there (Windows; Linux is **expected not to**) | |
| 3.8 | `drag-drop.html` | Drop files from the file manager onto the page | They are listed | |

## 4. Printing, files and secrets

| # | Page | Do | Expect | Result |
|---|---|---|---|---|
| 4.1 | `printing.html` | Export a PDF | A valid PDF with page breaks and repeated table headers | |
| 4.2 | `printing.html` | List printers; print to a real printer (or PDF printer) | Windows (PowerShell/winspool) and Linux (CUPS `lp`): the list matches the system; a job is queued | |
| 4.3 | `printing.html` | Send a receipt with ESC/POS (a thermal printer, or a network port 9100 listener such as `nc -l 9100`) | Bytes arrive; the receipt prints | |
| 4.4 | `mail.html` | Save a password (keychain); reload; load it back | Windows: appears in Credential Manager. Linux: needs a running keyring (GNOME Keyring or KWallet); without one the page reports it is unavailable | |
| 4.5 | `mail.html` | Send a test message to a real SMTP account (app password) | Delivered; try STARTTLS (587) and TLS (465) | |
| 4.6 | A scratch page | `import {reveal, moveToTrash, openWith} from "@env"` on a test file | Explorer / file manager shows the file; the file lands in the Recycle Bin / Trash and can be restored | |
| 4.7 | A scratch page | `fs.watch` on a folder; create, edit and delete a file | Events arrive within about a second (inotify / ReadDirectoryChangesW) | |

## 5. Devices

| # | Page | Do | Expect | Result |
|---|---|---|---|---|
| 5.1 | `serial.html` | Plug in a USB-serial adapter or use a null-modem pair (com0com on Windows, `socat` pty pair on Linux) | The port is listed; open it; text sent from the page appears at the other end and the reverse. Linux: add yourself to `dialout` first | |
| 5.2 | `media/` | Build it (`cargo run -p nile-extensions --example build-extension -- media --dest examples/media/extensions/media`), start the camera | Preview shows live video; **Take photo** writes a JPEG; Windows may show a privacy prompt; Linux needs `/dev/video0` access | |
| 5.3 | `media/` | Start the microphone, speak | The level bar moves; **Record** saves a WAV that plays | |

## 6. Packaging and updates

| # | Do | Expect | Result |
|---|---|---|---|
| 6.1 | `nile --pack examples/todo -o Todo --icon assets/icon-512.png --version 1.2.3 --company Acme` | Windows: `Todo.exe` shows the icon, and Properties > Details shows version and company. Linux: `Todo` plus a `.desktop` file and icon | |
| 6.2 | Run the packed program on a machine without the repository | Starts and works with no other files | |
| 6.3 | Windows: run it with SmartScreen enabled | Note the warning (unsigned): expected until the program is signed | |
| 6.4 | `examples/updater` (steps in the [updates guide](../guide/updates.md)) | A signed update is found, downloaded and applied after restart | |

## 7. Accessibility

| # | Do | Expect | Result |
|---|---|---|---|
| 7.1 | Windows: turn on Narrator (Win+Ctrl+Enter), open `forms.html` | Controls are announced with their names and roles; Tab moves between them | |
| 7.2 | Linux: Orca (`orca`), same page | Same | |
| 7.3 | A page with `aria-live` or `role="alert"` | The change is spoken | |

## Reporting

For each failure note: the row number, the OS and version, X11 or Wayland, what you expected, what happened, and any text in the
terminal. File it as an issue titled `[Windows] 3.2 tray icon does not appear` and add it to the **Known gaps** in the
README only if it is intended behaviour.
