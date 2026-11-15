extends Node
## Preview web-only adapter. Keep this node and its callback alive for the launch.
## Results are {ok, value} or {ok:false, error, status}; never treat errors as empty saves.

class Pending:
	extends RefCounted
	signal finished(result: Dictionary)

var _bridge: JavaScriptObject
var _callback: JavaScriptObject
var _pending: Dictionary = {}
var _sequence := 0

func _ready() -> void:
	if OS.has_feature("web"):
		JavaScriptBridge.eval(FileAccess.get_file_as_string("res://addons/infuser/bridge.js"), true)
		_bridge = JavaScriptBridge.get_interface("InfuserGodot")
		_callback = JavaScriptBridge.create_callback(_completed)

func call_api(operation: String, args: Dictionary = {}) -> Dictionary:
	if _bridge == null:
		return {"ok": false, "error": "Open the web export through Infuser.", "status": 0}
	_sequence += 1
	var id := _sequence
	var pending := Pending.new()
	_pending[id] = pending
	_bridge.call("call", id, operation, JSON.stringify(args), _callback)
	return await pending.finished

func _completed(args: Array) -> void:
	var id := int(args[0])
	if _pending.has(id):
		var pending: Pending = _pending[id]
		_pending.erase(id)
		pending.finished.emit(JSON.parse_string(str(args[1])))

func options() -> Dictionary:
	return JSON.parse_string(str(_bridge.options())) if _bridge != null else {}

func uuid() -> String:
	return str(_bridge.uuid())

func network(status: String) -> void:
	if _bridge != null:
		_bridge.network(status)
