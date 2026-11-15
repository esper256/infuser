extends Window
## Reusable, explicit chooser for one retained JSON save conflict.
## The game supplies schema validation; Infuser never guesses how files merge.
signal finished(result: Dictionary)
signal prepared
var _group := ""
var _candidate := ""
var _validate: Callable
var _cloud: Dictionary = {}
var _device: Dictionary = {}
var _pending: Dictionary = {}
var _busy := false
var _message: Label
var _cloud_button: Button
var _device_button: Button
var _retry: Button

func _ready() -> void:
	title = "Choose your progress"
	size = Vector2i(540, 270)
	unresizable = true
	exclusive = true
	transient = true
	close_requested.connect(func():
		if not _busy:
			hide()
			finished.emit({"ok": false, "error": "Save choice postponed. Both copies are safe."}))
	var column := VBoxContainer.new()
	column.set_anchors_and_offsets_preset(Control.PRESET_FULL_RECT)
	column.offset_left = 20
	column.offset_right = -20
	column.offset_top = 16
	column.offset_bottom = -16
	column.add_theme_constant_override("separation", 12)
	add_child(column)
	_message = Label.new()
	_message.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	column.add_child(_message)
	_cloud_button = Button.new()
	_cloud_button.custom_minimum_size.y = 64
	_cloud_button.pressed.connect(func(): _choose(_cloud))
	column.add_child(_cloud_button)
	_device_button = Button.new()
	_device_button.custom_minimum_size.y = 64
	_device_button.pressed.connect(func(): _choose(_device))
	column.add_child(_device_button)
	_retry = Button.new()
	_retry.text = "Try again"
	_retry.pressed.connect(func():
		if _pending.is_empty():
			_load_choices()
		else:
			_commit())
	column.add_child(_retry)
	hide()

func choose(group: String, candidate: String, validate_files: Callable) -> Dictionary:
	_group = group
	_candidate = candidate
	_validate = validate_files
	_pending = {}
	popup_centered()
	_load_choices()
	return await finished

func _lock(message: String) -> void:
	_busy = true
	_message.text = message
	_cloud_button.disabled = true
	_device_button.disabled = true
	_retry.hide()

func _load_choices() -> void:
	_lock("Two adventures were saved. Which one do you want to continue?")
	var cloud: Dictionary = await Infuser.call_api("load", {"group": _group})
	var device: Dictionary = await Infuser.call_api("read", {"group": _group, "revision": _candidate})
	_busy = false
	if not cloud.ok or not device.ok:
		_message.text = "Couldn't load both copies. Your progress is safe. Try again."
		_retry.show()
		return
	_cloud = cloud.value
	_device = device.value
	# Another device may already have resolved this candidate. Let the game
	# reload the acknowledged cloud head, never silently resurrect the loser.
	if not _cloud.conflicts.any(func(c: Dictionary): return c.candidate_revision == _candidate):
		hide()
		finished.emit({"ok": true, "value": _cloud})
		return
	_cloud_button.text = "Cloud · " + _label(_cloud)
	_device_button.text = "Other saved copy · " + _label(_device)
	_cloud_button.disabled = not _validate.call(_cloud.files)
	_device_button.disabled = not _validate.call(_device.files)
	if _cloud_button.disabled or _device_button.disabled:
		_message.text = "A copy needs a different game version. Choose a compatible copy, or return later."
	_cloud_button.grab_focus()
	prepared.emit()

func can_choose() -> bool:
	return visible and not _busy and _cloud_button != null and not _cloud_button.disabled

func _label(snapshot: Dictionary) -> String:
	var summary: String = snapshot.get("summary", "Saved progress")
	return summary + "\n" + Time.get_datetime_string_from_unix_time(int(snapshot.get("created_at", 0))).replace("T", " ") + " UTC"

func _choose(snapshot: Dictionary) -> void:
	if _busy or not _validate.call(snapshot.files):
		return
	_pending = {"candidateRevision": _candidate, "expectedRevision": _cloud.revision, "commitId": Infuser.uuid(), "files": snapshot.files.duplicate(true), "summary": snapshot.summary}
	_commit()

func _commit() -> void:
	_lock("Saving your choice…")
	var result: Dictionary = await Infuser.call_api("resolve", {"group": _group, "checkpoint": _pending})
	_busy = false
	if not result.ok:
		if result.get("code") == "conflict_already_resolved":
			_pending = {}
			_load_choices()
			return
		_message.text = "Couldn't confirm your choice. Try again; both copies are safe."
		_retry.show() # Retry the exact commit ID and bytes, even after a timeout.
		return
	_pending = {}
	if result.value.status == "conflict":
		_candidate = result.value.candidate_revision
		_load_choices()
		return
	var latest: Dictionary = await Infuser.call_api("load", {"group": _group})
	if not latest.ok:
		_message.text = "Choice saved. Try again to load it."
		_retry.show()
		return
	hide()
	finished.emit(latest)
