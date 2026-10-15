# JavaScript, modules and JSX

[Guide index](README.md) · Next: [Controls and menus](controls-and-menus.md)

Nile runs JavaScript with QuickJS. It supplies a DOM bridge, timers, window
commands and optional host functions. It also supplies fetch/XHR, storage,
observers, URL utilities and built-in modules. See [data and services](data-and-services.md)
for persistence, networking features and their limits. Host functions connect
application-specific services to the page.

## Load scripts

```html
<script>
  console.log('Hello from an inline script');
</script>
<script src="app.js"></script>
<script type="module" src="app.jsx"></script>
```

Put scripts after the elements they use, or listen for `DOMContentLoaded`.
Classic script paths resolve relative to the page. Modules import relative to
the importing file:

```js
import { createStore } from './store.js';
```

Relative and absolute filesystem module paths work, together with `@nile`,
`@sciter` (an alias), `@env`, `@sys`, `@debug` and `@storage`. Bare npm package resolution is not implemented; `@sys` is
documented in [packaging](packaging.md#bootstrap-scripts-runjs). An inline module resolves imports from the HTML file's
folder. See [built-in modules](data-and-services.md#built-in-modules).

## Find and update elements

```js
const panel = document.querySelector('#panel');
const buttons = panel.querySelectorAll('button');
panel.classList.toggle('expanded');
panel.style.backgroundColor = '#e0f0ec';
panel.style.setProperty('--accent', '#19796a');
panel.dataset.state = 'ready';
```

Available operations include attributes, `classList`, `style`, `dataset`,
`textContent`, `innerHTML`, `createElement`, `appendChild`, tree edits, `matches`
and `closest`. `querySelectorAll()` returns an array in this implementation, so
array methods such as `.map()` and `.filter()` work directly.

Use `textContent` or created text nodes to display user data. If you generate HTML
strings, escape interpolated text and attribute values before assigning `innerHTML`.
Replacing a container's contents also replaces its controls; avoid doing that on
every keystroke if you want to preserve focus and text selection.

## Handle events with delegation

```js
document.addEventListener('click', (event) => {
  const button = event.target.closest('[data-action]');
  if (!button || button.disabled) return;
  console.log('command:', button.dataset.action);
});
```

Listeners support capture, bubbling and `once`. `preventDefault()` cancels
supported default actions. Keyboard events expose `key`, `ctrlKey`, `metaKey`,
`altKey` and `shiftKey`. Use named keys such as `Enter`, `Escape` and `ArrowDown`.
Synthetic `element.click()` dispatches a click event; use actual engine input
when testing native control default actions such as checking a checkbox.

Nile also provides Sciter-style helpers:

```js
$('#list').on('click.tasks', '.task', function (event, matched) {
  matched.classList.toggle('done');
});
$('#list').off('.tasks');
```

`$()` finds one element, `$$()` finds all. Elements expose `$`/`$$` too. The
delegated handler receives `(event, matched)` and `this` is the element the
listener was attached to. Event names support namespaces and a `^` capture prefix.
`once()` and `off()` are available. See [todo/app.jsx](../../examples/todo/app.jsx)
and [the implementation](../../crates/nile-script/src/prelude.js) for exact behavior.

## Render JSX

```js
const Task = ({ title, done }) => (
  <li class={{ task: true, done }}>
    <span>{title}</span>
  </li>
);

$('#list').content([
  <Task title="Read the guide" done={false} />,
  <Task title="Run the example" done={true} />
]);
```

This snippet assumes `<ul id="list"></ul>` exists. Nile transforms JSX in scripts
and modules without a separate compiler. The default `JSX` function creates
virtual-node arrays, not DOM elements. `append`, `prepend`, `content` and `patch`
materialize or reconcile those values. `content()` replaces children;
`patch(vnode, true)` reconciles children. Function components receive props,
children and a parent argument. Reconciliation is simplified, matching children by key, id, then tag.

### Class components

A class that extends `Element` is a component when used as a JSX tag. Its element is a
`<div>`; the root of what `render()` returns lends its attributes and children to it.

```js
class Counter extends Element {
  count = 0;
  this(props, kids) { this.props = props; }     // after the constructor, and again on each parent render
  render() { return <div class="counter">{this.props.label}: {this.count}</div>; }
  componentDidMount() { this.on("click", () => this.componentUpdate({ count: this.count + 1 })); }
  componentDidUpdate() {}
  componentWillUnmount() {}
}
$("#app").content(<Counter label="Clicks"/>);
```

`componentUpdate({ count: 1 })` merges the fields and re-renders on the next turn (several calls
batch into one render; nothing happens if no value changed; without an argument it always
re-renders). When a parent re-renders with `patch`, matching component elements are reused (by
`key`, else class) and get `this(props, kids)` + `render()` again. `Element.create(jsx)` builds an
element from JSX, `el.post(fn)` runs `fn` after the current event. `examples/components.html`.

A custom control (a combobox, say) is a class component: render an input and a button, keep the text in a field, expose it as `get value`/`set value`, open a list with `popup()` (remove the list element when the popup's `close` fires), and fire `input`/`change` yourself; give the root an ARIA `role` and `aria-expanded` for assistive technology. `examples/combobox.html` is a complete one with filtering and arrow-key navigation. A control is created with `Element.create(<ComboBox .../>)` or as JSX inside `content()`/`append()`; a class can also be bound to a tag from CSS, see below.

### `Element.value` of a form

`<form>` (or any element with `behavior="form"`) reports its named controls as an object:
strings for text, numbers for `number`/`range`, booleans for checkboxes, the chosen value for a
radio group. Assigning an object sets the controls it names.

### Classes bound from CSS

`prototype: ClassName url(file.js)` in a rule makes every matching element an instance of the
class (`export class ClassName extends Element`, from the file, or a class your page scripts
define). Plain markup then gets behaviour with no script on the page:

```css
stepper-box { prototype: StepperBox url(stepper.js) }
```
```js
export class StepperBox extends Element {
  count = 0;                                   // fields run before componentDidMount
  componentDidMount() { this.state.focusable = true; /* build content, read attributes */ }
  componentWillUnmount() {}
  ["on click at button.inc"](event, button) { this.count++; return true; }   // true: handled
  ["on ^keydown"](event) { /* ^ = capture phase */ }
}
```

Handlers are methods named `on <events> [at <selector>]`. Selectors are read from the element:
`>strip>label` means children of it, and `:root` is the element itself, also in
`this.$(...)`. The class loads asynchronously, so `componentDidMount` runs a moment after the page
appears. `examples/prototype/` is a working one. `aspect:` is not supported, and removing the
`prototype` rule from an element doesn't unbind it.

## `@debug` and `@storage`

`import * as debug from "@debug"` offers `setUnhandledExceptionHandler(fn(err))`,
`setConsoleOutputHandler(fn(subsystem, severity, msg))` (return `true` to swallow the line; severity
0 info, 1 warning, 2 error) and `callStackAt(level)`; the breakpoint and inspector functions exist but
do nothing. `import * as DB from "@storage"` (also the global `Storage.open`) gives
`open(path, allowWrite = true)` -> `{ root, commit(), close(), registerClass(cls) }`: the `root`
object graph is saved as JSON on `commit()`/`close()`, with `Date`s and registered class instances
restored. Sciter's own file format, indexes (`createIndex`) and passwords are not supported.

## Timers and measurements

`setTimeout`, `setInterval`, their cancellation methods, promises and
`requestAnimationFrame` are available. Cancel repeating timers when they are
no longer needed.

```js
const scroller = document.getElementById('scroller');
const rect = scroller.getBoundingClientRect();
console.log(rect.width, getComputedStyle(scroller).backgroundColor);
scroller.scrollTo({ top: 100, left: 0 });
```

Layout queries synchronize pending DOM changes before answering. Batch writes
before reads instead of repeatedly changing a style and measuring it in a loop.
`getComputedStyle()` exposes implemented properties, not the full browser set.
`offsetLeft`/`offsetTop` use the parent as their reference; `offsetParent` is not exposed. `devicePixelRatio` reflects the
host's render scale. Element scroll properties and scroll events work; page
scrolling is vertical. `ResizeObserver` and `IntersectionObserver` are available;
intersection `rootMargin` is currently ignored.

## Call application services

```js
if (host.has('saveDocument')) {
  const result = host.saveDocument({ title: 'Draft', text: 'Hello' });
  console.log(result);
}
```

`host.name(...)` is a synchronous JSON bridge to a function registered by the
embedding application. It is not a built-in filesystem or networking API. The
development shell does not register a `saveDocument` service.
See [embedding](embedding.md) for registration and host-to-script calls.

For the member-by-member API, see [DOM/events](../reference/dom.md) and
[built-in modules](../reference/modules.md).
