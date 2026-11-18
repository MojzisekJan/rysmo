# Development handoff, 2026-11-18

## Transfer status

The working checkout is `/home/jan/src/rysmo`, branch `main`, based on
`41e2942882a8d48ed463aaafe969c54433a8f59e`. The performance and SKP changes
described below are included in the handoff commit. On another computer, update
from `origin/main` and start with the remaining tasks below. Build outputs under
`target/` are reproducible and should not be committed.

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

## Resume here

1. Complete actual browser GUI verification of the current SKP build. Import
   `crates/rysmo_io/tests/fixtures/skp/shared-2017.skp` via File > Import > Model,
   choose Open as new document, and verify the previous tab survives and the new
   document has no native save path. Export and download both SKP versions through
   File > Export > SKP. Reimport downloads and check geometry. Repeat with
   `texture-2026.skp` to inspect warped texture placement. Record the results in
   `verification/skp-interoperability-2026-11-18.json`, whose browser status is
   currently unfinished.
2. On Windows/macOS, open exported files in the target application and compare
   hierarchy, geometry/holes, textures, tags and cameras. This has not been done.
   Inspect representative real user projects and report omitted data honestly.
3. Review remaining conversion gaps in `skp-support.md` and the English TODO
   comments in the SKP modules. Prioritize real-project failures found above.
4. Further performance targets are large selection/highlight/move operations,
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
