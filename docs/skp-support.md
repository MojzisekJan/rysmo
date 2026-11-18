# SKP interchange

File > Import accepts `.skp` alongside the existing model formats. The preview
reports conversion warnings before changing a document. Choose **Place in model**
to insert an editable component, or **Open as new document** to retain imported
project settings. The existing document remains in its own tab. Converted projects
start unsaved and have no native Save destination; save them as `.rysmo`.

File > Export > SKP prepares a separate file, reports conversion warnings, and
then offers Save SKP. Choose **2017 compatibility** or the **2026 container**.
Export never marks the Rysmo document saved or changes its native save path.
Changing the model, camera or chosen version invalidates a prepared export. Desktop import
and export preparation run on background threads. Browser parsing/encoding uses
the same portable Rust implementation on the main thread.

The implementation uses pinned Skpwright 0.6.1, not an external converter or an
installed vendor application. Its documented coverage includes older stream
containers and modern ZIP-based containers. Local interoperability fixtures cover
2017 and 2026, with cross-container read/write tests. Other versions are upstream
coverage, not independently verified application compatibility. Files authored by
Rysmo still need testing in the target applications on Windows/macOS.

| Data | Import | Export |
|---|---|---|
| Editable polygon meshes, holes and loose edges | Yes | Yes |
| Non-planar source faces | Triangulated with a warning; vertices, holes and texture placement retained | Planar triangles |
| Shared nested components and groups | Yes | Yes |
| Translation, rotation, nonuniform scale and mirroring | Yes | Yes |
| Solid colours, opacity and embedded PNG/JPEG textures | Yes | Yes |
| Projective texture placement | Yes, with equivalent integer tile rebasing when needed | Yes |
| Independent front/back materials | Yes | Yes |
| Independent front/back UV placement | Uses front placement with a warning | Uses Rysmo's shared placement |
| Soft, smooth and hidden edges/faces | Yes | Yes |
| Curves and arc/circle parameters | Yes | Yes |
| Instance tags and current tag visibility | Yes | Yes |
| Tags assigned directly to mesh entities | Visibility is baked, warning shown | Not represented by Rysmo |
| Camera, units and basic appearance | Yes | Yes |
| Named scene cameras and basic appearance | Yes | Yes |
| Per-scene visibility/section overrides | Warning, not converted | Warning, not converted |
| Construction lines/points and section planes | Yes | Yes |
| Existing glued-instance attachments | Warning, not converted | Opening holes baked into static editable geometry, warning shown |
| Custom attributes, procedural records and advanced material/environment data | Warning, not converted | Warning for unsupported Rysmo data |
| Text/dimension annotations, placed images and decorative polylines | Warning, not converted | Warning for unsupported Rysmo data |

Current import limits are 64 MiB input, 128 MiB per expanded outer archive entry,
256 MiB total declared outer archive expansion, 200,000 vertices, 300,000 edges,
100,000 faces, 50,000 stored placements and 10,000 definitions. Shared instances
remain shared rather than flattening geometry. Existing hierarchy/expanded-geometry
limits also apply. Texture images are limited to 16 million pixels each.
Unrecognized containers, invalid geometry and invalid transforms fail without
modifying the current project. Partial parser recovery is reported.

Missing external texture images become solid colours with a warning. Fonts,
shader graphs, animations and application-specific editing history are not portable
through this conversion. Native `.rysmo` remains the full-fidelity project format.

The implementation and fixture provenance are described in [NOTICE](../NOTICE.md)
and the [fixture record](../crates/rysmo_io/tests/fixtures/skp/README.md).
No network service is used when importing or exporting a user's model.
