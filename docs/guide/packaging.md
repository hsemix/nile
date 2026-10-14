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
  menus) is build-dependent; measure your release executable and packed assets rather than
  treating an earlier measurement as a size guarantee.
- Branding/version resources and Linux desktop entries are described below. A CLI
  option to unpack is not provided.

## An icon and details for the program

```sh
nile --pack my-app -o MyApp --icon icon.png --version 1.2.0 --company "Acme" \
     --description "Does the thing" --copyright "(c) Acme"
```

`--name` sets the display name (default: the output's name). The details go where each system shows them:

- **macOS:** makes `MyApp.app` (Finder and the Dock show the icon): `Contents/MacOS/MyApp` is the plain program,
  `Contents/Resources/app.nilepack` holds your files (a signed executable can't have an archive appended
  without failing strict validation) and the `.png` becomes `AppIcon.icns` through `sips` and `iconutil`
  (or give an `.icns`). The bundle is signed ad hoc; use `codesign` with your identity and notarize it to
  distribute. A `-o Name.app` output gets a bundle even without an icon.
- **Windows:** the `.ico` (or a `.png`) is written into the `.exe` as its application icon, through the
  resource-update API, before the archive is appended, together with a version resource (file description,
  file and product version, company, copyright: Explorer's Properties > Details). Even without `--icon`,
  `--version`/`--company`/... write the version resource. (Written without a Windows machine to hand: CI
  compiles it, but it hasn't run. The version block's layout is checked by a unit test.)
- Without `--icon`, a program gets Nile's own icon (`assets/icon.html`, drawn with Nile itself) where the system shows one: an `.app` bundle on macOS, an `.exe` on Windows, and as the window icon of every Nile window that doesn't set its own (the page's `window-icon`, or the `.png` it was packed with, win).
- `nile --brand program.exe -o out.exe [--icon ...] [--version ...]` writes the same icon and file details into any Windows program; the release workflow uses it for `nile.exe` (Linux archives carry `nile.png` and a `nile.desktop`).
- **macOS:** the same options fill the bundle's `Info.plist` (`CFBundleShortVersionString`, copyright).
- **Linux:** executables have no embedded icon, so `--pack` with `--icon`, or `--category "Utility;"`, also
  writes `MyApp.desktop` and `MyApp.png` next to the program (Exec, Icon, Categories, `StartupWMClass`);
  `--install` copies the entry to `~/.local/share/applications`. The program tells the window system its
  name as its application id, which is how Wayland and GNOME match the window to that entry.
- The icon of the window itself (title bar, taskbar): a packed program uses the `.png` it was packed with,
  or the page's `<html window-icon="icon.png">`, which wins, on every platform.

## Bootstrap scripts (`run.js`)

Like `scapp`, a start file named `run.js` (or `nile run.js`) is a bootstrap script: it runs first, in a hidden
window of its own, and opens the real windows. `examples/bootstrap/` is a working one.

```js
import * as env from "@env";
import * as sys from "@sys";

const main = new Window({ url: __DIR__ + "hello.htm", parameters: { who: "me" }, width: 460, height: 240 });
main.on("close", () => application.quit(0));
application.run();
```

- `__DIR__` and `__FILE__` are the module's folder (with a trailing slash) and file as URLs, `this://app/...` when
  packed. In classic scripts they are the page's.
- `application.quit(code)` ends the program with that exit code. `application.start(gfx)` is accepted and
  ignored (the shell chooses its graphics), and `application.run()` returns 0 at once: the program's own loop is
  already running, so code after it runs immediately, not after the last window closes. Without a `quit`, the
  program ends when the last window the script opened closes.
- `@sys` has the file-system part of Sciter's module: `fs.stat/statSync/lstat`, `readdir/readdirSync`,
  `readFile/readFileSync` (an ArrayBuffer), `unlink`, `rename`, `mkdir`, `rmdir`, `copyfile`, `realpath`,
  `splitpath` and the type constants, with paths or `file://` / `this://app/` URLs (a packed app's files can be
  read, not written). `writeFile`/`writeFileSync` are a Nile addition. Also `fs.open/openSync` (a `File` with
  `read/write/stat/close` and sync forms, flags `r r+ w w+ a a+`, `x`), `fs.opendir`, `sys.TCP` and `sys.UDP`
  (`bind/listen/accept/connect/read/write/send/recv/getsockname/getpeername/shutdown/close`), `sys.spawn`
  (a `Process` with `pid`, `stdin/stdout/stderr` pipes, `wait()`, `kill()`; the `env` option adds to the
  environment), and `hrtime`, `gettimeofday`, `uname`, `isatty`, `environ`, `getenv/setenv/unsetenv`, `cwd`,
  `chdir`, `homedir`, `tmpdir`, `exepath`, `random` (not cryptographic). Blocking calls return promises.
  Not there: named `Pipe`s, `TTY`, `fs.watch`.
- The bootstrap window is not stripped down the way Sciter's is: a bootstrap script can use the DOM and timers.
