# Northstar application studio

Nine interactive desktop interfaces built with Nile's existing HTML/CSS/JS APIs.
All examples share one shell, and keep their sample data when switching apps.
Data resets when the page is reloaded. No engine changes are required.

```sh
cargo run -p nile-shell -- examples/workbench/index.html
```

You can also open `settings.html`, `dashboard.html`, `kanban.html`, `mail.html`,
`ide.html`, `inventory.html`, `calendar.html`, `media.html` or `slides.html` to
start directly in a particular interface. The sidebar works in every entry page.

| Interface | Working interactions |
|---|---|
| Settings | Edit profile, preview light/dark theme and density, toggle preferences, save/reset |
| Dashboard | Change week/month and team filters; metrics and chart update |
| Kanban | Search, create/edit tasks, move tasks forward/backward between columns |
| Mail | Search, read, star, archive, compose/reply, preserve drafts, add to local Sent folder |
| Code studio | Switch files/tabs, edit text buffers, save and restore session snapshots |
| Inventory | Search, category filter, sort, pagination, create/edit products, validate values and SKU uniqueness |
| Calendar | Previous/next month, select dates, view agenda, add/remove events, validate dates/times |
| Media library | Filter/search, select assets, favorite selections, inspect details |
| Presentations | Thumbnails, previous/next, arrow keys, themes, editable speaker notes, presentation mode/Escape |

The examples use in-memory data. Mail does not transmit email; the code studio
does not execute code or write files; settings do not change the OS. Gallery
assets use the images already bundled in `examples/images/` and CSS artwork.
The calendar starts at October 2026 so its sample events always remain visible.
Presentation mode expands the slide inside the app; it does not use OS fullscreen.

## Implementation

Each application has its own JavaScript module. `app.js` hosts navigation and
delegated events; `core.js` provides shared controls, escaping, dialogs and notices.
`styles.js` contains the shared CSS in a template string and injects a `<style>`
element, since the engine does not load external CSS links. HTML entry files are
small wrappers that select the starting application with `data-start`.

## Validation

Run the workflow checks through Nile's actual DOM and JavaScript runtime:

```sh
cargo run -p nile-shell -- examples/workbench/smoke.html --png /tmp/nile-workbench-smoke.png --width 1280 --height 900
```

The terminal must contain `WORKBENCH CHECKS PASSED: 38`. These checks exercise
the rendered controls, validation, state across tabs/apps, and keyboard navigation.
Look for the completion marker: the shell can still write a PNG after a JS error.

Render any interface for visual inspection:

```sh
cargo run -p nile-shell -- examples/workbench/inventory.html --png /tmp/nile-inventory.png --width 1280 --height 900 --scale 2
```

Reference renders for all nine interfaces are in `docs/workbench/`.
