# Illustrated game reactions

Original artwork generated for Infuser with OpenAI image generation.
The PNGs are 512×512 transparent source exports; lossless 256×256 WebP files
are embedded by the server. No external image service or font is required.

| Persisted emoji | Asset | Label |
| --- | --- | --- |
| 🐦‍⬛ | atmosphere | Amazing atmosphere |
| 🍵 | cozy | Cozy world |
| 💎 | gem | A little gem |
| 🧩 | clever | Clever design |
| 🎢 | wild | Wild ride |
| 🪄 | surprises | Full of surprises |
| 🎨 | art | Beautiful art |
| 🎵 | music | Great soundtrack |
| 😄 | smile | Made me smile |
| 🤝 | together | Fun together |

Choose affirming labels that help players describe what they enjoyed. The smile
expresses delight, without tears or embarrassment; the fist bump celebrates
playing together. The midnight-blue/charcoal headphones use coherent three-quarter perspective
and similar-sized silver-blue notes with different shapes and rotations. They protrude along one side in a connected
silhouette; the top note connects only to the note below, leaving the center open.
Avoid angry, disgusted, mocking, or disappointed reactions that prime a negative review.

Keep the established illustrative style: strong freestanding silhouettes, skillful
painted shading, rich color, and transparent surroundings. No photographic materials,
babyish mascots, ground shadows, surrounding haze, or rectangular backdrops. The
puzzle decoration is painted on a flat face; the cup is round and solid; the gem
has a canonical diamond silhouette and no surrounding smudges.

`reactionFace()` in clubhouse.js shares these assets across card buzz, details,
reviews, Recently, the picker, and the celebration. Native emoji remains the
fallback if an asset cannot load. Other reactions retain their native emoji.
New entries must also join the server reaction allowlist and API/browser coverage.
Labels remain accessible independently of the decorative images. Both themes use
the same renderer. Card reactions overlap with the leftmost on top, with a
4px-right/2px-down silhouette shadow (60% black, 0.5px blur) separating each face
from its neighbor. Keep backgrounds transparent; do not add circular backings. Reduced motion suppresses the celebration.
