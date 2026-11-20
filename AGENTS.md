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

## Changelog lines

The Build and release workflow writes the changelog for `build-<sha>`. It
lists merged pull requests and commits since the previous release. That
changelog is the release notes, the `changelog.json` asset, and the text
embedded in the server binary. Recently uses it when Infuser itself updates.
Do not add changelog fields to `release.json`; the updater on a host rejects
unknown manifest fields.

Write the pull request title as a short sentence a kid or parent can read.
That title is the line they may see. Example: `Games remember where you left off`.
Do not put commit hashes in the title.

A change is included in Recently when it touches the product people use: the
website, the Play app, saves, chat, games, or other server behavior outside
the internal set below. A pull request that only touches internal paths is
kept in the release notes under "Behind the scenes" and is not posted in
Recently. Internal paths are CI (`.github/`), docs, tests, deploy, examples,
the SDK, vendored web files, packaging, and host tooling such as the updater,
rollback, backup, and health checks. You do not need to add a label.

If that automatic choice is wrong, start the title with `family:` or
`internal:`. Those words are removed from the sentence people read.
`family:` shows a docs-only note in Recently. `internal:` keeps a website
refactor out of Recently when nobody will notice it.

