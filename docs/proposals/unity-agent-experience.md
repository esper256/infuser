# Unity integration: agent experience review

Audience: Infuser developers.

This is an implementation review, not instructions for game authors. Authors use
the [creation guide](../create-game.md), [Unity recipe](../engines/unity-web.md),
and [publishing checklist](../agents.md).

## Implementing-agent wishlist

| Need | Work and acceptance |
| --- | --- |
| Find the correct instructions from any entry point | Distinguish creating, integrating, publishing, operating, and developing Infuser. Engine recipes must not demand credentials during design. |
| Publish a game its owner can actually play | Normal publication is the default; `--preview` is explicitly an admin-only fixture option. |
| Install the adapter with Unity's existing tools | Provide a UPM package with stable asset metadata and separate runtime/Editor assemblies; preserve the copy-folder option. Resolve bridge assets for both layouts. |
| Use normal C# objects for common features | Typed presence, peer state, dedicated join/resume, and text/binary messages. Keep raw JSON access and existing methods compatible. |
| Keep save failures recoverable | Validate local save inputs before retaining a pending checkpoint; preserve exact retry data and explicit conflict resolution. Test the C# save helper itself. |
| Know whether networking really stopped | Cancel pending admission, close connections, and release the dedicated runtime lease on deliberate leave. Fence late callbacks. |
| Run useful checks without an Infuser toolchain | Include Unity Editor tests and an export validation command for an existing project. Platform developers additionally compile against explicit test doubles outside Unity. |
| Know what remains unverified | Track real Editor/IL2CPP import, build, browser callbacks, and two-client relay acceptance separately from compiler/JS fixtures. |

The adapter remains experimental until actual Unity acceptance passes. No new
cloud-save format, gameplay transport, or SDK major is introduced by this work.
Offline save journals, host migration, and automatic adapters for arbitrary Unity
networking packages require separate designs and are outside this review.

## Delivered and checked

All wishlist implementation items above are included. The engine pages route
creation and publishing separately; the Unity recipe defaults to visible
publication. Public design records describe platform behavior rather than one
installation's status or a private conversation. Repository-relative links were
checked across the documentation set.

The package has stable metadata and runtime/Editor/test assembly definitions.
Its export hook resolves copied assets and UPM cache paths. The publisher catches
missing bridges for either installation. C# typed helpers preserve raw APIs;
optional targets are omitted rather than serialized as empty Unity objects.
Dedicated leave releases its lease after disconnect, prevents overlapping rejoin,
and cancels late admission; transport loss retains the resume reservation.

Verified on 2026-11-16:

- C# adapter compilation in Editor and WebGL conditional configurations against
  explicit test doubles, plus save validation/retry/conflict and export-hook checks.
- JavaScript SDK suites, including exact-target joins, text/binary transport,
  late admission, congestion, and leave/rejoin lifecycle checks.
- Publisher journey with copied and UPM adapters; real Chromium/HTTPS bridge
  journey with cloud saves, conflicts, guests, compression, and expired identity.
- Rust formatting and Clippy.

Not verified: importing this package into Unity, running its Editor tests,
IL2CPP builds, stripping, or playing a compiled Unity game. The Editor acceptance
suite and reference project are supplied for those checks. Follow
[Unity adapter checks](../development.md#unity-adapter-checks) and record actual
results before changing the support label. No Unity Editor/license or compiled
game was available for this review.
