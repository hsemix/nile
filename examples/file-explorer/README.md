# File explorer

Run from the repository root:

```sh
cargo run -p nile-shell -- examples/file-explorer/index.html
```

Browses real files through Nile's existing `@sys` module. Starts in the examples
folder. Enter a folder path or use **Open folder…** to browse elsewhere.

- Expand folders with their disclosure arrow; click their name to navigate.
- Double-click or press Enter to open a folder. Text files appear in Details.
- Filter the current directory, sort by column, and toggle hidden files.
- Back/Forward retain navigation history; Up or Backspace opens the parent.
- Arrow keys, Home and End select files. Right-click opens a command menu.
- Drag either native frameset splitter to resize the panes.

The example reads files only. Text previews support common source/document
extensions and files up to 64 KB. It does not launch external applications or
modify files. Directory enumeration and file reads use the synchronous APIs;
very large directories or slow network drives can pause the UI. Folder trees
expand to five levels; deeper folders remain accessible through the file list.
