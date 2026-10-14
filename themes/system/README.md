# Optional system appearance

This package lives outside the engine. Include it in a window when you want
platform-inspired controls and common desktop components. Omitting it leaves
Nile's existing defaults and your application CSS in effect. The package has no
JavaScript, native hooks or extra runtime dependency.

## Load automatically

Copy this directory into your application as `themes/system/`, then put this
inside `<head>` before your own stylesheet:

```html
<include src="themes/system/system.htm"></include>
<link rel="stylesheet" href="app.css">
```

`system.htm` loads `base.css` plus exactly one sheet using Nile's existing
include conditions: Windows → `windows.css`, OSX → `macos.css`, Linux →
`linux.css`. Nested includes resolve relative to the package, so loading does
not depend on the page's directory beyond the first include path.

Copy the whole directory into the folder passed to `nile --pack`; referenced
assets outside that folder are not automatically bundled. Each separate window
or frame document needs its own include. Styles are not shared across documents.

## Force an appearance or load plain CSS

For a platform preview, replace `system.htm` with `windows.htm`, `macos.htm` or
`linux.htm`. These entries each load the shared base and the named sheet.
`NILE_THEME` controls the engine's built-in theme, not this external package's
platform condition. Use the explicit entries to override this package's choice.

You can also load CSS directly when your app/host chooses the platform:

```html
<link rel="stylesheet" href="themes/system/base.css">
<link rel="stylesheet" href="themes/system/windows.css">
<link rel="stylesheet" href="app.css">
```

Include just one entry per document. No JavaScript platform detection is needed.

## Styled elements and states

- Buttons and button/submit/reset inputs: default, primary, hover, pressed,
  keyboard focus and disabled.
- Inputs, textareas and selects: normal, hover, focus, read-only and disabled.
- Checkboxes/radios: unchecked, checked, hover, pressed, focus and disabled.
- File/color/range/number controls: outer spacing and surfaces within the engine's
  supported styling; see the internal-paint limitation below.
- Links, headings, fieldsets, legends, rules, details/summary and tables.
- Engine popup/context-menu surfaces, list items, splitters and inline message boxes.

The stylesheet preserves native appearances and input behavior. It does not
replace controls with scripted imitations. Existing engine themes still provide
base behavior and any visual parts not exposed to CSS.

## Optional components

| Class | Markup / purpose |
|---|---|
| `.primary` | Primary button; normal buttons need no class |
| `.ui-panel` | Bordered surface with padding |
| `.ui-toolbar` | Flex row of command controls |
| `.ui-tabs` | Tab row; child buttons use `aria-selected="true"` or current state |
| `.ui-list` | List; direct li children use `aria-selected="true"` or selected state |
| `.ui-dialog` | Dialog panel surface; application supplies backdrop and focus handling |
| `.ui-menu-item` | Popup command row, with hover/focus/disabled styling |
| `.ui-muted` | Secondary text |

```html
<div class="ui-panel">
  <div class="ui-toolbar">
    <button>Cancel</button>
    <button class="primary">Save</button>
  </div>
  <div class="ui-tabs" role="tablist">
    <button role="tab" aria-selected="true">Overview</button>
    <button role="tab" aria-selected="false">Activity</button>
  </div>
</div>
```

Classes and ARIA state provide appearance/semantics, not behavior. Your app wires
commands, tab panels, list selection and dialogs. Use native `disabled` on buttons
and controls. For custom items, set both semantic attributes and supported runtime
state, and reject disabled activation in your handler. Do not add aria-selected
and assume the engine will implement a listbox interaction model.

## Customize tokens

Load overrides after the theme:

```css
:root {
  --ui-accent: #7951be;
  --ui-accent-hover: #6840ad;
  --ui-accent-pressed: #583493;
  --ui-selected: #eee7fa;
  --ui-focus: #7951be66;
}
```

| Token group | Tokens |
|---|---|
| Typography | `--ui-font` |
| Surfaces | `--ui-bg`, `--ui-surface`, `--ui-soft` |
| Text and border | `--ui-text`, `--ui-muted`, `--ui-border` |
| Interaction | `--ui-hover`, `--ui-pressed` |
| Accent states | `--ui-accent`, `--ui-accent-hover`, `--ui-accent-pressed` |
| Selection/focus | `--ui-selected`, `--ui-focus` |
| Geometry | `--ui-radius`, `--ui-control-y`, `--ui-control-x` |

Tokens inherit through the document. Override them on a subtree to customize a
component. You can also override individual selectors normally; no `!important`
is used in the package. For a separate dark theme, supply all surface/text/state
tokens consistently; live OS dark-mode/contrast settings are not detected here.

## Limits

These are light Windows, macOS and Adwaita-inspired Linux defaults, not native
widgets or a readout of the current OS theme. Linux desktops have many themes;
this package intentionally chooses one visual family. System accent, high
contrast, dark mode and theme changes require app/host information.

Nile's built-in range track/thumb, progress/meter fill, color swatch, spinner
arrows and overlay scrollbars contain renderer-defined visuals. CSS cannot fully
skin those internal parts or their hover states today. The theme styles their
outer boxes where possible. It cannot style OS-native menu bars/file dialogs;
inline dialogs and DOM popups are styleable. Unsupported/custom elements receive
inherited typography but do not gain an OS component appearance automatically.

There is no separate animation or global hover listener: CSS :hover/:active
uses existing engine input/state handling. Enable the package only in windows
that need it; application-owned designs can omit it.

## Demo and validation

```sh
cargo run -p nile-shell -- examples/system-theme/index.html
cargo run -p nile-shell -- examples/system-theme/windows.html
cargo run -p nile-shell -- examples/system-theme/macos.html
cargo run -p nile-shell -- examples/system-theme/linux.html
```

The demo wires tabs, selectable items, popup commands and an inline message box.
Platform preview links open separate native windows. Hover/press/Tab and disabled
controls can be checked without changing application markup. Forced previews
change this package's appearance; engine-painted internals may still reflect
built-in defaults.
