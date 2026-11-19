# Editor performance

## Storage and editing

Vertices, edges, faces, curves, instances and definitions use ordered `BlockMap`
storage. Each block covers a range of 256 identifiers and stores sorted key and
value vectors behind `Arc`. Lookups use binary search. A snapshot shares blocks;
changing a shared block copies that block. Definitions are also shared through
`Arc`. Undo retains up to 200 document snapshots, rather than serializing projects
or cloning every geometry element on each edit.

Mesh adjacency and edge lookup maps avoid repeated full geometry searches. Mesh
views retain bounding-volume trees and hash-based face adjacency. Small edits
compare shared blocks and add local index changes; sufficiently large changes
rebuild the spatial snapshot. Component hierarchy budget checks memoize expansion
counts instead of expanding every occurrence just to count it.

Materials, image records, tags and annotations still use `BTreeMap`. Encoded image
bytes are shared, but document snapshots copy these maps and other owned document
metadata. Their cost should be measured with material-heavy projects before
changing their representation. Ordered storage also gives deterministic output.

## Native loading and saving, 2026-11-17

The loader now inserts geometry directly into its final block maps. Previously it
allocated intermediate `BTreeMap` nodes and then converted all elements into block
maps. Global duplicate identifier detection now uses the standard randomized
`HashSet`; it does not need sorted traversal. Missing-reference, geometry,
hierarchy, work-budget and identifier checks remain enabled.

The serializer writes element JSON directly into one byte buffer and converts that
buffer into a UTF-8 `String` without another allocation. It previously allocated
and copied a temporary JSON string for every element. Edge and face records borrow
their source geometry, avoiding cloned face boundary/hole vectors. The native
format version, section order, escaping and line layout remain unchanged.

Native CPU medians from 21 samples per operation:

| Fixture | Load before | Load after | Serialize before | Serialize after |
| --- | ---: | ---: | ---: | ---: |
| 12,000 instances of one cube, 0.94 MB | 13.76 ms | 8.92 ms | 7.97 ms | 5.38 ms |
| 12,000 independent cubes, 72,000 faces, 17.16 MB | 494.15 ms | 389.48 ms | 145.59 ms | 92.78 ms |

Both fixtures have the same expanded face count. Sharing definitions greatly
reduces file size and load work. The independent fixture still moves one instance
in about 15 ms in this benchmark. This change does not establish faster editing;
it targets persistence. Snapshot clone/drop is below 0.2 ms and Undo/Redo below
0.01 ms for these fixtures. Those figures exclude renderer updates.

Measurements use the native development profile on Rust 1.100.0. The I/O crate is
optimized at level 1 and geometry/model crates at level 3. They include allocation,
validation, index construction and result destruction, but exclude filesystem
reads/writes, GPU work, browser execution and peak-memory measurements. Background
desktop activity is uncontrolled. See the [raw results](verification/persistence-performance-2026-11-17.json)
for p95 values and exact fixture hashes.

Verification passed 714 library tests across I/O, mesh, model and tools, including
new escaping/layout and reversed-record-order regressions. Strict all-target
Clippy passed for I/O and mesh, as did formatting and whitespace checks. No new
browser package or desktop application was linked or installed for this change.

Generate the fixtures and repeat the benchmark locally:

```sh
python3 scripts/create-persistence-benchmark.py
cargo run --locked --offline -p rysmo_io --example benchmark_core -- \
  target/persistence-benchmark/shared.rysmo \
  target/persistence-benchmark/unique.rysmo
```

Use the same toolchain, profile and fixture bytes for both revisions. Build both
binaries before measuring and run them sequentially. The benchmark verifies a
save/load round trip and Move/Undo/Redo restoration. Generated models stay under
the ignored `target` directory. No hosted automation is required.

## Recovery snapshots and annotation edits, 2026-11-17

Browser autosave now serializes the active project once and shares that immutable
text with both recovery writes. A cache reuses document text by tab identity,
session generation, content revision and camera. Selection-only changes do not
invalidate it. Save destinations, dirty flags, the active tab and library recovery
metadata are rebuilt from current state on every snapshot.

Workspace JSON is serialized from typed records instead of constructing a JSON
value tree containing copies of every project string. The cache retains at most
64 MiB of string allocation capacity, including spare capacity. It prioritizes the
active project, drops closed documents and avoids cyclic eviction of inactive
tabs when the workspace exceeds the budget. Oversized texts still participate in
recovery without being retained. This is a retention limit, not a bound on the
complete workspace JSON or transient serialization memory.

Cache entries do not acknowledge writes or mark documents saved. An asynchronous
write keeps its own immutable text even if later edits replace a cache entry. The
existing completion identities, retry timer and serialized browser storage writes
remain responsible for persistence and stale-result handling.

Annotation refresh still resolves references before and after commands. It now
writes only values that actually changed, preserving shared root/component
storage for unchanged texts and dimensions. Group cleanup skips occurrence
collection and sorting when there are no group definitions to remove. Component
and group semantics remain distinct; unused reusable components are retained.

Selection changes also no longer start the autosave timer's pending work. Recovery
uses the editor's content revision; camera, dirty and library state have separate
checks. File-picker and file-save completion guards still use the full revision.

The 17.16 MB independent-cube fixture produced these native CPU medians in the
portable application test profile, with 21 samples per case:

| Recovery preparation | Median | p95 |
| --- | ---: | ---: |
| Reference double serialization | 409.99 ms | 484.46 ms |
| New cold cache | 206.52 ms | 213.95 ms |
| Reused unchanged text | 72.84 ms | 78.82 ms |

The warm cache retained 36 MiB of string capacity for this fixture. In three
sequential baseline/candidate pairs, changing units with 5,000 texts and 5,000
dimensions took 9.69 to 13.06 ms before and 1.73 to 3.49 ms after, using the development
profile. Each run took 41 samples after three warmups. The candidate retained all
100 definition allocations shared with the initial document; the baseline retained
none shared. This measures an annotation-heavy settings edit, not arbitrary drawing
or viewport frame time. Exact samples, profiles and source hashes are in the
[raw results](verification/recovery-annotation-performance-2026-11-17.json).

Verification passed 209 application tests, 349 model tests, 151 tools tests and 114
I/O tests, plus 26 JavaScript file/recovery tests. The manual recovery benchmark
also passed. Strict native all-target Clippy for application/model, the web target
check, formatting and whitespace checks passed. Clippy on Rust 1.100 additionally
required replacing existing indexed photo-guide loops with equivalent enumerated
iterators. No browser package was relinked and no installed app was updated.

The CPU-only annotation fixture contains 100 definitions, 5,000 texts and 5,000
dimensions. It alternates unit changes and verifies contents and Undo/Redo. Run it
with `cargo run --locked --offline -p rysmo_model --example perf_annotation_edits`.
For the recovery benchmark, generate the persistence fixtures above, then run:

```sh
RYSMO_RECOVERY_BENCH_MODEL=target/persistence-benchmark/unique.rysmo \
  cargo test --locked --offline -p rysmo_app --lib --profile portable \
  benchmark_unchanged_workspace_recovery -- --ignored --nocapture --test-threads=1
```

This manual benchmark compares the previous double-serialization path, a cold
cache and an unchanged warm cache. It checks recovery JSON equivalence and excludes
file reads, model parsing, IndexedDB writes, browser/GPU execution and peak memory.
The old path uses the current native serializer, so this comparison isolates
recovery overhead from the earlier serializer improvement. A warm-cache result
measures reuse within a requested workspace snapshot, not periodic idle work:
autosave already skips writes when nothing is pending. Cold-cache timing matters
for newly changed active projects. Full workspace JSON escaping, JavaScript
parsing and IndexedDB cloning still remain; future browser profiling must include
those costs before introducing incremental storage or worker protocols.

## Background desktop files and glue membership, 2026-11-18

Desktop File > Open, recent-model opening and Save now submit work to one bounded
worker queue. Reading, native parsing, default-camera bounds, serialization and
atomic writing run outside the GUI thread. The status bar shows the actual phase.
Opening can be cancelled; a parser already running may finish on the worker, but
its cancelled result is not installed. One operation per tab can be pending, with
up to 16 queued requests across tabs. Repeated Save in a busy tab asks the user to
retry instead of silently replacing or multiplying snapshots.

Save captures a shared document snapshot and an opaque editor/save-state token.
Completion updates its original tab. Newer edits stay dirty; Undo to the saved
state becomes clean. Replacing the editor rejects the old token. Preview clones
share the history ID allocator, so independently edited clones cannot reuse a
saved state's numeric identity. Save completion preserves the originally requested
filesystem path, including non-UTF-8 native paths on Linux.

Loaded documents preserve edits made during the read. If their originating tab
is inactive, a loaded model opens in a background tab without taking focus.
An active unsaved-changes prompt or pending exit also sends the result to a
background tab, preserving the decision in progress. The worker prepares its initial camera before handing it to the GUI. Closing waits
for saves, with an option to cancel closing, then rechecks current dirty states.
The final exit check runs in `Last`, after UI and live MCP mutations. A prior
Discard applies only to the accepted document/collection version. Live MCP
requests wait behind pending desktop saves to keep older queued snapshots from
overwriting a newer synchronous MCP save.

System file pickers remain native modal dialogs. Startup command-line loading,
headless MCP, exports and mobile file handling retain their existing synchronous
paths. Snapshot metadata cloning and document installation still happen on the
GUI thread. Renderer extraction, GPU uploads and first-pick latency are separate
work and are not covered by the file-dispatch benchmark.

The model now keeps a derived index of glued instance IDs by owning definition.
Shared block identity skips unchanged storage; changed blocks compare only the
relevant membership or definition pointer. A warmed index avoids three complete
instance walks during command maintenance. Sparse owners use indexed lookup;
dense owners retain contiguous scans. The index shares its outer map until
membership actually changes. Geometry, visibility and opening validation still
read current model data and are not skipped.

Known detach updates adjust the index directly. Before glue transforms are
written, cached definition references are released so they do not force another
copy of the definition. The updated membership is then seeded without another
full instance scan. The cache is excluded from equality and serialization.
Document clones start with an empty derived cache, preventing ownership cycles
through saved procedure inputs. Undo/Redo rebuild it on first use.

Keep index queries at document-maintenance boundaries. Their validity check walks
block lists; querying separately for every rendered occurrence would multiply
that cost. Renderer use would need one frame-level index snapshot.

Manual benchmarks, after generating the persistence fixtures above:

```sh
RYSMO_RECOVERY_BENCH_MODEL=target/persistence-benchmark/unique.rysmo \
  cargo test --locked --offline -p rysmo_app --lib --profile portable \
  benchmark_background_file_dispatch -- --ignored --nocapture --test-threads=1
cargo test --locked --offline -p rysmo_model --lib --profile portable \
  benchmark_sparse_glue_capture -- --ignored --nocapture --test-threads=1
```

File dispatch samples include actual atomic temporary-file replacement and
synchronization. Source parsing and test-application construction happen before
submission timing. Completion wall time includes the worker and a 1 ms polling
interval. Poll timing covers result installation, not renderer work. Input reads
use warmed filesystem caches. Glue timings isolate the capture phase in a model
with 50,000 ordinary instances; they are not whole-command or frame timings.

The 17.16 MB fixture produced the following medians after three warmups and 21
samples per operation. Dispatch includes snapshot capture for Save. The longest
GUI poll per operation is summarized at p95:

| Operation | GUI dispatch median | Completion median | p95 longest GUI poll |
| --- | ---: | ---: | ---: |
| Save | 0.040 ms | 174.99 ms | 0.038 ms |
| Open | 0.014 ms | 377.09 ms | 0.079 ms |

For 50,000 non-glued instances, the former full capture scan took 0.400 ms median.
The cold index took 0.199 ms, a warm index 0.0014 ms and an index updated after one
instance-block change 0.0103 ms. Each case has 41 samples after three warmups;
the block mutation and cache reset happen outside capture timing. These results
do not establish whole-editor speedups or dense-glue performance. Hierarchy and
annotation checks still contribute to complete command time.

Verification passed 945 targeted Rust library tests: application 223, I/O 114,
mesh 101, model 356 and tools 151. Both manual benchmarks and 26 JavaScript
file/recovery tests passed. Strict all-target Clippy for application/model/mesh,
web target compilation, formatting and whitespace checks passed. Tests cover
completion into inactive/replaced sessions, newer edits, saved Undo states,
preview history branches, failed writes, cancelled opening, non-UTF-8 paths,
close waiting/rechecks, MCP write ordering and index equivalence after edits,
owner deletion, detach, Undo/Redo and serialization. Raw values and source/binary
hashes are in the [verification record](verification/background-io-glue-performance-2026-11-18.json).
No browser package or installed desktop binary was updated.

## Command finalization and scene preparation, 2026-11-18

Hierarchy budget validation now retains its result while all of its inputs remain
unchanged. It compares face/vertex counts and instance-to-definition bindings,
including unused definitions. Changed instance blocks ignore transforms, names,
visibility and other metadata. Adding or removing a definition/instance, retargeting
an instance or changing geometry counts triggers the complete existing validator.
Prospective copy validation still performs its full expansion-budget check.

The cache holds weak definition identities and instance-map snapshots, not strong
references to definitions or procedural document inputs. Document/history clones
start with an empty cache. Serialization and equality ignore derived state. This
avoids ownership cycles and keeps rollback/Undo states independent.

Annotation refresh still resolves associations across contexts. It now clones text
only when its stored anchor changes, skips screen text, and skips cloning unchanged
linear dimensions and detached radial dimensions. Geometry deletion retains the
same last-known-position fallback. This does not yet index annotation dependencies.

Picking now reuses mesh indices by actual element storage rather than definition
identity. Metadata changes retain the mesh index. Evaluated opening geometry and
healed geometry must match their own mesh snapshots, preventing a stale opening
index from being reused. Renderer face preparation can likewise retain unchanged
triangulated chunks after definition metadata changes.

For small placement edits, the picking accelerator refits the existing tree and
retains inverses of unchanged transforms. A change to empty/nonempty membership,
a different occurrence count or cumulative changes exceeding one eighth of the
placements rebuilds the tree. A single changed placement is allowed for small
scenes. Refit updates every parent bound, so moved geometry remains queryable even
when it crosses the original tree partitions. The accelerator now also retains
one forward matrix per placement, an additional 128 bytes per occurrence before
vector capacity overhead. Mesh-index entries also retain shallow mesh snapshots
whose element blocks remain shared.

Reproduce the CPU measurements locally:

```sh
python3 scripts/create-persistence-benchmark.py --raw
cargo run --locked --offline --profile portable -p rysmo_model \
  --example perf_command_finalization
cargo run --locked --offline --profile portable -p rysmo_tools --example perf_pick
RYSMO_RECOVERY_BENCH_MODEL=target/persistence-benchmark/raw.rysmo \
  cargo test --locked --offline --profile portable -p rysmo_app --lib \
  benchmark_face_preparation -- --ignored --nocapture --test-threads=1
```

The hierarchy fixture contains 50,000 placements of an empty shared definition.
Its warm validation and single-instance movement have 41 samples after three
warmups. The hidden-change measurement includes the flag mutation. The picking
fixture contains 4,000 shared cubes and has 21 samples after three warmups. Cold
scene timing resets the tool cache, while underlying mesh lookup caches are warm.
Movement happens outside the subsequent-pick timing. The measured sequence does
not reach the cumulative tree-rebuild threshold; correctness tests exercise that
path. Cold face preparation resets triangulation chunks but retains warmed mesh
lookup caches. Rendering measurements cover CPU triangulation/chunk preparation only. They exclude Bevy asset extraction,
GPU uploads, rendered frame latency and browser execution.

Both comparisons ran with CPU affinity fixed to CPU 7, then ran again in reverse
order. Host CPU contention was high. The table shows the range of the two run
medians; these measurements do not support one fixed speedup factor.

| CPU work | Before, run medians | After, run medians |
| --- | ---: | ---: |
| Warm hierarchy validation, 50,000 instances | 1.72–2.88 ms | 0.0014–0.0015 ms |
| Hidden flag mutation and hierarchy validation | 1.68–1.69 ms | 0.0186–0.0214 ms |
| Complete move-one command, 50,000 instances | 2.01–4.67 ms | 0.077–0.088 ms |
| Scene preparation and pick after moving one of 4,000 cubes | 5.55–17.29 ms | 2.99–3.63 ms |
| Repeated pick without an edit | 0.099–0.104 ms | 0.097–0.175 ms |
| Cold tool scene and pick | 12.11–17.57 ms | 6.41–17.48 ms |

Repeated picks and cold scenes do not establish an improvement under this load.
The largest consistent reductions affect hierarchy maintenance and preparation
following a small edit.

For 12,000 disconnected cubes in one root mesh, the 12.38 MB fixture has 72,000
faces. The former metadata-change preparation path took 16.95 ms median; retaining
the prepared faces took 0.019 ms in the same binary. Cold face preparation still
took 406 ms median, with warm mesh lookup caches. This remains a GUI-thread stall
candidate and is the next renderer priority. The annotation command smoke
benchmark retained all 100 original definitions through edits and Undo/Redo; its
4.70 ms median is candidate-only and has no paired baseline in this record.

Verification passed 979 Rust library tests: application 224, inference 29, I/O 114,
mesh 102, model 358 and tools 152. The four manual CPU benchmark cases, 26
JavaScript file/recovery tests, strict all-target Clippy, web compilation,
formatting and whitespace checks passed. Tests compare refitted picking and
inference with rebuilt indices, including mirrored/nonuniform transforms, empty
geometry transitions, membership changes and periodic rebuilding. Hierarchy tests
cover cycles in unused definitions, retargeting, missing definitions, changed mesh
counts, transaction rollback, Undo and independent document branches.

Raw values, both comparison orders and source/binary hashes are in the
[verification record](verification/editor-finalization-performance-2026-11-18.json).
No installed binary or browser package was updated.

## Background surfaces, annotation dependencies and structural edits, 2026-11-18

Native surface preparation now runs in one read-only thread job per renderer. The
job owns a document snapshot and independent spatial lookup caches, so building a
lookup cannot hold the interactive mesh cache mutex. Native jobs reuse an available
immutable lookup tree through a new mutex; a busy source cache is skipped without
waiting. The renderer checks both
document generation and content revision before installing results. A superseded
job cancels between definitions; a triangulation already running may finish.
Switching documents releases prepared geometry even when definition IDs recur.

The browser uses cooperative preparation without requiring threads or special
hosting headers. It collects face adjacency in batches of at most 512 faces and
builds triangulation/smoothing batches of at most 128 faces. It preserves unaffected
chunks after geometry changes and yields after a soft six-millisecond budget.
Mesh-asset submission likewise yields between pieces. Individual polygons,
opening operations, image decoding, one render piece and allocator/scheduler work
can exceed that budget. This is not a hard real-time guarantee.

The renderer stays awake until preparation and asset submission complete. The
status bar reports preparation. Highlighting explicitly follows the surface
system and waits for its results instead of falling back to synchronous work.
Old document entities are removed on a generation change. Snapshot cancellation
never changes the edited model, saved state or Undo history.

Annotation maintenance now indexes exact referenced vertices, edges, curves,
faces and instance paths. Mesh changes include removed IDs. Instance transforms
and retargeting invalidate references through that instance; missing paths retain
registration at the missing prefix so restored geometry can resolve again. A
combined refresh resolves only selected annotations and retains the old deletion
fallback. Changed annotation records update their own dependencies. Empty models
reuse an annotation-free definition snapshot. Document clones and restored history
rebuild derived indices; caches never enter serialized files or document equality.
Detecting changes to annotation records in a modified owner can still scan its
annotation maps, and those maps still have their existing copy-on-write granularity.

Hierarchy maintenance keeps child-definition multiplicities and memoized expansion
counts. Structural edits update changed instance blocks and invalidate ancestors
through both old and new graphs. This also handles newly introduced cycles and
removed definitions. Prospective copy checks use the compressed graph. Every
unused definition still participates in limit validation; no safety budgets were
removed.

### Native measurements

The cold 72,000-face root mesh job took 0.165 ms to dispatch, 377 ms to finish on
the worker and at most 0.0204 ms per completion poll. This excludes edge-cache
preparation, image decoding and GPU work. The same cooperative algorithm, measured
natively, took 50 slices with a 6.57 ms p95 and 43.58 ms maximum. These are algorithm
measurements; browser results must be assessed separately.

For 50,001 shared empty instances, adding/removing one instance took 0.019 ms median
in incremental validation versus 0.816 ms in the current uncached validator. That
reference already aggregates repeated children and is not the original historical
implementation. Both cases have 41 samples after three warmups; mutation happens
outside validation timing.

The 5,000 screen-text / 5,000 detached-dimension SetUnits benchmark took 0.016 ms
median versus 1.023 ms in the saved pre-change executable. All 100 definitions
remained shared, and Undo/Redo assertions passed. This measures unrelated annotation
maintenance, not the rendering or motion of 10,000 visible attached labels. Exact
linked dependencies and cross-context paths have full-refresh oracle tests.

Linux process memory probes load and save each fixture, retain 200 Undo steps,
Undo/Redo the complete retained history and replace the document. GNU time maximum
RSS was approximately 142.7 MiB for the 17.16 MB unique-definition fixture and
198.2 MiB for its 52.17 MB version with six PNG textures. Native I/O retains
compressed image payloads; it does not create decoded GPU textures. RSS did not
immediately fall to its initial level after replacement. The allocator can retain
freed pages, and these probes cannot distinguish that from all forms of live
allocation. Flat RSS during history also does not establish allocation-free Undo.

Reproduce locally, without CI:

```sh
python3 scripts/create-persistence-benchmark.py --raw
python3 scripts/create-memory-benchmark.py
cargo build --locked --offline --profile portable --workspace --example perf_memory
/usr/bin/time -v target/portable/examples/perf_memory target/persistence-benchmark/unique.rysmo
/usr/bin/time -v target/portable/examples/perf_memory target/persistence-benchmark/textured.rysmo
RYSMO_RECOVERY_BENCH_MODEL=target/persistence-benchmark/raw.rysmo \
  cargo test --locked --offline --profile portable -p rysmo_app --lib \
  benchmark_surface_jobs -- --ignored --nocapture --test-threads=1
cargo test --locked --offline --profile portable -p rysmo_model --lib \
  benchmark_structural_validation -- --ignored --nocapture --test-threads=1
```

### Browser measurement protocol

An isolated release build and fresh browser contexts prevent interference with
users' documents, recovery data and release packages. `scripts/stage-web-profile.py`
stages fixtures and replaces only the startup restore adapter. The probe uses the
current diagnostic JavaScript imports with the same Rust ABI. The three cases
cover 1,200 shared cubes, textured/lit shared cubes and 72,000 root faces.

The browser runner confirms coloured model pixels in a viewport screenshot after
surface preparation. Its first-visible time is an upper bound including polling
and capture overhead. It records first pointer handling, repeated hover, alternating
moves through actual editor controls, verified history and Undo/Redo. Actual editor
updates are separate from browser RAF callbacks, including separate active phases.
Optional disjoint GPU queries span asset preparation and rendering. They are not
isolated VRAM-copy bandwidth measurements. Unsupported/disjoint/error cases cannot
be treated as successful GPU measurements.

WASM buffer size is allocated linear-memory capacity, not live model bytes.
JavaScript heap snapshots and Chrome process RSS are separate observations. Process
RSS sums include shared pages multiple times and do not measure total physical RAM
or GPU memory. Timings and samples are opt-in with `?diagnostics`, bounded, local
to the page and never written to user storage or sent to a service.

```sh
python3 scripts/create-web-benchmark.py
python3 scripts/stage-web-profile.py --dist target/web-profile-build
python3 scripts/serve-web.py target/web-profile-probe --port 8109
# In another terminal, with an existing Playwright installation:
RYSMO_PLAYWRIGHT=/path/to/playwright/index.mjs RYSMO_CHROMIUM=/path/to/chrome \
  node scripts/profile-web.mjs http://127.0.0.1:8109 target/web-profile-results
```

The isolated release build and all three hardware browser cases passed. The
renderer was WebGL2 through ANGLE on AMD Radeon 610M, with working disjoint GPU
timers. Both shared-instance cases completed 24 alternating whole-model moves and
verified Undo/Redo; no page or console errors occurred. These measurements precede
the separate SKP integration.

| Browser case | First visible after model install, upper bound | Pointer-phase editor FPS | Pointer handler p95 | Allocated WASM capacity |
|---|---:|---:|---:|---:|
| 1,200 shared cubes, flat | 0.96 s | 50.5 | 2.3 ms | 75.1 MiB |
| 1,200 shared cubes, textured/lit | 2.64 s | 20.0 | 7.4 ms | 78.6 MiB |
| 72,000 root faces | 11.15 s | 16.0 | 13.1 ms | 211.8 MiB |

The raw browser fixture retains the native fixture's topology, but enlarges cubes
from 0.2 m to 1.5 m at 2 m spacing so they occupy enough pixels for viewport
confirmation. Source fixture checksums distinguish the cases. The raw cooperative
surface slices had a 12.8 ms p95 and 36.9 ms maximum in this browser run. Its first
pointer handler took 770 ms, exposing further cold picking work. CPU asset
preparation p95 was 3.5 / 6.4 / 7.5 ms and median GPU query duration was
8.1 / 23.8 / 12.6 ms for flat / textured / raw respectively. GPU queries include
the preparation/render scope described above.

Large whole-model selection and movement remain slow. Long-task medians across
the two shared-instance runs were 951 and 1,584 ms. These are real browser
main-thread tasks, not idle sleeps. Script-observed editing cadence also includes
automation and intentional gaps, so it is not a pure application FPS measurement.
The pointer-phase figures above must not be presented as sustained editing speed.
No browser before/after speedup is established by these candidate-only runs.
Other applications on the shared desktop host were not controlled.

The complete phase summaries, environment, fixture/build checksums and process
memory observations are in the
[verification record](verification/editor-background-performance-2026-11-18.json).

## Browser conversion and large previews, 2026-11-18 follow-up

Model import and SKP export now execute in an isolated, cancellable WASM worker.
Closing the dialog terminates the worker; option/version changes and document
revision checks prevent stale results from being applied. The package fingerprints
all worker assets together. Conversion no longer occupies the editor thread, but
copying input bytes, serializing the document for transfer and applying the returned
model still do. A worker is not a guarantee of a bounded frame time or memory use.

The Move preview previously traversed the entire occurrence hierarchy once for
**each selected instance**, and rebuilt that same preview for dimensions, text,
overlays and highlighting. It now traverses the hierarchy once for the selected
instance set, shares one preview across overlay consumers, and queries face/instance
highlights without constructing preview lines or annotations. A regression covers
nested transforms, active editing contexts and exclusion of unselected siblings.

Actual cursor checks on a public theater project selected 863 elements, with
1,172 visible occurrences and 24,934 visible faces. Before the hierarchy fix,
active editor updates were about 2.2 FPS; after that first fix they were about
20 FPS. Median browser long tasks fell from 462 to 50 ms. Selection alone produced
no long tasks in either run. These intermediate measurements precede the additional
shared-preview/highlight-query optimization. They are not a controlled universal
speedup: the old drag timed out and was cancelled, while the new drag completed
and was undone, and the observed intervals have different durations. Idle browser
RAF frequency is not presented as active editor performance.

Native texture loading now borrows unescaped base64 directly from the input JSON.
A 4 MiB binary payload consequently avoids a 5,592,408-byte temporary encoded string.
Escaped JSON falls back to owned decoding; a regression verifies both paths through
the full loader. This is an allocation reduction, not a new streaming parser or a
claim about peak process RSS. Geometry records, validation, backwards compatibility
and arbitrary record ordering remain unchanged.

The cursor-check summaries and limitations are recorded in
[the browser follow-up](verification/editor-browser-followup-2026-11-18.json).
The conversion checks and successful native Edge Save As are recorded separately
in [the interoperability verification](verification/skp-interoperability-2026-11-18.json).

The final build, including shared preview consumers, showed 34.3 active editor
updates per second during the same theater drag, with one 53 ms long task in the
recording. The drag completed, and Undo restored the model with no undo entries
and Redo available. This remains a short, single-machine cursor check rather than
a sustained-rate guarantee.

A further candidate-only stress check imported a 72,000-face / 144,000-edge root
mesh through the worker into a new document. Rendering completed, Select All
highlighted 216,000 elements, and an individual selection worked afterwards.
There were no console errors or crashes. The first pointer handler took 123.2 ms;
subsequent measured handlers took at most 5.8 ms. Select All produced one 93 ms long
task. Cooperative surface batches peaked at 6.2 ms in this run. The earlier Linux
770 ms cold-pick result used a different host and is not a paired comparison.

### Established limits

The changes above remove repeated hierarchy traversals, redundant previews and a
texture-payload copy; they do not make all model operations incremental. At that revision, cold
spatial-index construction and the large selection highlight remained synchronous,
as demonstrated by the stress check. Individual polygons, glued openings,
edge-cache preparation, image decoding, main-thread worker transfer and individual
render pieces can still exceed a frame budget. The six-millisecond cooperative
budget is checked between operations, not a hard deadline. Linked visible
annotations and dense glued geometry are outside this cursor check's coverage.
Geometry input records remain owned; this change does not replace the loader with
a streaming parser. These are explicit performance/coverage limits, not claims of
completed worst-case latency or peak-memory verification.

Final verification passed 1,039 Rust library tests (six manual benchmarks ignored),
33 JavaScript tests, five packaging tests, the local HTTP-server test, formatting,
strict workspace all-target Clippy and WASM app/worker Clippy. The web release and
its static module/asset validation passed. Four MCP integration tests passed earlier
in this change. Independent SketchUp acceptance remains explicitly unverified at
the user's request; internal round trips are not a substitute.

## Selection latency, 2026-11-19

Large selection highlights now prepare and submit multiple small meshes instead of
one complete skin. Work yields after a soft three-millisecond budget (checked every
64 steps) or 12,288 generated vertices. Even one large polygon yields between its
triangles. The unchanged selection keeps the renderer awake until all pieces have
been submitted. A changed selection, preview, content revision or document generation
cancels pending work and removes its already submitted pieces. Hidden geometry,
section cuts, occurrence transforms and the editing context retain their previous
semantics. Uncut triangles avoid an intermediate clipping allocation, and small
selections only visit chunks containing selected faces. One material is shared by
all highlight pieces.

Select All now builds its ordered set directly from the active context's eligible
records. It does not look up every identifier a second time or allocate a separate
intermediate vector. The existing hidden-geometry, tag, instance and section-plane
rules are unchanged. It remains a selection change, without changing content
revision or Undo history; arbitrary externally supplied selections still validate
identifiers.

Cold edge/face picking no longer builds the vertex-query tree or face adjacency.
Those immutable indices initialize once when their first actual consumer needs
them. Edge and face bounds use a temporary vertex-position table instead of
repeated ordered-block searches. The table is released after index construction.
Placement bounds reuse the spatial snapshot's conservative vertex bounds plus
curve centres, including small edits, instead of rescanning every edge. Old or
removed vertices may conservatively enlarge bounds until the next rebuild; exact
query filters still use the current geometry. This trades a temporary position
lookup table for fewer searches and defers indices unused by ordinary picking.

The temporary position table reserves the known vertex count up front. Web release
builds optimize the mesh and inference crates for execution speed (`opt-level = 3`);
other crates retain size optimization, and the final package still uses wasm-opt
`-Oz`. This specifically targets the remaining cold spatial-tree work. The package
size and cursor verification are recorded alongside the structural changes.

### Cursor verification and remaining limits

On the same 72,000-face / 144,000-edge fixture at 1280 × 720, the structural
fix reduced the first pointer handler from 141.6 to 75.2 ms. Select All previously
produced an 81 ms browser long task; the candidate produced none above the browser's
50 ms threshold. Highlight generation ran in 37 measured batches (maximum 6.6 ms),
finishing over approximately 0.99 seconds while yielding between frames. These are
single paired desktop observations, not worst-case guarantees.

The final build additionally reserves the position table and enables the crate
optimization overrides. Cursor verification confirmed selection of all 216,000
elements, complete highlights, and clean replacement with one selected element;
there were no console errors. Its profile download could not be retrieved through
the automation interface, so no additional speedup is attributed to those last
changes. Main WASM gzip size is 10,449,967 bytes, versus 10,287,991 at baseline.

Cold spatial-tree construction still has synchronous work: the measured 75.2 ms
handler remains above a frame budget. Select All set construction and initial work
queue preparation also remain synchronous; the cooperative three-millisecond limit
is soft and excludes upload/setup costs. The other established limits above remain
outside this selection-focused change. Independent SketchUp acceptance remains
explicitly unverified.

Validation passed 1,012 relevant Rust tests (six manual benchmarks ignored), strict
workspace all-target Clippy, formatting, and web release/static package checks.
The final reservation change was rechecked with 134 mesh/inference tests and Clippy.
Detailed provenance and measurement limitations are in
[selection-latency-2026-11-19.json](verification/selection-latency-2026-11-19.json).

## Native loading and malformed input, 2026-11-19

Reviewed `native::load`, mesh assembly/reindexing/validation, document hierarchy
budgets, native file-worker loading, and `files::open_text` installation. This is
an initial loading-focused pass, not an exhaustive audit of every renderer or codec.

Mesh validation now reuses its existing unordered-edge index to detect duplicate
edges instead of building another ordered set. It walks face boundaries without
allocating a temporary edge list and reuses one polygon-point buffer across faces.
The duplicate check also detects a stale edge index: every stored edge must map
back to its own identifier. Forward and reversed duplicate edges and references
crossing definition boundaries have regression coverage.

The same pass found that empty or two-vertex holes were accepted. Regression tests
reproduced acceptance before the fix; this was not a reproduced application crash.
All face loops now need at least three vertices. Mesh validation also refuses
non-finite vertex positions and non-finite derived planes/distances, including
arithmetic overflow from otherwise finite JSON numbers. Existing valid round trips
and modelling operations are covered by the mesh/model/I/O/tool test suites.

A deterministic malformed-input corpus covers truncated JSON, deep JSON nesting,
wrong top-level and record field types, missing references and extreme identifiers.
An accepted mutation must validate and survive another save/load. An application
regression test verifies that an unsuccessful Open preserves the dirty document,
selection, camera, active tab, document generation, path and Undo history, and that
Undo still works afterwards. This corpus is not a proof against all malformed files
or memory exhaustion from arbitrarily large input.

`benchmark_load` is a repeatable CPU-only experiment: 3 warmups and 21 measured
load/validate/drop runs on an already resident string, with a save/load stability
check outside the timing. It supports root meshes without instances, unlike the
existing movement benchmark. It does not measure disk reads, browser frames or GPU
upload. Run with `cargo run --profile portable -p rysmo_io --example benchmark_load
-- FILE.rysmo` (plus the host target where the workspace requires it).

Remaining candidates: the native browser Open path still calls `open_text` on the
main thread; import workers do not make that path asynchronous. JSON currently
retains deserialized record vectors while constructing the document, and embedded
images add decoded allocations. Reindexing builds vertex adjacency through an
intermediate ordered map. Rendering preparation and image decoding need separate
browser/peak-memory measurements before changing their scheduling or storage.

After compilation completed, two sequential baseline/candidate comparisons on the
15,071,084-byte, 72,000-face fixture measured median loading at 138.84 → 131.19 ms
and 139.08 → 130.44 ms (5.5–6.2% faster). The small dimension fixture measured
0.0255 → 0.0260 ms, effectively unchanged at this scale. All 751 distinct relevant
mesh/model/I/O/tools and failed-Open application tests passed (two manual benchmarks
ignored), as did strict workspace all-target Clippy and formatting.
[Measurement details](verification/native-load-2026-11-19.json) retain the fixture
hashes, p95 values and scope limitations. No browser speedup is inferred from these
native CPU measurements.


## Record ordering and adjacency construction, 2026-11-19

Follow-up review covered browser Open (`web::take_arrived`), worker transfer
(`web_worker`), `Mesh` deserialization, `BlockMap` insertion and `Mesh::reindex`.
The existing worker result deserializes and assembles meshes again on the main
thread. Merely sending native Open through that protocol would retain substantial
synchronous work and add copying. No asynchronous native-Open claim is made here;
a bounded transfer/assembly design still needs a separate implementation and test.

Two smaller changes were retained after measurement:

- Vertex-to-edge adjacency accumulates in a randomized standard `HashMap`, then
  sorts vertices once before populating block storage. Edge lists retain ascending
  IDs, so editing behaviour is deterministic. Capacity is bounded by the smaller
  of vertex count and twice edge count; isolated-vertex models allocate no large
  empty table. This is a temporary construction structure, not a persistent cache.
- Native vertices, edges, faces, curves and instances are sorted in place by ID
  before insertion. Arbitrary record order remains supported, and duplicate IDs
  remain errors. Descending sparse IDs previously shifted the block vector for
  every inserted record, causing quadratic work. Sorting does not renumber IDs or
  copy geometry payloads. Other derived-index worst cases are not eliminated.

Paired CPU measurements against `e806014`, after compilation, show the 72,000-face
fixture improving from 139.56 to 132.96 ms and from 138.01 to 133.29 ms (3.4–4.7%).
A valid 914,590-byte fixture with 30,000 isolated vertices and descending sparse
IDs improved from 104.48 to 5.21 ms (about 20×). The small dimension fixture remained
approximately 0.026 ms. These observations supersede noisier preliminary runs;
there is no inferred browser-frame or peak-memory guarantee.

Regression tests compare adjacency with a direct edge scan, including sparse IDs,
isolated and missing vertices, repeated rebuilding, and editing a shared snapshot.
Native loading tests check canonical round trips for reversed/rotated sparse
records and rejection of duplicate IDs. All 752 mesh/model/I/O/tool tests passed
(two manual benchmarks ignored), along with strict workspace all-target Clippy.
[Full measurements and fixture provenance](verification/native-load-order-2026-11-19.json).

## Edge-pair indexing and worker snapshot reconstruction, 2026-11-19

Reviewed the derived endpoint-to-edge index and the `BlockMap` serde path used by
worker snapshots and stored procedural inputs. Sorting native records by their own
IDs did not bound derived-index insertion: ascending edge IDs can reference sparse
endpoint pairs in descending order. `Mesh::reindex` now sorts canonical endpoint
pairs before constructing the block map. Equal pairs still retain the highest edge
ID during construction, and validation continues to reject duplicate geometry.
The temporary vector costs one pair-plus-ID record per edge; this pass does not
claim lower peak memory. Lookup tests exercise reversed endpoints, sparse IDs,
missing edges and repeated reindexing against the original snapshot.

`BlockMap` deserialization no longer always allocates an intermediate tree. The
normal ordered snapshot path accumulates records directly and replaces adjacent
duplicate values immediately. At the first out-of-order key it falls back to the
previous tree-based construction, consuming subsequent entries into that tree.
This preserves arbitrary map order and last-value-wins semantics without retaining
all duplicate payloads or introducing quadratic unordered block insertion. Native
`.rysmo` record IDs still have their separate duplicate-rejection check. Mesh
reconstruction still validates geometry; no trusted-input bypass was introduced.

The new `benchmark_transfer` example measures native loading, document snapshot
serialization and recipient deserialization separately, verifies canonical native
round-trip equality, and uses 3 warmups plus 21 samples. It is a native CPU proxy,
not a browser measurement: it excludes worker startup, JS/WASM transfers, file I/O
and rendering. It demonstrates why merely putting native parsing into the existing
worker protocol does not eliminate a large synchronous step. A bounded recipient
assembly design is still required before claiming responsive asynchronous Open.

Final sequential CPU comparisons after compilation:

| Operation / fixture | Before | After |
| --- | ---: | ---: |
| Native load, 72,000 faces | 127.86 ms | 129.42 ms |
| Native load, 20,000 reversed sparse endpoint pairs | 56.29 ms | 15.56 ms |
| Native load, small dimension fixture | 0.02571 ms | 0.02571 ms |
| Recipient snapshot reconstruction, 72,000 faces | 111.32 ms | 91.31 ms |
| Snapshot serialization, same model | 28.02 ms | 28.05 ms |

The roughly 1.2% normal-load cost is an explicit tradeoff for bounding hostile
endpoint ordering, not a general native-loading speedup. Snapshot reconstruction
improved about 18%; its baseline already includes edge sorting, isolating the
ordered-map deserialization change. The 15,409,280-byte transfer still has a
substantial synchronous reconstruction cost. These results do not establish browser
latency, peak memory or completely asynchronous Open. All 756 mesh/model/I/O/tools/
worker tests passed (two manual benchmarks ignored), as did strict workspace
all-target Clippy and formatting. [Full evidence](verification/edge-index-transfer-2026-11-19.json).

## Cancellable native Open worker, 2026-11-19

File > Open and Open Recent now run native parsing/validation and initial camera
framing in the existing isolated WASM worker. The recipient reconstructs the
validated snapshot and only then installs it. No geometry checks were disabled.
The old document, history and Save destination remain active until success. Cancel
terminates the worker; staged file handles are rejected on every unsuccessful
exit. Request sequence numbers reject an older picker/recent-file response after
a newer request, while document generation/revision checks reject stale results.

Cursor testing caught a new ownership bug before release: keeping the worker state
inside an inactive tab parked its result until the user returned. The state is now
shared across tab changes along with application preferences, so the active poll
sees the changed generation and cancels the request. This is why switching away
and back is part of the regression protocol, not just inspecting the other tab.

This change moves work off the UI thread but does not claim a bounded complete
Open. Source-string copying, snapshot reconstruction, workspace persistence and
render preparation still contribute main-thread work; startup recovery is not
converted here. Existing CPU snapshot measurements do not establish browser frame
latency or peak memory for this new path.

Validation: 3 worker Rust tests and 33 JavaScript worker/file tests passed, along
with strict native workspace and WASM app/worker Clippy, formatting and static
package validation. Cursor testing of the final build verified small and
72,000-face files, switching away and back during a pending Open, and rejection
of a missing-vertex file without changing the active large model. The earlier
candidate also verified explicit Cancel preserving a dirty document and history.
A local iframe exercised the standard file-input fallback; a 15-second worker
HTTP delay made cancellation deterministic. This is functional verification, not
a browser performance benchmark. The native File System Access picker itself was
not automated. [Full evidence](verification/native-open-worker-2026-11-19.json).

## Geometry validation experiment, 2026-11-19

Reviewed the native Open worker recipient, mesh index construction, face validation,
installation and recovery serialization. `benchmark_transfer` now also measures
geometry validation alone and native serialization, in addition to loading and
snapshot reconstruction. These are sequential native CPU measurements (3 warmups,
21 samples), not browser frame times or end-to-end opening latency.

An experiment replacing the adjacency hash table with sorted endpoint/edge pairs
was rejected: large-model reconstruction was 95.71 versus 95.35 ms and native load
135.46 versus 135.91 ms, insufficient evidence of a useful improvement. Production
adjacency construction is unchanged; the high-valence regression test is retained.

Face validation already resolves outer-loop positions to construct the plane. It
now reuses those positions for distance checks instead of looking up every outer
vertex again. Hole vertices still undergo the same checks, as do missing edges,
non-finite coordinates, degenerate planes and overflow. Tests displace an outer
vertex and a vertex in each of two holes, checking rejection by direct validation,
assembly and snapshot deserialization; a valid holed mesh still round-trips.

No asynchronous installation or new trusted-input path was introduced. Recovery
serialization and final rendering preparation remain separate optimization targets.
The public web package was not rebuilt or deployed for this experiment.

Final 72,000-face CPU medians: geometry validation 33.58 → 26.18 ms,
snapshot reconstruction 95.06 → 89.24 ms, and native load 134.08 → 128.43 ms.
A preceding pair run in the opposite order measured reconstruction 96.61 →
91.84 ms. Unchanged serialization also varies between runs, so these are observed
measurements, not a guaranteed percentage speedup. The small fixture's load
median increased from 0.02654 to 0.02904 ms; no small-model speedup is claimed.
Native save still costs about 32–35 ms on the large fixture, independently of
workspace JSON wrapping and browser storage.
[Measurements and reviewed areas](verification/geometry-validation-2026-11-19.json).

Validation passed 759 library tests (two manual benchmarks ignored), strict
all-target native Clippy for mesh/model/I/O/tools/worker, formatting and diff checks.

## Camera-only recovery updates, 2026-11-19

Reviewed recovery cache invalidation, native camera serialization, immutable
snapshot ownership, cache eviction and workspace JSON construction. Previously a
camera change serialized the entire document even when its content revision was
unchanged. `native::SavedText` now keeps the camera byte range recorded by the
writer. A camera-only update builds a new immutable string from the unchanged
prefix/suffix and the canonical camera record; it does not search or parse text.
The ordinary writer and this update share rounding and formatting. The range
cannot be supplied by a file or changed by a caller.

The recovery cache uses this only when generation/content revision still match.
Edits, undo/redo and document replacement use full serialization. Old asynchronous
snapshots retain their own immutable strings. The 64 MiB retained-capacity budget
and admission policy remain in place, including after camera updates and eviction.

On the 72,000-face fixture, native full serialization took 31.93 ms versus 0.316 ms
for replacing a cached camera record. The whole single-document workspace snapshot
(including JSON wrapping) took 55.43 ms with full serialization versus 23.17 ms with
camera reuse. These are native CPU medians (3 warmups, 21 samples), not browser
latency. The full-serialization proxy clears its cache outside the timed operation;
each measured snapshot is checked against the legacy content after timing.
The small dimension fixture measured 0.01246 ms versus 0.000542 ms for text creation.

Tests cover absent/existing cameras, repeated updates, canonical rounding, Unicode
and escaped camera-like text, old snapshot immutability, content edits after camera
updates, undo/redo, tab identity and memory-budget exhaustion. All 132 I/O and 238
application tests passed (five manual tests ignored); the camera benchmark and
strict native app/I/O all-target Clippy also passed.

First opening and content edits still pay full serialization. The optimized path
still copies native text and escapes it into workspace JSON; browser transfer and
IndexedDB costs are not included. No web rebuild or deployment was done here.
[Measurements and reviewed areas](verification/camera-recovery-2026-11-19.json).
