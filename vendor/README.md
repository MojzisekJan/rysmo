# Vendored crates

`bevy_pbr` is bevy_pbr 0.20.0 from crates.io (MIT OR Apache-2.0, licence files included)
with one change: `MAX_UNIFORM_BUFFER_CLUSTERABLE_OBJECTS` is 64 instead of 204, in
`src/cluster/mod.rs` and `src/render/mesh_view_types.wesl`. The array is used only where
storage buffers are unavailable (WebGL2). At 204 entries Firefox on macOS spent about 7 s
linking the main material shader; at 64 about 1.8 s. Rysmo places at most 64 point lights.
Remove the patch when upgrading Bevy and re-measure.

`bevy_egui` is bevy_egui 0.43.1 from its upstream `dependabot/cargo/egui-0.37` branch
(MIT OR Apache-2.0) with two changes for egui 0.37: mouse wheel events report an unknown
`source`, and `OutputCommand::TextSelectionSettled` is ignored. `egui-phosphor` is
egui-phosphor 0.14.0 (MIT OR Apache-2.0) with its egui requirement raised to 0.37. Both let
Rysmo use egui_native_look 0.2, which needs egui 0.37. Drop them once bevy_egui and
egui-phosphor release egui 0.37 support.
