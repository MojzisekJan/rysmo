//! Selection skins are prepared and uploaded in bounded pieces, including on desktop.
use super::*;
use std::collections::VecDeque;

type Key = (u64, u64, Vec<FaceId>, Vec<InstanceId>);

#[derive(Resource, Default)]
pub(crate) struct Highlight {
    key: Option<Key>,
    pub(super) entities: Vec<Entity>,
    material: Option<Handle<StandardMaterial>>,
    work: Option<Work>,
}

impl Highlight {
    pub(crate) fn busy(&self) -> bool {
        self.work.is_some()
    }
}

struct Piece {
    drawn: Arc<DrawnFaces>,
    chunk: Arc<FaceChunk>,
    transform: DMat4,
    cuts: Vec<SectionPlane>,
    whole: bool,
}

struct Work {
    pieces: VecDeque<Piece>,
    faces: HashSet<FaceId>,
    show_hidden: bool,
    face: usize,
    triangle: usize,
}

impl Work {
    /// One face visit or triangle; even a single large polygon can yield between triangles.
    fn step(&mut self, buffer: &mut Buffers) -> bool {
        let Some(piece) = self.pieces.front() else { return true };
        let Some(face) = piece.chunk.faces.get(self.face) else {
            self.pieces.pop_front();
            self.face = 0;
            self.triangle = 0;
            return self.pieces.is_empty();
        };
        if (!piece.whole && !self.faces.contains(&face.id))
            || piece.drawn.mesh.face(face.id).is_none_or(|f| f.hidden && !self.show_hidden)
            || self.triangle == face.triangles.len()
        {
            self.face += 1;
            self.triangle = 0;
            return false;
        }
        let points = face.triangles[self.triangle].map(|v| piece.transform.transform_point3(piece.drawn.mesh.pos(v)));
        self.triangle += 1;
        let colour = [SELECTED[0], SELECTED[1], SELECTED[2], 0.45];
        if piece.cuts.is_empty() {
            // Most selections have no section cuts: avoid one allocation per triangle.
            for point in points {
                buffer.push(point, DVec3::Z, colour, [0., 0.]);
            }
        } else {
            let polygon = rysmo_model::sections::clip_triangle(points, &piece.cuts);
            for i in 1..polygon.len().saturating_sub(1) {
                for point in [polygon[0].0, polygon[i].0, polygon[i + 1].0] {
                    buffer.push(point, DVec3::Z, colour, [0., 0.]);
                }
            }
        }
        false
    }
}

pub(super) fn rebuild(
    mut commands: Commands,
    app: Res<App3d>,
    mut state: ResMut<Highlight>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    edges: Res<EdgeCache>,
    mut drawn: ResMut<DrawnCache>,
) {
    let preview = app.tools.highlights(&app.editor);
    let key = (app.document_generation, app.editor.revision(), preview.faces, preview.instances);
    if state.key.as_ref() == Some(&key) && !state.busy() {
        return;
    }
    #[cfg(target_arch = "wasm32")]
    let _profile = crate::web::ProfileSpan::new("highlight_batch", app.document_generation, app.editor.revision());
    if state.key.as_ref() != Some(&key) {
        for entity in state.entities.drain(..) {
            commands.entity(entity).despawn();
        }
        state.work = None;
        state.key = None;
        let editor = &app.editor;
        let doc = editor.doc();
        drawn.prepare(doc, (app.document_generation, editor.content_revision()));
        if drawn.busy() {
            return;
        }
        let mut faces: HashSet<_> = key.2.iter().copied().collect();
        let mut instances: HashSet<_> = key.3.iter().copied().collect();
        for element in editor.selection() {
            match element {
                Element::Face(id) => {
                    faces.insert(*id);
                }
                Element::Instance(id) => {
                    instances.insert(*id);
                }
                _ => {}
            }
        }
        let mut pieces = VecDeque::new();
        let blocks: HashSet<_> = faces.iter().map(|id| id.0 >> CHUNK_BITS).collect();
        if doc.style.faces != FaceStyle::Wireframe && (!faces.is_empty() || !instances.is_empty()) {
            for placed in &edges.placed {
                let occ = &placed.occurrence;
                let whole = occ.top.is_some_and(|id| instances.contains(&id));
                if (!whole && !placed.active) || doc.def(occ.definition).is_none() {
                    continue;
                }
                let drawn = drawn.get(doc, occ.definition);
                for chunk in &drawn.chunks {
                    if !whole && !blocks.contains(&chunk.block) {
                        continue;
                    }
                    pieces.push_back(Piece {
                        drawn: drawn.clone(),
                        chunk: chunk.clone(),
                        transform: occ.transform,
                        cuts: placed.cuts.clone(),
                        whole,
                    });
                }
            }
        }
        if !pieces.is_empty() {
            state.work =
                Some(Work { pieces, faces, show_hidden: doc.style.show_hidden_geometry, face: 0, triangle: 0 });
        }
        state.key = Some(key);
    }
    let Some(work) = &mut state.work else { return };
    let start = bevy::platform::time::Instant::now();
    let mut buffer = Buffers::default();
    let mut steps = 0;
    loop {
        if work.step(&mut buffer) {
            state.work = None;
            break;
        }
        steps += 1;
        // Bound both CPU work and the size of an individual GPU upload.
        if buffer.pos.len() >= 12_288 || (steps % 64 == 0 && start.elapsed().as_secs_f64() >= 0.003) {
            break;
        }
    }
    if buffer.idx.is_empty() {
        return;
    }
    let material = state
        .material
        .get_or_insert_with(|| {
            materials.add(StandardMaterial {
                base_color: Color::WHITE,
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                cull_mode: None,
                depth_bias: 1.0,
                ..default()
            })
        })
        .clone();
    let transform = Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2));
    state
        .entities
        .push(commands.spawn((Mesh3d(meshes.add(buffer.into_mesh())), MeshMaterial3d(material), transform)).id());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rectangle_work(cuts: Vec<SectionPlane>, whole: bool) -> Work {
        let mut editor = rysmo_model::Editor::new();
        editor
            .apply(rysmo_model::Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None })
            .unwrap();
        let drawn = Arc::new(DrawnFaces::new(editor.mesh().clone(), None));
        let pieces = drawn
            .chunks
            .iter()
            .map(|chunk| Piece {
                drawn: drawn.clone(),
                chunk: chunk.clone(),
                transform: DMat4::from_translation(DVec3::new(2., 0., 0.)),
                cuts: cuts.clone(),
                whole,
            })
            .collect();
        Work { pieces, faces: HashSet::new(), show_hidden: false, face: 0, triangle: 0 }
    }

    #[test]
    fn incremental_skin_preserves_transforms_sections_and_selection() {
        for clipped in [false, true] {
            let cuts = if clipped {
                vec![SectionPlane { name: "cut".into(), origin: DVec3::new(2.5, 0., 0.), normal: DVec3::X }]
            } else {
                vec![]
            };
            let mut work = rectangle_work(cuts, true);
            let mut buffer = Buffers::default();
            assert!(!work.step(&mut buffer));
            assert!(!buffer.pos.is_empty(), "first triangle is available before the whole skin");
            while !work.step(&mut buffer) {}
            let area: f32 = buffer
                .pos
                .as_chunks::<3>()
                .0
                .iter()
                .map(|t| {
                    (Vec3::from(t[1]) - Vec3::from(t[0])).cross(Vec3::from(t[2]) - Vec3::from(t[0])).length() * 0.5
                })
                .sum();
            assert!((area - if clipped { 0.5 } else { 1. }).abs() < 1e-6);
            assert!(buffer.pos.iter().all(|p| p[0] >= 2. && p[0] <= if clipped { 2.5 } else { 3. }));
        }
        let mut work = rectangle_work(vec![], false);
        let mut buffer = Buffers::default();
        while !work.step(&mut buffer) {}
        assert!(buffer.pos.is_empty(), "unselected faces must not leak into the skin");
    }

    #[test]
    fn document_switch_cancels_partial_skin_and_removes_uploaded_pieces() {
        let mut app = App::new();
        let old_entity = app.world_mut().spawn_empty().id();
        app.insert_resource(App3d::new(rysmo_model::Editor::new()))
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<EdgeCache>()
            .init_resource::<DrawnCache>()
            .insert_resource(Highlight {
                key: Some((u64::MAX, 0, vec![], vec![])),
                entities: vec![old_entity],
                work: Some(rectangle_work(vec![], true)),
                ..default()
            })
            .add_systems(Update, rebuild);
        app.update();
        assert!(!app.world().resource::<Highlight>().busy());
        assert!(app.world().resource::<Highlight>().entities.is_empty());
        assert!(app.world().get_entity(old_entity).is_err());
    }

    #[test]
    fn unfinished_highlight_keeps_renderer_awake_until_completion() {
        use bevy::winit::{UpdateMode, WinitSettings};
        let mut app = App::new();
        app.insert_resource(App3d::new(rysmo_model::Editor::new()))
            .insert_resource(Highlight { work: Some(rectangle_work(vec![], true)), ..default() });
        crate::idle::plugin(&mut app);
        for _ in 0..200 {
            app.update();
        }
        assert_eq!(app.world().resource::<WinitSettings>().focused_mode, UpdateMode::Continuous);
        app.world_mut().resource_mut::<Highlight>().work = None;
        app.update();
        assert_ne!(app.world().resource::<WinitSettings>().focused_mode, UpdateMode::Continuous);
    }
}
