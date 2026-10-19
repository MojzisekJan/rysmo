//! End to end for the live server's token: a TCP server like the window's (the same
//! `listen`, with a session answering on another thread), reached raw and through the
//! `rysmo-mcp --connect` bridge.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

/// Starts a token-checking server with a headless session behind it. Returns the port.
fn server(token: &str) -> u16 {
    let (port, rx) = rysmo_mcp::listen(0, token.to_string()).unwrap();
    std::thread::spawn(move || {
        let mut session = rysmo_mcp::Session::new();
        for (line, reply) in rx {
            let _ = reply.send(session.handle_line(&line));
        }
    });
    port
}

fn raw(port: u16, lines: &[Value]) -> Vec<Value> {
    let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let mut write = stream.try_clone().unwrap();
    let mut read = BufReader::new(stream);
    let mut out = Vec::new();
    for l in lines {
        writeln!(write, "{l}").unwrap();
        if l.get("id").is_some() {
            let mut s = String::new();
            read.read_line(&mut s).unwrap();
            out.push(serde_json::from_str(&s).unwrap());
        }
    }
    out
}

#[test]
fn requests_without_the_token_are_refused() {
    let token = rysmo_mcp::new_token();
    assert_eq!(token.len(), 32);
    let port = server(&token);
    let replies = raw(
        port,
        &[
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {"_meta": {"rysmo/token": "wrong"}}}),
            // A notification without the token gets no reply; the next request still works.
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/list", "params": {"_meta": {"rysmo/token": token}}}),
        ],
    );
    assert_eq!(replies[0]["error"]["code"], -32001);
    assert!(replies[0]["error"]["message"].as_str().unwrap().contains("token"));
    assert_eq!(replies[1]["error"]["code"], -32001);
    assert_eq!(replies[2]["id"], 3);
    assert!(replies[2]["result"]["tools"].as_array().unwrap().len() > 30);
}

fn bridge(port: u16, token_arg: Option<&str>, token_env: Option<&str>, lines: &[Value]) -> Vec<Value> {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rysmo-mcp"));
    cmd.arg("--connect").arg(format!("127.0.0.1:{port}"));
    if let Some(t) = token_arg {
        cmd.arg("--token").arg(t);
    }
    cmd.env_remove("RYSMO_MCP_TOKEN");
    if let Some(t) = token_env {
        cmd.env("RYSMO_MCP_TOKEN", t);
    }
    let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for l in lines {
        writeln!(stdin, "{l}").unwrap();
    }
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    String::from_utf8(out.stdout).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

#[test]
fn the_bridge_adds_the_token() {
    let token = rysmo_mcp::new_token();
    let port = server(&token);
    let session = [
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "draw_rectangle", "arguments": {"corner": [0, 0, 0], "width": 1, "depth": 1}}}),
    ];
    let with_arg = bridge(port, Some(&token), None, &session);
    assert_eq!(with_arg.len(), 2);
    assert_eq!(with_arg[0]["result"]["serverInfo"]["name"], "rysmo");
    assert_eq!(with_arg[1]["result"]["structuredContent"]["faces"].as_array().unwrap().len(), 1);
    let with_env = bridge(port, None, Some(&token), &session[2..]);
    assert_eq!(with_env[0]["result"]["isError"], json!(false));
    let without = bridge(port, None, None, &session[..1]);
    assert_eq!(without[0]["error"]["code"], -32001);
    let wrong = bridge(port, Some("0000"), None, &session[..1]);
    assert_eq!(wrong[0]["error"]["code"], -32001);
}
