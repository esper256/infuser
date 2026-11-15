# Bevy: browser export first

This is the fourth engine study, after Three.js, Godot web, and Godot multiplayer.
The first route is an existing WASM/web export served as a normal static build.
Use the project's existing toolchain (for example, Trunk), then publish its output:

```sh
infuser publish /private/game.credentials.json /path/to/web-output --solo
```

Do not switch the game's renderer or networking stack just to fit an example.
Check the actual browser requirements and wire the game's Rust/WASM checkpoint
system to the browser SDK. The same isolated guest origin and cloud-save contract
apply. A reusable wasm-bindgen save adapter and a complete Bevy dogfood journey
are still pending. Native server packaging comes after the browser study.
