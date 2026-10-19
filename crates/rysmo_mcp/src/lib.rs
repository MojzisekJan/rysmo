//! Rysmo's MCP server: the headless editor behind JSON-RPC 2.0.
//!
//! Every tool call that changes the model runs one [`rysmo_model::Command`] (or a few)
//! through [`rysmo_model::Editor::apply`], so it is an undo step like any edit made by
//! hand, and the same sticky geometry rules apply. Nothing here depends on Bevy: the
//! `rysmo-mcp` binary serves stdio, and the app can serve a [`Session`] over TCP
//! against the document open in its window.
//!
//! The server is dual-era (see `docs/mcp.md`): it answers the `initialize` handshake of
//! protocol revisions up to 2025-11-25, and it serves requests that carry the protocol
//! version in `_meta`, as revision 2026-07-28 does, including `server/discover`.

#![forbid(unsafe_code)]

mod describe;
mod tools;

use std::path::PathBuf;

use rysmo_model::Editor;
use serde_json::{Value, json};

pub use tools::tool_definitions;

/// The current protocol revision, served statelessly per request.
pub const MODERN_VERSIONS: &[&str] = &["2026-07-28"];
/// Handshake-based revisions answered through `initialize`.
pub const LEGACY_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const META_VERSION: &str = "io.modelcontextprotocol/protocolVersion";

/// One open document and where it lives on disk.
pub struct Session {
    pub editor: Editor,
    pub path: Option<PathBuf>,
    /// The revision agreed through `initialize`, if a legacy client opened that way.
    pub legacy: Option<String>,
}

impl Default for Session {
    fn default() -> Self {
        Session { editor: Editor::new(), path: None, legacy: None }
    }
}

fn reply(id: Value, result: Value) -> String {
    json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string()
}

fn error(id: Value, code: i64, message: &str, data: Option<Value>) -> String {
    let mut e = json!({"code": code, "message": message});
    if let Some(d) = data {
        e["data"] = d;
    }
    json!({"jsonrpc": "2.0", "id": id, "error": e}).to_string()
}

fn all_versions() -> Vec<&'static str> {
    MODERN_VERSIONS.iter().chain(LEGACY_VERSIONS).copied().collect()
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    /// Handles one JSON-RPC message (one line on stdio). Returns the reply, or `None`
    /// for notifications, which want none.
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }
        let msg: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => return Some(error(Value::Null, -32700, &format!("parse error: {e}"), None)),
        };
        if let Some(batch) = msg.as_array() {
            // JSON-RPC batches: answer each request, skip notifications.
            let replies: Vec<Value> =
                batch.iter().filter_map(|m| self.handle(m)).filter_map(|r| serde_json::from_str(&r).ok()).collect();
            return (!replies.is_empty()).then(|| Value::Array(replies).to_string());
        }
        self.handle(&msg)
    }

    fn handle(&mut self, msg: &Value) -> Option<String> {
        if msg.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            let id = msg.get("id").cloned().unwrap_or(Value::Null);
            return Some(error(id, -32600, "not a JSON-RPC 2.0 message", None));
        }
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let Some(id) = msg.get("id").cloned() else {
            // A notification (initialized, cancelled): nothing to send back.
            return None;
        };
        if !(id.is_string() || id.is_i64() || id.is_u64()) {
            return Some(error(Value::Null, -32600, "id must be a string or an integer", None));
        }
        let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));
        if !params.is_object() {
            return Some(error(id, -32602, "params must be an object", None));
        }
        // A modern request names its revision in _meta; check it before anything else.
        let requested = params.get("_meta").and_then(|m| m.get(META_VERSION)).and_then(Value::as_str);
        let modern = match requested {
            Some(v) if MODERN_VERSIONS.contains(&v) => true,
            Some(v) if LEGACY_VERSIONS.contains(&v) && self.legacy.is_some() => false,
            Some(v) => {
                return Some(error(
                    id,
                    -32022,
                    "Unsupported protocol version",
                    Some(json!({"supported": all_versions(), "requested": v})),
                ));
            }
            None => false,
        };
        let complete = |mut v: Value| {
            if modern {
                v["resultType"] = json!("complete");
            }
            v
        };
        match method {
            "initialize" => {
                let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("");
                let agreed = if LEGACY_VERSIONS.contains(&asked) { asked } else { LEGACY_VERSIONS[0] };
                self.legacy = Some(agreed.to_string());
                Some(reply(
                    id,
                    json!({
                        "protocolVersion": agreed,
                        "capabilities": {"tools": {}},
                        "serverInfo": {"name": "rysmo", "version": env!("CARGO_PKG_VERSION")},
                        "instructions": INSTRUCTIONS,
                    }),
                ))
            }
            "server/discover" => Some(reply(
                id,
                json!({
                    "resultType": "complete",
                    "supportedVersions": all_versions(),
                    "capabilities": {"tools": {}},
                    "_meta": {"io.modelcontextprotocol/serverInfo": {"name": "rysmo", "version": env!("CARGO_PKG_VERSION")}},
                    "instructions": INSTRUCTIONS,
                }),
            )),
            "ping" => Some(reply(id, complete(json!({})))),
            "tools/list" => Some(reply(id, complete(json!({"tools": tool_definitions()})))),
            "tools/call" => {
                let Some(name) = params.get("name").and_then(Value::as_str) else {
                    return Some(error(id, -32602, "tools/call needs a tool name", None));
                };
                let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
                let result = match tools::call(self, name, &args) {
                    Ok(Some(out)) => out.into_result(),
                    Ok(None) => return Some(error(id, -32602, &format!("Unknown tool: {name}"), None)),
                    Err(message) => json!({"content": [{"type": "text", "text": message}], "isError": true}),
                };
                Some(reply(id, complete(result)))
            }
            _ => Some(error(id, -32601, &format!("unknown method: {method}"), None)),
        }
    }
}

/// What a tool returns: text for the model and the same facts as structured data.
pub(crate) struct ToolOutput {
    pub text: String,
    pub data: Value,
}

impl ToolOutput {
    fn into_result(self) -> Value {
        json!({
            "content": [{"type": "text", "text": self.text}],
            "structuredContent": self.data,
            "isError": false,
        })
    }
}

const INSTRUCTIONS: &str = "Rysmo is a 3D modeller. Coordinates are metres, Z up (red X, green Y, blue Z). \
Geometry is sticky: lines split where they cross and closed planar loops become faces. \
Call describe_model to see ids; every element id is unique in the document, so tools take plain integer ids. \
Faces on the ground face down, so push_pull them by a negative distance to pull them up. \
Every change is an undo step (undo, redo).";

/// Serves a session over stdin and stdout, one JSON-RPC message per line, until stdin
/// closes. Diagnostics go to stderr.
pub fn run_stdio(mut session: Session) -> std::io::Result<()> {
    use std::io::{BufRead, Write};
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if let Some(out) = session.handle_line(&line) {
            writeln!(stdout, "{out}")?;
            stdout.flush()?;
        }
    }
    Ok(())
}

/// The `_meta` key that carries the live server's token on every request.
pub const TOKEN_KEY: &str = "rysmo/token";

/// JSON-RPC error code for a request without the right token.
pub const UNAUTHORIZED: i64 = -32001;

/// A fresh random token: 128 bits as 32 hex digits.
pub fn new_token() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("the system has a random source");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Compares in time independent of where the strings differ.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// What the live server does with a line before it reaches the editor.
#[derive(Debug, PartialEq)]
pub enum Gate {
    /// The token is right: handle it.
    Pass,
    /// Refuse it, with this reply (none for a notification).
    Reject(Option<String>),
}

/// Checks that a message (or every message of a batch) carries `token` in
/// `params._meta["rysmo/token"]`.
pub fn check_token(line: &str, token: &str) -> Gate {
    let Ok(msg) = serde_json::from_str::<Value>(line) else { return Gate::Pass };
    let messages: Vec<&Value> = match &msg {
        Value::Array(a) => a.iter().collect(),
        m => vec![m],
    };
    let ok = |m: &Value| {
        m.get("params")
            .and_then(|p| p.get("_meta"))
            .and_then(|m| m.get(TOKEN_KEY))
            .and_then(Value::as_str)
            .is_some_and(|t| same(t, token))
    };
    if messages.iter().all(|m| ok(m)) {
        return Gate::Pass;
    }
    // Answer the first request that lacks it; notifications get no reply.
    let id = messages.iter().find(|m| !ok(m)).and_then(|m| m.get("id").cloned());
    Gate::Reject(id.map(|id| {
        error(
            id,
            UNAUTHORIZED,
            "Unauthorized: this Rysmo window wants its token in params._meta[\"rysmo/token\"]. \
             The window shows it (Window > MCP server) and prints it when it starts; \
             rysmo-mcp --connect adds it with --token or RYSMO_MCP_TOKEN.",
            None,
        )
    }))
}

/// Puts the token into every message of a line, for the stdio bridge.
pub fn add_token(line: &str, token: &str) -> String {
    let Ok(mut msg) = serde_json::from_str::<Value>(line) else { return line.to_string() };
    let mut stamp = |m: &mut Value| {
        if !m.is_object() {
            return;
        }
        if !m.get("params").is_some_and(Value::is_object) {
            m["params"] = json!({});
        }
        if !m["params"].get("_meta").is_some_and(Value::is_object) {
            m["params"]["_meta"] = json!({});
        }
        m["params"]["_meta"][TOKEN_KEY] = json!(token);
    };
    match &mut msg {
        Value::Array(a) => a.iter_mut().for_each(&mut stamp),
        m => stamp(m),
    }
    msg.to_string()
}

/// A request from a socket and where its reply goes.
pub type Request = (String, std::sync::mpsc::Sender<Option<String>>);

/// Listens on `127.0.0.1:port` (0 picks a free port) and passes every line that carries
/// `token` to the returned receiver; lines without it are answered with an error right
/// away. Whoever owns the editor (the app's main thread, or a test) answers the
/// requests. Returns the port.
pub fn listen(port: u16, token: String) -> std::io::Result<(u16, std::sync::mpsc::Receiver<Request>)> {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind(("127.0.0.1", port))?;
    let port = listener.local_addr()?.port();
    let (tx, rx) = std::sync::mpsc::channel::<Request>();
    std::thread::Builder::new().name("mcp-listen".into()).spawn(move || {
        for stream in listener.incoming().flatten() {
            let tx = tx.clone();
            let token = token.clone();
            let _ = std::thread::Builder::new().name("mcp-client".into()).spawn(move || {
                let Ok(mut write) = stream.try_clone() else { return };
                for line in BufReader::new(stream).lines() {
                    let Ok(line) = line else { break };
                    let reply = match check_token(&line, &token) {
                        Gate::Reject(reply) => reply,
                        Gate::Pass => {
                            let (reply_tx, reply_rx) = std::sync::mpsc::channel();
                            if tx.send((line, reply_tx)).is_err() {
                                break;
                            }
                            match reply_rx.recv() {
                                Ok(reply) => reply,
                                Err(_) => break,
                            }
                        }
                    };
                    if let Some(reply) = reply
                        && writeln!(write, "{reply}").and_then(|_| write.flush()).is_err()
                    {
                        break;
                    }
                }
            });
        }
    })?;
    Ok((port, rx))
}

/// Forwards stdio to a Rysmo window serving MCP on a local TCP port, so an MCP client
/// that only speaks stdio can drive the open window. `token` is added to every message.
pub fn bridge_stdio_to(addr: &str, token: Option<&str>) -> std::io::Result<()> {
    use std::io::{BufRead, BufReader, Write};
    let stream = std::net::TcpStream::connect(addr)?;
    let mut to_app = stream.try_clone()?;
    let from_app = BufReader::new(stream);
    let reader = std::thread::spawn(move || -> std::io::Result<()> {
        let mut stdout = std::io::stdout();
        for line in from_app.lines() {
            writeln!(stdout, "{}", line?)?;
            stdout.flush()?;
        }
        Ok(())
    });
    for line in std::io::stdin().lock().lines() {
        let line = line?;
        let line = match token {
            Some(t) => add_token(&line, t),
            None => line,
        };
        writeln!(to_app, "{line}")?;
        to_app.flush()?;
    }
    to_app.shutdown(std::net::Shutdown::Write)?;
    let _ = reader.join();
    Ok(())
}

#[cfg(test)]
mod tests;
