# Infuser identity

Audience: Infuser developers.

## Product names

- **Infuser** is the website: browse the Library, find friends and publish games.
- **Infuser Play** is the native Windows/Linux app: download, update and run games.

Use **Open Infuser Play**, **Install Infuser Play** and **Connect Infuser Play**
for app actions. Use **Open Infuser** or **Back to Library** for website actions.
Keep the main game-card action **Play**. Do not label the app “Infuser Desktop”
or simply “Infuser.” Both products use the existing icon; no separate logo has
been approved for Infuser Play.

Internal IDs, the `infuser://` protocol and existing save/credential directories
stay stable across this naming change so installed copies upgrade in place.

## Visual identity

The approved identity is the **glass infuser** in [`infuser/`](infuser/): a rigid
rounded-square glass chamber in a graphite cradle, ghostly mint vapor emerging
broadly from a recessed illuminated slit, a small champagne LED, and an ivory
lowercase wordmark. The user approved this direction after refining the vapor
and slit. Preserve the broad ribbon; do not reintroduce a pinched stem, raised
bar, distorted glass, or saturated orange indicator.

The earlier `open-world-concept.png` is retained as historical exploration, not
the current identity. Google's official sign-in assets remain separate and
unmodified.

## Asset pack

All paths below are under `infuser/`. These are raster assets, not vector artwork
or a Blender scene. The three 1254×1254 masters were exported using the image
tool from the approved render; transparent variants are generated conversions,
not a claim of pixel-identical background extraction. Keep masters intact.

| Files | Use |
| --- | --- |
| `lockup-navy-master.png` | Full artwork on its original dark background |
| `lockup-light-master.png` | Real RGBA transparency, ivory text for dark surfaces |
| `lockup-dark-master.png` | Real RGBA transparency, dark text for light surfaces |
| `lockup-{navy,light,dark}-{256,512,1024}.png` | Square lockups at the named pixel size |
| `header-{light,dark}-{320,640}.png` | Horizontal transparent lockups, 320×80 and 640×160; use the latter at 320×80 for 2× screens |
| `header-{icon,wordmark}-master.png` | Lossless native-resolution crops from the PNG master; separate Recently icon and static Library wordmark |
| `header-{icon,wordmark}.webp` | Lossless high-density header exports, 140×176 and 480×111 |
| `icon-{64,128,180,192,256,512}.png` | Square transparent device-and-vapor marks without text |
| `favicon-{16,32,48}.png`, `favicon.ico` | Tighter device crop for tiny tabs; ICO includes all three sizes |
| `apple-touch-icon.png` | Opaque 180×180 dark-background touch icon |
| `icon-maskable-{192,512}.png` | Opaque launcher icons on `#071019`; the glyph sits inside the centered circle whose diameter is 80% of the canvas. Served as `/assets/app-maskable-{192,512}.png` with purpose `maskable` |
| `preview-backgrounds.png` | Review sheet showing light/dark text on intended surfaces |

Use the supplied small exports rather than scaling the complete wordmark to
favicon size. The favicon deliberately crops most of the upper vapor to keep
the device recognizable. Do not stretch aspect ratios. Give header images the
accessible name `Infuser`; decorative duplicates should have empty alt text.
Do not add slogans or taglines.

Run `./export.sh` from this folder (or any working directory) with ImageMagick 7
to regenerate derived assets from the masters. It only writes exports beside
the script. It does not change masters. The signed-out domain landing page embeds `landing-logo.webp` as a data URL in
`web/index.html`, supported by the existing image CSP. This avoids adding a backend
asset route. If regenerating that file, update its embedded copy too. The signed-in
Library header embeds `header-icon.webp` and `header-wordmark.webp` separately.
Only the icon opens Recently and signals unread events; the static wordmark links
to the Library. The split starts from the original PNG master, never a lossy WebP,
and both cropped masters and served exports are lossless. Update the matching
data URLs in `web/index.html` when regenerating these exports. The Library favicon
embeds `favicon-32.png`. Cards without game artwork use overlapping abstract color loops drawn in CSS. Keep the Infuser identity in the site branding, not as a stand-in for
game artwork. The fallback requires no asset request.

## Pixel and transparency review

Inspected the navy RGB image, both transparent RGBA variants, the alpha channel,
and magnified vapor/glass crops. No baked-in checkerboard was observed in the
reviewed source crops or on solid-background composites. A simple pixel-parity
check across vapor, glass and background samples also found no material one-pixel
checkerboard bias (absolute mean diagonal alternation below 0.05/255); this is a
spot check, not proof against every possible periodic texture.

The raster masters retain fine texture. Transparent conversions contain noisy
RGB values at near-zero alpha around some edges; these can appear conspicuous
in previews that display or mishandle nearly invisible pixels. Normal alpha
compositing on the intended dark/light surfaces was visually checked. Do not
flatten against a checkerboard, discard alpha, or use the transparent image's
raw RGB channels as a new master. Glass has baked lighting; this is not a physical
refraction simulation for arbitrary backgrounds. Use the navy version when exact
background appearance matters. A Blender rebuild is deferred.
