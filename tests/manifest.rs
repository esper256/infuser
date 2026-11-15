use infuser::manifest::Manifest;

#[test]
fn dual_membership_requires_explicit_networking_and_a_relay() {
    let mut value: serde_json::Value =
        serde_json::from_str(include_str!("../examples/game-manifest.json")).unwrap();
    let parse = |v: &serde_json::Value| {
        serde_json::from_value::<Manifest>(v.clone())
            .unwrap()
            .validate()
    };
    assert!(parse(&value).is_ok());
    value["multiplayer"]["relay_fallback"] = serde_json::Value::Null;
    assert!(parse(&value).is_err());
    value["multiplayer"]["relay_fallback"] = "required".into();
    value["multiplayer"]["transport"] = "udp".into();
    assert!(parse(&value).is_err());
    value["multiplayer"]["transport"] = "webrtc".into();
    value["play_modes"] = serde_json::json!(["solo"]);
    assert!(parse(&value).is_err());
    value["multiplayer"] = serde_json::Value::Null;
    assert!(parse(&value).is_ok());
}
