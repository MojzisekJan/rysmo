//! End to end: spawn the `rysmo-mcp` binary, talk JSON-RPC over its stdio, and check
//! the replies and the files it writes.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

struct Server {
    child: std::process::Child,
    out: BufReader<std::process::ChildStdout>,
    next: u64,
}

impl Server {
    fn start(args: &[&str]) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_rysmo-mcp"))
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("the server starts");
        let out = BufReader::new(child.stdout.take().unwrap());
        Server { child, out, next: 1 }
    }

    fn send(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        let msg = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(stdin, "{msg}").unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        self.out.read_line(&mut line).unwrap();
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["id"], id);
        v
    }

    fn notify(&mut self, method: &str) {
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(stdin, "{}", json!({"jsonrpc": "2.0", "method": method})).unwrap();
    }

    fn tool(&mut self, name: &str, args: Value) -> Value {
        let r = self.send("tools/call", json!({"name": name, "arguments": args}));
        assert_eq!(r["result"]["isError"], json!(false), "{name} failed: {r}");
        r["result"]["structuredContent"].clone()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        drop(self.child.stdin.take());
        let _ = self.child.wait();
    }
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rysmo-mcp-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

#[test]
fn session_over_stdio_builds_saves_and_exports() {
    let mut s = Server::start(&[]);
    let init = s.send(
        "initialize",
        json!({"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}),
    );
    assert_eq!(init["result"]["serverInfo"]["name"], "rysmo");
    s.notify("notifications/initialized");
    let tools = s.send("tools/list", json!({}));
    assert!(tools["result"]["tools"].as_array().unwrap().len() >= 30);

    let face = s.tool("draw_rectangle", json!({"corner": [0, 0, 0], "width": 3.0, "depth": 2.0}))["faces"][0]
        .as_u64()
        .unwrap();
    s.tool("push_pull", json!({"face": face, "distance": -2.5}));
    let model = s.tool("describe_model", json!({}));
    assert_eq!(model["counts"]["faces"], 6);
    let front =
        model["faces"].as_array().unwrap().iter().find(|f| f["normal"] == json!([0.0, -1.0, 0.0])).unwrap()["id"]
            .as_u64()
            .unwrap();
    s.tool("draw_rectangle", json!({"corner": [1.0, 0.0, 0.0], "u": [1.0, 0.0, 0.0], "v": [0.0, 0.0, 2.0]}));
    let door = s.tool("describe_model", json!({}))["faces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["normal"] == json!([0.0, -1.0, 0.0]) && f["area"].as_f64().unwrap() < 2.5)
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    assert_ne!(door, front);
    s.tool("push_pull", json!({"face": door, "distance": -0.2}));
    s.tool("add_material", json!({"name": "Brick", "color": [0.66, 0.3, 0.22]}));
    s.tool("paint", json!({"elements": [front], "material": "Brick"}));
    s.tool("add_tag", json!({"name": "Walls"}));

    let path = scratch("house.rysmo");
    s.tool("save_document", json!({"path": path.display().to_string()}));
    let glb = scratch("house.glb");
    s.tool("export", json!({"path": glb.display().to_string()}));
    let obj = scratch("house.obj");
    s.tool("export", json!({"path": obj.display().to_string(), "format": "obj"}));
    drop(s);

    let doc = rysmo_io::load(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(doc.root.mesh.face_count(), 11, "box, door recess sides and its back");
    assert!(doc.root.mesh.faces().any(|(_, f)| f.material.is_some()));
    assert_eq!(doc.tags.len(), 1);
    assert_eq!(&std::fs::read(&glb).unwrap()[0..4], b"glTF");
    assert!(std::fs::read_to_string(&obj).unwrap().contains("usemtl Brick"));
    assert!(obj.with_extension("mtl").exists());

    // Open the saved file in a new server, as an argument, and keep editing.
    let mut s = Server::start(&[&path.display().to_string()]);
    let model =
        s.send("tools/call", json!({"name": "describe_model", "arguments": {"detail": "summary"}, "_meta": {}}));
    assert_eq!(model["result"]["structuredContent"]["counts"]["faces"], 11);
    // A freshly opened file has no history.
    let r = s.send("tools/call", json!({"name": "undo", "arguments": {}}));
    assert_eq!(r["result"]["isError"], json!(true));
    s.tool("draw_line", json!({"points": [[0, -1, 0], [3, -1, 0]]}));
    assert_eq!(s.tool("undo", json!({}))["undone"], "Line");
}

#[test]
fn modern_client_without_handshake() {
    let mut s = Server::start(&[]);
    let meta = json!({"io.modelcontextprotocol/protocolVersion": "2026-07-28"});
    let d = s.send("server/discover", json!({"_meta": meta}));
    assert_eq!(d["result"]["resultType"], "complete");
    let r = s.send("tools/call", json!({"_meta": meta, "name": "draw_line", "arguments": {"points": [[0, 0, 0], [1, 0, 0], [1, 1, 0]], "closed": true}}));
    assert_eq!(r["result"]["resultType"], "complete");
    assert_eq!(r["result"]["structuredContent"]["faces"].as_array().unwrap().len(), 1);
}
