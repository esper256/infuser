# Theme capabilities

Audience: Infuser developers. This is a design note for review. The
screenshots in the pull request were rendered from the real Library
(`web/index.html`, `web/clubhouse.css`, `web/clubhouse.js`) with a throwaway
stylesheet. Nothing here is registered, served, or turned on. Nightglass
stays the default, and Cartridge Club stays a preview.

## What is true today

A theme is one stylesheet. `web/themes.js` sets `data-theme` on the document
element and persists `infuser.theme`. It does not load logic, sound, or
markup. Unknown and retired ids fall back to Nightglass. `tests/themes.mjs`
covers that registry.

`web/index.html` loads, in order, `themes.js`, `nightglass.css`,
`clubhouse.css`, then `cartridge-club.css`. `web/profiles.html` loads
`themes.js`, `nightglass.css`, `cartridge-club.css`, then `clubhouse.css`.
On the profile picker, a Cartridge Club rule loses a tie to `clubhouse.css`.
On the Library, the same rule wins. Custom properties set on
`:root[data-theme="cartridge-club"]` win on both pages, because that
selector is more specific than `:root`.

`web/desktop.html` loads `nightglass.css` and `clubhouse.css` only. It never
runs `themes.js`, so it never selects Cartridge Club. `web/game-player.html`
loads `player.css` only. The native Play app in `desktop/src/ui.rs` is egui,
with Noto Sans and its own color constants. Those three surfaces do not
participate in Library themes.

The Library and profile pages send
`default-src 'self'; img-src 'self' data:; style-src 'self'; script-src 'self'; connect-src 'self'`.
`font-src` is unset, so it falls back to `default-src 'self'`. A same-origin
font file and a same-origin image are allowed. A `data:` image is allowed.
A `data:` font is blocked. An external CDN is blocked. The player page is
stricter: `default-src 'none'` and no `img-src` or `font-src`, so a theme
image or font there is blocked until that policy grows.

Every Library asset is a hand-written route with `include_bytes!` or
`include_str!`. `src/portal.rs` has one route each for `nightglass.css` and
`cartridge-club.css`. There is no directory route. A theme stylesheet can
name `url(/assets/...)`, and the request 404s until someone adds a route.
The hypothesis holds: a theme cannot ship its own files today. The security
policy would allow those files once they are same-origin.

`web/themes/cartridge-club.css` is 76 lines. It already changes toolbar
placement, gaps, radii, and shadows, so it is more than a recolor, and it
is still mostly a recolor. It has no font, texture, or image of its own.

`web/clubhouse.css` (898 lines) contains 216 hex colors, 168 of them unique,
and 364 `var(--token)` uses. 185 declarations contain a hex color. One
declaration uses `rgb()`. A token override of `--page` or `--ink` does not
recolor a surface whose color is a literal. Cartridge Club has to name that
surface itself. The hypothesis is right, and the counts match this tree.

A theme can restyle elements that already exist. It can also use
`::backdrop`, `::placeholder`, `::marker`, `::selection`, multiple
backgrounds, `border-image`, `mask`, `clip-path`, `filter`, and grid
placement. It is not limited to `::before` and `::after`. The practical
limit is sharper than that: each element has one `::before` and one
`::after`, and the base already spends several of them.

- `.game::before` is the card scrim.
- Missing-art `.game-art::before` and `::after` are the fallback loops.
- `.detail-art.has-game-art::after` is the edge fade.
- `#recent-toggle::before` is the Recently halo.
- `.profile-menu > summary::after` is the pointed online marker.

A theme that sets the same pseudo replaces that decoration. It does not
stack a third box on it. `clip-path` on `.game` would also clip the player
menu, because the card is `overflow: visible` on purpose. `content` with
words is announced by some screen readers, so generated content in this
proposal stays empty. CSS cannot add a control, and it must not hide one.

Themes do not run JavaScript. That part of the hypothesis holds. CSS
animation is available, and `clubhouse.css` already sets
`animation: none !important` and `transition: none !important` under
`prefers-reduced-motion`. A theme rule with a more specific `!important`
could break that. This proposal uses no `!important`. Sound has no CSS
path. A declarative manifest would be how shared code learned a sound or
a motion parameter. The first pass does not add that reader. The showcase
does not need it.

`web/clubhouse.css` line 469 names Cartridge Club inside the shared file:

```css
header, [data-theme="cartridge-club"] header { column-gap: 8px; }
```

At 380px the attribute selector and the theme's own `gap` have the same
specificity, and the theme file loads later on the Library, so the shared
copy does not win there. It is a theme name in the base for a rule that
does not do the job it looks like it does.

## What a theme should be able to do

A theme stays one presentation of the same DOM, the same keyboard order,
and the same features. The first implementation gives it four things.

1. **Scoped presentation CSS**, as now, including typography, materials,
   spacing, grid placement, and decorative pseudos. New keyframe names are
   prefixed with the theme id. No `!important` on animation or transition.
   Under `forced-colors: active` the theme drops its images, shadows, and
   custom fonts. Under reduced motion it adds no transform that the shared
   rule would have cleared.

2. **A folder of same-origin files** and one route,
   `/assets/themes/{*path}`, which keeps the URLs already in `index.html`
   (`/assets/themes/nightglass.css`, `/assets/themes/cartridge-club.css`)
   and also serves `/assets/themes/cartridge-club/lilita-one-latin.woff2`.
   The handler rejects `..`, backslashes, a theme id outside `[a-z0-9-]`,
   and any extension other than css, woff2, svg, webp, and png. The body
   is `include_bytes!` of a file in the repo. There is no disk read and no
   upload. Image and font responses also send
   `Content-Security-Policy: default-src 'none'`, so an SVG opened as a
   document cannot run script. The page CSP does not change.

3. **No new markup and no new script** for this showcase. The card, the
   dialog, Recently, and the profile menu already have enough elements.
   The label is `.game-art` inset inside the plastic `.game`. The pins
   are a background on `.game`. The screws are the existing `::before`,
   which replaces the scrim only while Cartridge Club is selected.
   Missing-art loops stay, recolored. The online marker stays.

4. **A later manifest**, not in the first pass, for the things CSS cannot
   do. Shape, when that pass exists:

```json
{
  "id": "cartridge-club",
  "browserChrome": "#14071f",
  "sounds": {}
}
```

Shared code would read it. Theme code would not run. Sounds would play
only on a gesture the page already has, would stay off under reduced
motion, and would not autoplay.

The cleaner alternatives, and why they are not the first patch:

- **Tokens defaulting to today's values.** Move the 185 hex-bearing
  declarations onto custom properties whose Nightglass value is the
  current literal, and let a theme override the variable. That removes
  the selector fight. It is also a large edit to `clubhouse.css`. A
  shorthand that is not quite the old longhand changes Nightglass. The
  pixel diff below is the gate that makes it safe, and it is still a
  bigger base change than this note recommends for the first landing.
  Do it when a second showcase theme appears, or sooner if the 355-line
  Cartridge Club sheet proves brittle in review.

- **Neutral hooks.** An empty `<span data-theme-ornament>` per card would
  give a theme a free box without spending `::before`. It is also a new
  node in `clubhouse.js` on every card, present for Nightglass, which has
  nothing to put in it. The showcase did not need it. Add a hook only
  when a real theme cannot reach the decoration any other way.

- **`include_dir` instead of a match list.** One crate walks
  `web/themes` at compile time, so a new file needs no Rust edit. The
  first pass has four Cartridge Club files besides the stylesheet. A
  match arm per file is smaller than a new dependency. Switch when a
  third theme makes the match noisy.

- **Per-theme markup or script.** This remains out. It forks behavior,
  keyboard order, and the review gate.

## The showcase

Cartridge Club in the mockups is a plastic toy console. It is deliberately
loud, and it is still the same Library.

- Titles use Lilita One. Controls use Fredoka at 600. Both are SIL Open
  Font License 1.1, Latin subsets, embedded. Lilita One is 11KB, Fredoka
  is 16KB. Small text (timestamps, hints) stays on the shared UI font.
  Licenses are in the prototype folder. Nothing is loaded from a CDN.
- Cards are cartridges: a grain texture, a cream label around the existing
  art, gold contact pins, and a hard shadow. `--game-ratio` moves from
  1.2 to 1.05, so the card is a little taller and the grid is the same
  grid. Missing art keeps the shared loops, in brighter colors.
- The header is a console faceplate. The wordmark image is the shared
  brand asset. Recently is a memory card with a yellow stripe. The
  profile menu is a pause menu. The theme picker still says
  “Cartridge Club · Preview”.
- The detail dialog keeps the shared two-column layout, and on a phone
  it still stacks the art above the text. A light scanline sits on the
  art. The title, description, Play, players, and feedback are the
  existing sections.
- At 360px and below, filters, search, and Add a game stack. That matches
  DOM order, so tab order and reading order stay together. At 390px they
  stay on two rows, as the shared layout does. The 320px Nightglass
  placeholder “Find a game…” fits on one row; the plastic button is wider,
  and squeezing it clipped the placeholder. Stacking is the fix.
- The reaction stack stays on the card, above the pins, instead of
  hanging into the next card.
- Focus rings use `--focus` (`#fff36a`) and the shared outline. No
  control is removed. The lift on hover exists only under
  `prefers-reduced-motion: no-preference`.

The throwaway sheet is `docs/design-explorations/cartridge-club-showcase/theme.css`
(355 lines). `shoot.mjs` in that folder serves the real page against a
mock library. Game art in the shots is the existing fictional Firefly
Drift illustration and crops of the existing fictional atlas. It is not
production metadata.

Side-by-side shots, same fixture, reduced motion:

| | Desktop 1440 | Tablet 768 | Phone 390 | Phone 320 |
| --- | --- | --- | --- | --- |
| Library grid | yes | yes | yes | yes |
| Game detail | yes | yes | yes | yes |
| Recently | yes | yes | yes | yes |
| Profile menu | yes | yes | yes | yes |

320 is in the review gate and in the capture. The pull request leads with
desktop, tablet, and 390, which is the set this review asked to see, and
includes the 320 shots as well.

## Base changes, and what Nightglass can see

The mockups required **zero** edits to `index.html`, `clubhouse.js`,
`clubhouse.css`, `nightglass.css`, `themes.js`, or the server. The
proposed implementation, if this direction is accepted, is this:

| File | Change | Size | Painted in Nightglass |
| --- | --- | --- | --- |
| `src/portal/themes.rs` | New. One allowlisted asset handler. | about 70 lines | No. New URLs only. |
| `src/portal.rs` | `mod themes;` and one route in place of the two theme CSS routes. | about +6 lines net | No. The old CSS URLs still resolve. |
| `src/portal/client.rs` | Hash the new font and texture bytes into `version()`. | about +4 lines | No. The version meta is not painted. |
| `web/profiles.html` | Load `clubhouse.css` before `cartridge-club.css`, matching the Library. | 1 line moved | No. `nightglass.css` still precedes `clubhouse.css`. |
| `web/clubhouse.css` | Line 469. Keep `header { column-gap: 8px; }` and drop the Cartridge Club selector. | 1 line | No. Measured below. |
| `web/index.html` | None. | 0 | |
| `web/clubhouse.js` | None. | 0 | |
| `web/themes.js` | None. | 0 | |
| `web/themes/nightglass.css` | None. | 0 | |

Nightglass can see none of that. Cartridge Club’s own stylesheet and its
four assets are the theme, not the base. The profile-picker reorder
changes Cartridge Club only: its rules start winning the ties they
already win on the Library.

Sketch of the route, enough to implement without inventing a second design:

```rust
pub(super) async fn asset(headers: HeaderMap, Path(path): Path<String>) -> Response {
    let (theme, file) = path.split_once('/').unwrap_or((path.as_str(), ""));
    // theme id [a-z0-9-]+; file is either empty (the path was "nightglass.css")
    // or a single name. Reject "..", slashes, and other extensions.
    let (body, mime) = match (theme, file) {
        ("", "") => return StatusCode::NOT_FOUND.into_response(),
        _ => match_bytes(path), // include_bytes! arms, or NOT_FOUND
    };
    let mut response = conditional::static_asset(&headers, mime, body);
    if mime != "text/css" {
        response.headers_mut().insert(
            header::CONTENT_SECURITY_POLICY,
            "default-src 'none'".parse().unwrap(),
        );
    }
    response
}
```

`match_bytes` maps `nightglass.css` and `cartridge-club.css` to the files
those URLs serve today, and maps `cartridge-club/lilita-one-latin.woff2`,
`cartridge-club/fredoka-600-latin.woff2`, `cartridge-club/shelf.svg`, and
`cartridge-club/grain.svg` to the prototype assets. Adding a file means
adding an arm. That is the whole server change.

## Nightglass test

The check renders the real Library twice in one Chrome, same fixture,
`prefers-reduced-motion: reduce`, device scale 1. The second frame uses
`clubhouse.css` with the Cartridge Club selector removed from line 469.
Anything else is the current file. It fails if any pixel differs.

Captured sizes: 1440×1180, 768×1100, 390×980, and 320×780. Scenes: the
grid, the Firefly Drift detail dialog, Recently, and the profile menu.
That is 16 comparisons. **Every one was 0 pixels** in the Chrome used for
this note. The pull request image is that diff, with the per-scene counts.

Two related measurements, same fixture, desktop grid:

- Replacing `clubhouse.css` with a byte-identical copy: 0 pixels.
- Linking the prototype sheet with the `@font-face` rules removed, theme
  still Nightglass: 0 pixels, on two runs. Computed `background-image`,
  `font-family`, and header radius stayed on the Nightglass values.
- Linking the full prototype sheet, including `@font-face`, changed on
  the order of 1,400 pixels once (maximum channel delta 24, and the count
  moved between runs) even though no Nightglass element uses Lilita One
  or Fredoka and the computed styles did not change. The next two frames,
  after `document.fonts.ready`, differed by 0.

So the rules do not leak. Registering a font face makes this Chrome
repaint existing glyphs once. A stored “before” PNG from another machine
or another Chrome build is the wrong oracle. The implementation test
should do both of these in one browser session:

1. Shared CSS before and after, candidate fonts already settled or not
   yet referenced. Fail on any pixel. This is the image in the pull request.
2. With every candidate stylesheet linked and `data-theme` still
   `nightglass`, two frames after `document.fonts.ready`. Fail on any
   pixel. That catches an unscoped rule after the font repaint has gone
   quiet.

Run it the way `tests/library_detail_journey.mjs` runs: an opt-in browser
command, not part of `cargo test`. Asserting computed style of `body`
background, the root font, and a card title is a cheap second lock.

## First pass, and what waits

The first implementation is the route, the profile stylesheet order, the
one-line selector cleanup, the Cartridge Club font and texture files, and
the pixel diff. Cartridge Club stays `preview`. Nightglass stays
`defaultId`. The same DOM and the same script serve both.

Left for a later change:

- **The game player page.** It needs `themes.js`, the theme stylesheets,
  and a player stylesheet that either uses the shared tokens or has its
  own scoped overrides. Its CSP has to allow same-origin images and fonts
  (`img-src` and `font-src` are absent, and `default-src` is `'none'`).
  The player chrome needs the same before/after pixel diff, so a Library
  theme does not move the Nightglass player bar.
- **`desktop.html`.** It needs the same stylesheet links and `themes.js`.
  It is a short connect page. It was not part of these mockups.
- **The Play app.** egui does not read CSS. A manifest of colors and a
  font file mapped onto `Color32` and `FontData` is a separate program.
  The Library theme work does not imply it.
- **Sound, and any scripted motion parameter.** Those need the manifest
  and a reader in shared code. The first pass has neither. CSS animation
  remains available inside the reduced-motion rule that already exists.

## Risks

The 216 literals are still in the base. A new hex in `clubhouse.css`
shows through Cartridge Club until the theme names that property. That is
the cost of keeping the base diff small. The token pass is the way out,
and it is large enough to deserve its own review under the same pixel diff.

Replacing `.game::before` drops the Nightglass scrim for Cartridge Club
only. Replacing the missing-art pseudos has to keep a visible “no
artwork” state. The online dot is `.profile-menu > summary::after`; a
theme that restyles it has to keep a marker, because that shape is how
the current player is distinguished. The showcase leaves it alone.

`order` and a grid area that disagrees with DOM order will pass a
screenshot and fail a keyboard pass. The 320px stack was checked against
DOM order (filters, search, add). A later theme that moves Play visually
away from its tab position should be rejected in review.

Long titles still use the shared two-line clamp. Lilita One is wide. The
mockup’s long title, “Firefly Drift and the Mystery of Lantern Lake”,
wraps inside the label. Body text is not set in that face.

Yellow `#ffe14a` is for large titles and for buttons whose text is
`#2a1248`. Small text stays cream `#fff6e4` on the purple shell. The
forced-colors block drops the textures so a high-contrast palette is not
painted on top of a photograph.

The asset route is a new way to serve repo bytes. The allowlist, the
`..` rejection, and the SVG response CSP are there because a loose
wildcard would be worse than today’s one-route-per-file list.

## Open questions

1. Is this plastic-shelf direction the Cartridge Club to build, or should
   the first patch land the route and a quieter use of it?
2. Tokenize the 185 hex-bearing declarations in the same change, or after
   the showcase has been on the review gate? This note waits.
3. Is a match list acceptable until a third theme, or is `include_dir`
   worth a dependency now?
4. Confirm the player page, `desktop.html`, the Play app, and sound stay
   out of the first implementation.
5. Are Lilita One and Fredoka acceptable embedded faces for the kids, or
   should the display face be drawn here?
6. Should the pixel diff ship as an opt-in browser journey, beside
   `library_detail_journey.mjs`, rather than in CI? This note does that.
7. Is stacking search and Add a game at 320px the right trade, given that
   Nightglass still fits them on one row?
8. The browser `theme-color` meta is `#0c1017` in `index.html`. About five
   lines in `themes.js` could copy a `--browser-chrome` token onto it,
   with Nightglass set to the current value. Worth doing in the first
   pass, or leave the meta alone?
