//! The faces on screen: meshes built from the occurrence list, one per material group
//! (untextured, each texture image, see-through), styled by the model's face style; and
//! the edge list that the gizmos draw from.
//!
//! In the flat lighting the faces are unlit and shaded here from a fixed light, which is
//! the default look and the same on every GPU. In sun lighting the materials are
//! lit by the sun and the sky, and can receive shadows and ambient occlusion.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::image::{
    CompressedImageFormats, ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor, ImageType,
};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use rysmo_math::DVec3;
use rysmo_mesh::{DefinitionId, EdgeId, Element, FaceId, ImageId, InstanceId, triangulate_face};
use rysmo_model::{FaceStyle, Lighting};

use crate::state::App3d;

pub const FRONT: [f32; 3] = [0.96, 0.95, 0.92];
pub const BACK: [f32; 3] = [0.64, 0.70, 0.80];
pub const SELECTED: [f32; 3] = [0.35, 0.55, 1.0];

pub fn plugin(app: &mut App) {
    app.init_resource::<FaceMeshes>()
        .init_resource::<EdgeCache>()
        .add_systems(PostUpdate, (rebuild_faces, update_edge_cache).before(TransformSystems::Propagate));
}

/// Which mesh a face's triangles go into.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Group {
    Plain,
    Textured(ImageId),
    /// Blended faces, with their texture if they have one.
    SeeThrough(Option<ImageId>),
}

#[derive(Resource, Default)]
struct FaceMeshes {
    key: Option<(u64, u64, Vec<FaceId>, Vec<InstanceId>)>,
    texture_settings: Option<(u32, u16)>,
    entities: Vec<Entity>,
    /// Decoded images, by id, with the bytes they came from.
    images: HashMap<ImageId, (Arc<Vec<u8>>, Handle<Image>)>,
}

/// An edge as the gizmos draw it: ends, the normals of its faces, a colour for
/// colour-by-material.
pub struct EdgeInfo {
    pub ghost: bool,
    pub id: EdgeId,
    pub a: DVec3,
    pub b: DVec3,
    pub normals: Vec<DVec3>,
    pub material: [f32; 3],
}

/// Visible edges of every definition, rebuilt when the document changes, so profiles
/// can be found every frame without searching for faces.
#[derive(Resource, Default)]
pub struct EdgeCache {
    revision: Option<u64>,
    pub defs: HashMap<Option<DefinitionId>, Vec<EdgeInfo>>,
}

pub fn update_edge_cache(app: Res<App3d>, mut cache: ResMut<EdgeCache>) {
    let revision = app.editor.revision();
    if cache.revision == Some(revision) {
        return;
    }
    let doc = app.editor.doc();
    cache.defs.clear();
    let defs = std::iter::once((None, &doc.root)).chain(doc.definitions.iter().map(|(id, d)| (Some(*id), d)));
    for (id, _def) in defs {
        let evaluated = doc.effective_mesh(id);
        let mesh = &*evaluated;
        let mut faces_of: HashMap<(rysmo_mesh::VertexId, rysmo_mesh::VertexId), Vec<FaceId>> = HashMap::new();
        for (fid, f) in mesh.faces() {
            for (a, b) in f.loop_edges() {
                faces_of.entry(if a < b { (a, b) } else { (b, a) }).or_default().push(fid);
            }
        }
        let list = mesh
            .edges()
            .filter(|(_, e)| !e.procedural && (e.visible() || doc.style.show_hidden_geometry))
            .map(|(eid, e)| {
                let k = if e.v[0] < e.v[1] { (e.v[0], e.v[1]) } else { (e.v[1], e.v[0]) };
                let fs = faces_of.get(&k).cloned().unwrap_or_default();
                let material = e
                    .material
                    .and_then(|id| doc.materials.get(&id))
                    .map(|m| [m.color[0], m.color[1], m.color[2]])
                    .unwrap_or([0., 0., 0.]);
                let (a, b) = mesh.edge_points(eid);
                EdgeInfo {
                    ghost: !e.visible(),
                    id: eid,
                    a,
                    b,
                    normals: fs.iter().filter_map(|f| mesh.face_normal(*f)).collect(),
                    material,
                }
            })
            .collect();
        cache.defs.insert(id, list);
    }
    cache.revision = Some(revision);
}

/// Lambert shading from a fixed light, kept bright so colours stay true.
fn shade(color: [f32; 4], normal: DVec3) -> [f32; 4] {
    let sun = DVec3::new(-0.35, -0.55, 0.76).normalize();
    let k = (0.62 + 0.38 * normal.dot(sun).max(0.0)) as f32;
    [color[0] * k, color[1] * k, color[2] * k, color[3]]
}

fn tint(c: [f32; 4]) -> [f32; 4] {
    [
        c[0] * 0.55 + SELECTED[0] * 0.45,
        c[1] * 0.55 + SELECTED[1] * 0.45,
        c[2] * 0.55 + SELECTED[2] * 0.45,
        c[3].max(0.6),
    ]
}

/// Geometry outside the context being edited is drawn faded.
fn fade(c: [f32; 4]) -> [f32; 4] {
    [c[0] * 0.35 + 0.6, c[1] * 0.35 + 0.62, c[2] * 0.35 + 0.65, c[3]]
}

#[derive(Default)]
struct Buffers {
    pos: Vec<[f32; 3]>,
    nrm: Vec<[f32; 3]>,
    col: Vec<[f32; 4]>,
    uv: Vec<[f32; 2]>,
    idx: Vec<u32>,
}

impl Buffers {
    fn push(&mut self, p: DVec3, n: DVec3, c: [f32; 4], uv: [f64; 2]) {
        self.idx.push(self.pos.len() as u32);
        self.pos.push([p.x as f32, p.y as f32, p.z as f32]);
        self.nrm.push([n.x as f32, n.y as f32, n.z as f32]);
        self.col.push(c);
        self.uv.push([uv[0] as f32, uv[1] as f32]);
    }

    fn into_mesh(self) -> Mesh {
        // Picking and edits use the model mesh. These disposable draw buffers
        // can leave CPU memory once uploaded, and are rebuilt on model changes.
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.nrm)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.col)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uv)
            .with_inserted_indices(Indices::U32(self.idx))
    }
}

fn decode(bytes: &[u8], mime: &str, limit: u32, anisotropy: u16) -> Option<Image> {
    let decoded = Image::from_buffer(
        bytes,
        ImageType::MimeType(mime),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::default(),
        RenderAssetUsages::default(),
    )
    .ok()?;
    let rgba = decoded.try_into_dynamic().ok()?.to_rgba8();
    let cap = if cfg!(target_arch = "wasm32") { 4096 } else { 8192 };
    let limit = if limit == 0 { cap } else { limit.min(cap) };
    let dynamic = image::DynamicImage::ImageRgba8(rgba);
    let mut level = if dynamic.width().max(dynamic.height()) > limit {
        dynamic.resize(limit, limit, image::imageops::FilterType::Triangle).to_rgba8()
    } else {
        dynamic.to_rgba8()
    };
    let (width, height) = level.dimensions();
    let mut data = Vec::new();
    let mut levels = 0;
    loop {
        data.extend_from_slice(level.as_raw());
        levels += 1;
        if level.width() == 1 && level.height() == 1 {
            break;
        }
        level = image::imageops::resize(
            &level,
            (level.width() / 2).max(1),
            (level.height() / 2).max(1),
            image::imageops::FilterType::Triangle,
        );
    }
    let mut image = Image::new_uninit(
        bevy::render::render_resource::Extent3d { width, height, depth_or_array_layers: 1 },
        bevy::render::render_resource::TextureDimension::D2,
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.mip_level_count = levels;
    image.data = Some(data);
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        min_filter: ImageFilterMode::Linear,
        mag_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: anisotropy,
        ..ImageSamplerDescriptor::linear()
    });
    Some(image)
}

#[allow(clippy::too_many_arguments)]
fn rebuild_faces(
    mut commands: Commands,
    app: Res<App3d>,
    mut state: ResMut<FaceMeshes>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let preview = app.tools.preview(&app.editor);
    let key = (app.document_generation, app.editor.revision(), preview.faces.clone(), preview.instances.clone());
    if state.key.as_ref() == Some(&key) {
        return;
    }
    for e in state.entities.drain(..) {
        commands.entity(e).despawn();
    }
    let editor = &app.editor;
    let doc = editor.doc();
    let style = doc.style;
    let lit = style.lighting != Lighting::Flat;
    if state.texture_settings != Some((style.texture_limit, style.texture_anisotropy)) {
        state.images.clear();
        state.texture_settings = Some((style.texture_limit, style.texture_anisotropy));
    }
    // Images: decode new ones, drop those gone.
    state.images.retain(|id, (bytes, _)| doc.images.get(id).is_some_and(|i| Arc::ptr_eq(&i.bytes, bytes)));
    for (id, img) in &doc.images {
        if !state.images.contains_key(id)
            && let Some(decoded) = decode(&img.bytes, &img.mime, style.texture_limit, style.texture_anisotropy)
        {
            let handle = images.add(decoded);
            state.images.insert(*id, (img.bytes.clone(), handle));
        }
    }
    let highlighted: BTreeSet<FaceId> = preview.faces.iter().copied().collect();
    let lit_instances: BTreeSet<InstanceId> = preview.instances.iter().copied().collect();
    let mut groups: HashMap<Group, Buffers> = HashMap::new();
    let background = [style.background[0], style.background[1], style.background[2], 1.0];
    let colour = |m: Option<rysmo_mesh::MaterialId>, default: [f32; 3]| -> ([f32; 4], Option<rysmo_model::Texture>) {
        let rgba = [default[0], default[1], default[2], 1.0];
        match style.faces {
            FaceStyle::Monochrome | FaceStyle::Wireframe => (rgba, None),
            FaceStyle::HiddenLine => (background, None),
            FaceStyle::XRay => (rgba, None),
            FaceStyle::Shaded => (m.and_then(|m| doc.materials.get(&m)).map(|m| m.color).unwrap_or(rgba), None),
            FaceStyle::Textured => match m.and_then(|m| doc.materials.get(&m)) {
                Some(mat) => match mat.texture.filter(|t| state.images.contains_key(&t.image)) {
                    // The texture carries the colour; the vertex colour only shades it.
                    Some(t) => ([1.0, 1.0, 1.0, mat.color[3]], Some(t)),
                    None => (mat.color, None),
                },
                None => (rgba, None),
            },
        }
    };
    if style.faces != FaceStyle::Wireframe {
        let mut active_seen = false;
        for occ in doc.occurrences(editor.context()) {
            let Some(_def) = doc.def(occ.definition) else { continue };
            let evaluated = doc.effective_mesh(occ.definition);
            let mesh = &*evaluated;
            let is_active = occ.in_context && !active_seen;
            if occ.in_context {
                active_seen = true;
            }
            let t = occ.transform;
            let section_planes = doc.section_planes(&occ.path);
            let nt = t.inverse().transpose();
            let mirrored = t.determinant() < 0.0;
            let selected_instance =
                occ.top.is_some_and(|i| editor.is_selected(Element::Instance(i)) || lit_instances.contains(&i));
            let smooth = mesh.smooth_normals();
            for (fid, face) in mesh.faces() {
                if face.hidden && !style.show_hidden_geometry {
                    continue;
                }
                let Some(n) = mesh.face_normal(fid) else { continue };
                let selected = selected_instance
                    || (is_active && (editor.is_selected(Element::Face(fid)) || highlighted.contains(&fid)));
                let tris = triangulate_face(mesh, fid);
                let placement = face.uv.unwrap_or_default();
                for (side, material, default) in
                    [(1.0f64, face.material.or(occ.material), FRONT), (-1.0, face.back_material.or(occ.material), BACK)]
                {
                    let (mut color, texture) = colour(material, default);
                    if face.hidden {
                        color[3] = 0.12;
                    }
                    if style.faces == FaceStyle::XRay {
                        color[3] = style.xray_opacity;
                    }
                    if selected {
                        color = tint(color);
                    }
                    if !occ.in_context {
                        color = fade(color);
                    }
                    let group = if color[3] < 0.999 {
                        Group::SeeThrough(texture.map(|t| t.image))
                    } else if let Some(t) = texture {
                        Group::Textured(t.image)
                    } else {
                        Group::Plain
                    };
                    let buf = groups.entry(group).or_default();
                    let inverse = t.inverse();
                    let perspective = placement
                        .pins
                        .and_then(rysmo_model::pin_mapping)
                        .is_some_and(|m| m.x_axis.z.abs() + m.y_axis.z.abs() > 1e-12);
                    let detail = if texture.is_some() && perspective { 8 } else { 1 };
                    let sample_weights = subdivided_weights(detail);
                    for tri in &tris {
                        let mut order = if side > 0.0 { [tri[0], tri[1], tri[2]] } else { [tri[0], tri[2], tri[1]] };
                        if mirrored {
                            order.swap(1, 2);
                        }
                        let points = order.map(|v| t.transform_point3(mesh.pos(v)));
                        let normals = order.map(|v| {
                            nt.transform_vector3(smooth.get(&(fid, v)).copied().unwrap_or(n) * side)
                                .normalize_or(DVec3::Z)
                        });
                        let uvs = order.map(|v| {
                            texture
                                .map(|tx| rysmo_model::uv_at(mesh.pos(v), n, tx.size, &placement))
                                .unwrap_or([0., 0.])
                        });
                        let polygon = rysmo_model::sections::clip_triangle(points, &section_planes);
                        for i in 1..polygon.len().saturating_sub(1) {
                            let triangle = [polygon[0], polygon[i], polygon[i + 1]];
                            for samples in &sample_weights {
                                for sample in samples {
                                    let point =
                                        triangle[0].0 * sample.x + triangle[1].0 * sample.y + triangle[2].0 * sample.z;
                                    let weights: [f64; 3] = std::array::from_fn(|j| {
                                        triangle[0].1[j] * sample.x
                                            + triangle[1].1[j] * sample.y
                                            + triangle[2].1[j] * sample.z
                                    });
                                    let world_n =
                                        (normals[0] * weights[0] + normals[1] * weights[1] + normals[2] * weights[2])
                                            .normalize_or(DVec3::Z);
                                    let shaded = if lit || style.faces == FaceStyle::HiddenLine {
                                        color
                                    } else {
                                        shade(color, world_n)
                                    };
                                    let uv = std::array::from_fn(|j| {
                                        uvs[0][j] * weights[0] + uvs[1][j] * weights[1] + uvs[2][j] * weights[2]
                                    });
                                    let uv = if let Some(tx) = texture.filter(|_| placement.pins.is_some()) {
                                        rysmo_model::uv_at(inverse.transform_point3(point), n, tx.size, &placement)
                                    } else {
                                        uv
                                    };
                                    buf.push(point, world_n, shaded, uv);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    // The model is Z up; Bevy is Y up. One rotation on each mesh maps one to the other.
    let to_y_up = Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2));
    let unlit = !lit || style.faces == FaceStyle::HiddenLine;
    for (group, buf) in groups {
        if buf.idx.is_empty() {
            continue;
        }
        let texture = match group {
            Group::Textured(id) | Group::SeeThrough(Some(id)) => state.images.get(&id).map(|(_, h)| h.clone()),
            _ => None,
        };
        let material = materials.add(StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: texture,
            unlit,
            perceptual_roughness: 0.85,
            reflectance: 0.2,
            alpha_mode: if matches!(group, Group::SeeThrough(_)) { AlphaMode::Blend } else { AlphaMode::Opaque },
            // Each side is its own triangles, wound to face its way; culling shows one.
            ..default()
        });
        let e = commands.spawn((Mesh3d(meshes.add(buf.into_mesh())), MeshMaterial3d(material), to_y_up)).id();
        state.entities.push(e);
    }
    state.key = Some(key);
}

#[cfg(test)]
mod edge_material_tests {
    use super::*;
    #[test]
    fn edge_cache_uses_explicit_edge_color_without_recoloring_adjacent_faces() {
        use rysmo_model::{Command, Editor, Element};
        let mut editor = Editor::new();
        editor.apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None }).unwrap();
        let face = editor.mesh().faces().next().unwrap().0;
        let edge = editor.mesh().face_edges(face)[0];
        let red = editor
            .apply(Command::AddMaterial { name: "Red face".into(), color: [1., 0., 0., 1.], texture: None })
            .unwrap()
            .material;
        let blue = editor
            .apply(Command::AddMaterial { name: "Blue edge".into(), color: [0., 0., 1., 1.], texture: None })
            .unwrap()
            .material;
        editor.apply(Command::Paint { faces: vec![face], material: red, back: false }).unwrap();
        let mut app = App::new();
        app.insert_resource(App3d::new(editor)).init_resource::<EdgeCache>().add_systems(Update, update_edge_cache);
        app.update();
        assert!(app.world().resource::<EdgeCache>().defs[&None].iter().all(|e| e.material == [0., 0., 0.]));
        app.world_mut()
            .resource_mut::<App3d>()
            .editor
            .apply(Command::PaintElements { elements: vec![Element::Edge(edge)], material: blue, back: false })
            .unwrap();
        app.update();
        let cache = app.world().resource::<EdgeCache>();
        assert_eq!(cache.defs[&None].iter().find(|e| e.id == edge).unwrap().material, [0., 0., 1.]);
        assert_eq!(app.world().resource::<App3d>().editor.mesh().face(face).unwrap().material, red);
    }
}

/// Barycentric samples for a regular triangle grid. Used only for warped textures.
fn subdivided_weights(count: usize) -> Vec<[DVec3; 3]> {
    let at = |i: usize, j: usize| {
        DVec3::new(1. - (i + j) as f64 / count as f64, i as f64 / count as f64, j as f64 / count as f64)
    };
    let mut out = Vec::with_capacity(count * count);
    for i in 0..count {
        for j in 0..count - i {
            out.push([at(i, j), at(i + 1, j), at(i, j + 1)]);
            if i + j + 1 < count {
                out.push([at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)]);
            }
        }
    }
    out
}

#[cfg(test)]
mod subdivision_tests {
    use super::*;
    #[test]
    fn warped_texture_subdivision_covers_triangle_without_reversing_normals() {
        for count in [1, 8] {
            let tris = subdivided_weights(count);
            assert_eq!(tris.len(), count * count);
            let mut area = 0.;
            for tri in tris {
                for p in tri {
                    assert!((p.element_sum() - 1.).abs() < 1e-12);
                    assert!(p.min_element() >= -1e-12);
                }
                let cross = (tri[1] - tri[0]).cross(tri[2] - tri[0]);
                assert!(cross.min_element() > 0.);
                area += cross.length() / 2.;
            }
            assert!((area - 3_f64.sqrt() / 2.).abs() < 1e-12);
        }
    }
}

#[cfg(test)]
mod document_cache_tests {
    use super::*;
    use rysmo_model::{Command, Editor};
    #[test]
    fn opening_another_model_refreshes_edge_geometry_and_hidden_view_exposes_soft_edges() {
        let mut first = Editor::new();
        first.apply(Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::X]], facing: None }).unwrap();
        let mut second = Editor::new();
        second
            .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X * 2., v: DVec3::Y * 2., facing: None })
            .unwrap();
        assert_eq!(first.revision(), second.revision());
        let mut app = App::new();
        app.insert_resource(App3d::new(first)).init_resource::<EdgeCache>().add_systems(Update, update_edge_cache);
        app.update();
        assert_eq!(app.world().resource::<EdgeCache>().defs[&None].len(), 1);
        let text = rysmo_io::save(second.doc());
        crate::files::open_text(&mut app.world_mut().resource_mut::<App3d>(), &text, "second.rysmo").unwrap();
        app.update();
        assert_eq!(app.world().resource::<EdgeCache>().defs[&None].len(), 4);
        {
            let mut state = app.world_mut().resource_mut::<App3d>();
            let edges = state.editor.mesh().edges().map(|(id, _)| id).collect();
            state.editor.apply(Command::SetEdges { edges, soft: Some(true), smooth: None, hidden: None }).unwrap();
        }
        app.update();
        assert!(app.world().resource::<EdgeCache>().defs[&None].is_empty());
        {
            let mut state = app.world_mut().resource_mut::<App3d>();
            let mut style = state.editor.doc().style;
            style.show_hidden_geometry = true;
            state.editor.apply(Command::SetStyle { style }).unwrap();
        }
        app.update();
        assert_eq!(app.world().resource::<EdgeCache>().defs[&None].len(), 4);
        assert!(app.world().resource::<EdgeCache>().defs[&None].iter().all(|edge| edge.ghost));
    }
}

#[cfg(test)]
mod texture_quality_tests {
    use super::*;
    #[test]
    fn display_texture_limit_preserves_aspect_and_builds_complete_mips() {
        let source = image::RgbaImage::from_pixel(1024, 512, image::Rgba([200, 100, 50, 128]));
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(source).write_to(&mut png, image::ImageFormat::Png).unwrap();
        let bytes = png.into_inner();
        let original = bytes.clone();
        let image = decode(&bytes, "image/png", 512, 8).unwrap();
        assert_eq!(image.texture_descriptor.size.width, 512);
        assert_eq!(image.texture_descriptor.size.height, 256);
        assert_eq!(image.texture_descriptor.mip_level_count, 10);
        assert_eq!(&image.data.as_ref().unwrap()[..4], &[200, 100, 50, 128]);
        assert_eq!(
            image.data.as_ref().unwrap().len(),
            (0..10).map(|i| 4 * (512usize >> i).max(1) * (256usize >> i).max(1)).sum::<usize>()
        );
        let ImageSampler::Descriptor(sampler) = image.sampler else { panic!() };
        assert_eq!(sampler.anisotropy_clamp, 8);
        assert_eq!(bytes, original);
        let full = decode(&bytes, "image/png", 0, 1).unwrap();
        assert_eq!(full.texture_descriptor.size.width, 1024);
    }
}
