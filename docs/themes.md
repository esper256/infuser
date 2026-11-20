# Library themes

Audience: Infuser developers.

Cartridge Club is stable. Funny Pages and Field Notes are previews that
also change the Library's layout. What shipped, and what is still deferred, is in
[Theme capabilities](theme-capabilities.md). Nightglass stays the default.

Themes change presentation, not the application's features or identity. The
profile dropdown contains **Change profile picture**, **Theme**, and **Sign out**.
Theme choices apply immediately, persist in this browser, and sync across its open
tabs. The presence stack includes the current player beside their friends; the
current player’s portrait opens the account menu. Its online marker has a round
top and pointed bottom to suggest the dropdown; other players keep round dots.
Theme preferences are not account settings and do not change a player's handle or avatar.

## Architecture

- `web/index.html` and `web/clubhouse.js` own the single shared component tree,
  accessibility, state, menus, API calls, and behavior. Never fork them per theme.
- `web/clubhouse.css` owns shared component structure, responsive behavior,
  interaction states, and baseline treatments. Fix functionality here once for
  every theme. Preserve reduced-motion and forced-colors support.
- `web/themes/nightglass.css` supplies the baseline tokens. It always loads, so
  candidates can override only the tokens and presentation rules they need.
- Candidate stylesheets are opt-in. Every selector is scoped to that theme's
  `data-theme` value, so a sheet cannot affect the baseline or another theme.
  The HTML does not link them. `web/themes.js` inserts a render-blocking
  `<link>` for the saved theme while the document is still parsing, and loads
  a different sheet when the Theme menu changes. `data-theme` changes only
  after that sheet's `load` event, so the previous theme stays up until the
  new one can paint.
- `web/themes.js` is the catalog (id, name, status) and the preference
  bootstrap. It runs in the head after `nightglass.css` and `clubhouse.css`,
  still before first paint. It contains no game or account logic. After styles
  load, it copies the `--browser-chrome` token onto `<meta name="theme-color">`.
  Nightglass sets that token to `#0c1017`. Unknown/retired preferences fall
  back to `defaultId`; unavailable browser storage does not prevent selection
  for the current visit.
- The server embeds theme files and serves them from one allowlisted route,
  `/assets/themes/{*path}`. The stylesheet list is the top-level `*.css`
  entries in `src/portal/themes.rs`. `themes.js` is served with that list
  prepended (`InfuserThemeSheets`), including a `?v=` content hash for each
  sheet. A request whose `v` matches the sheet's bytes is
  `Cache-Control: public, max-age=31536000, immutable`. A plain URL, including
  fonts and textures, keeps the same `no-cache` plus ETag behavior as other
  `/assets`. `source_fingerprint()` still folds every registry file into the
  client version. A theme's fonts, textures, and license notices live under
  `/assets/themes/<id>/`. No theme-specific endpoints or database migrations
  are needed. The Library and the profile picker load `nightglass.css`, then
  `clubhouse.css`, then the one active candidate. The service worker only
  handles navigations, so these stylesheets stay on the network. The game
  player page and the Play app do not load this system. `desktop.html` and
  the member-link page keep nightglass only.

Shared functionality continues to evolve under every design. The profile menu
and Recently panel are shared features; switching back to Nightglass does not
remove them. Nightglass preserves the previous visual language, not a frozen copy
of the old application. The card fallback and official branding assets are shared.

Theme rules may change colors, typography, materials, spacing, grid placement,
and decorative treatments. A theme may also reposition existing regions with
its own stylesheet: Funny Pages turns the header into a left-hand masthead on
wide screens, gives the game list an asymmetric panel grid, and opens Recently
as a flyout beside that masthead. Chat connectors are the one shared hook:
a theme may set `--chat-stack` to `row` or `column` and `--chat-anchor` to
`below`, `above`, `left`, or `right`. Leaving them unset keeps the horizontal
row and the upward triangles. Funny Pages stacks the circles vertically at
960px and wider and opens chat to their right, so each triangle can meet a
circle. Message tails inside the chat stay on. Themes must not remove
functional controls or permission checks.
The shared layout visually hides the large Library heading while retaining
it for assistive technology. Do not hide application features with theme CSS.
Keep DOM and keyboard order coherent when changing layout. Prefer a placement
that follows source order when a visual move would send keyboard focus
somewhere other than the next thing on screen. Funny Pages therefore keeps
the masthead's controls in source order, and its panel spans still read
left to right, top to bottom. The wide grid starts when the library column
itself is at least 880px, so the masthead does not steal the space the
panels need. At 360px the masthead returns to the top bar
and the panels become one column. Field Notes leaves the header as a top
bar and turns the game list into one column of horizontal entries: a
specimen plate on the left, the title and Play control on the right. That
row stays horizontal on a phone. No shared wrapper was added for it.

## Cards and game details

Card artwork and titles open the shared game-detail dialog. Play launches directly;
the author portrait opens the reaction picker. Both views use the same launch
controls, including guest mode, Join, native launch, and sharing. Admin visibility
actions appear only in the full game popup’s Admin actions (⋯) dropdown. Reaction
counts stay out of Play menus.
Active player portraits sit at the upper right of each card with a subtle rotating
ring (static under reduced motion). Their menu names all active players and guests
and offers Join actions only for server-advertised joinable sessions;
the visible stack is capped at three portraits plus an overflow count. Play menus
keep launch and Join actions without repeating the player list. The tag row
reserves room for the stack, and presence polling preserves unchanged badges and covers.
Recently entries open the referenced game, focusing the relevant reaction when
present. Feedback portraits use the same opaque player IDs and approved avatar
catalog as presence; email addresses are never sent for this presentation.

Hidden games live in a collapsed admin-only shelf, using the existing visibility
permissions. The detail dialog uses actual descriptions, update times, tags, and
reactions. Missing artwork uses the shared abstract fallback. No presentation
fixture becomes production game data.

The shared responsive layout stacks detail artwork above information on phones.
Themes supply `--detail-panel`, `--feedback-bubble`, `--presence-online`, and
`--game-ratio` alongside the existing color/material tokens. Dialog focus, scroll,
permissions, and launch behavior stay in shared code. Nightglass's quieter tag
chrome is scoped to that theme; Cartridge Club retains its own materials.

## Lifecycle

The registry gives each theme a stable ID, display name, and status:

| Status | Meaning |
| --- | --- |
| `preview` | Selectable, explicitly labeled Preview; not adopted by default |
| `stable` | Reviewed and available; one stable theme is the default |
| `retired` | Not offered; saved selections resolve to the current default |

Nightglass remains the default. Cartridge Club is stable, so the Theme menu
does not label it Preview, and it is still not the default. Funny Pages
and Field Notes stay previews until they earn adoption. Shipping a preview
is not approval to make it the default.

To add a candidate, register a new ID as `preview` in the `web/themes.js`
catalog, add a scoped stylesheet, and name its files in `src/portal/themes.rs`
(`FILES` and a `bytes` arm). A top-level `{id}.css` entry joins the stylesheet
list the client already reads. Do not add a `<link>` in HTML, and do not edit
`client.rs`: `source_fingerprint()` already covers every registry file.
Compare the theme with the baseline on the same real UI. Do not clone markup
or JavaScript. To adopt one after review, mark it `stable` and change
`defaultId`. Existing explicit preferences remain respected. To retire one,
mark it `retired`; choose a different stable default first if necessary.
Its optional scoped stylesheet and registry entries can then be removed.
Baseline tokens remain shared even if the Nightglass choice is ever retired.
No feature revert is involved in these operations.

## Review gate

Compare desktop, tablet, and 320px/390px phones with identical content. Include
long titles/handles/tags, loaded and missing artwork, loading/empty/error states,
keyboard focus, reduced motion, and the signed-out landing page. Verify search,
filters, normal/guest/join launch, copy link, creator reactions, tag editing,
profile selection, the single Add a game prompt, and admin tools under each
selectable theme. Check open dialogs survive a presentation change.

The header's glass Infuser icon opens Recently; the separate wordmark stays static
and links to the Library. A halo and intermittently timed wiggle mean an event
appeared since the panel was last viewed during this page visit. Opening Recently
clears that state. Motion stops in hidden tabs and with reduced-motion preferences,
which retain a static halo. Only the newest event has a rounded relative time;
older events stay uncluttered. This is not a persistent inbox or server unread count.

Card reaction stacks display up to eight faces with diminishing growth. Small
stacks have modest breathing room, gradually returning to the original 15px overlap
at eight faces. The Play dock uses 8px padding to leave more artwork visible.

Run `node --test tests/themes.mjs` for registry and browser-chrome checks.
Browser checks must exercise the shared UI in every selectable theme;
screenshots alone do not demonstrate functional parity. A zero-tolerance Nightglass pixel diff
is not in CI: Chrome does not reproduce text pixels across two page loads, and
disabling the candidate stylesheet repaints glyphs even when computed style
does not change. See [Theme capabilities](theme-capabilities.md). Keep temporary
fixtures and comparison screenshots in `.local/`.

Run `node tests/library_detail_journey.mjs` for the isolated mock-API browser
journey. Set `INFUSER_PLAYWRIGHT` and `INFUSER_CHROMIUM` for an existing browser
installation, and `INFUSER_TOUCH=1` to include coarse-pointer and long-press checks.
