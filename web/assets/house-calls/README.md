# Shipped house-call images

The manifest supplies stable IDs and default labels. Group settings decide which
images are enabled and who may send them; no image uploads or arbitrary URLs.

These original 512×512 PNGs reuse the approved high-resolution reaction artwork
without resizing or recompression:

- `confetti-cannon.png`: `assets/reactions/celebrate.png`
- `tea-break.png`: `assets/reactions/cozy.png`
- `helping-hands.png`: `assets/reactions/together.png`
- `headphones.png`: `assets/reactions/music.png`

For a new release image, add its ID/default label to `manifest.json`, its PNG here,
and its embedded asset match in `src/portal/house_calls.rs`. New images default to
disabled for every group. Keep existing IDs stable for chat history. The server
journey checks that every manifest entry has a served image.
