# Library themes

Themes change presentation, not the application's features or identity. The
profile dropdown contains **Change profile picture**, **Theme**, and **Sign out**.
Theme choices apply immediately, persist in this browser, and sync across its open
tabs. The presence stack includes the current player beside their friends; the
account menu uses a rectangular handle button rather than a duplicate portrait.
They are not account settings and do not change a player's handle or avatar.

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
checks. Cartridge Club visually hides the large Library heading while retaining
it for assistive technology. Do not hide application features with theme CSS.
Keep DOM and keyboard order coherent when changing layout.

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
profile selection, the single Add a game prompt, and adult tools under each
selectable theme. Check open dialogs survive a presentation change.

Recently shows existing updates and reactions. Its dot means an event appeared
since the panel was last viewed during this page visit; it is not a server-side
unread count or a persistent inbox. Opening the panel clears the dot. Its brief
hop is disabled by reduced-motion preferences.

Run `node --test tests/themes.mjs` for registry/preference checks. Browser checks
must exercise the shared UI in both themes; screenshots alone do not demonstrate
functional parity. Keep temporary fixtures and comparison screenshots in `.local/`.
