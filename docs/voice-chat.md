# Voice chat

Audience: server operators. Game authors do not need to integrate voice.

Voice chat is optional. When configured, the Library shows a headset toggle
beside text chat along with a Voice mixer button (and a quick mic mute button
while connected). On joins the group's voice conversation and captures the
microphone; off stops capture and listening. A small headset badge identifies
connected profiles, and an active speaking ring highlights who is talking.

Players can adjust their microphone gain (0%–200% with a live level meter and
optional pre-call mic test), master voice volume (0%–200%), per-participant
volume and local mute (in the Voice mixer or a friend's avatar card), quick
mute/deafen, and browser audio options (device selection, auto gain control,
noise suppression, and echo cancellation). Gain, volume, per-participant mix,
and processing preferences are saved per profile in SQLite; mic mute and
deafen reset to unmuted on a fresh join. There are no room menus, recordings,
video, or game-specific channels. Refreshing or leaving the Library disconnects;
launching a game in another tab leaves the Library's voice connection running.

While a player has an active voice connection in their Library tab, non-guest
game tabs show compact Mute and Voice controls in the slim player header bar
above the game. Adjustments sync to the owning Library tab through
`/_infuser/voice` (authenticated by a per-page token consumed by the player shell
before the game iframe loads) and `/api/voice/control`; the game bar displays
the state confirmed applied by the Library tab via `/api/voice/ack`, and game
origins can never acquire the microphone or mint LiveKit tokens.

The connected tab plays a soft rising three-note chime for joins and a falling
chime for departures, including its own connection/disconnection. These tones are
local playback, never a published audio track. All tones share a four-second
cooldown; excess events are dropped rather than queued. Existing participants
are not announced individually on entry, and reconnects suppress roster sounds
for two seconds. Other tabs and profiles outside voice remain silent.

One profile may hold one voice connection. Other tabs show its status and can turn
it off. They never automatically acquire the microphone. Joining another profile
or visit requires turning voice on again. Browser microphone permission is required.
Mobile operating systems may suspend background browser audio; the website cannot
promise native-app background behavior.

## Install the optional service

Use LiveKit server **1.13.11** and the pinned image in
[service images](../deploy/services/compose.yaml). Its image includes ARM64.
Keep it running; rooms are created on demand and disappear when empty. Redis,
recording services and the LiveKit cloud service are not required.

1. Generate a dedicated random API key and at least 32 random secret characters.
   Keep a JSON file readable by the controller, with keys `api_key` and
   `api_secret`, under `/etc/infuser/voice/credentials.json`. Mode `0640`, owner
   `root:infuser`, is suitable. Never reuse the control token or TURN secret.
2. Make a **private** copy of [LiveKit configuration](../deploy/services/voice.example.yaml).
   Replace the public IP and key/secret with this installation's values. Store it
   at `/etc/infuser/voice/livekit.yaml`, readable by container UID 65532 only
   (or its assigned group), with no world access. Protect the parent directory.
3. Combine [service images](../deploy/services/compose.yaml) with a private copy of
   [voice Compose configuration](../deploy/services/voice.compose.example.yaml).
   Validate with `docker compose ... config --quiet`, then start **only** `livekit`.
   The example limits are a starting allocation, not a measured capacity guarantee.
4. Allow TCP 7881 and UDP 7882 through the cloud and host firewalls. Add them to
   Infuser's IP-admission rules when using that protection. They must not overlap
   allocated game ports or TURN. **Never publish API port 7880 beyond loopback.**
5. After installing a controller release that supports voice, add:

   ```toml
   [controller.voice]
   port = 7880
   key_file = "/etc/infuser/voice/credentials.json"
   ```

   Restart through idle maintenance. The toggle appears after reconciliation
   succeeds. An older controller rejects this new configuration; remove the voice
   section and stop the sidecar before rolling back to a release without voice.

The existing Caddy site routes `/voice/rtc` to Infuser. Do not add a public proxy
straight to LiveKit, including its validation or administrative endpoints.

Keep fixed-service changes under [Ansible host maintenance](ansible-operations.md):
add the two private configuration files as explicit `infuser_extra_files` entries,
select only `livekit` in its stack, and retain the private Compose override. The
tracked image participates in the existing dependency update lane. Config copies
must preserve their restrictive ownership; do not put credentials in inventory
tracked by Git. Review and apply the inventory during idle maintenance.

## Connectivity and operations

WebRTC normally uses UDP 7882, with TCP 7881 fallback. Restricted networks can
add TURN through LiveKit's `rtc.turn_servers` configuration. Treat that as an
explicit deployment choice: an existing game relay may deliberately deny access
to this host's private addresses. Do not weaken its private/metadata address deny
rules to make voice work. Test the selected relay path before claiming support.

Inspect the LiveKit container log and controller journal for failures. The
controller logs reconciliation failures without tokens. Do not enable access logs
that record `/voice/rtc` query strings: these contain bearer credentials. Service
health appears in the authenticated operator health response under `voice`.

Normal automatic updates wait while voice leases are active. The existing
operator override can interrupt them. A restart disconnects voice, clears old
rooms before accepting new joins, and requires users to opt in again. Voice
membership is temporary, contains no saved audio, and needs no database migration
or backup. Monitor memory on the actual host before raising service allocations.

## Authorization boundary

Infuser grants access using the existing conversation/channel policy. The default
chat channel has one voice room; room identity is separate from private-group
identity so future channels need no token-format redesign. Inspection cannot join.
Visitor grants remain tied to the exact visit and end with it.

A browser gets a signed, microphone-only token for one opaque participant and room.
It cannot publish camera/screenshare/data tracks, administer rooms, change its
identity, or choose another room. The participant identity is newly generated on
every admission; reusing a request ID after leaving never revives an old token.

Every initial connection and reconnect passes through Infuser's gateway. Even a
LiveKit-refreshed token needs a live lease and current channel access. The sidecar's
control API is private, and only the gateway's two exact signaling paths are public.
Cookie and Infuser control credentials are never forwarded to the sidecar.

The owning tab renews its lease every 20 seconds; it expires after 90 seconds
without renewal. Off revokes admission before asking LiveKit to remove the
participant. Reconciliation runs every two seconds, rechecks access, removes
unauthorized media participants, and deletes orphan rooms. Closing a WebSocket
alone is insufficient because media has its own transport.

If LiveKit's administrative API fails, Infuser pauses admission and signaling and
retries removal. A network-partitioned sidecar can retain existing media until
removal succeeds or its own connection timeout fires; token expiry alone is not a
hard media cutoff. Keep both services on the same host/private network and include
this failure case in operational monitoring. See the
[developer acceptance journey](developer-testing.md#voice-chat) before rollout.
