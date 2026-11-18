use rysmo_math::DVec3;
use rysmo_mesh::{Curve, CurveId, CurveKind, Edge, EdgeId, Face, FaceId, MaterialId, Mesh, Vertex, VertexId};
use rysmo_model::Document;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn read(
    doc: &mut Document,
    hidden_layers: &BTreeSet<u16>,
    run: &skpwright::GeometryRun,
    materials: &BTreeMap<u16, MaterialId>,
    scale: f64,
    warnings: &mut BTreeSet<String>,
) -> Result<Mesh, String> {
    let mut vertices = BTreeMap::new();
    let mut ids = Vec::with_capacity(run.mesh.vertices.len());
    for &p in &run.mesh.vertices {
        let pos = DVec3::from_array(p) * scale;
        if !pos.is_finite() || pos.abs().max_element() > 1e9 {
            return Err("SKP geometry contains non-finite or excessive coordinates.".into());
        }
        let id = VertexId(doc.reserve_id());
        vertices.insert(id, Vertex { pos });
        ids.push(id);
    }
    let vertex = |index: u32| ids.get(index as usize).copied().ok_or("SKP references a missing vertex.");
    let hidden = |layer: u16, own: bool| own || hidden_layers.contains(&layer);
    let material = |slot: Option<u16>| slot.and_then(|slot| materials.get(&slot).copied());
    let mut edges = BTreeMap::new();
    let mut pairs = BTreeMap::new();
    let mut members = BTreeMap::<u32, Vec<EdgeId>>::new();
    for e in &run.mesh.edges {
        let a = vertex(e.v0)?;
        let b = vertex(e.v1)?;
        let pair = (a.min(b), a.max(b));
        if a == b {
            return Err("SKP contains a degenerate edge.".into());
        }
        if pairs.contains_key(&pair) {
            return Err("SKP contains duplicate mesh edges.".into());
        }
        let id = EdgeId(doc.reserve_id());
        edges.insert(
            id,
            Edge {
                soft: e.soft,
                smooth: e.smooth,
                hidden: hidden(e.layer, e.hidden),
                material: material(e.material),
                ..Edge::new(a, b)
            },
        );
        pairs.insert(pair, id);
        if let Some(curve) = e.curve {
            members.entry(curve).or_default().push(id);
        }
        if e.layer != 0 {
            warnings.insert("Edge and face tags are baked into visibility; only instance tags remain editable.".into());
        }
    }
    let mut faces = BTreeMap::new();
    for face in &run.mesh.faces {
        let outer: Vec<_> = face.outer.iter().map(|&i| vertex(i)).collect::<Result<_, _>>()?;
        let holes: Vec<Vec<_>> =
            face.holes.iter().map(|ring| ring.iter().rev().map(|&i| vertex(i)).collect()).collect::<Result<_, _>>()?;
        if outer.len() < 3 || outer.len() > 16_384 || holes.iter().any(|ring| ring.len() < 3 || ring.len() > 16_384) {
            return Err("SKP contains an invalid face loop.".into());
        }
        let front = material(face.front_material);
        let back = material(face.back_material);
        let normal = DVec3::from_array(face.normal);
        if !normal.is_finite() || (normal.length() - 1.).abs() > 1e-5 {
            return Err("SKP contains an invalid face normal.".into());
        }
        let side = if front.and_then(|id| doc.materials[&id].texture).is_some() {
            skpwright::Side::Front
        } else {
            skpwright::Side::Back
        };
        let texture_id = match side {
            skpwright::Side::Front => front,
            skpwright::Side::Back => back,
        };
        let uv = if let Some(texture) = texture_id.and_then(|id| doc.materials[&id].texture) {
            let size = texture.size;
            let placement = super::mapping::placement(
                face,
                vertices[&outer[0]].pos,
                size,
                (size[0] * skpwright::INCH / scale, size[1] * skpwright::INCH / scale),
                scale,
                side,
            );
            if placement.is_none() {
                warnings
                    .insert("A singular or folded face texture mapping was replaced by the default placement.".into());
            }
            if let Some(source_texture) = &face.texture
                && source_texture.front != source_texture.back
                && front.and_then(|id| doc.materials[&id].texture).is_some()
                && back.and_then(|id| doc.materials[&id].texture).is_some()
            {
                // TODO: Represent independent front/back UVs in the native model
                // before claiming lossless conversion of two-sided textures.
                warnings.insert("Independent front/back texture placements use the front placement in Rysmo.".into());
            }
            placement
        } else {
            None
        };
        let id = FaceId(doc.reserve_id());
        let imported =
            Face { outer, holes, material: front, back_material: back, uv, hidden: hidden(face.layer, face.hidden) };
        for (a, b) in imported.loop_edges() {
            let pair = (a.min(b), a.max(b));
            pairs.entry(pair).or_insert_with(|| {
                let edge = EdgeId(doc.reserve_id());
                edges.insert(edge, Edge::new(a, b));
                edge
            });
        }
        faces.insert(id, imported);
        if face.layer != 0 {
            warnings.insert("Edge and face tags are baked into visibility; only instance tags remain editable.".into());
        }
    }
    let mut curves = BTreeMap::new();
    for (index, edges_in_curve) in members {
        let source = run.curves.get(index as usize).ok_or("SKP edge references a missing curve.")?;
        let segments = u32::try_from(edges_in_curve.len()).map_err(|_| "Too many curve edges.")?;
        let mut curve = Curve {
            kind: CurveKind::Polyline,
            faceted: false,
            centre: DVec3::ZERO,
            normal: DVec3::Z,
            start: DVec3::X,
            radius: 0.,
            segments,
            sweep: 0.,
        };
        if let Some(arc) = &source.arc {
            let sweep = (arc.end_rad - arc.start_rad).to_degrees();
            let start = DVec3::from_array(arc.x_axis_m) * arc.start_rad.cos()
                + DVec3::from_array(arc.y_axis_m) * arc.start_rad.sin();
            curve = Curve {
                kind: if (sweep.abs() - 360.).abs() < 1e-5 { CurveKind::Circle } else { CurveKind::Arc },
                centre: DVec3::from_array(arc.center_m) * scale,
                normal: DVec3::from_array(arc.normal),
                start: start.normalize_or_zero(),
                radius: arc.radius_m * scale,
                sweep,
                ..curve
            };
        }
        curve.validate().map_err(str::to_owned)?;
        let id = CurveId(doc.reserve_id());
        for edge in edges_in_curve {
            edges.get_mut(&edge).unwrap().curve = Some(id);
        }
        curves.insert(id, curve);
    }
    let mesh = Mesh::from_parts_with_curves(vertices, edges, faces, curves)
        .map_err(|e| format!("Invalid SKP geometry: {e}"))?;
    Ok(mesh)
}
