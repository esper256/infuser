# Cartridge Club showcase prototype

THROWAWAY. This folder is a design mockup for [Theme capabilities](../../theme-capabilities.md).
It is not registered in `web/themes.js`, not embedded by the server, and not
a candidate to ship as the implementation.

`theme.css` is the proposed Cartridge Club sheet. It is scoped to
`[data-theme="cartridge-club"]` and loaded by `shoot.mjs` in place of
`web/themes/cartridge-club.css`, on the real `web/index.html`, `clubhouse.css`,
and `clubhouse.js`, with a mock library. Fonts are SIL OFL 1.1 subsets
(see `assets/NOTICE.md`). `shelf.svg` and `grain.svg` are original.

```sh
npm install --prefix /tmp/theme-shots puppeteer-core pngjs
python3 -m pip install pillow
node docs/design-explorations/cartridge-club-showcase/shoot.mjs
```

The script writes comparison shots and a Nightglass pixel diff under
`/tmp/cartridge-shots`. `STABILITY=patch` checks the one-line shared-selector
edit. `STABILITY=theme` links this sheet onto Nightglass. `STABILITY=settled`
compares two frames after the fonts have loaded.
