use serde_json::{Value, json};

use crate::Session;

fn call(s: &mut Session, id: u64, name: &str, args: Value) -> Value {
    let msg = json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": name, "arguments": args}});
    serde_json::from_str(&s.handle_line(&msg.to_string()).expect("a reply")).unwrap()
}

fn ok(v: &Value) -> &Value {
    assert_eq!(v["result"]["isError"], json!(false), "tool failed: {v}");
    &v["result"]["structuredContent"]
}

#[test]
fn legacy_handshake_and_tool_list() {
    let mut s = Session::new();
    let r: Value = serde_json::from_str(
        &s.handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}"#)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(r["result"]["protocolVersion"], "2025-06-18");
    assert!(s.handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none());
    let list: Value =
        serde_json::from_str(&s.handle_line(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#).unwrap()).unwrap();
    let names: Vec<&str> =
        list["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    for want in [
        "draw_rectangle",
        "push_pull",
        "describe_model",
        "make_component",
        "place_component",
        "paint",
        "undo",
        "export",
    ] {
        assert!(names.contains(&want), "missing {want}");
    }
    // An unknown version asked for in initialize gets the newest legacy one.
    let mut s = Session::new();
    let r: Value = serde_json::from_str(
        &s.handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2099-01-01"}}"#)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(r["result"]["protocolVersion"], "2025-11-25");
}

#[test]
fn modern_requests_and_version_errors() {
    let mut s = Session::new();
    let d: Value = serde_json::from_str(
        &s.handle_line(r#"{"jsonrpc":"2.0","id":"d","method":"server/discover","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}}}"#)
            .unwrap(),
    )
    .unwrap();
    assert!(d["result"]["supportedVersions"].as_array().unwrap().contains(&json!("2026-07-28")));
    let list: Value = serde_json::from_str(
        &s.handle_line(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}}}"#)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(list["result"]["resultType"], "complete");
    let bad: Value = serde_json::from_str(
        &s.handle_line(r#"{"jsonrpc":"2.0","id":3,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"1900-01-01"}}}"#)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(bad["error"]["code"], -32022);
    assert_eq!(bad["error"]["data"]["requested"], "1900-01-01");
}

#[test]
fn protocol_errors() {
    let mut s = Session::new();
    let r: Value = serde_json::from_str(&s.handle_line("{not json").unwrap()).unwrap();
    assert_eq!(r["error"]["code"], -32700);
    let r: Value =
        serde_json::from_str(&s.handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"nope"}"#).unwrap()).unwrap();
    assert_eq!(r["error"]["code"], -32601);
    let r = call(&mut s, 2, "no_such_tool", json!({}));
    assert_eq!(r["error"]["code"], -32602);
    // A tool that fails reports it in the result, so the model can correct itself.
    let r = call(&mut s, 3, "push_pull", json!({"face": 999, "distance": 1.0}));
    assert_eq!(r["result"]["isError"], json!(true));
    assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("999"));
}

#[test]
fn build_a_box_with_a_component_and_undo() {
    let mut s = Session::new();
    let r = call(&mut s, 1, "draw_rectangle", json!({"corner": [0, 0, 0], "width": 2.0, "depth": 1.0}));
    let face = ok(&r)["faces"][0].as_u64().unwrap();
    let r = call(&mut s, 2, "push_pull", json!({"face": face, "distance": -1.0}));
    ok(&r);
    let d = call(&mut s, 3, "describe_model", json!({}));
    let data = ok(&d);
    assert_eq!(data["counts"]["faces"], 6);
    assert_eq!(data["bounds"]["max"], json!([2.0, 1.0, 1.0]));
    let all: Vec<u64> = data["faces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["id"].as_u64().unwrap())
        .chain(data["edges"].as_array().unwrap().iter().map(|e| e["id"].as_u64().unwrap()))
        .collect();
    let r = call(&mut s, 4, "make_component", json!({"elements": all, "name": "Crate"}));
    let def = ok(&r)["definition"].as_u64().unwrap();
    let r = call(&mut s, 5, "place_component", json!({"definition": def, "origin": [5, 0, 0], "rotation": 45}));
    ok(&r);
    let r = call(&mut s, 6, "add_material", json!({"name": "Pine", "color": [0.8, 0.7, 0.5]}));
    ok(&r);
    let inst = ok(&call(&mut s, 7, "describe_model", json!({})))["instances"][1]["id"].as_u64().unwrap();
    ok(&call(&mut s, 8, "paint", json!({"elements": [inst], "material": "Pine"})));
    assert_eq!(s.editor.doc().root.instances.len(), 2);
    ok(&call(&mut s, 9, "undo", json!({})));
    ok(&call(&mut s, 10, "undo", json!({})));
    ok(&call(&mut s, 11, "undo", json!({})));
    assert_eq!(s.editor.doc().root.instances.len(), 1);
    ok(&call(&mut s, 12, "redo", json!({})));
    assert_eq!(s.editor.doc().root.instances.len(), 2);
}

#[test]
fn draw_on_a_face_and_edit_inside_a_group() {
    let mut s = Session::new();
    let face =
        ok(&call(&mut s, 1, "draw_rectangle", json!({"corner": [0, 0, 0], "width": 1.0, "depth": 1.0})))["faces"][0]
            .as_u64()
            .unwrap();
    ok(&call(&mut s, 2, "push_pull", json!({"face": face, "distance": -1.0})));
    let top = ok(&call(&mut s, 3, "describe_model", json!({})))["faces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["normal"] == json!([0.0, 0.0, 1.0]))
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    let r = call(&mut s, 4, "draw_circle", json!({"centre": [0.5, 0.5, 1.0], "radius": 0.3, "face": top}));
    assert_eq!(ok(&r)["faces"].as_array().unwrap().len(), 1);
    let r = call(&mut s, 5, "draw_arc", json!({"start": [2, 0, 0], "end": [3, 0, 0], "bulge": 0.5, "segments": 8}));
    assert_eq!(ok(&r)["edges"].as_array().unwrap().len(), 8);
    let r = call(&mut s, 6, "draw_polygon", json!({"centre": [5, 0, 0], "radius": 0.5, "sides": 6}));
    assert_eq!(ok(&r)["faces"].as_array().unwrap().len(), 1);
    let poly = ok(&r)["faces"][0].as_u64().unwrap();
    let g = ok(&call(&mut s, 7, "make_group", json!({"elements": [poly]})))["instances"][0].as_u64().unwrap();
    ok(&call(&mut s, 8, "edit_component", json!({"instance": g})));
    let inside = call(&mut s, 9, "describe_model", json!({}));
    let hex = ok(&inside)["faces"][0]["id"].as_u64().unwrap();
    ok(&call(&mut s, 10, "push_pull", json!({"face": hex, "distance": -0.4})));
    ok(&call(&mut s, 11, "close_component", json!({})));
    assert!(s.editor.context().is_empty());
}
