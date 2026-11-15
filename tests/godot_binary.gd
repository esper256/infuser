extends SceneTree

func _initialize() -> void:
	var adapter = load("res://addons/infuser/infuser.gd").new()
	var bytes := PackedByteArray()
	for i in range(65537):
		bytes.append(i % 256)
	var encoded: Dictionary = adapter.encode_binary(bytes)
	var decoded: Dictionary = adapter.decode_binary(encoded)
	assert(decoded.ok and decoded.value == bytes)
	assert(adapter.decode_binary(adapter.encode_binary(PackedByteArray())).value.is_empty())
	assert(not adapter.decode_binary({"base64": 3}).ok)
	assert(not adapter.decode_binary({"base64": "AA==", "other": true}).ok)
	assert(not adapter.decode_binary({"base64": "AB=="}).ok)
	adapter.free()
	print("PASS: Godot binary helpers preserve every byte and reject invalid envelopes.")
	quit()
