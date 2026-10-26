# Showing, trashing and opening files

Three small helpers in `@env`, for apps that manage files:

```js
import { reveal, moveToTrash, openWith, launch } from "@env";
reveal("/Users/me/invoices/7.pdf");          // Finder / Explorer with the file selected (Linux: its folder)
moveToTrash("/Users/me/old.xlsx");           // the Trash / Recycle Bin, a file or a whole folder
openWith("/Users/me/logo.png", "Preview");   // a given program (macOS: an application name; elsewhere a program name or path)
launch("/Users/me/invoice.pdf");             // the default program for it (already there)
```

Each throws an `Error` with the system's message on failure (a missing path is `ENOENT: ...`).

- **macOS:** `reveal` uses `open -R`. `moveToTrash` moves the item into `~/.Trash`, which is what Finder shows, without
  asking for any permission; it does not record the original place, so Finder's "Put Back" is not offered.
- **Windows:** `reveal` runs `explorer /select`; `moveToTrash` sends the item to the Recycle Bin through PowerShell and
  the .NET `FileSystem` class (so it can be restored). Untested on Windows.
- **Linux:** `reveal` opens the containing folder with `xdg-open`; `moveToTrash` tries `gio trash`, then `trash-put`, then
  writes into `~/.local/share/Trash` itself with a `.trashinfo` file so file managers can restore it.

## Monitors, power, network and idle time

```js
for (let i = 0; i < Window.screens; i++) {
  const [x, y, w, h] = Window.screenBox(i, "frame");              // physical pixels
  const css = Window.screenBox(i, "frame", "dimension", false);   // CSS pixels
}
Window.this.screen;                                  // the monitor this window is on
addEventListener("resolutionchange", () => { /* monitors or scale changed */ });

import { online, power, idleSeconds } from "@env";   // Promises
await online();         // is there a route to the internet
await power();          // { battery, charging, percent }; battery is false on a desktop machine
await idleSeconds();    // seconds since the last key or mouse input
addEventListener("offline", () => showOfflineBanner());     // navigator.onLine follows
```

`screenBox` follows [Sciter's shape](https://docs.sciter.com/docs/DOM/Window): `what` is `"frame"`, `"workarea"`, `"device"` (the name), `"isPrimary"` or `"devicePixelRatio"`; `part` is `"xywh"` (default), `"rect"`, `"position"`, `"dimension"` or a single side. `workarea` is the same as `frame` (the window system doesn't tell Nile where the taskbar is); there is no `"snapshot"`.
`online()` asks the system for a route (no packet is sent), so it doesn't prove a server answers. The `online`/`offline` events and `navigator.onLine` poll every 5 seconds once something listens. Idle time on Linux needs `xprintidle` (X11) and otherwise rejects with `ENOSYS`.
Checked on macOS (a real battery reading, idle time, route); Windows (PowerShell queries) and Linux are untested.
