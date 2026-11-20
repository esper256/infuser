extends SceneTree

class StubChannel:
	extends WebRTCDataChannelExtension

class StubConnection:
	extends WebRTCPeerConnectionExtension
	func _initialize(_config: Dictionary) -> Error:
		return OK
	func _get_connection_state() -> int:
		return WebRTCPeerConnection.STATE_NEW
	func _create_data_channel(_label: String, _options: Dictionary) -> WebRTCDataChannel:
		return StubChannel.new()

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
	var peers = load("res://addons/infuser/peers.gd").new()
	peers.room = {"host_id": "host"}
	assert(peers._number("host") == 1)
	var seen := {}
	for i in range(7):
		var godot_id: int = peers._number("member-%d" % i)
		assert(godot_id >= 2 and godot_id <= 2147483646)
		assert(not seen.has(godot_id))
		seen[godot_id] = true
	peers.free()
	assert(ClassDB.class_exists("WebRTCMultiplayerPeer"))
	var server := WebRTCMultiplayerPeer.new()
	assert(server.create_server() == OK)
	assert(server.is_server_relay_supported())
	for id in [2, 3, 4]:
		var connection := StubConnection.new()
		assert(connection.initialize({"iceServers": []}) == OK)
		assert(server.add_peer(connection, id) == OK)
		assert(server.has_peer(id))
	# Godot logs these refusals. They are the assertions, and the script continues.
	assert(server.add_peer(StubConnection.new(), 1) != OK)
	server.remove_peer(3)
	assert(not server.has_peer(3))
	var returned := StubConnection.new()
	assert(returned.initialize({"iceServers": []}) == OK)
	assert(server.add_peer(returned, 3) == OK)
	server.close()
	for id in [2, 3, 4]:
		var client := WebRTCMultiplayerPeer.new()
		assert(client.create_client(id) == OK)
		assert(client.is_server_relay_supported())
		var host_connection := StubConnection.new()
		assert(host_connection.initialize({"iceServers": []}) == OK)
		assert(client.add_peer(host_connection, 1) == OK)
		assert(client.add_peer(StubConnection.new(), 6) != OK)
		client.close()
	assert(WebRTCMultiplayerPeer.new().create_client(1) != OK)
	var mesh := WebRTCMultiplayerPeer.new()
	assert(mesh.create_mesh(4) == OK)
	assert(not mesh.is_server_relay_supported())
	mesh.close()
	assert(SceneMultiplayer.new().server_relay)
	print("PASS: host relay with three members, distinct peer ids, and a freed seat.")
	quit()
