extends Node
## Experimental two-player Godot adapter using Infuser's existing star signaling.
## No host migration: losing the host/lease ends the shared session.
signal ended(reason: String)
signal changed

var room: Dictionary = {}
var rtc: WebRTCMultiplayerPeer
var _links: Dictionary = {}
var _live := false
var _joining := false
var _run := 0
var _cursor := 0
var _lease_until := 0
var _next_poll := 0
var _polling := false
var _ice: Array = []
var _policy := "all"
var _refresh_at := 0

func is_active() -> bool:
	return _live and Time.get_ticks_msec() < _lease_until

func _current(id: String, link: Dictionary) -> bool:
	return is_active() and _links.get(id) == link

func _request(path: String, method: String = "GET", body: Variant = null) -> Dictionary:
	return await Infuser.call_api("peers", {"path": path, "method": method, "body": body})

func join(relay_only: bool = false) -> Dictionary:
	if _live or _joining:
		return {"ok": false, "error": "Already joining or playing."}
	_joining = true
	_run += 1
	var run := _run
	var sent := Time.get_ticks_msec()
	var result := await _request("/", "POST", {"connection_id": Infuser.uuid(), "relay_only": relay_only, "joinable": true})
	_joining = false
	if run != _run:
		if result.ok:
			_request("/" + str(result.value.self_id), "DELETE")
		return {"ok": false, "error": "Join cancelled."}
	if not result.ok:
		return result
	room = result.value
	_ice = room.ice_servers
	_policy = room.ice_transport_policy
	_cursor = 0
	_lease_until = sent + int(room.lease_ms)
	_refresh_at = sent + 1200000
	_next_poll = 0
	_polling = false
	_live = true
	rtc = WebRTCMultiplayerPeer.new()
	var err: int = rtc.create_server() if room.role == "host" else rtc.create_client(_number(room.self_id))
	if err != OK:
		close("Couldn't create the multiplayer connection.")
		return {"ok": false, "error": "WebRTC initialization failed."}
	multiplayer.multiplayer_peer = rtc
	rtc.peer_connected.connect(func(_id: int):
		Infuser.network("connected")
		changed.emit())
	rtc.peer_disconnected.connect(func(_id: int):
		Infuser.network("connecting")
		changed.emit())
	Infuser.network("connecting")
	changed.emit()
	return {"ok": true, "value": room}

func _number(id: String) -> int:
	# Godot reserves 1 for the host. IDs are session-local, not player identity.
	return 1 if id == room.host_id else 2 + (id.sha256_text().substr(0, 7).hex_to_int() % 2147483645)

func _process(_delta: float) -> void:
	if not _live:
		return
	if not is_active():
		close("Connection lost. Join again from the title screen.")
		return
	if not _polling and Time.get_ticks_msec() >= _next_poll:
		_poll()

func _poll() -> void:
	_polling = true
	var run := _run
	var sent := Time.get_ticks_msec()
	var result := await _request("/" + str(room.self_id) + "?after=" + str(_cursor))
	if run != _run or not is_active():
		return
	_polling = false
	_next_poll = Time.get_ticks_msec() + 750
	if not result.ok:
		if result.get("status", 0) in [401, 403, 404, 409, 410]:
			close(result.error)
		return # Brief network failures may recover within this lease.
	room.merge(result.value, true)
	_lease_until = sent + int(room.lease_ms)
	var present: Array = room.peers.map(func(p: Dictionary): return p.id)
	for id: String in _links.keys():
		if id not in present:
			_drop(id)
	for message: Dictionary in result.value.signals:
		_receive(message)
		if not is_active():
			return
		_cursor = int(message.sequence)
	if room.role == "host":
		for peer: Dictionary in room.peers:
			var id: String = peer.id
			if id == room.self_id:
				continue
			if not _links.has(id):
				var link := _connect(id, Infuser.uuid())
				if not link.is_empty() and link.pc.create_offer() != OK:
					close("Couldn't start the peer connection.")
	for link: Dictionary in _links.values():
		if Time.get_ticks_msec() - int(link.created) > 20000 and link.pc.get_connection_state() != WebRTCPeerConnection.STATE_CONNECTED:
			close("Couldn't reach the other player. Join again.")
			return
	if sent >= _refresh_at:
		_refresh_at = sent + 1200000
		var fresh := await _request("/" + str(room.self_id) + "/ice", "POST")
		if run == _run and is_active() and fresh.ok:
			_ice = fresh.value.ice_servers
			if room.role == "host":
				# Renew allocations before the old TURN credentials expire.
				for id: String in _links.keys():
					_drop(id)

func _connect(id: String, generation: String) -> Dictionary:
	_drop(id)
	for other: String in _links:
		if _number(other) == _number(id):
			close("Peer ID collision. Start a new shared session.")
			return {}
	var pc := WebRTCPeerConnection.new()
	var link := {"pc": pc, "generation": generation, "queue": [], "sending": false, "created": Time.get_ticks_msec()}
	if pc.initialize({"iceServers": _ice, "iceTransportPolicy": _policy}) != OK:
		close("WebRTC configuration failed.")
		return {}
	_links[id] = link
	pc.session_description_created.connect(_description.bind(id, link))
	pc.ice_candidate_created.connect(_candidate.bind(id, link))
	# Godot creates its reliable/unreliable/ordered channels before negotiation.
	if rtc.add_peer(pc, _number(id)) != OK:
		close("Couldn't attach the Godot multiplayer peer.")
		return {}
	return link

func _description(type: String, sdp: String, id: String, link: Dictionary) -> void:
	if not _current(id, link):
		return
	if link.pc.set_local_description(type, sdp) != OK:
		close("Couldn't negotiate multiplayer.")
		return
	_queue(id, link, {"type": type, "description": {"type": type, "sdp": sdp}})

func _candidate(media: String, index: int, candidate: String, id: String, link: Dictionary) -> void:
	_queue(id, link, {"type": "candidate", "candidate": {"sdpMid": media, "sdpMLineIndex": index, "candidate": candidate}})

func _queue(id: String, link: Dictionary, payload: Dictionary) -> void:
	if not _current(id, link):
		return
	payload["generation"] = link.generation
	link.queue.append(payload)
	if not link.sending:
		_flush(id, link)

func _flush(id: String, link: Dictionary) -> void:
	link.sending = true
	while _current(id, link) and not link.queue.is_empty():
		var result := await _request("/" + str(room.self_id) + "/signals", "POST", {"epoch": room.epoch, "to": id, "payload": link.queue[0]})
		if not _current(id, link):
			return
		if not result.ok:
			close("Signaling interrupted. Join again.")
			return
		link.queue.pop_front()
	link.sending = false

func _receive(message: Dictionary) -> void:
	if not is_active():
		return
	var id: String = message.from
	if not room.peers.any(func(p: Dictionary): return p.id == id):
		return
	var payload: Dictionary = message.payload
	var generation: String = payload.get("generation", "")
	if generation.is_empty() or generation.length() > 64:
		return
	if payload.type == "offer" and room.role == "member" and id == room.host_id:
		var link := _connect(id, generation)
		if not link.is_empty():
			link.pc.set_remote_description("offer", payload.description.sdp)
	elif _links.has(id) and _links[id].generation == generation:
		var pc: WebRTCPeerConnection = _links[id].pc
		if payload.type == "answer" and room.role == "host":
			pc.set_remote_description("answer", payload.description.sdp)
		elif payload.type == "candidate":
			var c: Dictionary = payload.candidate
			pc.add_ice_candidate(c.sdpMid, int(c.sdpMLineIndex), c.candidate)

func _drop(id: String) -> void:
	if _links.has(id):
		rtc.remove_peer(_number(id))
		_links[id].pc.close()
		_links.erase(id)

func close(reason: String = "Left multiplayer.") -> void:
	_run += 1
	if not _live:
		return
	_live = false
	for id: String in _links.keys():
		_drop(id)
	multiplayer.multiplayer_peer = OfflineMultiplayerPeer.new()
	rtc.close()
	_request("/" + str(room.self_id), "DELETE")
	Infuser.network("ended")
	ended.emit(reason)
