# Nile utilities

A static, opt-in utility stylesheet for Nile desktop apps. Use it directly: no
installation, configuration file, JavaScript styling runtime, or build step.
This is a curated Nile vocabulary, not a Tailwind compatibility layer.

```html
<link rel="stylesheet" href="path/to/themes/utilities/nile.css">

<section class="box-border flex flex-col gap-4 p-6 bg-surface text-foreground border rounded-lg">
  <h2 class="m-0 text-lg font-semibold">Appearance</h2>
  <button class="px-4 py-2 border-0 rounded bg-accent text-on-accent focus-visible:outline-accent">
    Apply
  </button>
</section>
```

Nile's include form also works:

```html
<style><include src="path/to/themes/utilities/nile.css"></include></style>
```

Run the [interactive example](../../examples/utilities.html):

```sh
cargo run -p nile-shell -- examples/utilities.html
```

## Vocabulary

The stylesheet is the authoritative list. Classes have ordinary CSS specificity;
put your application stylesheet after it to override equal-specificity rules.
Conflicting classes are resolved by stylesheet order, **not** their order in HTML.

| Group | Classes |
| --- | --- |
| Display | `block`, `inline-block`, `hidden`, `flex`, `inline-flex`, `grid` |
| Sizing model | `box-border` |
| Flex | `flex-row`, `flex-col`, `flex-wrap`, `flex-nowrap`, `flex-1`, `flex-none`, `grow`, `shrink-0` |
| Alignment | `items-start`, `items-center`, `items-end`, `items-stretch`; `justify-start`, `justify-center`, `justify-end`, `justify-between` |
| Grid | `grid-cols-1` through `grid-cols-4` (equal-width columns) |
| Spacing | `p`, `px`, `py`, `pt`, `pb`, `pl`, `pr`, `m`, `mx`, `my`, `mt`, `mb`, `ml`, `mr`, `gap`, followed by `-0`, `-1`, `-2`, `-3`, `-4`, `-5`, `-6`, `-8`, `-10`, `-12`, or `-16` |
| Automatic margins | `mx-auto`, `ml-auto`, `mt-auto` |
| Dimensions | `w-full`, `h-full`, `w-auto`, `h-auto`, `min-w-0`, `min-h-0`, `h-screen`, `min-h-screen` |
| Maximum width | `max-w-sm` (384px), `max-w-md` (448px), `max-w-lg` (512px), `max-w-xl` (576px), `max-w-4xl` (896px) |
| Overflow | `overflow-auto`, `overflow-hidden`, `overflow-y-auto`, `truncate` |
| Position | `relative`, `absolute`, `fixed`, `inset-0` |
| Font | `font-sans`, `font-normal`, `font-medium`, `font-semibold`, `font-bold` |
| Text size | `text-xs` (12px), `text-sm` (14px), `text-base` (16px), `text-lg` (18px), `text-xl` (20px), `text-2xl` (24px), `text-3xl` (30px) |
| Text layout | `text-left`, `text-center`, `text-right`, `leading-tight`, `leading-normal`, `leading-relaxed`, `whitespace-nowrap` |
| Background | `bg-surface`, `bg-subtle`, `bg-accent`, `bg-hover`, `bg-transparent` |
| Text color | `text-foreground`, `text-muted`, `text-accent`, `text-on-accent`, `text-white` |
| Border | `border`, `border-0`, `border-b` |
| Radius | `rounded-none`, `rounded-sm` (2px), `rounded` (4px), `rounded-md` (6px), `rounded-lg` (8px), `rounded-xl` (12px), `rounded-full` |
| Opacity | `opacity-50` |

Spacing numbers multiply by four pixels: `p-3` is 12px, `gap-6` is 24px.
`flex-1` also sets `min-width: 0` so content can shrink inside a row.
`truncate` requires a constrained width. Percentage heights require a definite
parent height. There is no reset: headings, margins and controls retain their
existing styles unless you opt in to utilities.

## Interaction states

These are the complete state variants; prefixes cannot be combined arbitrarily.
Colons are literal in HTML class names.

| Class | Behavior |
| --- | --- |
| `hover:bg-hover` | Hover background, excluding disabled controls |
| `hover:bg-subtle` | Subtle hover background, excluding disabled controls |
| `focus:border-accent` | Accent border while focused; pair with `border` |
| `focus-visible:outline-accent` | Two-pixel accent outline for keyboard focus |
| `disabled:opacity-50` | Half opacity when the control is disabled |

Dark mode uses semantic color tokens automatically; no `dark:` classes are needed.
Set `<meta name="color-scheme" content="light dark">` if your app follows the OS.

## Theme tokens

Override tokens in ordinary CSS **after** the utility stylesheet:

```css
:root {
  --nile-accent: #0067c0;
  --nile-on-accent: #ffffff;
  --nile-border: #d6d6d6;
}
```

`--nile-surface`, `--nile-text`, `--nile-muted`, `--nile-accent`, and
`--nile-on-accent` default to the system colors `Canvas`, `CanvasText`, `GrayText`,
`AccentColor`, and `AccentColorText`. `--nile-subtle`, `--nile-border`, and
`--nile-hover` supply neutral light/dark colors. Override both schemes if a custom
palette needs them. This sheet does not set an application's color scheme.

There are no arbitrary values (`w-[137px]`), generated variants, responsive
prefixes, or negative spacing classes. Use ordinary classes, inline styles, and
media queries for those cases. Dynamically toggled classes work immediately
because every supported utility is always present in the CSS file.

Native-style components remain a separate concern. This sheet provides layout
and presentation primitives; it does not change control behavior or replace the
optional [system theme](../system/README.md). When combining them, load the
system theme first and these utilities afterward.
