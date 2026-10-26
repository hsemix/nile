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
