# Infuser Play

Audience: Infuser developers.

The native companion connects an account, downloads and opens games, then stays
minimized while a game runs. The Library remains in the browser.

Cache verification, installation and run preparation share a per-game lock.
`src/run_storage.rs` retains current/previous versions and active runs, pruning
known-dead runs without touching saves. Linux prefers copy-on-write; other cases
copy files. See [storage notes](../docs/implementation-notes.md#native-run-storage)
and [regression tests](../docs/developer-testing.md#native-cache-and-run-cleanup).

Presentation lives in `src/ui.rs`; connection, installation and launch behavior
remain in `src/main.rs`, `src/client.rs` and `src/install.rs`. Use the approved
Infuser identity, charcoal surfaces, ivory text and mint primary actions. Keep
copy factual and short. Do not add slogans or a second game library. Long server
origins and errors scroll while actions stay visible. Activity is indeterminate:
never imply download percentages without real measurements from the client.

The About panel uses the running executable's embedded version/build and a
small update-check summary. The clipboard payload is an explicit list in
`src/about.rs`; do not add credentials, launch URLs, account data, local paths or
raw errors. Old updater state without an outcome must remain “status unavailable”
because its timestamp records an attempt, not a successful check.

## Visual review

Run the real presentation without credentials, network calls or game launches:

```sh
cargo run --manifest-path desktop/Cargo.toml --locked --example appearance -- connect
```

States: `idle`, `connect`, `opening`, `browser`, `playing`, `error`,
`invalid-link`, `long-server`, `long-title`, `long-error`, `about`, `about-failed`.

An optional output path saves the window content as a PNG and exits. Optional
width and height arguments exercise narrow windows and long messages:

```sh
mkdir -p .local/play-review
cargo run --manifest-path desktop/Cargo.toml --locked --example appearance -- \
  long-error .local/play-review/long-error.png 360 360
```

This needs a graphical display. It uses the same renderer as the installed app;
sample titles, hosts and errors exist only in the example. Font licenses are in
`assets/fonts/`.
