# Nightglass card option

Saved for comparison during the Library redesign. This is a standalone visual
prototype, not production UI or an agreed game-art contract.

The full-bleed hero sits behind a smoked-glass dock anchored at the bottom right.
The dock uses `width: max-content` with a card-relative maximum, so it fits its
title, creator avatar and compact Play controls instead of spanning the card.
On narrow cards the controls wrap to a second row. The creator portrait is the only reaction entry point. Hover or keyboard focus
shows “React to KwamiKwami’s game” in a thought bubble; click, Enter, or a phone
tap opens the positive emoji picker. There is no separate reaction button.
KwamiKwami is example copy; the avatar remains a placeholder. Artwork movement and pointer lighting respect reduced motion.

Open `index.html` in a browser, or serve this folder with any local static server.
`preview.png` records the desktop appearance. `hero.webp` is throwaway generated
concept art for Firefly Drift; it does not claim to depict actual gameplay. The
avatar reuses that art as an explicit placeholder. Buttons never launch a game.
No framework, external assets, API calls, credentials, or backend changes.

The angled Slipstream title treatment is not selected as a general Library-card
direction: it becomes repetitive across a grid. This Nightglass option remains
an alternative to the small diagonal-corner design, not a replacement decision.
