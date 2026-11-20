# Theme capabilities

Audience: Infuser developers.

Cartridge Club is a preview. Nightglass is the default. A theme is still one
presentation of the shared Library DOM and script. This page is what shipped.

## What a theme can ship

`web/themes.js` sets `data-theme` and, once styles have loaded, copies
`--browser-chrome` onto `<meta name="theme-color">`. Nightglass sets that
token to `#0c1017`, which is the value already in `web/index.html`. Cartridge
Club sets `#14071f`. The script does not load behavior, sound, or markup.

`web/index.html` and `web/profiles.html` load `nightglass.css`, then
`clubhouse.css`, then `cartridge-club.css`. On both pages a candidate rule
wins a tie with the shared sheet. Tokens on `:root[data-theme="…"]` override
`:root` on either page.

The server serves theme files from one route, `/assets/themes/{*path}`.
The handler in `src/portal/themes.rs` allows only an exact match from its
list: the two stylesheets, Cartridge Club's Lilita One and Fredoka subsets,
`shelf.svg`, `grain.svg`, both SIL Open Font License files, and `NOTICE.md`.
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
| `web/themes.js` | Copies `--browser-chrome` onto the theme-color meta. |
| `web/themes/nightglass.css` | `--browser-chrome: #0c1017`. |
| `web/themes/cartridge-club.css` | The preview presentation. |
| `web/themes/cartridge-club/` | Fonts, textures, OFL texts, and the notice. |

`web/index.html` and `web/clubhouse.js` are unchanged. Nightglass's token
value for the browser chrome is the color the meta tag already had.

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

`node --test tests/themes.mjs` checks the registry, and checks that a
`--browser-chrome` value is trimmed onto the meta tag.
