# Publish using tools already on the machine

The publishing API is a supported route, not an emergency workaround. An agent
on macOS or Windows can publish with `curl` and its existing JSON/file tools,
without installing Infuser, Rust, Docker, or a Linux environment. This replaces
the **upload tool**, not the game's engine, build, or SDK integration. Follow the
[engine recipe](agents.md) first, including saves and explicit multiplayer modes.

The examples below use a POSIX shell (including macOS). Windows agents can use
`curl.exe` with equivalent paths/quoting. The agent prepares and reads JSON using
its file tools or an already available project runtime; `jq` and Python are not
prerequisites. Do not parse JSON with regular expressions. Never ask the child to
copy permanent tokens, install a toolchain, or construct these requests manually.

## Connect once

1. Reuse an existing connection to this Infuser if the project has one. Keep its
   immutable app ID even if a new prompt reserves another. Otherwise use the API
   origin and app ID in the copied prompt. Require an HTTPS origin with no path,
   credentials, query, or fragment; do not guess the server from the repository.
2. Ignore `.local/` and `*.credentials.json` in Git **and exports**. Use private
   permissions (`umask 077` before creating files on Mac/Linux; a user-only ACL on
   Windows). Reserve a new `.local/APP.credentials.json` without overwriting an
   existing file, before consuming the setup code. Only one connect attempt may
   run at a time for this game.
3. Write `.local/claim.json` privately with `{"token":"HEX_SECRET","app_id":"APP"}`,
   taking `HEX_SECRET` from the prompt after stripping `infuser_setup_`. Never put
   that value in a command argument, transcript, console output, or Git commit.
4. POST once, capturing the response privately:

```sh
umask 077
# API contains only the HTTPS origin from the prompt; no secret.
curl --silent --show-error --fail --proto '=https' \
  --header 'Content-Type: application/json' \
  --data-binary @.local/claim.json \
  --output .local/claim-response.json "$API/api/publisher/claim"
```

Check exit status before reading JSON. Verify the returned `api_url` and `app_id`
exactly match the requested origin and app ID, and that `token` is a nonempty
string. Save the whole successful response in the reserved credentials file
(mode 600), then remove the temporary claim and response files. The resulting
connection also works with `infuser publish` if the CLI is installed later.

A code is single-use and lasts three hours. A 401 means invalid/already used; 410
means expired. First check for existing credentials before requesting **Refresh
code** for the same app. Do not create a duplicate game. If the request times out
after the server consumed it and no complete response was saved, a fresh setup
code is needed. Do not blindly retry a claim or discard a successful response.

For subsequent requests, create a private `.local/publisher.curl` containing
`header = "Authorization: Bearer TOKEN"`, substituting the stored token using
file tools, not shell interpolation into a visible command. Validate that it is
the expected hexadecimal token; never accept embedded quotes or line breaks.
Keep this file mode 600. Examples use `--config` so the secret stays out of the
process argument list. Do not use `-v`, shell tracing, `--location`, `--insecure`,
or print credentials. Delete temporary auth files when done; retain the permanent
credentials for updates. Use the validated connection's origin/app ID below.

## Prepare the existing game

```sh
curl --silent --show-error --fail --proto '=https' \
  --config .local/publisher.curl \
  --output .local/capabilities.json "$API/api/publisher/capabilities"
```

Check the advertised SDK majors, capabilities, runtime support, and limits.
Use the project's existing build command. Work from its **complete exported web
directory**, never the repository root by accident. For a plain HTML game, stage
only runtime files in a separate export directory. Preserve the game's name.

Prepare `.local/build.json` with the actual game's manifest. Minimal single-player
web example (replace APP and title, add real tags and feature declarations):

```json
{
  "manifest": {
    "schema_version": 1,
    "app_id": "APP",
    "title": "The Game's Existing Name",
    "play_modes": ["solo"],
    "solo_runtime": "client_only",
    "sdk": {"major": 1, "api_major": 1, "required_capabilities": ["identity.v1"]}
  },
  "server": null,
  "spa": false,
  "cross_origin_isolation": false
}
```

Use `spa:true` only for a client-side router that needs an index fallback; follow
the Godot recipe for cross-origin isolation. Do not declare a multiplayer game
as single-player to get it uploaded. Follow [cloud saves](cloud-save-design.md)
and [multiplayer](agents.md#explicit-play-modes) for the actual features.

Ensure the staged UTF-8 `index.html` contains this module script for SDK major 1:

```html
<script type="module" src="/_infuser/sdk/v1.js"></script>
```

The CLI normally injects it; this route must include it explicitly. Do not include
the legacy SDK script as well. Integrate the relevant runtime APIs—the script
alone does not implement saves or multiplayer for the game.

## Upload and publish

Choose a fresh lowercase slug build ID beginning with a letter, at most 48
characters (for example `b-` plus a timestamp and random hex suffix). Keep the
same build ID while resuming this upload; use a new one for the next update.
Set `BUILD_URL` to `$API/api/publisher/apps/$APP/builds/$BUILD` with validated IDs.

```sh
curl --silent --show-error --fail --proto '=https' \
  --config .local/publisher.curl --request PUT \
  --header 'Content-Type: application/json' --data-binary @.local/build.json \
  --output .local/build-response.json "$BUILD_URL"
```

Require success before proceeding. Enumerate **every** runtime file in the export.
Reject symlinks and exclude credentials, `.local`, hidden files, source maps,
`node_modules`, and art masters. Upload only the validated delivery artwork at
`infuser-art/card-hero.webp`; see the [art guide](card-art-guide.md). Check the
[file/count/size limits](public-api.md#publisher-credentials) before uploading.
For each file use its path relative to the export as `UPLOAD_PATH`, URL-encoding
each path segment individually while preserving directory separators. Quote
local paths so spaces work. `EXPORT_FILE` is the corresponding local file path.

```sh
curl --silent --show-error --fail --proto '=https' \
  --config .local/publisher.curl --request PUT \
  --header 'Content-Type: application/octet-stream' --data-binary "@$EXPORT_FILE" \
  --output .local/upload-response.json "$BUILD_URL/files/$UPLOAD_PATH"
```

Check **every** upload's exit status and response. On interruption, GET
`$BUILD_URL` to inspect the inventory and resume the same draft; don't activate a
partial build. Once all files are uploaded, write `.local/publish.json` containing
`{"listed":true}` (`false` for adult-only previews), then:

```sh
curl --silent --show-error --fail --proto '=https' \
  --config .local/publisher.curl --header 'Content-Type: application/json' \
  --data-binary @.local/publish.json \
  --output .local/publish-response.json "$BUILD_URL/publish"
```

Inspect the response: `state:"queued"` means a managed server or peer session is
still active. The update waits for shutdown/session end; do not call it live yet.
GET `$BUILD_URL` for `publication_state` and check the returned canonical game
URL in Infuser. Exercise Play, saves/guest mode, and relevant multiplayer behavior.
After first publication, save a factual description via the [metadata API](game-metadata.md)
and check the cover. Report missing artwork explicitly. An upload alone is not
acceptance. Use the same credentials/app ID and a fresh
build ID for each future “update it on Infuser.”

Report platform trouble following [the agent issue instructions](agents.md#reporting-platform-problems).
Redact tokens, setup prompts, private hostnames, and player information from
public issues; include the failing stage, status, sanitized error, and versions.
