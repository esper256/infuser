extends Node
## Integration overlay for the unmodified Dodge the Creeps gameplay.
## Best score is saved at game-over, never on an incoherent timer tick.
const Chooser = preload("res://addons/infuser/save_conflict.gd")
var best := 0
var revision: Variant = null
var pending: Dictionary = {}
var loaded := false
var guest := false
var _main: Node
var _label: Label
var _retry: Button

func _ready() -> void:
	await get_tree().process_frame
	_main = get_tree().current_scene
	_main.get_node("HUD/StartButton").disabled = true
	var overlay := CanvasLayer.new()
	add_child(overlay)
	var column := VBoxContainer.new()
	column.position = Vector2(14, 64)
	overlay.add_child(column)
	_label = Label.new()
	_label.text = "Loading your best…"
	column.add_child(_label)
	_retry = Button.new()
	_retry.text = "Try again"
	_retry.pressed.connect(func():
		_retry.hide()
		if pending.is_empty():
			_restore(await Infuser.call_api("load", {"group": "record"}))
		else:
			_commit())
	_retry.hide()
	column.add_child(_retry)
	var context: Dictionary = await Infuser.call_api("ready")
	if not context.ok:
		_label.text = context.error
		return
	guest = context.value.guest
	var result: Dictionary = await Infuser.call_api("load", {"group": "record"})
	await _restore(result)

func _valid(files: Dictionary) -> bool:
	var value: Variant = files.get("record.json", {"schema": 1, "best": 0})
	return value is Dictionary and value.get("schema") == 1 and (value.get("best") is float or value.get("best") is int)

func _restore(result: Dictionary) -> void:
	while result.ok and not result.value.get("conflicts", []).is_empty():
		var chooser := Chooser.new()
		add_child(chooser)
		result = await chooser.choose("record", result.value.conflicts[0].candidate_revision, _valid)
		chooser.queue_free()
	if not result.ok or not _valid(result.value.files):
		_label.text = result.get("error", "This save needs another game version.")
		_retry.show()
		return
	revision = result.value.revision
	best = int(result.value.files.get("record.json", {}).get("best", 0))
	loaded = true
	_main.get_node("HUD/StartButton").disabled = false
	_label.text = ("Guest best: " if guest else "Best: ") + str(best)

func save_score(score: int) -> void:
	if not loaded:
		return
	loaded = false
	_main.get_node("HUD/StartButton").disabled = true
	best = maxi(best, score)
	pending = {"expectedRevision": revision, "commitId": Infuser.uuid(), "files": {"record.json": {"schema": 1, "best": best}}, "summary": "Best: " + str(best)}
	_commit()

func _commit() -> void:
	_retry.hide()
	_label.text = "Saving your best…"
	var result: Dictionary = await Infuser.call_api("commit", {"group": "record", "checkpoint": pending})
	if not result.ok:
		_label.text = "Not saved yet. Retry when you're connected."
		_retry.show()
		return
	pending = {}
	var latest: Dictionary = await Infuser.call_api("load", {"group": "record"})
	await _restore(latest)

func _process(_delta: float) -> void:
	# Read-only probe for this acceptance study; not part of the reusable SDK.
	if OS.has_feature("web") and _main != null and _label != null:
		JavaScriptBridge.eval("window.dodgeProbe = " + JSON.stringify({"ready": loaded, "guest": guest, "best": best, "score": _main.score, "revision": revision, "status": _label.text}), true)
