# Frozen pre-versioning client

Audience: Infuser developers.

Snapshot of the browser SDK at commit 46d6c7c305f5593eee0d7623e35541a813fd36aa.
Do not update these files to make a compatibility test pass. The real HTTP
compatibility journey executes this old core SDK unchanged against the current
daemon. Keep its identity, save, guest and renewal behavior working as the SDK
and controller evolve. Future frozen versions should get separate fixtures.
