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
var _refreshing := false
var _tick_callback: JavaScriptObject
var _ticker := 0

func _tick(_args: Array) -> void:
	_process(0.0)

func _exit_tree() -> void:
	close()

func is_active() -> bool:
	return _live and Infuser.now_msec() < _lease_until

func _current(id: String, link: Dictionary) -> bool:
	return is_active() and _links.get(id) == link

func _request(path: String, method: String = "GET", body: Variant = null) -> Dictionary:
	return await Infuser.call_api("peers", {"path": path, "method": method, "body": body})

func create(relay_only: bool = false, joinable: bool = true) -> Dictionary:
	return await _enter(relay_only, {}, joinable, true)

func join(relay_only: bool = false, intent: Dictionary = {}, joinable: bool = true) -> Dictionary:
	return await _enter(relay_only, intent, joinable, false)

func _enter(relay_only: bool, intent: Dictionary, joinable: bool, create_room: bool) -> Dictionary:
	if _live or _joining:
		return {"ok": false, "error": "Already joining or playing."}
	_joining = true
	_run += 1
	var run := _run
	var sent: int = Infuser.now_msec()
	var body := {"connection_id": Infuser.uuid(), "relay_only": relay_only, "joinable": joinable}
	if create_room:
		body["create"] = true
	if not intent.is_empty():
		if intent.get("kind") != "join_friend" or not intent.has("session_id") or not intent.has("player_id"):
			_joining = false
			return {"ok": false, "error": "That friend's session is unavailable."}
		body["expected_epoch"] = intent.session_id
		body["expected_player"] = intent.player_id
	var result := await _request("/", "POST", body)
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
	_refresh_at = _renew_at(_ice, sent)
	_refreshing = false
	_next_poll = 0
	_polling = false
	_live = true
	if OS.has_feature("web"):
		# Browsers can pause rendering in a hidden tab. Coordinator renewal
		# must not depend on a game frame; long/full suspension still fences.
		_tick_callback = JavaScriptBridge.create_callback(_tick)
		_ticker = Infuser.start_ticker(_tick_callback)
	rtc = WebRTCMultiplayerPeer.new()
	var err: int = rtc.create_server() if room.role == "host" else rtc.create_client(_number(room.self_id))
	if err != OK:
		close("Couldn't create the multiplayer connection.")
		return {"ok": false, "error": "WebRTC initialization failed."}
	multiplayer.multiplayer_peer = rtc
	_bind_rtc()
	Infuser.network("connecting")
	changed.emit()
	return {"ok": true, "value": room}

func set_joinable(accepting: bool) -> Dictionary:
	if not is_active() or room.role != "host":
		return {"ok": false, "error": "Only the active host can change who may join."}
	return await _request("/" + str(room.self_id) + "/joinability", "PUT", {"epoch": room.epoch, "joinable": accepting})

func _bind_rtc() -> void:
	rtc.peer_connected.connect(func(_id: int):
		Infuser.network("connected")
		changed.emit())
	rtc.peer_disconnected.connect(func(_id: int):
		Infuser.network("connecting")
		changed.emit())

func _number(id: String) -> int:
	# Godot reserves 1 for the host. IDs are session-local, not player identity.
	return 1 if id == room.host_id else 2 + (id.sha256_text().substr(0, 7).hex_to_int() % 2147483645)

func _process(_delta: float) -> void:
	if not _live:
		return
	if not is_active():
		close("Connection lost. Join again from the title screen.")
		return
	if not _polling and Infuser.now_msec() >= _next_poll:
		_poll()
	if not _refreshing and Infuser.now_msec() >= _refresh_at:
		_refresh_ice()

func _renew_at(servers: Array, now: int) -> int:
	var deadline := now + 1200000
	for server: Dictionary in servers:
		if server.has("expires_at"):
			deadline = mini(deadline, int(server.expires_at) * 1000 - 120000)
	return maxi(now + 1000, deadline)

func _refresh_ice() -> void:
	_refreshing = true
	var run := _run
	var fresh := await _request("/" + str(room.self_id) + "/ice", "POST")
	if run != _run or not is_active():
		return
	_refreshing = false
	_refresh_at = Infuser.now_msec() + 5000
	if not fresh.ok:
		if fresh.get("status", 0) in [401, 403, 404, 409, 410]:
			close(fresh.error)
		return
	_ice = fresh.value.ice_servers
	_refresh_at = _renew_at(_ice, Infuser.now_msec())
	if room.role == "host":
		# New connections allocate with fresh TURN credentials. The host owns
		# negotiation; members never create a replacement shared session.
		for id: String in _links.keys():
			_drop(id)

func _poll() -> void:
	_polling = true
	var run := _run
	var sent: int = Infuser.now_msec()
	var result := await _request("/" + str(room.self_id) + "?after=" + str(_cursor))
	if run != _run or not is_active():
		return
	_polling = false
	_next_poll = Infuser.now_msec() + 750
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
		var state: int = link.pc.get_connection_state()
		if not rtc.has_peer(_number(link.id)):
			state = WebRTCPeerConnection.STATE_FAILED
		if state == WebRTCPeerConnection.STATE_CONNECTED:
			link.disconnected = 0
		elif state in [WebRTCPeerConnection.STATE_DISCONNECTED, WebRTCPeerConnection.STATE_FAILED, WebRTCPeerConnection.STATE_CLOSED]:
			if link.disconnected == 0:
				link.disconnected = Infuser.now_msec()
			if room.role == "host" and Infuser.now_msec() - int(link.disconnected) >= 2500:
				_drop(link.id)
				Infuser.network("connecting")
				continue
		var waiting_since: int = link.disconnected if link.disconnected != 0 else link.created
		if Infuser.now_msec() - waiting_since > 20000 and state != WebRTCPeerConnection.STATE_CONNECTED:
			close("Couldn't reach the other player. Join again.")
			return

func _connect(id: String, generation: String) -> Dictionary:
	var replacing := _links.has(id)
	_drop(id)
	if room.role == "member" and (replacing or rtc.get_connection_status() == MultiplayerPeer.CONNECTION_DISCONNECTED):
		# Godot stops polling a disconnected client MultiplayerPeer. Recreate
		# that local adapter, preserving coordinator identity and session epoch.
		rtc.close()
		rtc = WebRTCMultiplayerPeer.new()
		if rtc.create_client(_number(room.self_id)) != OK:
			close("Couldn't reconnect to the shared game.")
			return {}
		multiplayer.multiplayer_peer = rtc
		_bind_rtc()
	for other: String in _links:
		if _number(other) == _number(id):
			close("Peer ID collision. Start a new shared session.")
			return {}
	var pc := WebRTCPeerConnection.new()
	var link := {"id": id, "pc": pc, "generation": generation, "queue": [], "sending": false, "created": Infuser.now_msec(), "disconnected": 0}
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
			if result.get("status", 0) in [0, 429, 500, 502, 503, 504]:
				await get_tree().create_timer(0.75).timeout
				continue # Keep the exact queued payload after an uncertain send.
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
		if _links.has(id) and _links[id].generation == generation:
			return # An acknowledged-late retry must not reset this connection.
		var link := _connect(id, generation)
		if not link.is_empty():
			link.pc.set_remote_description("offer", payload.description.sdp)
	elif _links.has(id) and _links[id].generation == generation:
		var pc: WebRTCPeerConnection = _links[id].pc
		if payload.type == "answer" and room.role == "host" and pc.get_signaling_state() == WebRTCPeerConnection.SIGNALING_STATE_HAVE_LOCAL_OFFER:
			pc.set_remote_description("answer", payload.description.sdp)
		elif payload.type == "candidate":
			var c: Dictionary = payload.candidate
			pc.add_ice_candidate(c.sdpMid, int(c.sdpMLineIndex), c.candidate)

func _drop(id: String) -> void:
	if _links.has(id):
		if rtc.has_peer(_number(id)):
			rtc.remove_peer(_number(id))
		_links[id].pc.close()
		_links.erase(id)

func close(reason: String = "Left multiplayer.") -> void:
	_run += 1
	if not _live:
		return
	_live = false
	Infuser.stop_ticker(_ticker)
	_ticker = 0
	for id: String in _links.keys():
		_drop(id)
	multiplayer.multiplayer_peer = OfflineMultiplayerPeer.new()
	rtc.close()
	_request("/" + str(room.self_id), "DELETE")
	Infuser.network("ended")
	ended.emit(reason)
