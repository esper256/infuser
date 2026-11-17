# Library themes

Audience: Infuser developers.

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
- `web/themes/cartridge-club.css` is an opt-in candidate. Every selector is scoped
  to `[data-theme="cartridge-club"]`; it cannot affect the baseline.
- `web/themes.js` is the small theme registry and preference bootstrap. It runs
  before the stylesheets to restore `infuser.theme` without a flash. It contains
  no game or account logic. Unknown/retired preferences fall back to `defaultId`;
  unavailable browser storage does not prevent selection for the current visit.
- The server embeds and serves these files like the other Library assets. No
  dynamic stylesheet URLs, dependencies, theme-specific endpoints, or database
  migrations are needed.

Shared functionality continues to evolve under every design. The profile menu
and Recently panel are shared features; switching back to Nightglass does not
remove them. Nightglass preserves the previous visual language, not a frozen copy
of the old application. The card fallback and official branding assets are shared.

Theme rules may change colors, typography, materials, spacing, grid placement,
and decorative treatments. They must not remove functional controls or permission
checks. The shared layout visually hides the large Library heading while retaining
it for assistive technology. Do not hide application features with theme CSS.
Keep DOM and keyboard order coherent when changing layout.

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

Nightglass remains the default. Cartridge Club must earn adoption through side-
by-side review; shipping its preview is not approval to make it the default.

To add a candidate, register a new ID as `preview`, add a scoped stylesheet and
its embedded asset route/link, and compare it with the baseline on the same real
UI. Do not clone markup or JavaScript. To adopt one after review, mark it `stable`
and change `defaultId`. Existing explicit preferences remain respected. To retire
one, mark it `retired`; choose a different stable default first if necessary.
Its optional scoped stylesheet/link/route can then be removed. Baseline tokens
remain shared even if the Nightglass choice is ever retired. No feature revert
is involved in these operations.

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

Run `node --test tests/themes.mjs` for registry/preference checks. Browser checks
must exercise the shared UI in both themes; screenshots alone do not demonstrate
functional parity. Keep temporary fixtures and comparison screenshots in `.local/`.

Run `node tests/library_detail_journey.mjs` for the isolated mock-API browser
journey. Set `INFUSER_PLAYWRIGHT` and `INFUSER_CHROMIUM` for an existing browser
installation, and `INFUSER_TOUCH=1` to include coarse-pointer and long-press checks.
