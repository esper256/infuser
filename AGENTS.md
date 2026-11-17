# Working on Infuser

These instructions apply to changes in this repository. Read
[Development checks](docs/development.md) for the relevant validation commands.
The game-author integration guide at `docs/agents.md` is for publishing games,
not for contributing to Infuser itself.

## Format before submitting

Always run rustfmt before committing or submitting changes, including opening or
updating a pull request. From the repository root, format both Cargo packages and
then verify that formatting is clean:

```sh
cargo fmt --all
cargo fmt --manifest-path desktop/Cargo.toml --all
cargo fmt --all -- --check
cargo fmt --manifest-path desktop/Cargo.toml --all -- --check
```

Include the formatter's changes in the submission. Run it again after subsequent
Rust edits. Do not substitute manual formatting or rely on CI to format the code.

