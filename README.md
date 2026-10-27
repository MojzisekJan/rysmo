# Rysmo

![Rysmo](docs/screenshot.png)

A 3D modeller for architecture: buildings, rooms and furniture drawn to exact sizes.
Draw on the model, push and pull faces, and type exact sizes as you go. Runs on desktop and in the browser.
Written in Rust with Bevy and egui.

**[Open the editor in your browser](https://mojzisekjan.github.io/rysmo/editor/)**

Recent editor additions include OBJ/GLB/DXF import, portable component collections,
editable windows/doors/stairs, scaled SVG/PDF drawings, photo perspective matching,
material surface controls and nested solid operations. Editable workflows add live
operations, component connections and attributes, section hatching, document tabs
and multi-viewport drawing sheets. See [editable workflows](docs/editable-workflows.md) and
[editor workflows and limits](docs/editor-expansion.md).

Requires Rust 1.97.1 or newer.

```sh
cargo run -p rysmo_app --release
```

Build, verification and Rust review conventions are in [CONTRIBUTING.md](CONTRIBUTING.md).

## License

GPL-3.0-or-later. See [LICENSE](LICENSE) and [NOTICE.md](NOTICE.md).
