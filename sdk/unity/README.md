# Infuser Unity Web adapter

Audience: game authors and their agents.

Start with the [Unity recipe](../../docs/engines/unity-web.md). Copy `Assets/Infuser`
into your existing project, then build Web with your existing Unity Editor.
The Editor hook copies the browser bridge into the export automatically.
There is no Rust, Docker, npm, or additional Unity package requirement.

This is an experimental adapter for SDK/API major 1, which is still a preview.
Keep a copy with your game and update deliberately. It does not change the
platform's save, identity, visit, or multiplayer contracts. The C# integration
still needs a real Unity build and playtest; JavaScript tests do not substitute
for that check.

- `InfuserClient`: optional hosted identity, presence, Join Friend, peer messages,
  and a JSON escape hatch for the versioned checkpoint and dedicated-session APIs.
- `JsonSave<T>`: one declared JSON file per group, explicit load/save/retry/conflict
  resolution. Pending writes stay immutable in memory until acknowledged.
- `Infuser.jslib`: ES5 interop only. The separate module uses the browser SDK.
- `InfuserWebBuild`: copies the module after a Web build, without changing templates.

`Event` carries a name and JSON string: `peer_state`, `peer_message`, `peer_end`,
`socket_message`, `socket_end`, `socket_error`, or `session_error`. Subscribe before
joining. Check identity before selecting hosted or standalone behavior; a hosted
error must not select standalone mode. Do not destroy or rename the receiver
during play. No credentials belong in these files.
