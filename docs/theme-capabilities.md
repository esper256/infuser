# Theme capabilities

Audience: Infuser developers.

Funny Pages and Field Notes are previews. Cartridge Club is stable. Nightglass is the default. A
theme is still one presentation of the shared Library DOM and script. This
page is what shipped.

## What a theme can ship

`web/themes.js` sets `data-theme` and, once styles have loaded, copies
`--browser-chrome` onto `<meta name="theme-color">`. Nightglass sets that
token to `#0c1017`, which is the value already in `web/index.html`. Cartridge
Club sets `#14071f`. Funny Pages sets `#f4ecda`. Field Notes sets `#f6f0e4`. The script does not load
behavior, sound, or markup.

`web/index.html` and `web/profiles.html` load `nightglass.css`, then
`clubhouse.css`. `web/themes.js` then inserts only the active candidate
stylesheet, after those two, and only when one is selected. On both pages a
candidate rule wins a tie with the shared sheet by selector specificity.
Tokens on `:root[data-theme="…"]` override `:root` on either page. Candidate
sheets are scoped to different `data-theme` values, so one does not paint
the other. A Nightglass visit does not download the other sheets.

The server serves theme files from one route, `/assets/themes/{*path}`.
The handler in `src/portal/themes.rs` allows only an exact match from its
list: the stylesheets, Cartridge Club's Lilita One and Fredoka subsets,
Funny Pages' Bangers and Comic Neue subsets, Field Notes' Newsreader and
Caveat subsets, each theme's textures, the SIL
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
stylesheet. The shared chat code gained one hook: `--chat-stack` and
`--chat-anchor`, read while the chat window is open. Unset, that hook draws
the same horizontal row and upward triangles as before. The sheet does four
things:

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
- At 960px and wider, Recently opens as an absolute flyout just to the
  right of the masthead, about 360px wide, painted with the masthead so it
  sits over the game grid. The entry list scrolls inside the flyout, and a
  comic tail points back at the Recent button. Narrower widths keep the
  shared panel under the top bar, without that tail.
- At 960px and wider, `--chat-stack: column` and `--chat-anchor: right` stack
  the chatter circles down the masthead and open the chat panel to their
  right. The connector triangles point sideways at the circles whose messages
  are in view. Below 960px the circles stay in the shared horizontal row.
  Message tails inside the bubbles stay on, drawn as ink wedges.
- `#game-detail-content` is one column: the art on top, captions underneath.
  The close button stays the first control and sticks to the top corner.

Field Notes (`[data-theme="field-notes"]`) is the other layout preview. It
does not use the chat-stack hook. `#visible-games` is one column of
horizontal entries: the art is a specimen plate in the first column, and
the title, tags, and Play control sit in the second. The row stays
horizontal at 360px, with a narrower plate. The header stays the shared
top bar. Recently stays the shared dropdown.

`@font-face` for Bangers, Comic Neue, Newsreader, and Caveat is global by
CSS rules. Nightglass and Cartridge Club do not name those families. `forced-colors: active`
drops the theme's images, shadows, and custom fonts. The hover nudge is only under `prefers-reduced-motion:
no-preference`. At 360px and below, filters, search, and Add a game stack,
which matches DOM order.

Shipping the theme still registers it in `web/themes.js` and names its files
in the asset allowlist. The top-level css entry is the stylesheet the client
loads; HTML does not gain a `<link>`. Those edits do not change Nightglass
or Cartridge Club selectors.

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
| `web/themes/cartridge-club.css` | The Cartridge Club theme. |
| `web/themes/cartridge-club/` | Fonts, textures, OFL texts, and the notice. |
| `web/index.html`, `web/profiles.html` | `funny-pages.css` is linked after `cartridge-club.css`. |
| `web/themes/funny-pages.css` | The Funny Pages layout preview. |
| `web/themes/funny-pages/` | Bangers, Comic Neue, halftone textures, OFL texts, and the notice. |
| `src/portal/client.rs` | The Funny Pages sheet is part of the client version hash, with the other theme bytes. |

`web/clubhouse.js` and `web/clubhouse.css` read `--chat-stack` and
`--chat-anchor` and, only when a theme sets them, draw the connector from
that side. Nightglass and Cartridge Club leave both unset, so their row,
panel, and upward triangles stay on the previous path. Neither shared file
gained a Funny Pages selector. Nightglass's browser-chrome token is still
the color the meta tag already had.

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

Candidate sheets load only for the theme that is showing, so a Nightglass
page does not parse another theme's `@font-face` rules. An earlier Chrome
pass, from when every candidate sheet was linked, compared every Nightglass
element's computed style and border box with that sheet present and with
the sheet empty.
The states were the Library, the open profile menu, the open game detail,
and chat open with several people talking, scrolled to the top, the
middle, and the bottom, at 1440×900 and 360×780. Computed style covered
display, grid and flex placement, font, color, background, and box
metrics. Nightglass matched exactly: same elements, same computed values,
same border boxes. No tolerance was used. The pass is not a CI job. The
pixel diff stays out of CI for the reason above.
