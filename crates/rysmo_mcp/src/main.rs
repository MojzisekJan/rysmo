//! `rysmo-mcp`: Rysmo's MCP server on stdio.
//!
//! Usage:
//! - `rysmo-mcp [model.rysmo]` serves a headless editor on stdio, optionally with a
//!   file open;
//! - `rysmo-mcp --connect 127.0.0.1:PORT [--token TOKEN]` forwards stdio to a Rysmo
//!   window started with `--mcp-port PORT`, so a stdio MCP client can drive the open
//!   window. The window's token comes from `--token` or `RYSMO_MCP_TOKEN`.

#![forbid(unsafe_code)]

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.as_slice() {
        [flag, addr, rest @ ..] if flag == "--connect" => {
            let token = match rest {
                [t, value] if t == "--token" => Some(value.clone()),
                [] => std::env::var("RYSMO_MCP_TOKEN").ok(),
                _ => {
                    eprintln!("usage: rysmo-mcp --connect HOST:PORT [--token TOKEN]");
                    std::process::exit(2);
                }
            };
            if token.is_none() {
                eprintln!(
                    "rysmo-mcp: no token given; the window will refuse requests (use --token or RYSMO_MCP_TOKEN)"
                );
            }
            rysmo_mcp::bridge_stdio_to(addr, token.as_deref())
        }
        [flag, ..] if flag == "--help" || flag == "-h" => {
            println!("usage: rysmo-mcp [model.rysmo] | --connect HOST:PORT [--token TOKEN]");
            Ok(())
        }
        rest => {
            let mut session = rysmo_mcp::Session::new();
            if let Some(path) = rest.first() {
                match std::fs::read_to_string(path)
                    .map_err(|e| e.to_string())
                    .and_then(|t| rysmo_io::load(&t).map_err(|e| e.to_string()))
                {
                    Ok(doc) => {
                        session.editor.replace_document(doc);
                        session.path = Some(path.into());
                    }
                    Err(e) => {
                        eprintln!("rysmo-mcp: cannot open {path}: {e}");
                        std::process::exit(1);
                    }
                }
            }
            rysmo_mcp::run_stdio(session)
        }
    };
    if let Err(e) = result {
        eprintln!("rysmo-mcp: {e}");
        std::process::exit(1);
    }
}
