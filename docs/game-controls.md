# Game controls and controller prompts

Declare the ways a player can complete your game in its [publisher metadata](game-metadata.md#input-methods). Each entry represents a complete alternative. For a game that needs both a keyboard and a mouse, use `keyboard_mouse`. Use `keyboard` or `mouse` when either works on its own. Declare `touch` after testing on a touch screen.

## What earns the Gamepad label?

Start at your game's first screen after Infuser launches it. With the keyboard and mouse set aside, a player should be able to start, navigate menus, play, pause, resume and start another round. Include settings and dialogs encountered on that path. If a controller only works after a mouse click in your game's menu, finish the menu bindings before publishing the label.

Bind actions such as **Jump**, **Confirm**, **Back** and **Pause** in one place. Use those bindings for both input and prompts. Godot games can use Input Map actions and initial UI focus. Unity games can bind both gameplay and UI action maps. For web games:

- Poll `navigator.getGamepads?.()` inside the active game's animation loop. A browser can withhold a controller until the player presses a button on the focused game. Handle missing API access, empty slots and disconnects.
- For `pad.mapping === "standard"`, the four face buttons use the positions below. For another mapping, offer rebinding or use your engine's tested controller mapping.
- Read sticks with a dead zone. Trigger single actions on the transition to pressed. Give held menu directions a deliberate repeat delay, and a newly opened menu visible focus.
- On loss of focus or disconnect, clear held actions. Pause local play where appropriate. For multiplayer, idle the local player and let them reconnect. On return, sample a fresh state before accepting a new press.
- Switch prompts when the player actually uses a different device. A connected controller or a drifting stick alone should not change the prompts.

## Use the layout supplied by Infuser

In the Library, open **Profile → Settings → Gamepads** and press a button on each controller. Automatic uses known Sony and Nintendo identifiers and falls back to Xbox. A player can choose the layout printed on their controller. This browser remembers the choice by controller model across slots and profiles. Controllers reporting identical IDs share a preference.

On Play, Infuser carries the overrides for controllers visible to the Library into the game tab. They survive reloading the game. A newly seen model uses automatic detection. To apply a changed override, press a button in the Library and launch again.

Hosted web games can call the bundled SDK directly. Both browser SDK entry points expose these helpers; versioned games can check for `gamepad_layouts.v1` in SDK capabilities.

```js
// pad is the Gamepad object already driving this player's actions.
const family = Infuser.gamepads.layout(pad);       // xbox, playstation, nintendo
const jumpLabel = Infuser.gamepads.faceButton(pad, jumpBinding.button);
if (jumpLabel !== null) showJumpPrompt(jumpLabel);
```

`layout` guesses the controller family (or uses the player's override) for any mapping. `faceButton` returns a label only for indices 0–3 on a pad with `mapping === "standard"`. It returns null for other mappings or indices. Custom bindings should pass their actual standard face-button index. Engines that supply their own action mapping can use `layout(pad)` to pick their prompt artwork. On older Infuser installations, feature-detect `Infuser.gamepads` and use the game's usual prompts.

## Browsing the Library with a controller

Use the D-pad or left stick to highlight a game. Confirm opens its expanded card, where Play gets focus. Move right for Play options. Back closes the options first, then the card. The Library uses Nintendo A/B, PlayStation Cross/Circle or Xbox A/B for Confirm/Back according to the selected layout.

Library shortcuts appear after deliberate controller input and disappear on a keypress or pointer press. The input-method icons on the card describe the game's supported controls. Starting a web game with the controller opens it in the current tab. Each game decides which prompts to display during play.

## Face-button prompts

Use the physical position for the binding and the player's chosen layout for the label. These are the face labels for common standard-mapped controllers **when the binding stays in the same physical position**.

| Physical position | Web standard button | Xbox | PlayStation | Nintendo |
| --- | --- | --- | --- | --- |
| South (bottom) | 0 | A | × Cross | B |
| East (right) | 1 | B | ○ Circle | A |
| West (left) | 2 | X | □ Square | Y |
| North (top) | 3 | Y | △ Triangle | X |

For example, a jump bound to button 0 reads **A · Jump**, **× · Jump** or **B · Jump**. If your game changes the logical Confirm binding for Nintendo conventions, update both the action and the displayed prompt. The standard mapping identifies positions. Controller names vary across browsers, adapters and operating systems. Use the supplied layout when hosted. In standalone builds, offer a prompt layout setting or use position names such as **Bottom button**.

A letter or symbol inside a high-contrast outlined circle is enough for a face-button badge. Pair it with the action name. Give symbolic prompts an accessible name such as “Press Cross to jump”, and use shape or text in addition to color. This works with ordinary text, engine UI and local fonts.

For example, this HTML prompt works without an image download. Put the CSS in the game’s stylesheet. When the binding or layout changes, update both the symbol and its accessible name together.

```html
<p><span class="pad-face" role="img" aria-label="Cross button">×</span> Jump</p>
```

```css
.pad-face {
  display: inline-grid;
  place-items: center;
  box-sizing: border-box;
  width: 1.6em;
  height: 1.6em;
  border: 0.12em solid currentColor;
  border-radius: 50%;
  font: bold 1em/1 system-ui, sans-serif;
}
```

## Before publishing

Try a complete round, including opening and dismissing menus, using each declared method on the shipped build. For controller support, also test holding a direction, unplugging and reconnecting the controller, and switching back to another supported input. Check that a held Confirm does not activate the next screen. Verify that the prompt for each action agrees with the button you actually pressed. Record which controllers, browsers or native builds you exercised in the game's own test notes. Revisit the declaration when an update changes the controls.
