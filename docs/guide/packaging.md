# Shipping an app as one file

`nile` can pack a folder of HTML, CSS, JavaScript and images into a copy of itself. The result is one
executable with no loose files: copy it anywhere and it runs. This is the same idea as Sciter's `scapp`
assembly mode.

```sh
nile --pack my-app -o MyApp          # MyApp.exe on Windows
./MyApp
```

## What gets packed

Every file under the folder, deflated when that makes it smaller (hidden files, `node_modules` and
`target` are skipped). At startup the program finds the archive behind its own executable and serves
reads from it: the page itself, `<include>`, `<frame>`/`<iframe>`, images and `url()`, scripts and
`import`ed modules, and `fetch()`/`XMLHttpRequest` of relative paths. Scripts see the archive as
`this://app/`, so `location.href` is `this://app/index.html`, and `fetch("this://app/data.json")` works.

The app starts from the first of `main.html`, `index.html` (the `.htm` spellings and `scapp.html` too) in the
pack. The `window-*` attributes on `<html>` still size and style the window.

## Without packing

`nile` with no arguments looks for the same start files next to the program, then in the current folder,
and `nile folder` opens that folder's start file. So you can also ship `nile`, `index.html` and the rest side by
side.

## Good to know

- `localStorage` of a packed app is kept per executable name in the user's data folder, like any page.
- On macOS the packed binary is signed again with an ad-hoc signature so it starts on Apple silicon; sign it
  with your own identity (and notarize it) before distributing.
- The archive is read-only; `Window.this.selectFile` and the `@sys` module still use the real disk.
- Size: the packed app is the `nile` shell plus your files. The shell (with window, dialogs, accessibility and
  menus) is about 9.5 MB in a release build.
- Not here yet: the Sciter `run.js` bootstrap mode (`application.start`/`run`), icons and version resources for
  the executable, and a CLI option to unpack.
