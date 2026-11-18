//! MCP for the open window: a local TCP port that takes the same JSON-RPC messages as
//! `rysmo-mcp` on stdio, one per line, and applies them to the document in the window.
//!
//! Start the app with `--mcp-port PORT` (or `RYSMO_MCP_PORT=PORT`). It makes a random
//! token, prints it and shows it under Window > MCP server; every request must carry it
//! in `params._meta["rysmo/token"]`. An MCP client that only speaks stdio connects
//! through `rysmo-mcp --connect 127.0.0.1:PORT --token TOKEN`, which adds it. The
//! socket threads check the token and pass lines; the requests run on Bevy's main
//! thread, between frames, so the agent's edits are ordinary undo steps the user sees at
//! once.

use std::sync::Mutex;
use std::sync::mpsc;

use bevy::prelude::*;
use rysmo_mcp::{Request, Session};

use crate::state::App3d;

#[derive(Resource)]
struct LiveMcp {
    requests: Mutex<mpsc::Receiver<Request>>,
    /// The protocol revision a legacy client agreed in `initialize`.
    legacy: Option<String>,
}

/// Where the server listens and the token requests must carry, for the Window menu.
#[derive(Resource, Clone)]
pub struct McpInfo {
    pub port: u16,
    pub token: String,
}

/// Starts listening on `127.0.0.1:port` with a fresh token and adds the system that
/// answers requests. Returns the port and the token.
pub fn start(app: &mut App, port: u16) -> std::io::Result<McpInfo> {
    let token = rysmo_mcp::new_token();
    let (port, rx) = rysmo_mcp::listen(port, token.clone())?;
    let info = McpInfo { port, token };
    app.insert_resource(LiveMcp { requests: Mutex::new(rx), legacy: None })
        .insert_resource(info.clone())
        .add_systems(Update, serve);
    Ok(info)
}

fn serve(mut app: ResMut<App3d>, mut live: ResMut<LiveMcp>) {
    // MCP file writes are synchronous. Keep them behind already queued desktop
    // saves so an older worker snapshot cannot overwrite a newer MCP save.
    if crate::native_files::saving(&app) {
        return;
    }
    let pending: Vec<Request> = live.requests.lock().map(|rx| rx.try_iter().collect()).unwrap_or_default();
    for (line, reply) in pending {
        let mut session =
            Session { editor: std::mem::take(&mut app.editor), path: app.path.clone(), legacy: live.legacy.clone() };
        let out = session.handle_line(&line);
        app.editor = session.editor;
        app.path = session.path;
        live.legacy = session.legacy;
        let _ = reply.send(out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rysmo_math::DVec3;
    use rysmo_model::{Command, Editor};
    use std::time::{Duration, Instant};

    #[test]
    fn mcp_save_waits_for_the_older_desktop_save_and_keeps_the_newer_model() {
        let directory = std::env::temp_dir().join(format!(
            "rysmo-mcp-save-order-{}-{}",
            std::process::id(),
            crate::documents::generation()
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("model.rysmo");
        let mut state = App3d::new(Editor::new());
        crate::native_files::save(&mut state, path.clone());
        state.editor.apply(Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::X]], facing: None }).unwrap();
        let (sender, requests) = mpsc::channel();
        let (reply, response) = mpsc::channel();
        sender
            .send((
                serde_json::json!({"jsonrpc":"2.0", "id":1, "method":"tools/call",
            "params":{"name":"save_document", "arguments":{"path":path.to_string_lossy()}}})
                .to_string(),
                reply,
            ))
            .unwrap();
        let mut app = App::new();
        app.insert_resource(state).insert_resource(LiveMcp { requests: Mutex::new(requests), legacy: None });
        app.add_systems(Update, serve);
        app.update();
        assert!(matches!(response.try_recv(), Err(mpsc::TryRecvError::Empty)));
        app.add_systems(Update, crate::native_files::poll.before(serve));
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            app.update();
            if let Ok(Some(reply)) = response.try_recv() {
                let reply: serde_json::Value = serde_json::from_str(&reply).unwrap();
                assert_ne!(reply["result"]["isError"], true);
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        let saved = rysmo_io::load(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved.root.mesh.edge_count(), 1);
        assert!(!app.world().resource::<App3d>().editor.is_dirty());
        std::fs::remove_dir_all(directory).unwrap();
    }
}
