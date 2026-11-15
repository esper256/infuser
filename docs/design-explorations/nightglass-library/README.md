# Nightglass Library mockup

Open `index.html` with a local static server rooted at the repository. This is the
whole Library review with Nightglass on all nine cards, not an isolated card study.

All game art, authors, profile pictures, presence and activity are fictional review
fixtures. The generated atlas supplies nine different paintings; Firefly Drift uses
the earlier approved throwaway illustration. Avatars are selected crops of the
fictional artwork. Nothing in this mockup is production metadata or a publishing contract.

Every card has full-bleed art and a content-sized smoked-glass dock at bottom right.
Title and creator avatar share a row; the small joined Play/dropdown sits below on
narrow cards. Hover or keyboard focus on the avatar shows “React to {author}’s game.”
Click, Enter or tap opens the existing style of emoji picker. There is no second
reaction control. Categories and tag editing remain compact over the art.

Search, categories, tags, reactions, guest selection and the prompt flow are local
preview interactions. Play does not launch, and no API calls are made. Fixtures
reset on reload. The production HTML, CSS, JavaScript and backend are unchanged.

The mockup reuses the current Library stylesheet and the approved header image.
`mock.css` applies the card direction across the full grid; `mock.js` supplies only
fictional preview data and interactions. `art-atlas.webp` is generated concept art,
not a source of licensed third-party profile pictures or actual gameplay screenshots.

Verified in Chromium at 1280px desktop, 768px tablet and 320/390px phones, including
content-sized dock bounds, focus/keyboard and touch reactions, dropdown selection,
search/filter/tag previews, copyable mock prompt and reduced motion. See `desktop.webp`,
`phone.webp`, `phone-viewport.webp`, and `portrait-focus.webp` for captures.
