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

## Nightglass pixel diff

`node tests/nightglass_pixels.mjs` renders the real Library in one Chrome
with reduced motion, at 1440×1180, 768×1100, 390×980, and 320×780. For the
grid, the Firefly Drift detail, Recently, and the profile menu it requires:

1. Two settled Nightglass frames are identical.
2. Disabling Cartridge Club's rules, while the same `@font-face` rules stay
   registered, does not change a pixel.

Registering a font face makes Chrome repaint existing glyphs once, even when
no element uses the new family. The check waits until that repaint is over,
then compares. It has no retry and no tolerance. CI runs it as the
Nightglass pixels job. PR checks are advisory and use `continue-on-error`;
read that job's own conclusion.

`node --test tests/themes.mjs` still checks the registry, and checks that a
`--browser-chrome` value is trimmed onto the meta tag.
