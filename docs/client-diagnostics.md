# Read the private diagnostic timeline

Audience: Infuser operators and their SSH agents.

Start on the host with the player's profile and the approximate time something went wrong.

```sh
infuser-logs incident --profile Astra --at '2026-11-23T20:10:00-08:00' --window 10m
infuser-logs --profile Astra --kind link --kind op --src client --since 2h
infuser-logs summary --game moon --since 1d --json
infuser-logs memory --leaks --profile Astra
```

## Commands and filters

- The default timeline uses Pacific time (PST or PDT), current profile names and short IDs. `--profile` includes the player's rows and shared events with no assigned profile. Use a full profile ID if the name is ambiguous. `--no-shared` removes unattributed events. `--game` filters by app ID. Unknown games and profiles produce errors.
- `incident` shows the timeline on both sides of `--at` (ten minutes per side by default). It adds explanations for observed controller restart windows, link outages and failed or abandoned operations. The first reference to a runtime attempt includes the retained container outcome if available. Look up the same attempt in server diagnostics for container output.
- `summary` ranks error fingerprints with counts, games, builds and affected profiles. It totals observed link downtime by connection and reason, and shows operation count, failures, abandoned attempts and nearest-rank p50/p90 durations for successful attempts. The failure rate is failed attempts divided by all recorded attempts. Unreached fetches are grouped by method, route, status or error class, with controller restart correlations marked. Server-handled failures appear under failed calls.
- `memory` groups heap measurements by profile, game, tab and page. `memory --leaks` shows positive slopes, fastest first in MiB/hour. The slope uses all samples in the window. Check the sample count and duration before interpreting growth as a leak.
- For any command, combine `--kind 'voice.*'` (repeat for alternatives), `--level warn` or `--src client|server` (`--source` also works). The raw timeline accepts inclusive `--since` and `--until` bounds in seconds, minutes, hours or days before now (such as `45m`), Unix milliseconds, or ISO dates/times. Times without an offset use Pacific time. Specify an offset during a daylight-saving clock change.
- `--json` returns a JSON array or a structured report. `--jsonl` streams timeline or incident rows one per line. Raw events include full IDs, receive times and recorded versions. `infuser-logs --help` lists the options.

## What the signals mean

| Signal | What to look for |
| --- | --- |
| `page.load`, `page.exit` and lifecycle | Load cause distinguishes navigation, reload, an update reload and a browser discard. The Library includes the applied theme ID and its source. Exit lists active links. Visibility, freeze, resume, network status and restoration provide context. Some browsers report a discard only on the next load. |
| `link` | Voice, voice control, chat, multiplayer and launch connections record state changes. Stable polls are silent. A quick reversal within five seconds can fold into one row with a flap count. Completed down/degraded intervals include elapsed downtime. An open outage has no known end. |
| `op` | Voice join, multiplayer join and game load record once when they finish. Inspect outcome, last stage, elapsed time and time spent in each stage. A page exit abandons unfinished operations. A hard kill can prevent the last record. |
| Game load and vitals | Frame load, observed WebAssembly initialization and the following animation frame measure browser work. The first frame is a timing proxy for rendering. Resource timing adds transfer bytes, cache-hit percentage and longest request duration. Visible pages report DOM count, supported heap sizes, and observed long-task count and maximum duration every five minutes. Only games sample frame rate. Browsers and engines differ in which measurements they expose. |
| `fetch.fail`, `api.result` | Network errors and HTTP 502/503/504 responses without the controller's request header appear on the client. Matching consecutive failures carry a count. The server records handled failures and slow requests with a route template and request reference. Failure during an observed graceful stop/start interval is marked on read. The timing alone cannot establish a cause. |
| Server events | Match `launch.end` by launch reference and reason. Match room membership and changes between direct and relayed paths by room, and voice grants/releases/expiries by session. Voice lease events report `heartbeat_age_ms` in milliseconds at one-second resolution. Publication events explain queued or failed activations. Push delivery failures have a status class and a gone flag. Appearance saves record theme and scheme kind. |

`at` and `received_at` are Unix milliseconds. Browsers send immediately using their last clock estimate and refresh it from the batch response. Use page and sequence for ordering within a page and `uncertainty_ms` for close events across machines. Late arrivals can appear on a subsequent query. A missing event can mean a closed tab, lost connectivity, expired authorization or a process that could not shut down gracefully.

## Privacy, storage and installation

Browser events accept structured fields. Errors carry a fingerprint, exception class and available source basename and line/column. Up to five stack frames retain only basename:line:column. Message text, raw stacks, URL queries and fragments, IP addresses, credential values, device labels and custom colours have no diagnostic fields. A publisher can read a limited projection for its own game. Library activity, voice history, profile and group identity stay private. Full timelines are available over SSH.

The Ansible host role installs `/usr/local/bin/infuser-logs`. For a manual installation, copy `deploy/infuser-logs` there with mode 0755. It forwards arguments to `sudo -u infuser -- /usr/local/bin/infuser logs /etc/infuser/infuser.toml`. Use the underlying command for a custom configuration.

The private SQLite database lives beside the application state directory, normally in `/var/lib/infuser-diagnostics`. The systemd unit creates this directory. A custom `controller.diagnostics_dir` must be outside the application state directory and writable inside the service sandbox. The retention window is **48 hours**. The **512 MiB** database cap guards against runaway volume. Size pressure can evict older rows early. A pinned WAL, a full queue or the existing per-profile backstop can cause dropped events. The normal application backup excludes diagnostics. Whole-volume snapshots include them.

The browser keeps a small, expiring outbox per origin. Resending requires the same authorized profile and visit or game launch. Private-group operator inspection is marked separately.
