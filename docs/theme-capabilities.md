# Theme capabilities

Audience: Infuser developers.

Cartridge Club and Funny Pages are previews. Nightglass is the default. A
theme is still one presentation of the shared Library DOM and script. This
page is what shipped.

## What a theme can ship

`web/themes.js` sets `data-theme` and, once styles have loaded, copies
`--browser-chrome` onto `<meta name="theme-color">`. Nightglass sets that
token to `#0c1017`, which is the value already in `web/index.html`. Cartridge
Club sets `#14071f`. Funny Pages sets `#f4ecda`. The script does not load
behavior, sound, or markup.

`web/index.html` and `web/profiles.html` load `nightglass.css`, then
`clubhouse.css`, then `cartridge-club.css`, then `funny-pages.css`. On both
pages a candidate rule wins a tie with the shared sheet. Tokens on
`:root[data-theme="…"]` override `:root` on either page. The two candidate
sheets are scoped to different `data-theme` values, so neither one paints
the other.

The server serves theme files from one route, `/assets/themes/{*path}`.
The handler in `src/portal/themes.rs` allows only an exact match from its
list: the three stylesheets, Cartridge Club's Lilita One and Fredoka subsets,
Funny Pages' Bangers and Comic Neue subsets, each theme's textures, the SIL
Open Font License files, and each `NOTICE.md`.
A path with `..`, a backslash, an extra slash, or an extension outside
css, woff2, svg, webp, png, txt, and md is rejected. Image, font, and text
responses send `Content-Security-Policy: default-src 'none'`. Stylesheet
responses stay `text/css`, as before. Nothing is read from disk at runtime
and nothing is uploaded. There is no `include_dir` dependency.

`web/clubhouse.css` still has its hardcoded colors (216 hex values, 364
`var(--token)` uses). A theme that needs a surface whose color is a literal
names that surface in its own sheet. Moving those literals onto tokens is
left for later.

Cartridge Club's sheet is scoped to `[data-theme="cartridge-club"]`.
`@font-face` is global by CSS rules; Nightglass does not use Lilita One or
Fredoka. The sheet sets a plastic shell, a cream label around the existing
art, gold contact pins, a console header, a memory-card Recently panel, and
a pause-menu profile. `--game-ratio` is 1.05. At 360px and below, filters,
search, and Add a game stack, which matches DOM order. Wider widths keep
the shared row. Hover lift is only under `prefers-reduced-motion:
no-preference`. `forced-colors: active` drops the theme's images, shadows,
and custom fonts. Generated content stays empty. The online marker is left
on the shared `::after`.

## Layout

Funny Pages (`[data-theme="funny-pages"]`) changes layout from its own
stylesheet. `web/clubhouse.css`, `web/clubhouse.js`, and the shared markup
did not gain wrappers or layout hooks. The sheet does four things:

- At 960px and wider, while the Library is showing, `body` is a two-column
  grid. The header is column one: a sticky masthead, one viewport tall.
  `main` stays in column two. Below 960px the header is the shared top bar
  again, so a 360px phone does not keep a side column.
- `#visible-games` is a grid measured by a container query on `main`, so the
  masthead's width is not part of the card math. The first game is the
  splash: two columns and two rows when the library column is at least
  880px, the full width from 680px to 879px, and a taller single panel
  below that. In the wide query, cards `4n+5` and `4n+6` span two tracks.
  Auto-placement stays row-wise, so the next card is to the right or on the
  next row. That is the same order as Tab.
- On the wide masthead, `#activity` is `position: static`. Opening Recently
  inserts speech bubbles between the Recent button and the wordmark, which
  is their DOM order inside `.brand`. Narrow widths keep the shared
  absolute panel.
- `#game-detail-content` is one column: the art on top, captions underneath.
  The close button stays the first control and sticks to the top corner.

`@font-face` for Bangers and Comic Neue is global by CSS rules. Nightglass
and Cartridge Club do not name those families. `forced-colors: active`
drops the theme's images, shadows, and custom fonts. The bubble tail is an
empty `::before`. The hover nudge is only under `prefers-reduced-motion:
no-preference`. At 360px and below, filters, search, and Add a game stack,
which matches DOM order.

Shipping the theme still registers it, links `funny-pages.css` after the
other sheets, and names its files in the asset allowlist. Those edits do
not change Nightglass or Cartridge Club selectors.

The game player page, `desktop.html`, and the native Play app do not load
this system. Sound is not part of it. A manifest for those would be a later
change.

## Base files

| File | What changed |
| --- | --- |
| `src/portal/themes.rs` | New allowlisted asset handler and fingerprint. |
| `src/portal.rs` | `mod themes` and one route in place of the two theme CSS routes. |
| `src/portal/client.rs` | Theme asset bytes are part of the client version hash. |
| `web/profiles.html` | `clubhouse.css` loads before `cartridge-club.css`. |
| `web/clubhouse.css` | The 380px header rule no longer names Cartridge Club. |
| `web/themes.js` | Copies `--browser-chrome` onto the theme-color meta. Funny Pages is a `preview` entry. Nightglass stays `defaultId`. |
| `web/themes/nightglass.css` | `--browser-chrome: #0c1017`. |
| `web/themes/cartridge-club.css` | The Cartridge Club preview. |
| `web/themes/cartridge-club/` | Fonts, textures, OFL texts, and the notice. |
| `web/index.html`, `web/profiles.html` | `funny-pages.css` is linked after `cartridge-club.css`. |
| `web/themes/funny-pages.css` | The Funny Pages layout preview. |
| `web/themes/funny-pages/` | Bangers, Comic Neue, halftone textures, OFL texts, and the notice. |
| `src/portal/client.rs` | The Funny Pages sheet is part of the client version hash, with the other theme bytes. |

`web/clubhouse.js`, `web/clubhouse.css`, and `web/themes/nightglass.css` have
no Funny Pages edits. Nightglass's browser-chrome token is still the color
the meta tag already had.

## Nightglass pixels

Two settled frames of one Nightglass page match exactly, in the system
Chrome used for the mockups and in Puppeteer's Chrome. That only shows the
page is still. It does not show that the candidate sheet is invisible.

Comparing Nightglass with the full Cartridge Club sheet against Nightglass
with only that sheet's `@font-face` rules is not deterministic. On Puppeteer
24's Chrome, two loads of the identical full stylesheet already differed by
154 to 760 pixels on the grid and the detail dialog, while the profile menu
and Recently matched. Disabling the live stylesheet in one page, after
copying the same `@font-face` rules into another sheet, left geometry and
computed fonts unchanged and still moved hundreds of pixels along glyph
edges. The count was not stable across runs (a phone detail was 0, then
494). Chrome repaints the font atlas when a face is registered or when a
stylesheet that contains `@font-face` is removed. Those repaints are not a
layout or color change, and they are not bit-exact.

A CI job that failed on any difference would fail on that rasterizer noise.
A retry or a pixel tolerance would hide a real leak as well as the noise.
The check is not in CI for that reason.

`node --test tests/themes.mjs` checks the registry, checks that a
`--browser-chrome` value is trimmed onto the meta tag, and checks that
every Funny Pages rule is scoped to `[data-theme="funny-pages"]` (with
`@font-face` and conditional blocks allowed).

## Checking the other themes

Adding Funny Pages links another stylesheet, so Nightglass and Cartridge
Club pages load its `@font-face` rules even though they never name Bangers
or Comic Neue. A one-off Chrome pass compared every element's computed
style and border box with that sheet present and with the sheet empty.
The states were the Library, the open profile menu, and the open game
detail, at 1440×900 and 360×780. Computed style covered display, grid and
flex placement, font, color, background, and box metrics. After Cartridge
Club's own Fredoka and Lilita One faces had loaded, both themes matched
exactly: same elements, same computed values, same border boxes. No
tolerance was used. The pass is not a CI job. The pixel diff stays out of
CI for the reason above.
