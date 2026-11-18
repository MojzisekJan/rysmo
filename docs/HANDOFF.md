# Development handoff, 2026-11-18

## Transfer status

The macOS checkout is `main`, updated from `origin/main` at
`294fc9ae295b60f2d16dbbe4d867af271c440da5`. The follow-up below completes the
browser SKP checks and fixes a real-project import failure. Build outputs and
public sample downloads under `target/` are reproducible and are not committed.

The configured commit identity matches earlier commits:
`Jan Mojzisek <mojzisekjan@gmail.com>`. The user wants this as the sole author,
one-line commit messages, no co-author trailers, and no CI.

## Completed implementation

- Performance: background desktop file/surface work, cooperative browser surface
  preparation, revision guards, shared spatial indexes, incremental hierarchy,
  annotation and glue dependency caches, persistence/recovery improvements.
  See [performance results](performance.md) and `verification/*performance*.json`.
- SKP: pinned pure-Rust `skpwright = 0.6.1`, import into the current model or a
  new unsaved document, export as 2017 or 2026 containers, conversion warnings.
  Desktop preparation uses background workers. Browser conversion runs on the
  main thread. Shared hierarchy, editable geometry, holes, textures/projective
  UVs, tags, cameras, basic styles, scenes, guides and section planes are covered.
  See [coverage and limits](skp-support.md), `crates/rysmo_io/src/skp/` and
  `crates/rysmo_app/src/features/skp.rs`.
- Fixtures, dependency attribution and GPL license are included. Upstream claims
  clean-room provenance; this claim has not been independently audited.

Native workspace library tests passed: 1,033, with 6 ignored. Strict workspace
all-target Clippy passed. All 28 JavaScript tests, formatting and diff checks
passed during the handoff check. Two web release builds completed. Eight additional
upstream sample models passed 16 cross-container conversions, checking geometry
counts, visible bounds, texture payload size and tag counts. These are internal
read/write checks, not proof that an external application accepts every export.

## macOS follow-up

- Browser cursor checks completed for shared components and projective textures:
  import into a new unsaved tab, preservation of the prior edited tab, both SKP
  export versions, downloads, and reimport of all four downloads. Geometry counts,
  bounds and texture payload checks match. A downloaded house also rendered with
  textures and editable components. See the verification record for build hashes
  and the post-fix check of the larger theater project: 1,172 visible occurrences,
  24,934 visible faces, both GUI exports, and the downloaded 2026 file reimported
  and rendered without a crash.
- Public samples were downloaded from pinned GitHub commits; provenance and hashes
  are in `verification/skp-interoperability-2026-11-18.json`. These are internal
  conversions, not independent SketchUp acceptance tests.
- The theater project initially failed safely on 25 non-planar faces (maximum
  deviation about 0.025 mm). Such faces now triangulate without moving shared
  vertices. Holes, hidden flags, front/back materials and projective texture
  placement are retained; added diagonals are soft/smooth. Import warns about the
  conversion and checks the resulting geometry budget. Regression tests cover
  holes and textured bent faces. All 126 I/O library tests and strict I/O
  all-target Clippy passed; 28 JavaScript tests passed and the web release rebuilt.
- The user does not have SketchUp installed. No vendor application check was
  performed. Browser native Save-picker completion was not established by this
  run; the SKP download/reimport checks are recorded separately.

## Resume here

1. When SketchUp is available, open the downloaded 2017/2026 exports there and
   compare hierarchy, geometry/holes, textures, tags and cameras. This remains
   unverified; successful reads by Rysmo's own dependency are not independent
   compatibility evidence.
2. Review remaining conversion gaps in `skp-support.md` and the English TODO
   comments in the SKP modules. Continue prioritizing representative real-project
   failures and reporting omitted data honestly.
3. Further performance targets are large selection/highlight/move operations,
   cold picking, remaining indivisible browser work and parser allocations.
   Existing profiling shows significant long tasks for whole-model edits;
   background preparation does not eliminate these.

The user explicitly excluded `.blend` support. Do not add it. Native `.rysmo`
remains the full-fidelity format. SKP annotations, custom/procedural records,
advanced materials and some per-scene settings are not fully converted. Independent
front/back UVs use one placement with a warning. Existing glued openings are
baked into static geometry on export. Do not describe SKP support as lossless.

## Reproduce checks

```sh
cargo fmt --all -- --check
cargo clippy --locked --profile portable --workspace --all-targets -- -D warnings
cargo test --locked --profile portable --workspace --lib
node --experimental-vm-modules scripts/web-files.test.mjs
git diff --check
```

Use the repository's documented web build workflow. The original machine also
has release artifacts at `target/web-skp-release/` and
`target/web-skp-verified/`. The measured pre-SKP performance build and its hashes
are recorded separately; do not replace those hashes with a later build.
Temporary profiling corpora and helper programs under `target/` and `/tmp/` are
not needed to compile the project. Committed SKP test fixtures include provenance
and licensing in their own README.
