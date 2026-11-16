# Private publication capture

Audience: server operators.

Infuser can preserve publication requests for diagnosing problems and testing
future importer changes.
This is an opt-in diagnostic archive, not an audit log, a game backup, or a queue
that replays against production.

Enable it in the installation's private controller configuration:

```toml
[controller]
publish_capture_mb = 2048
```

The default is zero (off). Restart the controller after changing it. Upgrade the
binary before adding this setting; older binaries do not recognize it.

## What is preserved

For authorized mutations under `/api/publisher/apps/{app}` and
`/api/metadata/apps/{app}`, capture stores the exact request body before parsing.
This includes original manifest JSON, invalid/unsupported inputs, web and native
artifact uploads, publication, metadata, artwork, tags, slug edits and deletion
requests. Identical bodies share a SHA-256-addressed blob.

Each record has an ordered sequence, timestamp, controller build, method, route,
path, app, private group, body hash and resulting HTTP status. Only Content-Type,
Content-Encoding and the artwork prompt revision header are retained. URL query
strings, Authorization, cookies, IP addresses, emails from authentication,
credential exchanges and response bodies are not recorded. Raw game content may
still contain a secret an author accidentally embedded in a file; treat the whole
archive as private source material.

Requests are written before their handler runs. A null outcome means interruption
or failure to record the response, not proof that the action failed. Bodies that
cannot be completely read within the upload limit are not captured. Requests made
before capture was enabled, during disabled periods, or after storage failures are
not recoverable from this archive. A metadata patch alone is not a complete game.
References to external server images are preserved as references, not copied image
layers. Replay may need those dependencies separately. Sequence numbers record
capture order; concurrent requests can finish in a different order.

## Storage and visibility

The archive is `STATE_DIR/publish-captures/requests.sqlite`, separate from live
catalogs and save data. The directory is mode 0700 and the database is mode 0600.
There is no web API to download it. Access requires the host/operator account.
It can contain games from every private group and must not be handed to a family
admin or a game-specific agent wholesale.

The configured limit counts distinct request bodies plus record overhead. SQLite
has additional physical overhead; the existing host disk reserve also applies.
When the budget/reserve is reached, capture pauses, counts skipped requests, and
keeps the earlier samples. It does not silently evict the beginning of a publication
sequence. Publishing continues. Storage errors also leave publishing available.
A warning is logged once per daemon process when a gap is noticed.

Captured requests return `X-Infuser-Capture: recorded` and a local capture ID.
`incomplete` means capture was skipped or its outcome could not be recorded. These
headers are diagnostic; agents must use the actual publication response to decide
whether publishing succeeded.

## Export for offline testing

Run as the service user or operator, choosing a new directory:

```sh
infuser capture-export /etc/infuser/infuser.toml /private/import-fixtures
```

The export takes a consistent database snapshot while the daemon runs and writes:

- `capture.json`: format version, request count, logical bytes and known skips.
- `requests.jsonl`: records in sequence order; unknown outcomes stay explicit.
- `bodies/SHA256`: exact request bytes, verified against their filenames.

The export refuses an existing destination. It does not send any requests, start
containers, activate builds or clear the archive. Keep it outside the repository.
To resume after the cap is reached, export and raise the configured cap within the
host's available storage, or archive/reset the capture directory during a planned
controller stop. Do not move a live SQLite archive underneath a running writer.
