//! One pending preparation job. Native work uses a thread; web work yields per batch.
use super::*;
#[cfg(any(test, target_arch = "wasm32"))]
use std::collections::VecDeque;
type Revision = (u64, u64);
type Owner = Option<DefinitionId>;
type Prepared = (Owner, Arc<rysmo_model::Definition>, Arc<DrawnFaces>, bool);

#[cfg(not(target_arch = "wasm32"))]
pub(super) struct Job {
    revision: Revision,
    receiver: std::sync::Mutex<std::sync::mpsc::Receiver<Vec<Prepared>>>,
    cancel: Arc<std::sync::atomic::AtomicBool>,
}
#[cfg(not(target_arch = "wasm32"))]
impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}
#[cfg(target_arch = "wasm32")]
pub(super) struct Job {
    revision: Revision,
    work: Work,
}

impl DrawnCache {
    pub(crate) fn busy(&self) -> bool {
        self.job.is_some() || self.uploading
    }
    fn reusable(&self, doc: &rysmo_model::Document, owner: Owner) -> bool {
        let Some(definition) = doc.def_arc(owner) else { return false };
        self.kept.get(&owner).is_some_and(|(held, faces)| {
            self.evaluated.contains(&owner)
                || (!has_openings(definition)
                    && (Arc::ptr_eq(held, definition)
                        || (!has_openings(held) && faces.mesh.shares_storage_with(&definition.mesh))))
        })
    }
    pub(super) fn ready(&mut self, doc: &rysmo_model::Document, revision: Revision, owners: BTreeSet<Owner>) -> bool {
        self.prepare(doc, revision);
        #[cfg(not(target_arch = "wasm32"))]
        if self.job.is_some() {
            return false;
        }
        #[cfg(target_arch = "wasm32")]
        {
            if self.job.as_ref().is_some_and(|job| job.revision != revision) {
                self.job = None;
            }
            if let Some(job) = &mut self.job {
                let _profile = crate::web::ProfileSpan::new("surface_batch", revision.0, revision.1);
                let start = bevy::platform::time::Instant::now();
                while !job.work.step() {
                    if start.elapsed().as_secs_f64() >= 0.006 {
                        return false;
                    }
                }
                let result = self.job.take().expect("active job").work.output;
                self.install(result);
            }
        }
        let missing: Vec<_> = owners.into_iter().filter(|owner| !self.reusable(doc, *owner)).collect();
        if missing.is_empty() {
            return true;
        }
        let snapshot = doc.clone();
        let old: HashMap<_, _> =
            missing.iter().filter_map(|owner| self.kept.get(owner).map(|(_, faces)| (*owner, faces.clone()))).collect();
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (sender, receiver) = std::sync::mpsc::channel();
            let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let cancelled = cancel.clone();
            let spawned = std::thread::Builder::new().name("rysmo-surfaces".into()).spawn(move || {
                let mut result = Vec::new();
                for owner in missing {
                    if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
                        return;
                    }
                    let definition = snapshot.def_arc(owner).expect("visible definition").clone();
                    let mesh = snapshot.effective_mesh_isolated(owner);
                    let faces = Arc::new(DrawnFaces::new(mesh, old.get(&owner).map(Arc::as_ref)));
                    result.push((owner, definition.clone(), faces, has_openings(&definition)));
                }
                let _ = sender.send(result);
            });
            if spawned.is_ok() {
                self.job = Some(Job { revision, receiver: std::sync::Mutex::new(receiver), cancel });
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.job = Some(Job { revision, work: Work::new(snapshot, missing, old) });
        }
        false
    }
    pub(super) fn poll(&mut self, revision: Revision) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(job) = &self.job {
            if job.revision != revision {
                job.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            let result = job.receiver.lock().unwrap_or_else(std::sync::PoisonError::into_inner).try_recv();
            match result {
                Ok(result) => {
                    let matches = job.revision == revision;
                    self.job = None;
                    if matches {
                        self.install(result);
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.job = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        #[cfg(target_arch = "wasm32")]
        if self.job.as_ref().is_some_and(|job| job.revision != revision) {
            self.job = None;
        }
    }
    fn install(&mut self, result: Vec<Prepared>) {
        #[cfg(target_arch = "wasm32")]
        if let Some((generation, revision)) = self.revision {
            crate::web::profile_mark("surfaces_ready", generation, revision);
        }
        for (owner, definition, faces, evaluated) in result {
            if evaluated {
                self.evaluated.insert(owner);
            }
            self.kept.insert(owner, (definition, faces));
        }
    }
}

#[cfg(any(test, target_arch = "wasm32"))]
struct Builder {
    mesh: rysmo_mesh::Mesh,
    after: Option<FaceId>,
    collecting: bool,
    adjacency: HashMap<VertexId, Vec<FaceId>>,
    blocks: BTreeMap<u64, Vec<FaceId>>,
    active: Option<(u64, Vec<FaceId>, usize, FaceChunk)>,
    chunks: Vec<Arc<FaceChunk>>,
    old: Option<Arc<DrawnFaces>>,
    affected: HashSet<VertexId>,
    dirty: HashSet<u64>,
}
#[cfg(any(test, target_arch = "wasm32"))]
impl Builder {
    fn new(mesh: rysmo_mesh::Mesh, old: Option<Arc<DrawnFaces>>) -> Self {
        Self {
            mesh,
            after: None,
            collecting: true,
            adjacency: HashMap::new(),
            blocks: BTreeMap::new(),
            active: None,
            chunks: Vec::new(),
            old,
            affected: HashSet::new(),
            dirty: HashSet::new(),
        }
    }
    fn step(&mut self) -> bool {
        if self.collecting {
            let mut count = 0;
            for (id, face) in self.mesh.faces_after(self.after).take(512) {
                self.blocks.entry(id.0 >> CHUNK_BITS).or_default().push(id);
                for vertex in face.loops().flatten() {
                    let faces = self.adjacency.entry(*vertex).or_default();
                    if faces.last() != Some(&id) {
                        faces.push(id);
                    }
                }
                self.after = Some(id);
                count += 1;
            }
            if count > 0 {
                return false;
            }
            self.collecting = false;
            if let Some(old) = &self.old {
                let changes = self.mesh.changed_storage_since(&old.mesh);
                self.dirty.extend(changes.faces.iter().map(|id| id.0 >> CHUNK_BITS));
                let moved: HashSet<_> = changes.vertices.into_iter().collect();
                self.affected.extend(&moved);
                for id in changes.faces {
                    for face in self.mesh.face(id).into_iter().chain(old.mesh.face(id)) {
                        self.affected.extend(face.loops().flatten().copied());
                    }
                }
                for id in changes.edges {
                    for edge in self.mesh.edge(id).into_iter().chain(old.mesh.edge(id)) {
                        self.affected.extend(edge.v);
                    }
                }
                for vertex in moved {
                    for id in self.adjacency.get(&vertex).into_iter().flatten() {
                        if let Some(face) = self.mesh.face(*id) {
                            self.affected.extend(face.loops().flatten().copied());
                        }
                    }
                }
            }
            return false;
        }
        if self.active.is_none() {
            let Some((block, ids)) = self.blocks.pop_first() else { return true };
            if let Some(known) = self.old.as_ref().and_then(|old| old.chunk(block))
                && !self.dirty.contains(&block)
                && !known.vertices.iter().any(|v| self.affected.contains(v))
            {
                self.chunks.push(known.clone());
                return false;
            }
            self.active = Some((
                block,
                ids,
                0,
                FaceChunk { block, faces: Vec::new(), vertices: HashSet::new(), smooth: Default::default() },
            ));
        }
        let (block, ids, offset, result) = self.active.as_mut().expect("active chunk");
        let end = (*offset + 128).min(ids.len());
        let batch = FaceChunk::build(&self.mesh, *block, &ids[*offset..end], |vertices| {
            self.mesh.smooth_normals_using(vertices, |vertex| self.adjacency.get(&vertex).cloned().unwrap_or_default())
        });
        result.faces.extend(batch.faces);
        result.vertices.extend(batch.vertices);
        result.smooth.extend(batch.smooth);
        *offset = end;
        if end == ids.len() {
            let (_, _, _, chunk) = self.active.take().expect("finished chunk");
            self.chunks.push(Arc::new(chunk));
        }
        false
    }
    fn finish(self) -> DrawnFaces {
        DrawnFaces { mesh: self.mesh, chunks: self.chunks }
    }
}
#[cfg(any(test, target_arch = "wasm32"))]
struct Work {
    doc: rysmo_model::Document,
    waiting: VecDeque<Owner>,
    old: HashMap<Owner, Arc<DrawnFaces>>,
    active: Option<(Owner, Builder)>,
    output: Vec<Prepared>,
}
#[cfg(any(test, target_arch = "wasm32"))]
impl Work {
    fn new(doc: rysmo_model::Document, owners: Vec<Owner>, old: HashMap<Owner, Arc<DrawnFaces>>) -> Self {
        Self { doc, waiting: owners.into(), old, active: None, output: Vec::new() }
    }
    fn step(&mut self) -> bool {
        if self.active.is_none() {
            let Some(owner) = self.waiting.pop_front() else { return true };
            let mesh = self.doc.effective_mesh(owner).into_owned();
            self.active = Some((owner, Builder::new(mesh, self.old.remove(&owner))));
            return false;
        }
        if !self.active.as_mut().expect("active owner").1.step() {
            return false;
        }
        let (owner, builder) = self.active.take().expect("finished owner");
        let definition = self.doc.def_arc(owner).expect("visible definition").clone();
        self.output.push((owner, definition.clone(), Arc::new(builder.finish()), has_openings(&definition)));
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn document() -> rysmo_model::Document {
        let mut editor = rysmo_model::Editor::new();
        editor
            .apply(rysmo_model::Command::DrawRectangle {
                corner: DVec3::ZERO,
                u: DVec3::X * 4.,
                v: DVec3::Y * 4.,
                facing: None,
            })
            .unwrap();
        let face = editor.mesh().faces().next().unwrap().0;
        editor.apply(rysmo_model::Command::PushPull { face, distance: 1., keep_base: false }).unwrap();
        let mut doc = editor.doc().clone();
        doc.with_mesh(None, |mesh| {
            let ids: Vec<_> = mesh.edges().map(|(id, _)| id).collect();
            mesh.set_edge_flags(&ids, None, Some(true), None);
        });
        doc
    }
    fn finish(mut work: Work) -> Vec<Prepared> {
        for _ in 0..10000 {
            if work.step() {
                return work.output;
            }
        }
        panic!("cooperative preparation did not finish");
    }
    fn equal(actual: &DrawnFaces, expected: &DrawnFaces) {
        assert_eq!(actual.mesh, expected.mesh);
        assert_eq!(actual.chunks.len(), expected.chunks.len());
        for (a, b) in actual.chunks.iter().zip(&expected.chunks) {
            assert_eq!(a.block, b.block);
            assert_eq!(a.vertices, b.vertices);
            assert_eq!(a.smooth, b.smooth);
            assert_eq!(a.faces.len(), b.faces.len());
            for (a, b) in a.faces.iter().zip(&b.faces) {
                assert_eq!(a.id, b.id);
                assert_eq!(a.normal, b.normal);
                assert_eq!(a.triangles, b.triangles);
            }
        }
    }
    #[test]
    fn cooperative_chunks_match_synchronous_smoothing_and_edits() {
        let mut doc = document();
        let expected = Arc::new(DrawnFaces::new(doc.root.mesh.clone(), None));
        let output = finish(Work::new(doc.clone(), vec![None], HashMap::new()));
        equal(&output[0].2, &expected);
        let edge = doc.root.mesh.edges().next().unwrap().0;
        doc.with_mesh(None, |mesh| mesh.move_elements(&[Element::Edge(edge)], DVec3::Z * 0.3).unwrap());
        let mut old = HashMap::new();
        old.insert(None, expected.clone());
        let output = finish(Work::new(doc.clone(), vec![None], old));
        equal(&output[0].2, &DrawnFaces::new(doc.root.mesh.clone(), Some(&expected)));
    }
    #[test]
    fn background_results_cannot_install_into_a_replaced_document() {
        let doc = document();
        let mut cache = DrawnCache::default();
        assert!(!cache.ready(&doc, (1, 0), BTreeSet::from([None])));
        let replacement = rysmo_model::Document::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !cache.ready(&replacement, (2, 0), BTreeSet::from([None])) {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(cache.get(&replacement, None).mesh.face_count(), 0);
        assert!(!cache.busy());
    }

    #[test]
    fn selected_geometry_waits_for_surface_preparation_before_highlighting() {
        let mut editor = rysmo_model::Editor::with_document(document());
        let face = editor.mesh().faces().next().unwrap().0;
        editor.set_selection([Element::Face(face)]);
        let mut app = App::new();
        app.insert_resource(App3d::new(editor))
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Assets<Image>>();
        plugin(&mut app);
        app.update();
        assert!(app.world().resource::<DrawnCache>().busy());
        assert!(app.world().resource::<Highlight>().entities.is_empty());
    }
    #[test]
    fn face_scan_resumes_across_id_gaps_without_missing_faces() {
        let doc = document();
        let all: Vec<_> = doc.root.mesh.faces().map(|(id, _)| id).collect();
        for (i, id) in all.iter().enumerate() {
            let resumed: Vec<_> = doc.root.mesh.faces_after(Some(*id)).map(|(id, _)| id).collect();
            assert_eq!(resumed, all[i + 1..]);
        }
    }
}

#[cfg(test)]
mod performance_tests {
    use super::*;
    #[test]
    #[ignore = "manual native dispatch and cooperative preparation measurement"]
    fn benchmark_surface_jobs() {
        use std::time::{Duration, Instant};
        let path = std::env::var("RYSMO_RECOVERY_BENCH_MODEL").expect("root mesh fixture");
        let doc = rysmo_io::load(&std::fs::read_to_string(path).unwrap()).unwrap();
        let mut cache = DrawnCache::default();
        let start = Instant::now();
        assert!(!cache.ready(&doc, (1, 0), BTreeSet::from([None])));
        let dispatch = start.elapsed().as_secs_f64() * 1000.;
        let mut poll_max: f64 = 0.;
        loop {
            let poll = Instant::now();
            let ready = cache.ready(&doc, (1, 0), BTreeSet::from([None]));
            poll_max = poll_max.max(poll.elapsed().as_secs_f64() * 1000.);
            if ready {
                break;
            }
            assert!(start.elapsed() < Duration::from_secs(60));
            std::thread::sleep(Duration::from_millis(1));
        }
        let native_completion = start.elapsed().as_secs_f64() * 1000.;
        let mut work = Work::new(doc.clone(), vec![None], HashMap::new());
        let mut slices = Vec::new();
        loop {
            let slice = Instant::now();
            let done = loop {
                if work.step() {
                    break true;
                }
                if slice.elapsed().as_secs_f64() >= 0.006 {
                    break false;
                }
            };
            slices.push(slice.elapsed().as_secs_f64() * 1000.);
            if done {
                break;
            }
        }
        let frames = slices.len();
        slices.sort_by(f64::total_cmp);
        assert_eq!(cache.get(&doc, None).mesh, work.output[0].2.mesh);
        println!(
            "surface-jobs: {}",
            serde_json::json!({"faces":doc.root.mesh.face_count(),"dispatch_ms":dispatch,
            "native_completion_ms":native_completion,"maximum_poll_ms":poll_max,"cooperative_slices":frames,
            "cooperative_slice_median_ms":slices[frames/2],"cooperative_slice_p95_ms":slices[(frames*95).div_ceil(100)-1],
            "cooperative_slice_max_ms":slices[frames-1]})
        );
    }
}
