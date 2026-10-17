# Explorer-style tabs

Run from the repository root:

```sh
cargo run -p nile-shell -- examples/tabs/index.html
```

Rounded tabs share a custom window title bar with native window controls.
Use `+` to add a Home tab, `×` to close a tab, or “Open in new tab” to
duplicate the current folder. Closing the final tab opens a fresh Home tab.
The strip scrolls horizontally when there are too many tabs to fit.

Each tab retains its own folder, back/forward history, search, and selected
file. Click folders in Home or the sidebar to navigate. Files display a sample
selection message. All content is in memory; no filesystem access is used.

Keyboard shortcuts:

- Ctrl/Cmd+T: new tab.
- Ctrl/Cmd+W: close active tab.
- Ctrl+Tab / Ctrl+Shift+Tab: next / previous tab.
- Left/Right, Home/End while a tab is focused: select and focus a tab.

Drag the empty area beside the tabs to move the window. Tab reordering and
detaching tabs into other windows are not implemented.
