# Numbers, dates, currencies and sorting: `Intl`

Nile has `Intl` and the `toLocaleString` family, so business screens can show money, dates and sorted lists the way
the user's region expects. It is written in JS over a small table of locale data (no ICU, which would add megabytes to
every app), so it covers the locales below rather than every language in the world.

```js
(1234567.891).toLocaleString();                                   // in the user's own locale: "1,234,567.891" or "1.234.567,891"
(1234.5).toLocaleString("en-UG", { style: "currency", currency: "UGX" });   // "USh 1,235"
new Intl.NumberFormat("de-DE", { style: "currency", currency: "EUR" }).format(1234.5);   // "1.234,50 €"
new Date().toLocaleDateString("fr-FR", { dateStyle: "full" });    // "samedi 24 octobre 2026"
new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
["item 10", "item 2"].sort((a, b) => a.localeCompare(b, undefined, { numeric: true }));   // item 2, item 10
```

The default locale is the operating system's language and region (`navigator.language`, for example `en-GB`), so
`toLocaleString()` without arguments follows the user's settings.

## What is supported

- **`Intl.NumberFormat`** and `Number.prototype.toLocaleString`: decimal, percent, currency (`currencyDisplay`
  symbol, narrowSymbol, code, name), `minimumIntegerDigits`, minimum/maximum fraction digits, significant digits,
  `useGrouping`, `signDisplay`, `notation: "compact"` (1.2K, 1.2 million), simple `unit` style, `formatToParts`. Rounding is
  half away from zero on the shortest decimal form, so `1.005` gives `1.01`. Indian lakh grouping for `en-IN`/`hi`; locales
  that group only from five digits (`es`, `it`, `pl`).
- **`Intl.DateTimeFormat`**, `Date.prototype.toLocaleDateString` / `toLocaleTimeString` / `toLocaleString`: `dateStyle` and
  `timeStyle`, the component options (`weekday`, `year`, `month`, `day`, `hour`, `minute`, `second`, `hour12`), month and
  day names. In English the clock follows the region (12 hour in the US, 24 in the UK and most of Africa, as in ICU).
- **`Intl.PluralRules`** (cardinal and, for English, ordinal), **`Intl.ListFormat`**, **`Intl.RelativeTimeFormat`**
  (English), **`Intl.Collator`** and `String.prototype.localeCompare` (`numeric`, `sensitivity`, `caseFirst`,
  `ignorePunctuation`: accents sort next to their letters, lowercase before uppercase, digits before letters).
- **Locales with their own data:** `en` (US, GB, AU, CA, IN, ZA, UG, KE, TZ, NG, GH), `fr` (FR, CA, CH), `de` (DE, AT, CH), `es`
  (ES, MX, US), `it`, `pt` (BR, PT), `nl`, `sv`, `ru`, `pl`, `tr`, `sw`, `ja`, `zh`, `ko`, `hi`. Currency symbols follow the
  region (`USh` for UGX in `en-UG`, `₦` for NGN in `en-NG`, `R` for ZAR in `en-ZA`).

## What is not

- Other languages fall back to English month and day names, with their own number separators where known. No Arabic,
  Hebrew, Thai or other non-Latin digit systems, no right-to-left locale data, no language-specific sort orders (Swedish
  å/ä/ö after z, Spanish ñ as a letter).
- **Time zones:** local time and `timeZone: "UTC"` only; named IANA zones are formatted in local time.
- `Intl.Segmenter`, `DisplayNames`, `Locale`, `formatRange` as a real range, era and calendar options other than Gregorian.
- English-only `RelativeTimeFormat`; the narrow no-break space (U+202F) before AM/PM and in French groups follows current CLDR,
  as browsers do now.

Outputs are tested against known ICU results in the engine tests; if a locale you ship needs more, the data is one table
in `crates/nile-script/src/intl.js`.
