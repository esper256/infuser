# Player experience

Infuser is a private game library. Use **Library**, **Single player**, and
**Multiplayer** in player-facing copy. Keep internal protocol names distinct from
UI wording. Adult invitations and administration belong in a quiet admin area.

One heading per task. Avoid stacking a slogan, title, explanation, and field
question that all say the same thing. Cut redundant text aggressively; let the
action and state explain themselves. Player-facing copy should name the task,
action, or useful state. Do not add slogans, sentimental taglines, motivational
filler, or decorative sentences such as “Your games. Your crew.” or “Make games.
Play together.” Do not reintroduce them in headers, sign-in screens, empty states,
footers, or notifications. Prefer fewer words and omit copy that adds no useful
information.

Keep the Library usable on narrow phones: avoid horizontal overflow, keep Play
prominent with secondary options behind its adjacent dropdown, and preserve
comfortable touch targets and reduced-motion support.

Add a game supplies a copyable prompt for the existing game-making agent. The
agent detects the existing title; do not ask the child to name their game again.
No credential file chores for kids.

Play opens the game's title screen. The game chooses gameplay modes; Infuser
keeps the guest option for device sharing. Join friend shortcuts require
explicit peer-hosted game support; the contract remains a preview before SDK 1.0.

The fullscreen control belongs in the slim title bar **above** the game. Reserve
that space outside the game's viewport; never float controls over its canvas.
The player frame keeps fixed-position and full-height game layouts below the bar.
Opening a game means "game open," not "joinable multiplayer." Online presence,
an open game, and an explicitly joinable session are distinct states.

Use a dark, game-like visual style and small, responsive feedback throughout
player flows: pressed controls, subtle card movement, clear launch progress,
and a brief acknowledgement when an action succeeds. Respect reduced motion.
The games get the spotlight. Keep adult tools straightforward.
Do not label cards Single player or Multiplayer; tags and launch options convey
those choices. Keep unlisted games in the collapsed adult-only Hidden shelf.

When creator portraits are available, use the game author’s portrait as the
reaction entry point on cards, without a separate card control. The larger game
view also exposes a prominent row of positive reaction buttons above feedback.
Hover and keyboard focus on the portrait show a thought bubble: “React to {author}’s game.” Click, Enter, or a phone
tap opens the picker directly; never require hover. Give the portrait the same
accessible action name. Players open the account menu from their own portrait
in the online-player list. A small gap, subtle halo, and online marker with a round top and pointed
bottom distinguish this badge; the menu includes the portrait picker, theme choice, and sign out.
Keep it mounted during presence refreshes. The chosen portrait appears on their
creator and presence badges.
Initials remain available. Never invent author identities. Cards use the Nightglass layout with
a content-sized glass dock. Missing or unavailable game artwork uses a muted,
abstract composition of overlapping colored loops, without logos, invented game imagery, or filler
copy. Keep it static and subordinate to the real game covers. A game with no
cover and a pending art brief uses the approved “Generating...” variant; never
substitute it for a real cover or publish it into the game metadata. Fictional art stays
in mockups. The [card hero art guide](card-art-guide.md) defines composition,
export, and publishing requirements. Publish artwork with a build or through the
[metadata API](game-metadata.md); committing an image alone does not publish it.
Game art may use polished cover illustration while preserving the game's real
identity. Allow flexible compositions within the crop-safe area. Protect the art
by capping titles at two lines and tags at one row. Keep legible spacing and
limit visible card tags to three; an ellipsis reveals the rest on hover, keyboard
focus, or tap. Library filters use a compact More dropdown for overflow. Do not
show horizontal scrollbars or overlap tag text. The
existing dropdown exposes the full title, player names, and reaction counts;
metadata must not grow indefinitely over the image.

Clicking card artwork or a title opens the larger game view; Play launches directly.
Show real artwork, the author, description, tags, update date, and feedback with
player portraits. Recently entries open the referenced game and reaction. Keep
feedback positive and qualitative. The shared dialog stacks on phones, retains
scroll and focus during refresh, and uses the active theme's materials.

Online players appear as overlapping avatars beside the logo. Filters and search
share a row, with the game count after the final filter. Search matches game
titles, tags, and author handles, case-insensitively. A small bottom-left stack
sits just inside the left edge, with about 20% of each emoji hanging below the
card. It shows reactions updated in the last 28 days:
`min(n, 6, ceil(sqrt(2*n)))` faces.
Prefer varied recent emoji, then fill with repeats. Show none for zero recent
reactions; retain older feedback in the game view. Keep the stack clear of the
title and Play controls; clicking it opens the newest recent feedback. Tag editing appears on
tag-area hover or keyboard focus; a touch hold on an editable card opens the same
picker. Scrolling and multi-touch cancel the hold.

Reactions begin with large positive emoji in a speech bubble. Selecting one
gets a brief colorful burst and reveals a single-line, large-type optional comment
with the prompt “Say something nice (optional).” Send or Enter submits it;
emoji-only is complete. Do not send anything just for selecting an emoji.
Recently uses a bright accent fill, bold label, unread dot, and a repeating gentle
pulse/hop until opened. Reduced motion keeps the highlight without animation.
Card hover scales evenly from the center with a deeper shadow; do not shift
cards upward. Hover animation is mouse-only and respects reduced motion.
Do not force a review after playing or imply a numerical rating.

Reference: [Juice it or lose it, Martin Jonasson and Petri Purho](https://www.youtube.com/watch?v=Fy0aCDmgnxg)
and the authors' [Juicy Breakout demo](https://github.com/grapefrukt/juicy-breakout).

Visual experiments use the [theme system](themes.md). Nightglass stays the default
until a candidate earns adoption. Cartridge Club is an opt-in preview in the
profile dropdown. Profile, Recently, and all game/admin behavior are shared across
themes; never duplicate functionality to preserve an older visual design.

[Library chat](library-chat.md) uses compact speech bubbles with tails aimed at
the speaker's profile picture. Keep the shared audience explicit, drafts safe,
and the closed control small. New messages must not interrupt reading older ones.
