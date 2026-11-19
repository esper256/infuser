# SDK 1.0 decisions

Audience: Infuser developers.

Next review: [stabilization plan](proposals/sdk-1.0-stabilization.md). It proposes
the release boundary, remaining acceptance evidence, and implementation order;
it does not mark the candidate stable.

**Accepted direction; SDK 1.0 is still a candidate.** Keep publishing and learning
from real games. A small catalog makes coordinated migrations possible, but does
not justify silently changing identities, save ownership or session semantics.
The detailed [dedicated-server design](proposals/sdk-1.0-dedicated-servers.md)
remains the implementation proposal; its illustrative field and method names are
not a published stable contract.

Track implementation and verified milestones in the
[dedicated SDK build plan](proposals/dedicated-sdk-implementation.md).

Before new game-facing capabilities ship, record their request/response shapes,
errors, ownership, retry behavior and compatibility in the relevant contract page.
A short schema/example review and a focused journey are sufficient. This does not
introduce a new CI or approval process for ordinary development.

The durable decisions are:

- [Temporary visits](proposals/temporary-visits.md) separate permanent home
  membership from temporary play authorization. Only the operator arranges visits
  and selects individual visitors and brought games. A borrowed deployment starts
  fresh once, then always resumes the same host-group/game progress on later visits.
  Games created by visitors remain home-owned and become temporarily available to
  the visit automatically. Residents keep chat history; visitors receive only their
  active admission window. This is implemented as an opt-in preview; transport
  limits and remaining hardware acceptance are recorded in the visit design.
- Player, guest, launch, backend lease, runtime instance, gameplay session and
  connection are distinct identities. Existing IDs and save namespaces retain
  their meanings. All authority is scoped to the authenticated game/private group.
- Infuser verifies identity and platform access. The game owns its simulation,
  room creation and final atomic admission. Presence is a discovery hint, not a
  seat reservation or an invitation.
- Each peer host can create an independent room. A dedicated game can run
  multiple sessions inside one server. For co-op, the game's Play button starts
  a fresh joinable room and Play Solo is an unchecked option. Friends can join a
  specific player from the Library. Its primary Play still opens the game.
  Playing alone may require the backend and does not imply creating a private
  multiplayer room. The game declares the actual activity and joinability.
- A targeted join must enter the observed session or fail visibly. It must not
  create a replacement world. Discovery and permission are separate: an unlisted
  room is not necessarily invitation-only, and Join Friend does not bypass access.
- Client/server artifacts activate as a matched release. Game updates drain
  existing runs. A code rollback cannot silently reverse a data migration.
- Personal JSON/binary checkpoints preserve both sides of a conflict. Guest saves
  remain temporary and isolated. Server-owned world state has separate authority.
- Existing game-facing contracts keep working through compatibility adapters.
  Additions are capability-gated; incompatible behavior requires a separately
  versioned contract. Do not rely on the number of uploaded games as a substitute.

Stabilize the dedicated Node/WebSocket path against an independently authored game and a small fixture
with different admission behavior before declaring these new capabilities stable.
Capture publication inputs now as [private importer fixtures](publication-capture.md),
so future importer changes can be tested against actual agent output. Capture is
not a substitute for preserving runtime SDK behavior or saved data.
