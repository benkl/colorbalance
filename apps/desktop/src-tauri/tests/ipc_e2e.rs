use std::path::PathBuf;

use colorbalance_desktop::{dropped_paths, DropPosition, NativeFileDrop};

#[test]
fn native_drop_event_serializes_for_frontend_contract() {
    let event = dropped_paths(
        &[
            PathBuf::from("C:/images/reference.dng"),
            PathBuf::from("C:/images/second.jpg"),
        ],
        128.0,
        64.0,
    );
    let value = serde_json::to_value(&event).expect("drop event serializes");
    assert_eq!(value["paths"].as_array().expect("paths array").len(), 2);
    assert_eq!(value["position"]["x"], 128.0);
    assert_eq!(value["position"]["y"], 64.0);
}

#[test]
fn native_drop_event_round_trips_without_losing_windows_paths() {
    let expected = NativeFileDrop {
        paths: vec!["C:\\Photos\\Reference Frame.dng".to_owned()],
        position: DropPosition { x: 42.5, y: 99.25 },
    };
    let json = serde_json::to_string(&expected).expect("drop serializes");
    let parsed: NativeFileDrop = serde_json::from_str(&json).expect("drop deserializes");
    assert_eq!(parsed, expected);
}
