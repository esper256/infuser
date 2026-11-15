extends Node
## Preview web/native adapter. Keep this node alive for the launch.
## Results are {ok, value} or {ok:false, error, status}; never treat errors as empty saves.

class Pending:
	extends RefCounted
	signal finished(result: Dictionary)

var _bridge: JavaScriptObject
var _callback: JavaScriptObject
var _pending: Dictionary = {}
var _sequence := 0
var _native: Dictionary = {}
var _native_error := "Open this game through Infuser."

func _ready() -> void:
	if OS.has_feature("web"):
		JavaScriptBridge.eval(FileAccess.get_file_as_string("res://addons/infuser/bridge.js"), true)
		_bridge = JavaScriptBridge.get_interface("InfuserGodot")
		_callback = JavaScriptBridge.create_callback(_completed)
	else:
		var path := OS.get_environment("INFUSER_LAUNCH_FILE")
		if not path.is_empty() and FileAccess.file_exists(path):
			var envelope: Variant = JSON.parse_string(FileAccess.get_file_as_string(path))
			if envelope is Dictionary and envelope.get("version") == 1 and str(envelope.get("game_origin", "")).begins_with("https://"):
				var expected := "/" + str(envelope.get("save_namespace", ""))
				if expected.length() > 1 and OS.get_user_data_dir().replace("\\", "/").ends_with(expected):
					_native = envelope
				else:
					_native_error = "Infuser couldn't isolate this game's saves. Ask its creator to enable Godot project setting overrides."

func call_api(operation: String, args: Dictionary = {}) -> Dictionary:
	if not OS.has_feature("web"):
		return await _native_api(operation, args)
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
		var result: Dictionary = JSON.parse_string(str(args[1]))
		# Godot's JSON parser returns numbers as floats; Array.has/in uses
		# strict types. Keep HTTP status checks such as `in [401, 410]` valid.
		result["status"] = int(result.get("status", 0))
		pending.finished.emit(result)

func options() -> Dictionary:
	return JSON.parse_string(str(_bridge.options())) if _bridge != null else {}

func uuid() -> String:
	return str(_bridge.uuid()) if _bridge != null else Crypto.new().generate_random_bytes(16).hex_encode()

func now_msec() -> int:
	return int(_bridge.now()) if _bridge != null else Time.get_ticks_msec()

func start_ticker(callback: JavaScriptObject) -> int:
	return int(_bridge.startTicker(callback)) if _bridge != null else 0

func stop_ticker(id: int) -> void:
	if _bridge != null:
		_bridge.stopTicker(id)

func network(status: String) -> void:
	if _bridge != null:
		_bridge.network(status)

func _native_api(operation: String, args: Dictionary) -> Dictionary:
	if _native.is_empty():
		return {"ok": false, "status": 0, "error": _native_error}
	var method := HTTPClient.METHOD_GET
	var path := ""
	var body: Variant = null
	match operation:
		"ready": path = "/context"
		"presence": path = "/presence/v1"
		"launch_intent":
			path = "/launch-intent/v1/consume"
			method = HTTPClient.METHOD_POST
		"load", "read", "commit", "resolve":
			path = "/cloud/v1/groups/" + str(args.group).uri_encode()
			if operation == "read":
				path += "/revisions/" + str(args.revision).uri_encode()
			elif operation in ["commit", "resolve"]:
				method = HTTPClient.METHOD_POST
				var checkpoint: Dictionary = args.checkpoint
				body = {"expected_revision": checkpoint.expectedRevision, "commit_id": checkpoint.commitId, "files": checkpoint.files, "summary": checkpoint.get("summary", ""), "resolve_candidate": checkpoint.get("candidateRevision", checkpoint.get("resolveCandidate"))}
		_:
			return {"ok": false, "status": 0, "error": "This Infuser native adapter does not support that operation yet."}
	var request := HTTPRequest.new()
	request.timeout = 10
	request.body_size_limit = 2 * 1024 * 1024
	add_child(request)
	var headers := PackedStringArray(["Authorization: Bearer " + str(_native.token), "Origin: " + str(_native.game_origin), "Content-Type: application/json"])
	var error := request.request(str(_native.game_origin) + "/_infuser/api/v1" + path, headers, method, "" if body == null else JSON.stringify(body))
	if error != OK:
		request.queue_free()
		return {"ok": false, "status": 0, "error": "Couldn't reach Infuser. Keep your pending save and try again."}
	var response: Array = await request.request_completed
	request.queue_free()
	if response[0] != HTTPRequest.RESULT_SUCCESS:
		return {"ok": false, "status": 0, "error": "Couldn't confirm the request. Try again with the same pending save."}
	var status := int(response[1])
	var value: Variant = JSON.parse_string(response[3].get_string_from_utf8())
	if not value is Dictionary:
		return {"ok": false, "status": status, "error": "Infuser returned an incomplete response. Try again."}
	if status >= 200 and status < 300 or status == 409 and value.get("status") == "conflict":
		return {"ok": true, "status": status, "value": value}
	var failure: Variant = value.get("error", "Infuser could not complete the request.")
	return {"ok": false, "status": status, "code": failure.get("code", "") if failure is Dictionary else "", "error": failure.get("message", "Request failed") if failure is Dictionary else str(failure)}
