# Northstar Workspace

A local desktop application for projects, tasks, rich notes, and managed file
attachments. Records live in SQLite, with recovery drafts and portable backups.
It starts empty and works offline; it does not require an account or a server.

## Run

From the repository root:

```sh
cargo run -p nile-extensions --example build-extension -- sqlite --dest examples/workspace/extensions/sqlite
cargo run -p nile-shell -- examples/workspace/index.html
```

The extension build requires Rust and a C compiler. Build the SQLite extension
on the same platform/architecture as the application. Generated libraries are
ignored by Git; assemble them again after a fresh checkout.

Create a project, add tasks, and use its **Notes & documents** and **Attachments**
tabs. Each project/document opens in a workspace tab. Right-click a task or a
project in the sidebar for its context menu. Drag the sidebar divider to resize
it, or focus it and use Left/Right. The empty title-bar area moves the window.

## Working features

- Projects: create/edit, archive/unarchive, and permanently delete with confirmation.
- Tasks: create/edit/delete, status, priority, optional deadline, text/status
  filters, selection, and transactional bulk completion.
- Notes: editable titles, headings, bold/italic/underline, lists, alignment,
  undo/redo, tables, inline images, and an optional paged view.
- Autosave after 700 ms of inactivity, explicit Save/Ctrl+S, synchronous recovery
  drafts before database saves, visible save errors, and close protection while
  a document has unsaved changes. Overview provides recovery/discard controls.
- Attachments: native file picker, managed copies, image and small text previews,
  export a copy, and remove attachment records without changing original files.
- Search: SQLite-backed search over project names/descriptions, task names,
  note titles/body text, and attachment names, including archived projects.
- Overview: active project cards, completion counts, overdue tasks, upcoming
  deadlines, and a Canvas 2D status chart.
- Read-only document snapshots in separate native windows. These deliberately
  avoid competing document writers; refresh by opening a new snapshot.
- Project PDF reports with task tables, attachment lists, and formatted notes.
  The export button invokes this application's executable with an argv array
  and the engine's vector PDF renderer, waits for completion, and cleans up its
  temporary files. No external PDF tool is needed. Print the exported PDF using
  the operating system.
- Portable JSON backup including attachment bytes. Restore validates records
  and references, writes a safety backup, stages blobs, and replaces records in
  one SQLite transaction. A SQL failure rolls the replacement back. Restore
  clears settings, tabs, and old recovery drafts.
- Light/dark appearance, restored tabs and sidebar width, command palette,
  keyboard navigation, dialog focus wrapping, and labelled controls.

## Storage

The default is `@env.path('appdata', 'Nile/Northstar')`:

- macOS: `~/Library/Application Support/Nile/Northstar`
- Windows: `%APPDATA%/Nile/Northstar`
- Linux: `$XDG_DATA_HOME/Nile/Northstar`, or `~/.local/share/Nile/Northstar`

`workspace.db` contains schema version 1 and preferences. `attachments/` holds
managed copies, and `drafts/` holds recoverable note edits. Keep one main
application instance open per data folder. The separately opened document
windows are snapshots, not database editors.

For an isolated workspace, set `NORTHSTAR_DATA_DIR` before launching:

```sh
NORTHSTAR_DATA_DIR=/tmp/northstar-sandbox cargo run -p nile-shell -- examples/workspace/index.html
```

Backups are unencrypted. Safety backups are named `before-restore-*.json` in the
data folder. Copies of removed or replaced attachment blobs remain on disk;
there is no automatic storage cleanup. The app never removes original files.

Limits: 160-character titles, 2,000-character project descriptions, 650 KB per
note including its indexed text and JSON encoding, 160 KB per inline image,
10 MB per attachment, 64 KB for text attachment previews, and 150 MB per
portable backup. Use attachments for larger images. Queries page through
records to respect the native extension's bounded reply sizes; the UI keeps the
loaded records in memory. Search returns at most 100 matches and uses substring
matching. This release targets personal workspaces rather than million-row
collections. It has no cloud sync, collaboration, tab detaching, or reminders.

Imported note markup is restricted to formatting tags and embedded image data;
scripts, event attributes, remote/file image sources, frames and arbitrary CSS
are removed before display. Restoring an externally supplied backup still
requires the explicit replacement confirmation.

## Package

After assembling the SQLite extension and building `nile-shell`:

```sh
cargo run -p nile-shell -- --pack examples/workspace \
  -o target/Northstar.app --name "Northstar Workspace" --version 1.0.0
```

Use `-o target/Northstar.exe` on Windows or `-o target/northstar` on Linux.
No Python packaging script is needed.

The macOS package contains a signed runtime and a separate asset archive,
including the declared SQLite extension. Nile generates its default icon (or
your `--icon` image) directly, without `sips` or `iconutil`. The bundle receives
a development ad-hoc signature. Other platforms use Nile's packed-application
format. Build each platform's
extension and runtime on that platform. Public distribution still needs the
platform's production signing/notarization process; that is not performed by
the built-in development packager.

## Verification

```sh
cargo test -p nile-shell --test workspace_app -- --nocapture
```

The Rust integration test copies the application into a temporary folder,
assembles SQLite there, and uses a separate database. It exercises UI handlers
and real SQLite: project/task validation,
bulk updates, autosave/recovery, large-note pagination and limits, search,
managed attachments, previews, backup/restore (including forced SQL rollback),
archive/unarchive, themes, PDF rendering, command-palette events, and restart
persistence. File chooser responses are substituted during checks. An expected
validation error is printed while checking a blank task title.

Native file dialogs, caption dragging/window controls, read-only child windows,
and screen-reader behavior need a GUI check on each target OS. macOS bundle
startup and signature validation have been checked locally; Windows/Linux
packages have not been exercised here.
