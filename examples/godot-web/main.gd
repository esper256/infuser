extends Node2D
## Disposable integration game. Autoplay is deterministic movement, not an AI.
const PeerAdapter = preload("res://addons/infuser/peers.gd")
const SaveChooser = preload("res://addons/infuser/save_conflict.gd")
var peers: Node
var chooser: Window
var conflict_candidate := ""
var choosing := false
var lanterns := 0
var revision: Variant = null
var saving := false
var pending_checkpoint: Dictionary = {}
var loaded := false
var guest := false
var position_local := Vector2(360, 330)
var position_other := Vector2(600, 330)
var received := 0
var ticks := 0.0
var send_clock := 0.0
var autoplay := false
var force_relay := false
var multiplayer_mode := false
var status: Label
var score: Label
var collect_button: Button
var save_button: Button
var join_button: Button
var leave_button: Button

func _ready() -> void:
	peers = PeerAdapter.new()
	peers.name = "InfuserPeers"
	add_child(peers)
	chooser = SaveChooser.new()
	add_child(chooser)
	chooser.prepared.connect(_report)
	peers.ended.connect(func(reason: String):
		multiplayer_mode = false
		status.text = reason
		join_button.disabled = false
		_report())
	var ui := CanvasLayer.new()
	add_child(ui)
	var column := VBoxContainer.new()
	column.position = Vector2(32, 26)
	column.add_theme_constant_override("separation", 12)
	ui.add_child(column)
	var title := Label.new()
	title.text = "LANTERN LAKE"
	title.add_theme_font_size_override("font_size", 36)
	column.add_child(title)
	score = Label.new()
	score.add_theme_font_size_override("font_size", 24)
	column.add_child(score)
	status = Label.new()
	status.text = "Loading your lanterns…"
	column.add_child(status)
	var row := HBoxContainer.new()
	row.add_theme_constant_override("separation", 12)
	column.add_child(row)
	collect_button = _button(row, "Collect lantern", _collect)
	save_button = _button(row, "Save", _save)
	join_button = _button(row, "Play together", _join)
	leave_button = _button(row, "Leave multiplayer", func(): peers.close())
	var hint := Label.new()
	hint.text = "Arrows: drift · C: collect · S: save · M: multiplayer · L: leave"
	hint.position = Vector2(32, 552)
	ui.add_child(hint)
	var options: Dictionary = Infuser.options()
	autoplay = options.get("autoplay", "") == "1"
	force_relay = options.get("relay", "") == "1"
	var context: Dictionary = await Infuser.call_api("ready")
	if not context.ok:
		status.text = context.error
		return
	guest = context.value.guest
	# Consume once even if loading/choosing saves prevents us acting on it.
	var launch_request: Dictionary = await Infuser.call_api("launch_intent")
	var intent: Dictionary = launch_request.get("value", {}).get("intent") if launch_request.get("value", {}).get("intent") is Dictionary else {}
	var result: Dictionary = await Infuser.call_api("load", {"group": "progress"})
	if not result.ok:
		status.text = result.error
		return
	while not result.value.get("conflicts", []).is_empty():
		intent = {} # Don't surprise-join later after a save decision.
		result = await chooser.choose("progress", result.value.conflicts[0].candidate_revision, _valid_save)
		if not result.ok:
			status.text = result.error + " Reopen to continue."
			return
	revision = result.value.revision
	var saved: Dictionary = result.value.files.get("progress.json", {"schema": 1, "lanterns": 0})
	if saved.get("schema", 0) != 1:
		status.text = "This save needs a different game version."
		return
	lanterns = int(saved.lanterns)
	loaded = true
	collect_button.disabled = false
	save_button.disabled = false
	join_button.disabled = false
	status.text = "Guest adventure — your friend's progress is safe." if guest else "Your adventure is ready."
	_report()
	if not intent.is_empty():
		_join(intent)
	elif options.get("multiplayer", "") == "1":
		_join()

func _valid_save(files: Dictionary) -> bool:
	var progress: Variant = files.get("progress.json", {"schema": 1, "lanterns": 0})
	return progress is Dictionary and progress.get("schema") == 1 and (progress.get("lanterns") is float or progress.get("lanterns") is int)

func _choose_conflict() -> void:
	choosing = true
	collect_button.disabled = true
	save_button.disabled = true
	join_button.disabled = true
	var result: Dictionary = await chooser.choose("progress", conflict_candidate, _valid_save)
	choosing = false
	if result.ok and _valid_save(result.value.files):
		conflict_candidate = ""
		revision = result.value.revision
		lanterns = int(result.value.files.get("progress.json", {}).get("lanterns", 0))
		if not result.value.get("conflicts", []).is_empty():
			conflict_candidate = result.value.conflicts[0].candidate_revision
		status.text = "Saved!" if conflict_candidate.is_empty() else "Another save needs your choice."
	else:
		status.text = result.get("error", "This save needs a different game version.")
	save_button.disabled = false
	collect_button.disabled = not conflict_candidate.is_empty()
	join_button.disabled = multiplayer_mode or not conflict_candidate.is_empty()
	_report()

func _button(row: HBoxContainer, title: String, action: Callable) -> Button:
	var button := Button.new()
	button.text = title
	button.custom_minimum_size.y = 44
	button.disabled = true
	button.pressed.connect(action)
	row.add_child(button)
	return button

func _collect() -> void:
	if loaded and pending_checkpoint.is_empty() and conflict_candidate.is_empty() and not choosing:
		lanterns += 1
		status.text = "A little brighter. Save when you're ready."
		_report()

func _save() -> void:
	if not loaded or saving or choosing:
		return
	if not conflict_candidate.is_empty():
		_choose_conflict()
		return
	saving = true
	save_button.disabled = true
	collect_button.disabled = true
	status.text = "Saving…"
	if pending_checkpoint.is_empty():
		pending_checkpoint = {"expectedRevision": revision, "commitId": Infuser.uuid(), "files": {"progress.json": {"schema": 1, "lanterns": lanterns}}, "summary": str(lanterns) + " lanterns"}
	var result: Dictionary = await Infuser.call_api("commit", {"group": "progress", "checkpoint": pending_checkpoint})
	saving = false
	save_button.disabled = false
	if not result.ok:
		status.text = "Not saved: " + str(result.error)
	elif result.value.status == "conflict":
		pending_checkpoint = {}
		conflict_candidate = result.value.candidate_revision
		status.text = "Another save arrived. Choose which adventure to continue."
		_report()
		_choose_conflict()
		return
	else:
		pending_checkpoint = {}
		revision = result.value.revision
		status.text = "Saved!"
	collect_button.disabled = not pending_checkpoint.is_empty()
	_report()

func _unhandled_key_input(event: InputEvent) -> void:
	if event is InputEventKey and event.pressed and not event.echo:
		match event.keycode:
			KEY_C: _collect()
			KEY_S: _save()
			KEY_M: _join()
			KEY_L: peers.close()

func _join(intent: Dictionary = {}) -> void:
	if not loaded or multiplayer_mode or choosing or not conflict_candidate.is_empty():
		return
	join_button.disabled = true
	status.text = "Finding the shared lake…"
	var result: Dictionary = await peers.join(force_relay, intent)
	if result.ok:
		multiplayer_mode = true
		leave_button.disabled = false
		status.text = "The lake is open. Waiting for a friend…"
	else:
		join_button.disabled = false
		status.text = result.error
	_report()

func _process(delta: float) -> void:
	ticks += delta
	if loaded:
		if autoplay:
			var offset := 0.0 if not multiplayer_mode or multiplayer.is_server() else PI
			position_local = Vector2(480 + cos(ticks + offset) * 170, 370 + sin(ticks * 1.3 + offset) * 65)
		else:
			position_local += Input.get_vector("ui_left", "ui_right", "ui_up", "ui_down") * delta * 180
			position_local = position_local.clamp(Vector2(50, 260), Vector2(910, 510))
	if multiplayer_mode and peers.is_active():
		send_clock += delta
		if send_clock >= 0.1 and not multiplayer.get_peers().is_empty():
			send_clock = 0
			_drift.rpc(position_local)
	queue_redraw()

@rpc("any_peer", "call_remote", "unreliable_ordered", 0)
func _drift(point: Vector2) -> void:
	if not peers.is_active() or not point.is_finite():
		return
	position_other = point.clamp(Vector2(50, 260), Vector2(910, 510))
	received += 1
	status.text = "Drifting together."
	_report()

func _draw() -> void:
	for i in range(7):
		draw_arc(Vector2(480, 420), 110 + i * 32, PI, TAU, 60, Color(0.08, 0.2, 0.29, 0.45), 2)
	for i in range(24):
		var p := Vector2(50 + fmod(i * 139, 860), 265 + fmod(i * 71, 235))
		p.y += sin(ticks + i) * 6
		draw_circle(p, 3, Color(0.95, 0.83, 0.4, 0.5 + sin(ticks + i) * 0.3))
	_lantern(position_local, Color(1, 0.78, 0.3))
	if multiplayer_mode and peers.is_active() and not multiplayer.get_peers().is_empty():
		_lantern(position_other, Color(0.3, 0.86, 1))

func _lantern(p: Vector2, color: Color) -> void:
	for radius in [34, 24, 15]:
		draw_circle(p, radius, Color(color, 0.08))
	draw_circle(p, 8, color)
	draw_line(p + Vector2(-11, 12), p + Vector2(11, 12), color, 3)

func _report() -> void:
	score.text = str(lanterns) + " lanterns"
	# Disposable game's read-only acceptance probe; no credentials or general SDK contract.
	if OS.has_feature("web"):
		var probe := {"loaded": loaded, "guest": guest, "lanterns": lanterns, "revision": revision, "received": received, "multiplayer": multiplayer_mode, "status": status.text, "save_choice_ready": chooser.can_choose(), "role": peers.room.get("role", ""), "session": peers.room.get("epoch", ""), "policy": peers.room.get("ice_transport_policy", "")}
		JavaScriptBridge.eval("window.lanternLake = " + JSON.stringify(probe), true)
