//! Reuse native document text across browser workspace recovery snapshots.
use std::collections::BTreeMap;
use std::sync::Arc;

use rysmo_math::OrbitCamera;
use serde::Serialize;

use crate::{autosave::DocumentVersion, state::App3d};

// Bound retained string allocations, including unused buffer capacity.
const MAX_BYTES: usize = 64 * 1024 * 1024;

struct Entry {
    version: DocumentVersion,
    camera: OrbitCamera,
    text: rysmo_io::native::SavedText,
}

pub(crate) struct Cache {
    entries: BTreeMap<u64, Entry>,
    bytes: usize,
    limit: usize,
}

impl Default for Cache {
    fn default() -> Self {
        Self { entries: BTreeMap::new(), bytes: 0, limit: MAX_BYTES }
    }
}

pub(crate) struct Snapshot {
    pub active_text: Arc<String>,
    pub workspace: String,
}

#[derive(Serialize)]
struct Record {
    id: u64,
    name: String,
    text: Arc<String>,
    dirty: bool,
}

impl Cache {
    fn text(&mut self, id: u64, session: &App3d, active: bool) -> Arc<String> {
        let version = DocumentVersion::for_recovery(&session.editor, session.document_generation);
        if let Some(entry) = self.entries.get(&id)
            && entry.version == version
            && entry.camera == session.camera
        {
            return entry.text.text().clone();
        }
        let previous = self.entries.remove(&id);
        if let Some(old) = &previous {
            self.bytes -= old.text.text().capacity();
        }
        let saved = if let Some(old) = previous.filter(|entry| entry.version == version) {
            old.text.with_camera(session.camera)
        } else {
            let mut document = session.editor.doc().clone();
            document.camera = Some(session.camera);
            rysmo_io::native::SavedText::new(&document)
        };
        let text = saved.text().clone();
        if text.capacity() <= self.limit {
            // Admit the active text preferentially. An oversized workspace otherwise
            // keeps its cached inactive texts instead of evicting them in a cycle.
            while active && self.bytes + text.capacity() > self.limit {
                let Some((_, old)) = self.entries.pop_first() else { break };
                self.bytes -= old.text.text().capacity();
            }
            if self.bytes + text.capacity() <= self.limit {
                self.bytes += text.capacity();
                self.entries.insert(id, Entry { version, camera: session.camera, text: saved });
            }
        }
        text
    }

    pub fn snapshot(&mut self, app: &App3d) -> Snapshot {
        let ids: Vec<_> = app.documents.sessions(app).map(|(id, _)| id).collect();
        self.entries.retain(|id, entry| {
            let keep = ids.contains(id);
            if !keep {
                self.bytes -= entry.text.text().capacity();
            }
            keep
        });
        let documents: Vec<_> = app
            .documents
            .sessions(app)
            .map(|(id, session)| Record {
                id,
                name: session
                    .path
                    .as_ref()
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Untitled.rysmo".into()),
                text: self.text(id, session, id == app.documents.active),
                dirty: session.editor.is_dirty(),
            })
            .collect();
        // The session iterator always yields the active document first. Keep the same
        // text alive for the legacy single-document recovery write, even if evicted.
        let active_text = documents[0].text.clone();
        #[derive(Serialize)]
        struct Workspace {
            active: u64,
            documents: Vec<Record>,
            library: serde_json::Value,
        }
        let workspace = serde_json::to_string(&Workspace {
            active: app.documents.active,
            documents,
            library: app.features.library.recovery(),
        })
        .expect("recovery data serialises");
        Snapshot { active_text, workspace }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rysmo_math::DVec3;
    use rysmo_model::{Command, Editor};

    fn app() -> App3d {
        let mut app = App3d::new(Editor::new());
        app.welcome = false;
        app
    }
    fn draw(app: &mut App3d) {
        app.editor.apply(Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::X]], facing: None }).unwrap();
    }
    fn decoded(snapshot: &Snapshot) -> serde_json::Value {
        serde_json::from_str(&snapshot.workspace).unwrap()
    }
    fn assert_legacy_equivalent(app: &App3d, snapshot: &Snapshot) {
        let documents: Vec<_> = app
            .documents
            .sessions(app)
            .map(|(id, session)| {
                let mut doc = session.editor.doc().clone();
                doc.camera = Some(session.camera);
                serde_json::json!({"id": id, "name": session.path.as_ref()
                .map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| "Untitled.rysmo".into()),
                "text": rysmo_io::save(&doc), "dirty": session.editor.is_dirty()})
            })
            .collect();
        assert_eq!(
            decoded(snapshot),
            serde_json::json!({"active": app.documents.active,
            "documents": documents, "library": app.features.library.recovery()})
        );
        assert_eq!(decoded(snapshot)["documents"][0]["text"].as_str().unwrap(), snapshot.active_text.as_str());
    }

    #[test]
    fn unchanged_documents_reuse_text_but_refresh_save_path_dirty_and_library_metadata() {
        let mut app = app();
        draw(&mut app);
        let mut cache = Cache::default();
        let first = cache.snapshot(&app);
        let edge = app.editor.mesh().edges().next().unwrap().0;
        app.editor.set_selection([rysmo_model::Element::Edge(edge)]);
        let selected = cache.snapshot(&app);
        assert!(Arc::ptr_eq(&first.active_text, &selected.active_text));
        app.editor.mark_saved();
        app.path = Some("renamed.rysmo".into());
        app.features.library.open = true;
        let saved = cache.snapshot(&app);
        assert!(Arc::ptr_eq(&first.active_text, &saved.active_text));
        assert_legacy_equivalent(&app, &saved);
        assert!(decoded(&first)["documents"][0]["dirty"].as_bool().unwrap());
        assert!(!decoded(&saved)["documents"][0]["dirty"].as_bool().unwrap());
        app.editor.mark_dirty();
        let dirty = cache.snapshot(&app);
        assert!(Arc::ptr_eq(&saved.active_text, &dirty.active_text));
        assert_legacy_equivalent(&app, &dirty);
    }

    #[test]
    fn equal_revision_tabs_retain_independent_text_and_closed_tabs_are_evicted() {
        let mut app = app();
        draw(&mut app);
        let first_id = app.documents.active;
        let mut cache = Cache::default();
        let first = cache.snapshot(&app);
        assert!(crate::documents::create(&mut app));
        draw(&mut app);
        let second_id = app.documents.active;
        let second = cache.snapshot(&app);
        assert!(!Arc::ptr_eq(&first.active_text, &second.active_text));
        assert_eq!(cache.entries.len(), 2);
        crate::documents::switch(&mut app, first_id);
        let switched = cache.snapshot(&app);
        assert!(Arc::ptr_eq(&first.active_text, &switched.active_text));
        assert_legacy_equivalent(&app, &switched);
        app.editor.apply(Command::DrawLines { lines: vec![[DVec3::Y, DVec3::X + DVec3::Y]], facing: None }).unwrap();
        let edited = cache.snapshot(&app);
        assert!(!Arc::ptr_eq(&switched.active_text, &edited.active_text));
        assert!(Arc::ptr_eq(&second.active_text, cache.entries[&second_id].text.text()));
        // An outstanding asynchronous write owns its old immutable text, not a
        // mutable reference into the cache that a later edit could overwrite.
        assert_eq!(rysmo_io::load(&switched.active_text).unwrap().root.mesh.edge_count(), 1);
        assert_eq!(rysmo_io::load(&edited.active_text).unwrap().root.mesh.edge_count(), 2);
        assert_legacy_equivalent(&app, &edited);
        crate::documents::close(&mut app);
        let closed = cache.snapshot(&app);
        assert_eq!(cache.entries.len(), 1);
        assert_eq!(cache.bytes, cache.entries.values().map(|entry| entry.text.text().capacity()).sum::<usize>());
        assert_legacy_equivalent(&app, &closed);
    }

    #[test]
    fn camera_edits_undo_redo_and_session_replacement_invalidate_cached_text() {
        let mut app = app();
        let mut cache = Cache::default();
        let initial = cache.snapshot(&app);
        app.camera.yaw += 0.25;
        let camera = cache.snapshot(&app);
        assert!(!Arc::ptr_eq(&initial.active_text, &camera.active_text));
        assert_legacy_equivalent(&app, &camera);
        draw(&mut app);
        let edited = cache.snapshot(&app);
        assert_legacy_equivalent(&app, &edited);
        app.editor.undo();
        let undone = cache.snapshot(&app);
        assert!(!Arc::ptr_eq(&edited.active_text, &undone.active_text));
        assert_legacy_equivalent(&app, &undone);
        app.editor.redo();
        let redone = cache.snapshot(&app);
        assert_legacy_equivalent(&app, &redone);
        app.document_generation = crate::documents::generation();
        let replaced = cache.snapshot(&app);
        assert!(!Arc::ptr_eq(&redone.active_text, &replaced.active_text));
        assert_legacy_equivalent(&app, &replaced);
    }

    #[test]
    fn memory_budget_bounds_retention_without_losing_any_snapshot_documents() {
        let mut app = app();
        draw(&mut app);
        let mut cache = Cache::default();
        let first = cache.snapshot(&app);
        cache.limit = first.active_text.capacity() + 32;
        assert!(crate::documents::create(&mut app));
        draw(&mut app);
        let snapshot = cache.snapshot(&app);
        assert!(cache.bytes <= cache.limit);
        assert_eq!(cache.entries.len(), 1);
        assert!(cache.entries.contains_key(&app.documents.active));
        assert_legacy_equivalent(&app, &snapshot);
        assert!(Arc::ptr_eq(&snapshot.active_text, &cache.snapshot(&app).active_text));
        let mut uncached = Cache { limit: 1, ..Default::default() };
        let snapshot = uncached.snapshot(&app);
        assert_eq!(uncached.bytes, 0);
        assert!(uncached.entries.is_empty());
        assert_legacy_equivalent(&app, &snapshot);
    }

    #[test]
    fn repeated_camera_updates_keep_snapshots_immutable_and_budget_exact() {
        let mut app = app();
        draw(&mut app);
        let mut cache = Cache::default();
        let first = cache.snapshot(&app);
        let old_text = first.active_text.to_string();
        for i in 0..24 {
            app.camera.yaw = i as f64 * 0.123456789123;
            app.camera.parallel = i % 2 == 0;
            app.editor.set_camera(app.camera);
            let snapshot = cache.snapshot(&app);
            assert_legacy_equivalent(&app, &snapshot);
            assert_eq!(cache.bytes, cache.entries.values().map(|entry| entry.text.text().capacity()).sum::<usize>());
            assert!(cache.bytes <= cache.limit);
            assert_eq!(first.active_text.as_str(), old_text);
        }
        cache.limit = 1;
        app.camera.yaw += 0.25;
        assert_legacy_equivalent(&app, &cache.snapshot(&app));
        assert_eq!(cache.bytes, 0);
        assert!(cache.entries.is_empty());
        cache.limit = MAX_BYTES;
        cache.snapshot(&app);
        // A content edit after camera-only updates must never reuse the old body.
        app.editor.apply(Command::DrawLines { lines: vec![[DVec3::Y, DVec3::X + DVec3::Y]], facing: None }).unwrap();
        assert_legacy_equivalent(&app, &cache.snapshot(&app));
        assert_eq!(rysmo_io::load(&cache.snapshot(&app).active_text).unwrap().root.mesh.edge_count(), 2);
    }

    #[test]
    #[ignore = "manual CPU benchmark; set RYSMO_RECOVERY_BENCH_MODEL to a native model path"]
    fn benchmark_camera_workspace_recovery() {
        use std::{hint::black_box, time::Instant};
        let source = std::fs::read_to_string(std::env::var("RYSMO_RECOVERY_BENCH_MODEL").unwrap()).unwrap();
        let mut app = app();
        app.editor = Editor::with_document(rysmo_io::load(&source).unwrap());
        let mut results = Vec::new();
        for reuse_camera in [false, true] {
            let mut cache = Cache::default();
            cache.snapshot(&app);
            let mut samples = Vec::new();
            for i in 0..24 {
                app.camera.yaw = i as f64 * 0.125;
                if !reuse_camera {
                    // Previous camera invalidation serialized the complete model.
                    cache = Cache::default();
                }
                let start = Instant::now();
                let snapshot = black_box(cache.snapshot(&app));
                let elapsed = start.elapsed().as_secs_f64() * 1000.;
                assert_legacy_equivalent(&app, &snapshot);
                if i >= 3 {
                    samples.push(elapsed);
                }
            }
            samples.sort_by(f64::total_cmp);
            results.push(serde_json::json!({"reuse_camera": reuse_camera,
                "median_ms": samples[10], "p95_ms": samples[19], "samples": 21}));
        }
        println!("camera-recovery-benchmark: {}", serde_json::json!(results));
    }

    #[test]
    #[ignore = "manual CPU benchmark; set RYSMO_RECOVERY_BENCH_MODEL to a native model path"]
    fn benchmark_unchanged_workspace_recovery() {
        use std::{hint::black_box, time::Instant};
        let path = std::env::var("RYSMO_RECOVERY_BENCH_MODEL").expect("native benchmark model path");
        let source = std::fs::read_to_string(path).unwrap();
        let document = rysmo_io::load(&source).unwrap();
        let mut app = app();
        app.camera = document.camera.unwrap_or_default();
        app.editor = Editor::with_document(document);
        let mut cache = Cache::default();
        fn legacy(app: &App3d) -> Snapshot {
            let mut active = app.editor.doc().clone();
            active.camera = Some(app.camera);
            let active_text = Arc::new(rysmo_io::save(&active));
            let documents: Vec<_> = app
                .documents
                .sessions(app)
                .map(|(id, session)| {
                    let mut doc = session.editor.doc().clone();
                    doc.camera = Some(session.camera);
                    serde_json::json!({"id": id, "name": session.path.as_ref()
                    .map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| "Untitled.rysmo".into()),
                    "text": rysmo_io::save(&doc), "dirty": session.editor.is_dirty()})
                })
                .collect();
            let workspace = serde_json::json!({"active": app.documents.active,
                "documents": documents, "library": app.features.library.recovery()})
            .to_string();
            Snapshot { active_text, workspace }
        }
        let expected = legacy(&app);
        let actual = cache.snapshot(&app);
        assert_eq!(decoded(&actual), decoded(&expected));
        assert_eq!(actual.active_text, expected.active_text);
        let mut measure = |mode: u8| {
            let mut samples = Vec::new();
            for index in 0..24 {
                let start = Instant::now();
                black_box(match mode {
                    0 => legacy(&app),
                    1 => cache.snapshot(&app),
                    _ => Cache::default().snapshot(&app),
                });
                if index >= 3 {
                    samples.push(start.elapsed().as_secs_f64() * 1000.);
                }
            }
            samples.sort_by(f64::total_cmp);
            serde_json::json!({"median_ms": samples[10], "p95_ms": samples[19]})
        };
        let before = measure(0);
        let after = measure(1);
        let cold = measure(2);
        println!(
            "recovery-benchmark: {}",
            serde_json::json!({
                "input_bytes": source.len(), "samples": 21,
                "before": before, "cached": after, "cold": cold, "cached_capacity_bytes": cache.bytes,
            })
        );
    }
}
