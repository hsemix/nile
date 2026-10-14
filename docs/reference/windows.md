# Windows, menus and dialogs

[Reference index](README.md) · [Controls tutorial](../guide/controls-and-menus.md)

Native operations are implemented by the shell or your embedding host. Rendering
a headless PNG does not test OS ownership, focus, dialogs or menu integration.

## Root window attributes

Put window attributes on `<html>`:

```html
<html window-title="Inventory" window-width="1100" window-height="760"
      window-min-width="800" window-min-height="600" window-alignment="5">
```

| Attribute | Values/behavior |
|---|---|
| `window-title` | Caption; falls back to `<title>` |
| `window-width`, `window-height` | Pixels/bare numbers, or percentage of desktop |
| `window-min-width/height`, `window-max-width/height` | Pixel constraints |
| `window-resizable` | Boolean, or edge-grab margin such as `6px` |
| `window-minimizable`, `window-maximizable` | Boolean flags |
| `window-state` | shown, minimized, maximized, full-screen, hidden |
| `window-alignment` | Numeric-keypad 1–9; 5 center, 7 top-left |
| `window-frame` | default/standard, extended, solid/solid-with-shadow, transparent |
| `window-icon` | Supported image resource for window icon |

Extended titlebar content is platform-dependent. Solid frames require your own
chrome; solid-with-shadow does not add a native shadow. Desktop alpha depends
on the surface and is not supplied by the software shell.

Chrome roles: `window-caption` drags (double-click maximizes),
`window-minimize`, `window-maximize`, `window-close` invoke native actions.
These special roles have engine behavior; ordinary ARIA roles do not.
Global moveTo/moveBy/resizeTo/resizeBy, close, screenX/Y, outerWidth/Height and
requestFullscreen/exitFullscreen expose host commands/metrics.

## Multiple windows

```js
const child = new Window({
  url:'windows/child.html', width:640, height:480,
  caption:'Product details', parameters:{productId:42}
});
child.on('close', event => console.log('Closed:', event.value));
const answer = await Window.this.modal({url:'windows/dialog.html',width:420,height:280});
```

The URLs in this snippet are relative to the opening page. Substitute your own
child pages or run [windows.html](../../examples/windows.html).

| Constructor parameter | Contract |
|---|---|
| `url` | Relative/page URL of document |
| `html` | Inline markup alternative |
| `caption` | Window title |
| `x`, `y`, `width`, `height` | Native placement/size requests |
| `alignment` | 1–9 screen positioning; negative keypad values request positioning relative to opener |
| `type` | frame/default, dialog, tool, popup, child; constants available on Window |
| `state` | shown/minimized/maximized/hidden/fullscreen, or state constants |
| `parent` | Explicit owner handle where supported; opener remains the script relationship |
| `parameters` | JSON-serializable data available in child |
| `screen`, `client` | Accepted construction fields; platform placement semantics are partial |

| Window member | Behavior |
|---|---|
| `Window.this` | This window's handle |
| `Window.all` | This context's known handles, not a complete OS/process registry |
| `Window.share` | Process-shared JSON key/value proxy |
| `parent`, `parameters` | Opener handle / construction data |
| `close(value)` | Request close; close value resolves opener's promise |
| `closed` | Promise for an opened handle's closure |
| `focus()` | Native focus request |
| `postMessage(data)` | JSON message to target; receive on global `message` event |
| `isValid`, `isActive` | This-window state; another window's isActive currently approximates validity |
| `caption` | This window read/write title; another handle's setter is not implemented |
| `on/off`, add/removeEventListener | Window-handle event subscriptions |

`closerequest` on Window.this is cancelable; `activate` reports focus changes;
`close` on an opened handle reports its value. `modal({ ... })` returns a Promise
and blocks input to the opener until closure; it cannot retrofit an existing
Window into a modal one. Use `await`, not a synchronous wait loop.

```js
addEventListener('message', event => console.log(event.data));
Window.this.parent?.postMessage({type:'saved',productId:42});
Window.this.on('closerequest', event => {
  if (hasUnsavedChanges()) event.preventDefault();
});
```

Messages/share/parameters cross as JSON: no DOM objects or functions. Dialog,
tool and popup ownership keeps windows above owners on macOS/Windows; Linux
ownership is limited. Explicit owner versus script opener has distinct semantics.
Do not assume that every child gets an independent close-veto opportunity when
its parent closes. Closing the main window exits the shell.

C hosts process the window request queue and create/notify child views; see
[native integration](native-api.md). No OS windows are created automatically by
the C engine library.

## File and message dialogs

| API | Result |
|---|---|
| `Window.this.selectFile({mode:'open',filter,caption,path})` | File URL or null |
| `selectFile({mode:'open-multiple',...})` | File URL array or null |
| `selectFile({mode:'save',extension,...})` | Destination URL or null; does not write a file |
| `selectFolder({caption,path})` | Folder URL or null |
| `modal(<info/alert/error/question>...</...>)` | Native answer string, or Promise for inline mode |
| `Window.this.messageBoxes='inline'` | Default to application-drawn boxes |

Filter strings use `Label (*.txt)|*.txt|All files (*.*)|*.*` style syntax.
Use `await` for message boxes to work with either mode:

```js
const answer = await Window.this.modal(
  <question inline caption="Unsaved changes">
    Save your work?
    <button value="save" default>Save</button>
    <button value="discard">Discard</button>
    <button value="cancel" cancel>Cancel</button>
  </question>
);
```

Inline dialogs are styleable HTML with focus/keyboard handling. `native` forces
the system box. Native info/alert/error normalize to yes; questions to yes/no.
An embedder needs a Dialogs implementation; absence can produce fallback answers,
so do not interpret API existence as evidence of an actual user interaction.
See [dialogs](../../examples/dialogs.html).

## Popup menus and detached popups

```js
const anchor = document.getElementById('open');
const menu = document.createElement('popup');
menu.innerHTML = '<button>Open document</button>';
const handle = anchor.popup(menu, {anchorAt:1,popupAt:7});
handle.on('close', () => menu.remove());
```

`element.popup(popupElement, options)` lifts a DOM element into a positioned
popup; the returned PopupWindow supplies root/anchor/isValid, close and event
subscriptions. Anchor/popup reference points use numeric-keypad positions.
Options include explicit x/y, anchorAt/popupAt and supported animation fields.
`popup-position`, `popup-anchor-reference-point`, `popup-reference-point` and
`popup-animation` CSS provide defaults. Outside-click/Escape dismissal is
handled; command choice, submenu navigation and menu semantics remain JS work.

`takeOff(options)` detaches/moves an element with its own popup handle.
`isMousePassthrough` writes pointer-events, whose cascade behavior is not implemented; resizable/topmost flags are partial
compatibility properties, not a general-purpose native window manager.

Pass `{window:'popup'}` or set `Window.this.popupWindows=true` to use an OS popup
portal. It remains an element of the owner page, painted into a separate native
surface. Do not treat it as a child document. Portals do not automatically close
when another application is clicked and are not fully monitor-clamped.

`contextmenu` can set `event.source` to a menu element, or call popup at the
pointer coordinates. `title`/`tooltip` attributes supply delayed tooltips;
`tooltiprequest` can customize them. Tooltips stay in their host window.
Examples: [popups](../../examples/popups.html), [menubar](../../examples/menubar.html).

## Native application menus

Assign a hidden `<menu>` tree to `Window.this.menu`. Native menu bars are
supported on macOS/Windows; Linux needs an application-drawn menu.

```html
<menu id="app" style="display:none">
  <li>File<menu>
    <li id="open" accesskey="CmdOrCtrl+O">Open</li>
    <hr><li role="quit">Quit</li>
  </menu></li>
</menu>
<script>Window.this.menu = document.getElementById('app');</script>
```

Nested menus, separators, label overrides, checked/disabled/hidden state and
keyboard accelerators are supported. Choice dispatches click on the item;
standard roles undo/redo/cut/copy/paste/selectall and window quit/close/minimize/zoom
invoke host behavior. `Window.this.menu=null` removes the menu.
See the [menu markup/accelerator reference](../guide/controls-and-menus.md#the-system-menu-bar-macos-and-windows)
and [native-menu example](../../examples/native-menu.html).
