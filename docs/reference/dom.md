# DOM, events and components

[Reference index](README.md) · [Scripting tutorial](../guide/scripting.md)

## HTML and scripts

Semantic blocks, headings, paragraphs, lists, links, tables and form markup are
parsed into the document. Unknown tags can be styled as custom containers; they
do not acquire native behavior. Canvas, SVG image rendering, audio/video playback
and browser page navigation are not built-in features. Use supported raster
assets and host services for those application capabilities.

### Scripts and documents

Classic inline/external scripts and `<script type="module">` run in QuickJS.
Relative modules resolve against the importing file. Absolute filesystem paths,
packed URLs and built-in module names work; npm package-name resolution does not.
Put DOM-dependent scripts after their markup or listen for `DOMContentLoaded`.
The runtime also emits `load` and exposes `document.readyState`.

| Document member | Usage/return |
|---|---|
| `documentElement`, `head`, `body`, `defaultView` | Root elements and global window |
| `title` | Read/write page title; shell updates the native caption |
| `activeElement` | Focused element, with the implementation's body fallback |
| `getElementById(id)`, `querySelector(selector)` | Element or `null` |
| `querySelectorAll(selector)` | Array of element wrappers, not a live NodeList |
| `createElement(tag)`, `createTextNode(text)` | Detached nodes |
| `createDocumentFragment()`, `createRange()` | Partial fragment/range APIs; see [editing](editing.md) |
| `getSelection()`, `execCommand(...)`, `queryCommand*()` | [Editing API](editing.md) |
| `fullscreenElement`, `exitFullscreen()` | Native window fullscreen state/command, not independent element fullscreen |

`window`, `self` and `globalThis` refer to the script global. Frames have separate
contexts. DOM wrappers preserve identity by native node ID.

## Nodes and traversal

| Members | Behavior |
|---|---|
| `nodeType`, `nodeName` | Node kind/name; elements also expose `tagName` and `localName` |
| `parentNode`, `parentElement` | Parent or `null` |
| `childNodes`, `children`, `childElementCount` | Arrays/count; `children` filters elements |
| `firstChild`, `lastChild`, `nextSibling`, `previousSibling` | Node traversal |
| `firstElementChild`, `lastElementChild`, `nextElementSibling`, `previousElementSibling` | Element traversal |
| `isConnected`, `contains(node)` | Attachment/ancestry |
| `textContent` | Read/write text; setting replaces children |
| `appendChild(node)`, `insertBefore(node, reference)` | Insert/move a node; null reference appends |
| `removeChild(node)`, `remove()` | Detach a node |
| `insertAfter(node, reference)` | Nile helper; inserts after reference inside the receiving parent |
| `elementIndex`, `nodeIndex` | Index among element siblings or all nodes |
| Text `data`, `nodeValue`, `splitText(offset)` | Text editing helpers |
| `cloneNode(deep)` | Clones supported nodes/attributes; does not copy event subscriptions |

```js
const row = document.createElement('div');
row.className = 'row';
row.textContent = 'A product name';
document.getElementById('list').appendChild(row);
```

Use `textContent` for external text. Replacing `innerHTML` removes the old
controls and can lose focus, selection and editing history. Detached arena
nodes are retained by the current DOM implementation; avoid endlessly replacing
large trees in long-running applications.

## Attributes and styles

| Members | Usage |
|---|---|
| `id`, `className` | Attribute-backed strings |
| `attributes` | Array-like name/value wrappers, getNamedItem and named lookup; partial NamedNodeMap |
| `getAttribute`, `setAttribute`, `removeAttribute`, `hasAttribute`, `toggleAttribute`, `getAttributeNames` | Ordinary attribute operations |
| `classList` | `add`, `remove`, `toggle(token, force)`, `contains`, `item`, `length`, string conversion |
| `dataset` | Proxy for `data-*` attributes using camelCase keys |
| `style` | Inline declarations: camelCase fields, `getPropertyValue`, `setProperty`, `removeProperty`, `cssText` |
| `innerHTML`, `outerHTML` | Serialize/replace child markup; outerHTML currently read-only |
| `contentEditable`, `isContentEditable` | Attribute/inherited editor state |
| `matches`, `closest`, element `querySelector(All)` | Supported CSS selector matching |
| `state` | Runtime booleans including checked/disabled/readonly/selected/current/expanded/collapsed/invalid/busy; interaction flags follow input |

```js
const panel = document.getElementById('panel');
panel.style.setProperty('--accent', '#7951be');
panel.dataset.state = 'ready';
panel.classList.toggle('active', true);
```

`getComputedStyle(element)` supplies implemented properties; it is not a
complete CSSOM. Unsupported queries can return empty strings.

## Events

`EventTarget` supports `addEventListener(type, fn, options)`,
`removeEventListener` and `dispatchEvent`. Options include `capture` and `once`.
Listeners run in capture → target → bubble order when applicable. `on*`
properties and supported inline event attributes are also accepted.

| Event object | Fields/operations |
|---|---|
| `Event` | `type`, `target`, `currentTarget`, `eventPhase`, `bubbles`, `cancelable`, `defaultPrevented`, `timeStamp` |
| Cancellation | `preventDefault`, `stopPropagation`, `stopImmediatePropagation` |
| `MouseEvent` | `clientX/Y`, `screenX/Y`, `button/buttons`, modifier flags and related fields from supported input |
| `KeyboardEvent` | `key`, `code`, modifier flags and repeat-related fields where supplied |
| `CustomEvent` | `detail` payload |
| `FocusEvent` / `UIEvent` | Event subclasses; fields are a partial browser implementation |

```js
document.addEventListener('mousedown', event => {
  if (event.target.closest('.ribbon button')) event.preventDefault();
}, true);
document.addEventListener('click', event => {
  const button = event.target.closest('[data-action]');
  if (button && !button.disabled) runAction(button.dataset.action);
});
```

The first listener uses **event-phase capture**; that is not pointer capture.
`setPointerCapture` and a complete PointerEvent API are not supplied.
`element.click()` dispatches a synthetic event; it does not replay the native
pointer/default-action pipeline. Test controls with actual shell input.

Native input supplies mouse movement/down/up/click, keyboard down/up and focus
changes. A `dblclick` constructor/event name exists, but ordinary native
application clicks do not currently dispatch `dblclick`; the file explorer
recognizes two close clicks in JS. Editor double/triple-click selection is a
separate engine behavior. A browser-style cancelable wheel event is not the
interface used by the shell; scrolling is driven by host wheel input.

Sciter-style `on`, `once`, `off`, `$`, `$$` are documented under
[built-in modules](modules.md#nile-and-sciter). Namespaced delegation survives
dynamic child creation.

## Controls and form values

| Control | Supported interaction/data | Limits |
|---|---|---|
| Text/password inputs | Value, placeholder, click caret, selection, typing, clipboard, undo/redo | Password display masks characters |
| Textarea | Multiline editing, wrapping, vertical movement, selection, caret scrolling | Home/End use hard line boundaries |
| Checkbox/radio | Checked state, groups, labels and input/change events | Use native input for default-action tests |
| Button | Pointer/Enter/Space activation, disabled state | Semantic roles on arbitrary elements do not create button behavior |
| Select/option/optgroup | Dropdown/listbox, headings, disabled choices, selectedIndex/selectedOptions/options/value | Native dropdown/listbox look is engine-drawn; list focus behavior is partial |
| Multiple select | Ctrl/Cmd toggles, Shift extends, pointer drag, keyboard and wheel | Not a full browser select implementation |
| Range | Drag track/thumb, arrows/Home/End, min/max/step | Value property remains attribute-backed |
| Number | Stepper buttons, numeric entry, arrows, min/max/step | No automatic form validation or `valueAsNumber` |
| Color | Swatch and normalized editable hex | No native color picker |
| Date/time/month/datetime-local | Digit/segment editing, keyboard steps, limits | No calendar/time popup |
| File | Native chooser, multiple/accept, `files` array with File objects | Host must answer requests; no drag/drop directory upload |
| Details/summary | Disclosure and `toggle` | Custom content should be wrapped for predictable visibility |
| Progress/meter | Range/value painting and accessibility | No application progress logic is implied |

Relevant properties include `value`, `checked`, `disabled`, `readOnly`, `type`,
`name`, `multiple`, `selected`, `selectedIndex`, `selectedOptions`, `options`,
`rowSpan`, `colSpan`, `cellIndex`. Form/behavior-form `.value` returns a named
control object; text values are strings, number/range values numbers, checkboxes
booleans and radio groups a selected value. Assign an object to populate named
controls. This is distinct from browser form submission: native submit and
constraint validation are not implemented. `FormData(form)` is available.

Text controls expose `selectionStart`, `selectionEnd`, `setSelectionRange` and
`select()`. Indices count Unicode characters, not UTF-16 code units. Their
selection is separate from document Selection/Range.

Call `focus()`/`blur()` and use `tabindex` for keyboard access. Tab/Shift+Tab
follow focus order. IME preedit/commit works in text inputs and contenteditable.
See [controls guide](../guide/controls-and-menus.md), [inputs](../../examples/inputs.html),
[listbox](../../examples/listbox.html), [IME](../../examples/ime.html).

## Measurements and scrolling

| API | Behavior |
|---|---|
| `getBoundingClientRect()` | Logical viewport coordinates, including supported transforms/scrolling |
| `offsetWidth/Height`, `offsetLeft/Top` | Rounded measurements; offsets are relative to parent; offsetParent is not exposed |
| `clientWidth/Height` | Inner dimensions excluding borders |
| `scrollTop/Left` | Read/write offsets clamped to element's range |
| `scrollWidth/Height` | Content extents |
| `scrollTo(x,y)` / `scrollTo({left,top})` | Immediate element scrolling |
| `scrollBy(...)` | Relative immediate scrolling |
| `scrollIntoView()` | Partial page-scrolling helper; browser alignment options/nested scrolling are not implemented |
| `scroll` | Notification when scroll changes |
| `innerWidth/Height`, `devicePixelRatio` | Host viewport and render scale |
| Global `scrollY`, `pageYOffset`, `scrollTo/By` | Vertical page scrolling; root horizontal scrolling is not supplied |
| `pageCount`, `pageNumber` | Paged-view metrics; see [printing](../guide/printing.md) |

There is no browser smooth-scroll behavior implied by passing
`{ behavior:'smooth' }`. Reading geometry synchronizes pending changes; batch
writes before reads. See [performance](performance.md).

## JSX and components

JSX is transformed by Nile in ordinary scripts and modules; no npm transpiler is
required. The default `JSX(tag, attrs, children)` creates vnode arrays. `append`,
`prepend`, `content` and `patch(vnode, onlyChildren)` materialize/reconcile them.
Function components receive `(props, children, parent)`.

Class components extend `Element` and implement `this(props,kids)`, `render`,
`componentDidMount`, `componentDidUpdate`, `componentWillUnmount` as needed.
`componentUpdate(fields)` merges fields and schedules a render; updates batch.
`Element.create(vnode)` builds a component/element; `post(fn, avoidDuplicates)`
schedules work. Matching uses key/id/tag and class-specific reuse; it is not a
React implementation. See the [component tutorial](../guide/scripting.md#class-components)
and [components example](../../examples/components.html).

## Timers and host calls

`setTimeout`, `setInterval`, `clearTimeout`, `clearInterval`,
`requestAnimationFrame`, `cancelAnimationFrame`, `queueMicrotask`, promises,
`performance.now()` and console logging are supplied. Animation-frame callbacks
are scheduled at approximately 16 ms through the tick loop, not synchronized to
an OS compositor. Custom hosts must keep ticking even when no input occurs.

`host.has(name)` checks registered JSON services; `host.name(...args)` calls a
service synchronously. Host-to-script calls use `Page::eval/call` or the C API.
See [native integration](native-api.md). No SQLite binding is built into the DOM.
