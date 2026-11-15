extends Node
## Copied only into the disposable native acceptance project, never a game SDK.
func _ready() -> void:
	var destination := OS.get_environment("INFUSER_ACCEPTANCE_REPORT")
	if destination.is_empty():
		return
	var deadline := Time.get_ticks_msec() + 30000
	while not InfuserProgress.loaded:
		if Time.get_ticks_msec() > deadline:
			get_tree().quit(1)
			return
		await get_tree().create_timer(0.1).timeout
	var before: int = InfuserProgress.best
	var previous: Variant = null
	if FileAccess.file_exists("user://acceptance.json"):
		previous = JSON.parse_string(FileAccess.get_file_as_string("user://acceptance.json"))
	var file := FileAccess.open("user://acceptance.json", FileAccess.WRITE)
	file.store_string(JSON.stringify({"best": before + 3}))
	file.close()
	InfuserProgress.save_score(before + 3)
	while not InfuserProgress.loaded:
		if Time.get_ticks_msec() > deadline:
			get_tree().quit(2)
			return
		await get_tree().create_timer(0.1).timeout
	var report := FileAccess.open(destination, FileAccess.WRITE)
	report.store_string(JSON.stringify({"before":before,"after":InfuserProgress.best,"previous_local":previous,"guest":InfuserProgress.guest,"user_dir":OS.get_user_data_dir(),"revision":InfuserProgress.revision}))
	report.close()
	get_tree().quit()
