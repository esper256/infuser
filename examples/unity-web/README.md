# Beacon Garden: Unity Web reference project

Audience: game authors and their agents.

`--preview` in this fixture is for admin-only testing. Publish normally when an
ordinary user needs to play; hidden builds are not available to them.

This small source fixture demonstrates personal star checkpoints, a conflict
chooser, guest progress, and a shared peer-hosted beacon. It uses built-in Unity
objects and UI, with no external art or gameplay packages. **It has not yet been
compiled or played in Unity.** Use it as an integration starting point, not proof
of engine compatibility. The target is Unity 6 / 6.3 LTS.

1. Create a separate empty 3D project in your existing Unity Editor, with Web
   Build Support installed. Keep your actual game untouched.
2. Copy this directory's `Assets` into that project's `Assets`. Copy
   [`sdk/unity/Assets/Infuser`](../../sdk/unity/Assets/Infuser), including its metadata,
   there too (or install the package from the [Unity recipe](../../docs/engines/unity-web.md)), and put
   this example's `infuser.game.json` at the project root. Built-in IMGUI, Physics,
   and JSON serialization modules must be enabled (see `Packages/manifest.json`).
3. Choose **Infuser → Build Beacon Garden Web**. This creates a scene and a gzip
   Web export in `export/`. For an existing Editor on an agent's machine, the same
   method is available with `-batchmode -quit -projectPath PROJECT -buildTarget WebGL
   -executeMethod BuildBeaconGarden.Build -logFile BUILD_LOG`.
4. Publish using an Infuser connection for this fixture:
   `infuser publish /private/beacon.credentials.json PROJECT --preview`.
   Keep the fixture unlisted. Its declared multiplayer needs the host's TURN service.

Press Collect, then Save. Reopen to restore personal stars. Guest mode starts
empty and cannot overwrite the owner's stars. Two normal tabs can deliberately
create a conflict; the chooser offers both copies. If an acknowledgement is lost,
Retry sends the same checkpoint. Pending writes do not survive closing the page.

This fixture exercises legacy automatic matchmaking. For a new co-op game's
Play button, use `CreatePeerRoomAsync()` as shown in the [Unity Web recipe](../../docs/engines/unity-web.md#multiplayer-and-join-friend).
In this fixture, Join multiplayer connects to the shared beacon. The host counts taps and sends
the shared pulse; personal stars remain personal. Force relay is a test switch.
Join Friend enters the exact existing session; an expired target returns an
error. The example is deliberately simple: it is not a general-purpose netcode
framework or a high-frequency action-game benchmark.

Outside Infuser, the example uses its separate standalone PlayerPrefs save and
does not start Infuser multiplayer. Hosted failures never fall back to PlayerPrefs.
The receiver and SDK remain alive across scene changes.
