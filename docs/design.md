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

Add a game needs a name, then a copyable
prompt for the existing game-making agent. No credential file chores for kids.

Play opens the game's title screen. The game chooses gameplay modes; Infuser
keeps the guest option for device sharing. Future Join friend shortcuts require
explicit game support, with the contract reviewed before SDK 1.0.

The fullscreen control belongs in the slim title bar **above** the game. Reserve
that space outside the game's viewport; never float controls over its canvas.
The player frame keeps fixed-position and full-height game layouts below the bar.
Opening a game means "game open," not "joinable multiplayer." Online presence,
an open game, and an explicitly joinable session are distinct states.

Use a dark, game-like visual style and small, responsive feedback throughout
player flows: pressed controls, subtle card movement, clear launch progress,
and a brief acknowledgement when an action succeeds. Respect reduced motion.
The games get the spotlight. Keep adult tools straightforward.

When creator portraits are available, use the game author’s portrait as the
reaction entry point, without a separate reaction control. Hover and keyboard
focus show a thought bubble: “React to {author}’s game.” Click, Enter, or a phone
tap opens the picker directly; never require hover. Give the portrait the same
accessible action name. Players open the account menu from their own portrait
in the online-player list. A small gap, subtle halo, and inset chevron distinguish
this badge; the menu includes the portrait picker, theme choice, and sign out.
Keep it mounted during presence refreshes. The chosen portrait appears on their
creator and presence badges.
Initials remain available. Never invent author identities. Cards use the Nightglass layout with
a content-sized glass dock. Missing or unavailable game artwork uses a muted,
abstract composition of overlapping colored loops, without logos, invented game imagery, or filler
copy. Keep it static and subordinate to the real game covers. Fictional art stays
in mockups. The [card hero art guide](card-art-guide.md) defines composition,
export, and publishing requirements. Artwork must ship with the game build;
committing an image to design explorations does not publish it.
Game art may use polished cover illustration while preserving the game's real
identity. Allow flexible compositions within the crop-safe area. Protect the art
by capping titles at two lines and tags at one horizontally scrollable row. The
existing dropdown exposes the full title, player names, and reaction counts;
metadata must not grow indefinitely over the image.

Online players appear as overlapping avatars beside the logo. Filters and search
share a row, with the game count after the final filter. Tag editing appears on
tag-area hover or keyboard focus; a touch hold on an editable card opens the same
picker. Scrolling and multi-touch cancel the hold.

Reactions begin with large positive emoji in a speech bubble. Selecting one
reveals a short optional comment and Enter-to-send. Emoji-only is complete.
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
