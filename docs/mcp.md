# MCP server

Rysmo speaks the [Model Context Protocol](https://modelcontextprotocol.io), so an AI
agent can model with it. The server is the headless editor from `rysmo_model`: every
tool that changes the model runs editing commands through `Editor::apply`, so each call
is one undo step, the same sticky geometry rules apply as in the app, and a failed call
leaves the model as it was.

The server lives in the `rysmo_mcp` crate, which does not depend on Bevy.

## Running it

Three ways, all speaking the same JSON-RPC 2.0 messages, one per line:

| Command | What it is |
| --- | --- |
| `rysmo-mcp [model.rysmo]` | Headless server on stdio. Small binary, starts at once. |
| `rysmo --mcp [model.rysmo]` | The same, from the app binary, without opening a window. |
| `rysmo --mcp-port 47321` | The app with its window, also serving MCP on `127.0.0.1:47321` (0 picks a free port). The agent's edits appear in the window as they happen and the user can undo them. `RYSMO_MCP_PORT` does the same. Needs the token, see below. |
| `rysmo-mcp --connect 127.0.0.1:47321 --token TOKEN` | A stdio bridge to that port, for MCP clients that only start stdio servers. It adds the token to every message. `RYSMO_MCP_TOKEN` can give the token instead of `--token`. |

Build the server with `cargo build --release -p rysmo_mcp`; the binary is
`target/release/rysmo-mcp`. To add it to Claude Code:

```sh
claude mcp add rysmo -- /path/to/rysmo-mcp
# or, to drive a window started with --mcp-port 47321:
claude mcp add rysmo-live -- /path/to/rysmo-mcp --connect 127.0.0.1:47321 --token TOKEN
```

### The window's token

The TCP port listens on 127.0.0.1 only, and is off unless asked for. When it starts, the
app makes a random token (128 bits, 32 hex digits), prints it to the terminal together
with the `rysmo-mcp --connect` command line, and shows both in the window under
Window > MCP server, with Copy buttons; that window opens by itself at start.

Every message on the port must carry the token in its `_meta`:

```json
{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"_meta":{"rysmo/token":"22af983856bda4d8e53d8ce60afb87c7"}}}
```

A request without it, or with a wrong one, gets JSON-RPC error -32001 ("Unauthorized")
and never reaches the model; a notification without it is dropped. The check happens on
the socket thread, before anything touches the editor. The bridge adds the token to
every message itself, so the MCP client needs no changes. The stdio servers
(`rysmo-mcp`, `rysmo --mcp`) have no token: whoever starts the process owns its
stdio.

## Protocol

The server is dual-era:

- **Handshake revisions** 2025-11-25, 2025-06-18, 2025-03-26 and 2024-11-05: the client
  sends `initialize` with its `protocolVersion`; the server answers with the same
  version if it knows it, otherwise with 2025-11-25.
- **Revision 2026-07-28** (the current one when this was written): requests carry
  `io.modelcontextprotocol/protocolVersion` in `params._meta`, results carry
  `"resultType": "complete"`, and `server/discover` lists the supported versions. A
  request naming a version the server does not know gets error -32022 with the
  supported versions in `data.supported`.

Methods: `initialize`, `server/discover`, `ping`, `tools/list`, `tools/call`.
Notifications (`notifications/initialized`, `notifications/cancelled`) get no reply.
JSON-RPC batches are answered as batches.

Tool results have a short `text` content item and the same facts as
`structuredContent`. A tool that fails returns `isError: true` with a message that says
what to fix (for example, "123 is not a face"); an unknown tool or malformed request is
a JSON-RPC error.

One server process holds one open document. The 2026-07-28 revision is stateless at the
protocol level; here the state is the document the process has open, the way a desktop
application's server behaves.

## Conventions

- **Units**: coordinates and distances are metres; angles are degrees. `set_units` only
  changes how lengths read in text replies and in the app.
- **Axes**: right-handed, Z up: red X, green Y, blue Z.
- **Ids**: every edge, face, group or component instance, guide, definition, material
  and tag has an integer id, unique in the whole document and stable across edits and
  saves. Tools take plain integer ids; the server works out what kind of element each
  is. `describe_model` lists them.
- **Facing**: faces drawn on the ground face down, so `push_pull` such a
  face by a negative distance to pull it up into a box. The new top faces up. Each
  face in `describe_model` has its `normal`.
- **Sticky geometry**: lines split where they cross, a closed planar loop becomes a
  face, a line across a face splits it. Drawing a rectangle on a face cuts it into the
  face.
- **Context**: tools work in the context being edited, the model or a group or
  component opened with `edit_component`, in that context's own coordinates.
  `describe_model` reports the open path. `close_component` goes back out.

## Tools

| Tool | Arguments | Does |
| --- | --- | --- |
| `new_document` | `units?` (`mm`, `m`) | Empty model. |
| `open_document` | `path` | Opens a `.rysmo` file. |
| `save_document` | `path?` | Saves as `.rysmo`; without a path, where it came from. |
| `export` | `path`, `format?` (`obj`, `glb`; else from the extension) | Exports the whole model, groups flattened. OBJ writes an MTL beside it. |
| `describe_model` | `detail?` (`summary`, `full`), `limit?` (500) | Counts, bounds, open context; with `full` also faces (id, normal, area, centre, vertices, materials), edges (end points, faces, soft, hidden), instances (definition, axes, tag, material, bounds), definitions, guides, materials, tags. |
| `get_document_text` | | The `.rysmo` text of the model (see `file-format.md`). |
| `draw_line` | `points`, `closed?` | Connected lines through the points. |
| `draw_rectangle` | `corner`, and `width` + `depth` (with `normal?` or `face?`) or `u` + `v` | A rectangle. Width and depth run along the plane's axes, lined up with the model axes. |
| `draw_circle` | `centre`, `radius`, `segments?` (24), `normal?` or `face?` | A circle. |
| `draw_polygon` | `centre`, `radius`, `sides`, `normal?` or `face?` | A regular polygon inscribed in the radius. |
| `draw_arc` | `start`, `end`, `bulge`, `segments?` (12), `normal?` or `face?` | A 2-point arc bulging by `bulge` to the left of the chord seen from the normal; half the chord is a half circle. |
| `push_pull` | `face`, `distance`, `keep_base?` | Push/Pull. Negative pushes into the face's back. |
| `move` | `elements`, `offset`, `copies?` | Moves, or makes `copies` copies each `offset` further. |
| `rotate` | `elements`, `centre`, `angle`, `axis?` ([0,0,1]), `copies?` | Rotates, or makes rotated copies. |
| `scale` | `elements`, `origin`, `factor` (number or `[x, y, z]`) | Scales; negative factors mirror. |
| `offset` | `distance`, and `face` or `edges` + `normal?` | Offsets a face's outline (positive inwards) or an edge chain (positive to the left). |
| `follow_me` | `face`, `path` (edge ids) | Sweeps the face along the path. |
| `erase` | `elements` | Erases. An edge between coplanar faces merges them. |
| `reverse_faces` | `faces` | Turns faces round. |
| `soften_edges` | `edges`, `angle?` (20) | Softens and smooths edges between faces meeting at less than the angle. |
| `make_group` | `elements` | Groups. Returns the instance id. |
| `make_component` | `elements`, `name?` | Makes a component. Returns the instance and the definition id. |
| `place_component` | `definition`, `origin`, `rotation?` (degrees about blue), `scale?` | Places another instance. |
| `explode` | `instance` | Replaces an instance by its geometry. |
| `make_unique` | `instance` | Gives an instance its own definition. |
| `edit_component` | `instance` | Opens a group or component; tools then work inside. |
| `close_component` | | Closes the innermost one. |
| `add_material` | `name`, `color` (`[r,g,b]` or `[r,g,b,a]`, sRGB 0 to 1) | New material. |
| `paint` | `elements`, `material` (id, name or null), `back?` | Paints faces (front or back) or instances. |
| `add_tag` | `name` | New tag. |
| `set_tag` | `tag` (id or name), `name?`, `visible?` | Renames, shows or hides a tag. |
| `assign_tag` | `instances`, `tag` (id, name or null) | Puts instances on a tag. |
| `add_guide` | `point`, or `origin` + `dir` | A guide point or line. |
| `set_units` | `units` | Display units. |
| `undo`, `redo` | | One step back or forward. |

There is no render or screenshot tool yet; rendering needs a GPU, which the headless
server does not start. It is on the roadmap.

## Example session

A box with a door recess, painted and saved. Lines starting `>` are sent, `<` are
replies (shortened).

```
> {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"demo","version":"1"}}}
< {"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"rysmo","version":"0.1.0"},"instructions":"..."}}
> {"jsonrpc":"2.0","method":"notifications/initialized"}
> {"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"draw_rectangle","arguments":{"corner":[0,0,0],"width":3,"depth":2}}}
< {"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"Rectangle done. Faces: [9]. Edges: [3, 5, 7, 8]."}],"structuredContent":{"faces":[9],"edges":[3,5,7,8],...},"isError":false}}
> {"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"push_pull","arguments":{"face":9,"distance":-2.5}}}
< ... "Push/Pull done. Faces: [22, 23, 24, 25, 9]." ...
> {"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"draw_rectangle","arguments":{"corner":[1,0,0],"u":[1,0,0],"v":[0,0,2]}}}
< ... "Rectangle done. Faces: [33]. ..." ...
> {"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"push_pull","arguments":{"face":33,"distance":-0.2}}}
> {"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"add_material","arguments":{"name":"Brick","color":[0.66,0.3,0.22]}}}
> {"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"paint","arguments":{"elements":[22],"material":"Brick"}}}
> {"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"save_document","arguments":{"path":"house.rysmo"}}}
< ... "Saved house.rysmo." ...
```

Face ids depend on what was drawn before; read them from each reply or from
`describe_model` rather than guessing. The same session with 2026-07-28 skips
`initialize` and adds
`"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}` to each `params`.

## Tests

`crates/rysmo_mcp/tests/live.rs` starts a token-checking TCP server like the
window's and checks that requests without the token, or with a wrong one, are refused,
and that the bridge adds it from `--token` and from `RYSMO_MCP_TOKEN`.
`crates/rysmo_mcp/src/tests.rs` drives a `Session` directly: both protocol eras,
version errors, protocol errors, tool failures, building, components, painting and
undo. `crates/rysmo_mcp/tests/stdio.rs` starts the `rysmo-mcp` binary, talks to it
over stdio, saves and exports, then loads the saved file with `rysmo_io` and checks
it, and reopens it in a second server.
