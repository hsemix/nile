# Tray icon, badge and progress

Two small desktop features that live outside the window.

```js
// Notification area (Windows) / menu bar (macOS)
Window.this.tray = {
  icon: "tray.png",             // PNG next to the page
  tooltip: "My app",
  menu: document.$("menu#tray"), // optional; same <menu> markup as Window.this.menu
  template: true,               // macOS: a one-colour icon the system tints for light/dark bars
};
document.on("trayclick", () => Window.this.focus());
document.on("traydoubleclick", () => Window.this.focus());
Window.this.tray = null;        // remove

// Dock icon (macOS) / taskbar button overlay (Windows)
application.badge = 3;          // a short text: a count or "!"
application.badge = null;       // clear
```

```js
// Progress on the same icon
application.progress = 0.4;            // 0 to 1
application.progress = "indeterminate"; // Windows marquee; macOS shows a full bar
application.progress = null;           // clear
```

- **macOS:** the tray is an `NSStatusItem`. With a `menu`, the system opens it on click (no `trayclick`);
  without one, `trayclick` fires. The badge is the dock tile's label. Progress is a bar drawn along the bottom of the dock icon (the dock tile's content view, which holds the application icon and the bar).
- **Windows:** the tray is a notification-area icon: left click is `trayclick`, double click
  `traydoubleclick`, right click opens the `menu`. Progress is the taskbar button's progress bar (`ITaskbarList3`). The badge is an overlay icon (a red disc with the text,
  3 characters at most, `99+` beyond that) on the taskbar button. Written without a Windows machine
  to hand; this is the first thing to check on one.
- **Linux:** not supported yet (the tray needs the StatusNotifierItem D-Bus protocol, which would add a
  D-Bus dependency); the calls do nothing and `trayclick` never fires.
- The window that sets the tray owns it and receives its events; menu items fire their `click` event and
  `role` items (`quit`, ...) work as in the application menu. The menu is read again when the document
  changes. Sciter has no tray API: this is a Nile design.

Example: [tray.html](../../examples/tray.html).
