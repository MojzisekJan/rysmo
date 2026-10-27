//! Command dispatch and shared geometry/glue finalization.

use super::drawing::{arc_points, centre_arc_points};
use super::groups::{explode, make_definition, make_unique, purge_unused};
use super::transforms::{copy_items, rotation, split, transform_items, validate_rotation};
use super::types::{Command, CommandError, MAX_GLOW, Outcome, invalid};
use super::{annotations, settings};
use crate::{Definition, Document, Guide, Image, Instance, Material, Tag, Texture};
use rysmo_math::{DMat4, DVec3, EPS, Plane};
use rysmo_mesh::{
    DefinitionId, EdgeId, Element, FaceId, GuideId, ImageId, InstanceId, MaterialId, Mesh, MeshError, TagId,
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

impl Command {
    /// Runs the command with `def` (the context being edited; `None` is the model).
    pub(crate) fn run(self, doc: &mut Document, def: Option<DefinitionId>) -> Result<Outcome, CommandError> {
        let glued_before = crate::components::capture_glue(doc);
        if doc.def(def).is_none() {
            return invalid("the context being edited no longer exists");
        }
        let text_transform = match &self {
            Command::Move { elements, .. } | Command::Copy { elements, .. } => {
                elements.iter().any(|e| matches!(e, Element::Text(_)))
            }
            _ => false,
        };
        if text_transform {
            return invalid("transforming selected text is not implemented yet");
        }
        let mut out = Outcome { changed: true, ..Default::default() };
        let faces_before: BTreeSet<FaceId> = doc.def(def).expect("checked").mesh.faces().map(|(f, _)| f).collect();
        let new_faces = |doc: &Document| -> Vec<Element> {
            doc.def(def)
                .expect("checked")
                .mesh
                .faces()
                .map(|(f, _)| f)
                .filter(|f| !faces_before.contains(f))
                .map(Element::Face)
                .collect()
        };
        let edges = |v: Vec<EdgeId>| v.into_iter().map(Element::Edge).collect::<Vec<_>>();
        let mesh_op = |doc: &mut Document, f: &mut dyn FnMut(&mut Mesh) -> Result<Vec<Element>, MeshError>| {
            doc.with_mesh(def, |m| f(m)).expect("checked").map_err(CommandError::from)
        };
        let polygon = matches!(&self, Command::DrawPolygon { .. });
        let scale_basis = if let Command::ScaleInAxes { x, y, .. } = &self {
            rysmo_math::DrawingAxes::new(DVec3::ZERO, *x, *y)
                .ok_or_else(|| CommandError::Invalid("invalid scale axes".into()))?
                .basis()
        } else {
            DMat4::IDENTITY
        };
        match self {
            Command::SetParametric { definition, spec } => {
                let previous = doc
                    .definitions
                    .get(&definition)
                    .ok_or_else(|| CommandError::Invalid("Missing parametric component".into()))?;
                if previous.parametric.is_none()
                    || !previous.instances.is_empty()
                    || !previous.dimensions.is_empty()
                    || !previous.texts.is_empty()
                    || !previous.guides.is_empty()
                    || !previous.sections.planes.is_empty()
                {
                    return invalid("Component contains manual additions; make a separate copy before regeneration");
                }
                if !previous
                    .parametric
                    .as_ref()
                    .unwrap()
                    .matches_geometry(&previous.mesh)
                    .map_err(CommandError::Invalid)?
                {
                    return invalid("Component geometry was edited manually; regeneration would replace those edits");
                }
                if previous.parametric.as_ref() == Some(&spec) {
                    out.changed = false;
                } else {
                    let mesh = spec.mesh().map_err(CommandError::Invalid)?.with_fresh_ids(|| doc.reserve_id())?;
                    let target = doc.def_mut(Some(definition)).unwrap();
                    target.mesh = mesh;
                    target.parametric = Some(spec);
                }
            }
            Command::SolidBoolean { left, right, operation, keep_inputs } => {
                out.created = crate::solids::run(doc, def, left, right, operation, keep_inputs)?;
                out.changed = !keep_inputs || !out.created.is_empty();
            }
            Command::Paste { clipboard, transform, reuse_definitions } => {
                out.created = crate::clipboard::paste(doc, def, &clipboard, transform, reuse_definitions)?;
            }
            Command::Smoove { vertices, centres, radius, normal, offset } => {
                out.changed = doc
                    .with_mesh(def, |mesh| crate::terrain::smoove(mesh, &vertices, &centres, radius, normal, offset))
                    .expect("checked")
                    .map_err(CommandError::Invalid)?;
            }
            Command::DrapeOnFace { sources, target } => {
                if sources.contains(&Element::Face(target)) {
                    return invalid("terrain cannot also be the drape source");
                }
                let parent = doc.def(def).unwrap();
                let axes = parent.axes;
                let frame = axes.basis().inverse() * DMat4::from_translation(-axes.origin);
                let segments = crate::terrain_footprint::source_segments(doc, def, &sources, frame)
                    .map_err(CommandError::Invalid)?
                    .into_iter()
                    .filter(|line| {
                        rysmo_math::DVec2::new(line[0].x - line[1].x, line[0].y - line[1].y).length_squared() > 1e-20
                    })
                    .collect::<Vec<_>>();
                if segments.is_empty() {
                    return invalid("select source edges with horizontal extent");
                }
                let mut terrain =
                    crate::terrain_region::connected(&parent.mesh, target).map_err(CommandError::Invalid)?;
                terrain.map_all(|p| frame.transform_point3(p));
                let lines = crate::terrain_surface::TerrainSurface::drape_downward(&terrain, &segments)
                    .map_err(CommandError::Invalid)?;
                if lines.is_empty() {
                    out.changed = false;
                    return Ok(out);
                }
                let inverse = frame.inverse();
                return Command::DrawLines {
                    lines: lines.into_iter().map(|line| line.map(|p| inverse.transform_point3(p))).collect(),
                    facing: Some(axes.z()),
                }
                .run(doc, def);
            }
            Command::DrapeEdges { edges, target } => {
                return Command::DrapeSources { sources: edges.into_iter().map(Element::Edge).collect(), target }
                    .run(doc, def);
            }
            Command::DrapeSources { sources, target } => {
                if sources.contains(&Element::Instance(target)) {
                    return invalid("terrain cannot also be the drape source");
                }
                let parent = doc.def(def).unwrap();
                let instance = parent
                    .instances
                    .get(&target)
                    .ok_or_else(|| CommandError::Invalid("terrain instance is missing".into()))?;
                let axes = parent.axes;
                let frame = axes.basis().inverse() * DMat4::from_translation(-axes.origin);
                let segments = crate::terrain_footprint::source_segments(doc, def, &sources, frame)
                    .map_err(CommandError::Invalid)?
                    .into_iter()
                    .filter(|line| {
                        rysmo_math::DVec2::new(line[0].x - line[1].x, line[0].y - line[1].y).length_squared() > 1e-20
                    })
                    .collect::<Vec<_>>();
                if segments.is_empty() {
                    return invalid("select source edges with horizontal extent");
                }
                let definition = doc
                    .def(Some(instance.definition))
                    .ok_or_else(|| CommandError::Invalid("terrain definition is missing".into()))?;
                if !definition.instances.is_empty() {
                    return invalid("terrain must be a triangular mesh without child instances");
                }
                let projection = frame * instance.transform;
                if !projection.inverse().is_finite() {
                    return invalid("terrain transform is singular");
                }
                let mut terrain = definition.mesh.clone();
                terrain.map_all(|p| projection.transform_point3(p));
                let lines = crate::terrain_surface::TerrainSurface::drape_downward(&terrain, &segments)
                    .map_err(CommandError::Invalid)?;
                if lines.is_empty() {
                    out.changed = false;
                    return Ok(out);
                }
                let inverse = frame.inverse();
                let lines = lines.into_iter().map(|line| line.map(|p| inverse.transform_point3(p))).collect();
                return Command::DrawLines { lines, facing: Some(axes.z()) }.run(doc, def);
            }
            Command::StampOnFace { sources, target, offset, height } => {
                let parent = doc.def(def).unwrap();
                let region = crate::terrain_region::connected(&parent.mesh, target).map_err(CommandError::Invalid)?;
                if sources.iter().any(|source| matches!(source, Element::Face(face) if region.face(*face).is_some())) {
                    return invalid("terrain cannot also be the stamp source");
                }
                let region = crate::terrain_region::with_curves(&parent.mesh, region).map_err(CommandError::Invalid)?;
                let axes = parent.axes;
                let frame = axes.basis().inverse() * DMat4::from_translation(-axes.origin);
                let outlines = crate::terrain_footprint::source_boundaries(doc, def, &sources, frame)
                    .map_err(CommandError::Invalid)?;
                let mut projected = region.clone();
                projected.map_all(|p| frame.transform_point3(p));
                let mut stamped = crate::terrain_surface::TerrainSurface::stamp_rings(
                    &projected,
                    &outlines,
                    offset,
                    height,
                    doc.next_id,
                )
                .map_err(CommandError::Invalid)?;
                let unchanged = stamped
                    .vertices()
                    .filter(|(id, vertex)| projected.vertex(*id) == Some(*vertex))
                    .map(|(id, _)| id)
                    .collect::<BTreeSet<_>>();
                let inverse = frame.inverse();
                stamped.map_all(|p| inverse.transform_point3(p));
                stamped = Mesh::from_parts_with_curves(
                    stamped
                        .vertices()
                        .map(|(id, vertex)| {
                            (
                                id,
                                if unchanged.contains(&id) {
                                    region.vertex(id).unwrap().clone()
                                } else {
                                    vertex.clone()
                                },
                            )
                        })
                        .collect(),
                    stamped.edges().map(|(id, edge)| (id, edge.clone())).collect(),
                    stamped.faces().map(|(id, face)| (id, face.clone())).collect(),
                    stamped.curves().map(|(id, curve)| (id, *curve)).collect(),
                )?;
                let mesh =
                    crate::terrain_region::replace(&parent.mesh, &region, &stamped).map_err(CommandError::Invalid)?;
                doc.with_mesh(def, |target| *target = mesh).expect("checked");
                out.created = new_faces(doc);
            }
            Command::StampTerrainInstance { sources, target, offset, height } => {
                if sources.contains(&Element::Instance(target)) {
                    return invalid("terrain cannot also be the stamp source");
                }
                let parent = doc.def(def).unwrap();
                let instance = parent
                    .instances
                    .get(&target)
                    .ok_or_else(|| CommandError::Invalid("terrain instance is missing".into()))?
                    .clone();
                if !instance.transform.is_finite() || !instance.transform.inverse().is_finite() {
                    return invalid("terrain instance transform is singular or non-finite");
                }
                let axes = parent.axes;
                let frame = axes.basis().inverse() * DMat4::from_translation(-axes.origin);
                let outlines = crate::terrain_footprint::source_boundaries(doc, def, &sources, frame)
                    .map_err(CommandError::Invalid)?;
                let definition = doc
                    .def(Some(instance.definition))
                    .ok_or_else(|| CommandError::Invalid("terrain definition is missing".into()))?;
                if !definition.instances.is_empty() {
                    return invalid("terrain must contain a triangular mesh without nested instances");
                }
                let projection = frame * instance.transform;
                let mut mesh = definition.mesh.clone();
                mesh.map_all(|p| projection.transform_point3(p));
                let mut stamped =
                    crate::terrain_surface::TerrainSurface::stamp_rings(&mesh, &outlines, offset, height, doc.next_id)
                        .map_err(CommandError::Invalid)?;
                let inverse = projection.inverse();
                stamped.map_all(|p| inverse.transform_point3(p));
                doc.with_mesh(Some(instance.definition), |mesh| *mesh = stamped).expect("checked");
                out.created.push(Element::Instance(target));
            }
            Command::StampTerrainOutline { outline, offset, height } => {
                return Command::StampTerrainRings { outlines: vec![outline], offset, height }.run(doc, def);
            }
            Command::StampTerrainRings { outlines, offset, height } => {
                let source = doc.def(def).unwrap();
                let axes = source.axes;
                let basis = axes.basis();
                let inverse = basis.inverse();
                let mut projected = source.mesh.clone();
                projected.map_all(|p| inverse.transform_vector3(p - axes.origin));
                let mut stamped = crate::terrain_surface::TerrainSurface::stamp_rings(
                    &projected,
                    &outlines,
                    offset,
                    height,
                    doc.next_id,
                )
                .map_err(CommandError::Invalid)?;
                stamped.map_all(|p| axes.origin + basis.transform_vector3(p));
                doc.with_mesh(def, |mesh| *mesh = stamped).expect("checked");
                out.created = new_faces(doc);
            }
            Command::AddTerrainDetail { face, point, offset } => {
                let (_, changed) =
                    doc.with_mesh(def, |mesh| mesh.add_terrain_detail(face, point, offset)).expect("checked")?;
                out.changed = changed;
                out.created = new_faces(doc);
            }
            Command::FlipTerrainEdge { edge } => {
                let new_edge = doc.with_mesh(def, |mesh| mesh.flip_terrain_edge(edge)).expect("checked")?;
                out.created.push(Element::Edge(new_edge));
            }
            Command::TerrainFromContours { edges } => {
                let source = doc.def(def).unwrap();
                let axes = source.axes;
                let basis = axes.basis();
                let inverse = basis.inverse();
                let unique = edges.into_iter().collect::<BTreeSet<_>>();
                if unique.is_empty() {
                    return invalid("select contour edges first");
                }
                let lines = unique
                    .iter()
                    .map(|id| {
                        let edge = source
                            .mesh
                            .edge(*id)
                            .ok_or_else(|| CommandError::Invalid("contour edge is missing".into()))?;
                        Ok(edge.v.map(|vertex| inverse.transform_vector3(source.mesh.pos(vertex) - axes.origin)))
                    })
                    .collect::<Result<Vec<_>, CommandError>>()?;
                let mut mesh = crate::contours::triangulate(&lines, doc.next_id).map_err(CommandError::Invalid)?;
                mesh.map_all(|point| axes.origin + basis.transform_vector3(point));
                let origin = mesh.bounds().unwrap().0;
                mesh.map_all(|point| point - origin);
                for (curve_id, curve) in source.mesh.curves() {
                    let original = source.mesh.curve_edges(curve_id);
                    let selected = original.iter().filter(|id| unique.contains(id)).copied().collect::<Vec<_>>();
                    if selected.is_empty() {
                        continue;
                    }
                    let mut members = BTreeSet::new();
                    for edge in &selected {
                        let endpoints = source.mesh.edge(*edge).unwrap().v.map(|id| source.mesh.pos(id) - origin);
                        members.extend(mesh.edges_along(endpoints[0], endpoints[1]));
                    }
                    if members.is_empty() {
                        continue;
                    }
                    let mut descriptor = curve.mapped(&|point| point - origin);
                    if selected.len() != original.len() || members.len() != selected.len() {
                        descriptor.kind = rysmo_mesh::CurveKind::Polyline;
                        descriptor.segments = members.len() as u32;
                    }
                    mesh.register_curve(&members.into_iter().collect::<Vec<_>>(), descriptor)?;
                }
                doc.next_id = doc.next_id.max(mesh.next_id());
                let definition = DefinitionId(doc.reserve_id());
                doc.definitions.insert(
                    definition,
                    Definition {
                        name: "Terrain from Contours".into(),
                        group: true,
                        mesh,
                        axes: rysmo_math::DrawingAxes { origin: DVec3::ZERO, x: axes.x, y: axes.y },
                        ..Default::default()
                    }
                    .into(),
                );
                let instance = InstanceId(doc.reserve_id());
                doc.def_mut(def).unwrap().instances.insert(
                    instance,
                    Instance {
                        glued_to: None,
                        definition,
                        transform: DMat4::from_translation(origin),
                        name: String::new(),
                        tag: None,
                        material: None,
                        hidden: false,
                    },
                );
                out.created.push(Element::Instance(instance));
            }
            Command::CreateTerrainGrid { origin, u, v, spacing } => {
                let axes = rysmo_math::DrawingAxes::new(origin, u, v)
                    .ok_or_else(|| CommandError::Invalid("invalid terrain plane".into()))?;
                if u.normalize().dot(v.normalize()).abs() > 1e-6 {
                    return invalid("terrain sides must be perpendicular");
                }
                let mut mesh = crate::terrain::grid_with_ids(u.length(), v.length(), spacing, doc.next_id)
                    .map_err(CommandError::Invalid)?;
                if axes.z().dot(doc.def(def).unwrap().axes.z()) < 0. {
                    let faces: Vec<_> = mesh.faces().map(|(id, _)| id).collect();
                    mesh.reverse_faces(&faces);
                }
                doc.next_id = doc.next_id.max(mesh.next_id());
                let definition = DefinitionId(doc.reserve_id());
                doc.definitions.insert(
                    definition,
                    Definition { name: "Terrain".into(), group: true, mesh, ..Default::default() }.into(),
                );
                let instance = InstanceId(doc.reserve_id());
                doc.def_mut(def).unwrap().instances.insert(
                    instance,
                    Instance {
                        glued_to: None,
                        definition,
                        transform: DMat4::from_translation(origin) * axes.basis(),
                        name: String::new(),
                        tag: None,
                        material: None,
                        hidden: false,
                    },
                );
                out.created.push(Element::Instance(instance));
            }
            Command::CreateText3d { content, settings, origin, x, y } => {
                let axes = rysmo_math::DrawingAxes::new(origin, x, y)
                    .ok_or_else(|| CommandError::Invalid("invalid 3D text placement axes".into()))?;
                let mut mesh = crate::text3d::build_text(&content, &settings).map_err(CommandError::Invalid)?;
                mesh.ensure_next_id(doc.next_id);
                let elements: Vec<_> = mesh
                    .edges()
                    .map(|(id, _)| Element::Edge(id))
                    .chain(mesh.faces().map(|(id, _)| Element::Face(id)))
                    .collect();
                let mesh = mesh.extract(&elements);
                doc.next_id = doc.next_id.max(mesh.next_id());
                let definition = DefinitionId(doc.reserve_id());
                doc.definitions.insert(
                    definition,
                    Definition {
                        name: content.lines().next().unwrap_or("3D Text").chars().take(80).collect(),
                        group: true,
                        mesh,
                        ..Default::default()
                    }
                    .into(),
                );
                let instance = InstanceId(doc.reserve_id());
                doc.def_mut(def).unwrap().instances.insert(
                    instance,
                    Instance {
                        glued_to: None,
                        definition,
                        transform: DMat4::from_translation(origin) * axes.basis(),
                        name: String::new(),
                        tag: None,
                        material: None,
                        hidden: false,
                    },
                );
                out.created.push(Element::Instance(instance));
            }
            Command::ApplyTextDefaults { ids, camera, viewport, transform } => {
                annotations::apply_text_defaults(doc, def, &mut out, ids, camera, viewport, transform)?;
            }
            Command::ApplyTextAppearance { ids } => {
                annotations::apply_text_appearance(doc, def, &mut out, ids)?;
            }
            Command::AddText { text } => {
                annotations::add_text(doc, def, &mut out, text)?;
            }
            Command::SetText { id, text } => {
                annotations::set_text(doc, def, &mut out, id, text)?;
            }
            Command::EraseTexts { ids } => {
                annotations::erase_texts(doc, def, &mut out, ids)?;
            }
            Command::AddDimension { dimension } => {
                annotations::add_dimension(doc, def, &mut out, dimension)?;
            }
            Command::SetDimension { id, dimension } => {
                annotations::set_dimension(doc, def, &mut out, id, dimension)?;
            }
            Command::ApplyDimensionDefaults { ids } => {
                annotations::apply_dimension_defaults(doc, def, &mut out, ids)?;
            }
            Command::EraseDimensions { ids } => {
                annotations::erase_dimensions(doc, def, &mut out, ids)?;
            }
            Command::SetCurveSegments { curve, segments } => {
                out.created = mesh_op(doc, &mut |mesh| mesh.set_curve_segments(curve, segments).map(edges))?;
            }
            Command::SetCurveRadius { curve, radius } => {
                mesh_op(doc, &mut |mesh| mesh.set_curve_radius(curve, radius).map(|_| Vec::new()))?;
            }
            Command::DrawFreehand { points, normal } => {
                if points.len() < 2
                    || !points.iter().all(|p| p.is_finite())
                    || !normal.is_finite()
                    || normal.length() < EPS
                {
                    return invalid("a curve needs at least two points");
                }
                let lines: Vec<_> = points.windows(2).map(|p| (p[0], p[1])).collect();
                out.created = mesh_op(doc, &mut |m| {
                    let members = m.add_lines(&lines, Some(normal))?;
                    m.register_curve(
                        &members,
                        rysmo_mesh::Curve {
                            kind: rysmo_mesh::CurveKind::Polyline,
                            faceted: false,
                            centre: points[0],
                            normal,
                            start: DVec3::X,
                            radius: 0.0,
                            segments: (points.len() - 1) as u32,
                            sweep: 0.0,
                        },
                    )?;
                    Ok(edges(members))
                })?;
                out.created.extend(new_faces(doc));
            }
            Command::DrawLines { lines, facing } => {
                let lines: Vec<_> = lines.into_iter().map(|[a, b]| (a, b)).collect();
                out.created = mesh_op(doc, &mut |m| m.add_lines(&lines, facing).map(edges))?;
                out.created.extend(new_faces(doc));
            }
            Command::DrawRectangle { corner, u, v, facing } => {
                if u.length() < EPS || v.length() < EPS || u.cross(v).length() < EPS * EPS {
                    return invalid("the rectangle has no area");
                }
                let p = [corner, corner + u, corner + u + v, corner + v];
                let lines: Vec<_> = (0..4).map(|i| (p[i], p[(i + 1) % 4])).collect();
                out.created = mesh_op(doc, &mut |m| m.add_lines(&lines, facing).map(edges))?;
                out.created.extend(new_faces(doc));
            }
            Command::DrawCircle { centre, normal, radius, segments, start }
            | Command::DrawPolygon { centre, normal, radius, segments, start } => {
                if !radius.is_finite()
                    || !centre.is_finite()
                    || !normal.is_finite()
                    || radius < EPS
                    || segments < 3
                    || normal.length() < EPS
                {
                    return invalid("a circle needs a radius and at least 3 segments");
                }
                let n = normal.normalize();
                let plane = Plane::new(n, centre);
                let u = start
                    .map(|s| s - n * s.dot(n))
                    .filter(|s| s.length() > EPS)
                    .map(|s| s.normalize())
                    .unwrap_or_else(|| plane.basis().0);
                let v = n.cross(u);
                let pts: Vec<DVec3> = (0..segments)
                    .map(|i| {
                        let a = i as f64 / segments as f64 * std::f64::consts::TAU;
                        centre + (u * a.cos() + v * a.sin()) * radius
                    })
                    .collect();
                let k = pts.len();
                let lines: Vec<_> = (0..k).map(|i| (pts[i], pts[(i + 1) % k])).collect();
                out.created = mesh_op(doc, &mut |m| {
                    let members = m.add_lines(&lines, Some(n))?;
                    m.register_curve(
                        &members,
                        rysmo_mesh::Curve {
                            faceted: polygon,
                            kind: if polygon { rysmo_mesh::CurveKind::Polygon } else { rysmo_mesh::CurveKind::Circle },
                            centre,
                            normal: n,
                            start: u,
                            radius,
                            segments,
                            sweep: 360.0,
                        },
                    )?;
                    Ok(edges(members))
                })?;
                out.created.extend(new_faces(doc));
            }
            Command::DrawArc { start, end, normal, bulge, segments } => {
                let pts = arc_points(start, end, normal, bulge, segments)
                    .ok_or_else(|| CommandError::Invalid("the arc needs a chord, a bulge and segments".into()))?;
                let lines: Vec<_> = pts.windows(2).map(|w| (w[0], w[1])).collect();
                let chord = end - start;
                let n = normal.normalize();
                let r = (chord.length_squared() * 0.25 + bulge * bulge) / (2.0 * bulge.abs());
                let centre = (start + end) * 0.5 + n.cross(chord.normalize()) * (bulge - bulge.signum() * r);
                let sweep = -bulge.signum() * 4.0 * (2.0 * bulge.abs() / chord.length()).atan().to_degrees();
                out.created = mesh_op(doc, &mut |m| {
                    let members = m.add_lines(&lines, Some(normal))?;
                    m.register_curve(
                        &members,
                        rysmo_mesh::Curve {
                            kind: rysmo_mesh::CurveKind::Arc,
                            faceted: false,
                            centre,
                            normal: n,
                            start: (start - centre).normalize(),
                            radius: r,
                            segments,
                            sweep,
                        },
                    )?;
                    Ok(edges(members))
                })?;
                out.created.extend(new_faces(doc));
            }
            Command::FilletEdges { edges, radius, segments } => {
                out.created = doc
                    .with_mesh(def, |m| m.fillet(edges, radius, segments))
                    .expect("checked")
                    .map_err(CommandError::Invalid)?;
            }
            Command::DrawThreePointArc { start, through, end, normal, segments } => {
                let (arc, points) =
                    crate::arcs::three_point_points(start, through, end, normal, segments).ok_or_else(|| {
                        CommandError::Invalid(
                            "three-point arc needs distinct, non-collinear coplanar points and 2-4096 segments".into(),
                        )
                    })?;
                let lines: Vec<_> = points.windows(2).map(|p| (p[0], p[1])).collect();
                out.created = mesh_op(doc, &mut |mesh| {
                    let members = mesh.add_lines(&lines, Some(arc.normal))?;
                    mesh.register_curve(
                        &members,
                        rysmo_mesh::Curve {
                            kind: rysmo_mesh::CurveKind::Arc,
                            faceted: false,
                            centre: arc.centre,
                            normal: arc.normal,
                            start: arc.start,
                            radius: arc.radius,
                            sweep: arc.sweep,
                            segments,
                        },
                    )?;
                    Ok(edges(members))
                })?;
                out.created.extend(new_faces(doc));
            }
            Command::DrawCentreArc { centre, normal, start, radius, sweep, segments, pie } => {
                let points=centre_arc_points(centre,normal,start,radius,sweep,segments)
                    .ok_or_else(||CommandError::Invalid("arc needs a finite centre/radius, perpendicular start, 2-4096 segments and a nonzero sweep below 360 degrees".into()))?;
                let lines: Vec<_> = points.windows(2).map(|p| (p[0], p[1])).collect();
                out.created = mesh_op(doc, &mut |mesh| {
                    let members = mesh.add_lines(&lines, Some(normal))?;
                    let mut all = members.clone();
                    if pie {
                        all.extend(
                            mesh.add_lines(&[(centre, points[0]), (*points.last().unwrap(), centre)], Some(normal))?,
                        );
                    }
                    mesh.register_curve(
                        &members,
                        rysmo_mesh::Curve {
                            kind: rysmo_mesh::CurveKind::Arc,
                            faceted: false,
                            centre,
                            normal: normal.normalize(),
                            start: start.normalize(),
                            radius,
                            segments,
                            sweep,
                        },
                    )?;
                    all.sort();
                    all.dedup();
                    Ok(edges(all))
                })?;
                out.created.extend(new_faces(doc));
            }
            Command::PushPull { face, distance, keep_base } => {
                out.created = mesh_op(doc, &mut |m| {
                    let r = m.push_pull(face, distance, keep_base)?;
                    let mut c: Vec<Element> = r.sides.into_iter().map(Element::Face).collect();
                    c.extend(r.face.map(Element::Face));
                    Ok(c)
                })?;
            }
            Command::CopyWithText { elements, offset, copies, pivot, view } => {
                if !offset.is_finite()
                    || !pivot.is_finite()
                    || !view.eye.is_finite()
                    || !view.size.is_finite()
                    || view.size.min_element() <= 0.
                    || !view.view.is_finite()
                    || !view.proj.is_finite()
                    || !view.view.inverse().is_finite()
                    || !view.proj.inverse().is_finite()
                    || copies == 0
                    || copies > 10000
                {
                    return invalid("invalid text copy count or view");
                }
                let source = doc.def(def).unwrap().mesh.clone();
                let originals = elements
                    .iter()
                    .filter_map(|e| if let Element::Text(id) = e { Some(*id) } else { None })
                    .map(|id| {
                        doc.def(def)
                            .unwrap()
                            .texts
                            .get(&id)
                            .cloned()
                            .ok_or_else(|| CommandError::Invalid(format!("no text {id} here")))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let geometry = elements.iter().copied().filter(|e| !matches!(e, Element::Text(_))).collect::<Vec<_>>();
                let old_instances = geometry
                    .iter()
                    .filter_map(|e| if let Element::Instance(id) = e { Some(*id) } else { None })
                    .collect::<Vec<_>>();
                if old_instances.iter().any(|id| !doc.def(def).unwrap().instances.contains_key(id)) {
                    return invalid("instance not found in this context");
                }
                for k in 1..=copies {
                    let delta = offset * k as f64;
                    let matrix = DMat4::from_translation(delta);
                    let made = if geometry.is_empty() {
                        Vec::new()
                    } else {
                        Command::Copy { elements: geometry.clone(), offset: delta, copies: 1 }.run(doc, def)?.created
                    };
                    let instances = old_instances
                        .iter()
                        .copied()
                        .zip(made.iter().filter_map(|e| if let Element::Instance(id) = e { Some(*id) } else { None }))
                        .collect::<BTreeMap<_, _>>();
                    out.created.extend(made);
                    let pixel_delta = view
                        .project(pivot + delta)
                        .zip(view.project(pivot))
                        .map(|(a, b)| a - b)
                        .ok_or_else(|| CommandError::Invalid("copy pivot is behind the camera".into()))?;
                    for original in &originals {
                        let mut text =
                            original.copied_links(&source, &doc.def(def).unwrap().mesh, matrix, &instances, &geometry);
                        match (&original.placement, &mut text.placement) {
                            (
                                crate::text::TextPlacement::Screen { position: old },
                                crate::text::TextPlacement::Screen { position },
                            ) => *position = *old + pixel_delta / view.size,
                            (
                                crate::text::TextPlacement::Attached { anchor: old, leader: old_leader },
                                crate::text::TextPlacement::Attached { anchor, leader },
                            ) => {
                                let old_point = old.point;
                                let point = anchor.resolve(doc, def).0;
                                match (old_leader, leader) {
                                    (
                                        crate::text::TextLeader::ViewBased { offset: old_offset },
                                        crate::text::TextLeader::ViewBased { offset: label_offset },
                                    ) => {
                                        if point.distance(old_point + delta) < EPS {
                                            *label_offset = *old_offset;
                                        } else {
                                            *label_offset = *old_offset + pixel_delta;
                                        }
                                    }
                                    (
                                        crate::text::TextLeader::Pushpin { offset: old_offset }
                                        | crate::text::TextLeader::None { offset: old_offset },
                                        crate::text::TextLeader::Pushpin { offset: label_offset }
                                        | crate::text::TextLeader::None { offset: label_offset },
                                    ) => *label_offset = old_point + *old_offset + delta - point,
                                    _ => unreachable!(),
                                }
                            }
                            _ => unreachable!(),
                        }
                        if !text.validate() {
                            return invalid("invalid copied text placement");
                        }
                        let id = rysmo_mesh::TextId(doc.reserve_id());
                        doc.def_mut(def).unwrap().texts.insert(id, text);
                        out.created.push(Element::Text(id));
                        out.texts.push(id);
                    }
                }
                out.changed = !out.created.is_empty();
            }
            Command::MoveWithText { elements, offset, pivot, view } => {
                if !offset.is_finite()
                    || !view.eye.is_finite()
                    || !pivot.is_finite()
                    || !view.size.is_finite()
                    || view.size.min_element() <= 0.
                    || !view.view.is_finite()
                    || !view.proj.is_finite()
                    || !view.view.inverse().is_finite()
                    || !view.proj.inverse().is_finite()
                {
                    return invalid("invalid text move view or offset");
                }
                if offset.length() < EPS {
                    if elements
                        .iter()
                        .any(|e| matches!(e,Element::Text(id) if !doc.def(def).unwrap().texts.contains_key(id)))
                    {
                        return invalid("text not found in this context");
                    }
                    out.changed = false;
                    return Ok(out);
                }
                let pixel_delta = view
                    .project(pivot + offset)
                    .zip(view.project(pivot))
                    .map(|(a, b)| a - b)
                    .ok_or_else(|| CommandError::Invalid("move pivot is behind the camera".into()))?;
                let mut targets = Vec::new();
                for element in &elements {
                    if let Element::Text(id) = element {
                        let text = doc
                            .def(def)
                            .unwrap()
                            .texts
                            .get(id)
                            .ok_or_else(|| CommandError::Invalid(format!("no text {id} here")))?
                            .clone();
                        let layout = text
                            .layout(doc, def, DMat4::IDENTITY, &view)
                            .ok_or_else(|| CommandError::Invalid("text cannot be moved in this view".into()))?;
                        let position = match &text.placement {
                            crate::text::TextPlacement::Screen { .. } => layout.position + pixel_delta,
                            crate::text::TextPlacement::Attached { anchor, leader } => {
                                let point = anchor.resolve(doc, def).0;
                                match leader {
                                    crate::text::TextLeader::ViewBased { .. } => layout.position + pixel_delta,
                                    crate::text::TextLeader::Pushpin { offset: label_offset }
                                    | crate::text::TextLeader::None { offset: label_offset } => {
                                        view.project(point + *label_offset + offset).ok_or_else(|| {
                                            CommandError::Invalid("moved text is behind the camera".into())
                                        })?
                                    }
                                }
                            }
                        };
                        targets.push((*id, text, position));
                    }
                }
                let geometry: Vec<_> = elements.iter().copied().filter(|e| !matches!(e, Element::Text(_))).collect();
                out.changed = if geometry.is_empty() {
                    false
                } else {
                    Command::Move { elements: geometry, offset }.run(doc, def)?.changed
                };
                for (id, mut original, mut position) in targets {
                    if let crate::text::TextPlacement::Attached {
                        anchor,
                        leader: crate::text::TextLeader::ViewBased { offset: label_offset },
                    } = &original.placement
                    {
                        let old = anchor.point;
                        let new = anchor.resolve(doc, def).0;
                        if new.distance(old + offset) < EPS && new.distance(old) > EPS {
                            position = view
                                .project(new)
                                .ok_or_else(|| CommandError::Invalid("moved attachment is behind the camera".into()))?
                                + *label_offset;
                        }
                    }
                    if let crate::text::TextPlacement::Attached {
                        anchor,
                        leader:
                            crate::text::TextLeader::Pushpin { offset: label_offset }
                            | crate::text::TextLeader::None { offset: label_offset },
                    } = &mut original.placement
                    {
                        *label_offset = anchor.point + *label_offset + offset - anchor.resolve(doc, def).0;
                    } else {
                        original = original
                            .at_label_position(doc, def, DMat4::IDENTITY, &view, position)
                            .ok_or_else(|| CommandError::Invalid("invalid moved text placement".into()))?;
                    }
                    if !original.validate() {
                        return invalid("invalid moved text placement");
                    }
                    let stored = doc.def_mut(def).unwrap().texts.get_mut(&id).unwrap();
                    out.changed |= *stored != original;
                    *stored = original;
                }
            }
            Command::Move { elements, offset } => {
                if offset.length() < EPS {
                    out.changed = false;
                } else {
                    let source_mesh = &doc.def(def).unwrap().mesh;
                    let moved_faces: Vec<_> = elements
                        .iter()
                        .filter_map(|element| {
                            let Element::Face(id) = element else { return None };
                            let face = source_mesh.face(*id)?;
                            let outer: Vec<_> = face.outer.iter().map(|v| source_mesh.pos(*v) + offset).collect();
                            let plane = rysmo_math::Plane::from_polygon(&outer)?;
                            let outer: Vec<_> = outer.into_iter().map(|p| plane.to_2d(p)).collect();
                            let holes: Vec<Vec<_>> = face
                                .holes
                                .iter()
                                .map(|hole| hole.iter().map(|v| plane.to_2d(source_mesh.pos(*v) + offset)).collect())
                                .collect();
                            Some((*id, plane, outer, holes))
                        })
                        .collect();
                    let moved_edges: Vec<_> = elements
                        .iter()
                        .filter_map(|e| match e {
                            Element::Edge(id) => doc.def(def).unwrap().mesh.edge(*id).map(|_| {
                                let (a, b) = doc.def(def).unwrap().mesh.edge_points(*id);
                                (*id, a + offset, b + offset)
                            }),
                            _ => None,
                        })
                        .collect();
                    // Apply placement before topology splits so every resulting
                    // region inherits the moved texture frame.
                    let mesh = &mut doc.def_mut(def).expect("checked").mesh;
                    for e in &elements {
                        if let Element::Face(f) = e
                            && let Some(n) = mesh.face_normal(*f)
                            && let Some(face) = mesh.face_mut(*f)
                        {
                            face.uv = Some(crate::texture::moved(face.uv.unwrap_or_default(), n, offset));
                        }
                    }
                    transform_items(doc, def, &elements, DMat4::from_translation(offset))?;
                    let mesh = &doc.def(def).expect("checked").mesh;
                    out.created = elements
                        .into_iter()
                        .flat_map(|element| {
                            if let Element::Edge(id) = element
                                && let Some((_, a, b)) = moved_edges.iter().find(|(old, _, _)| *old == id)
                            {
                                return mesh.edges_along(*a, *b).into_iter().map(Element::Edge).collect::<Vec<_>>();
                            }
                            if let Element::Face(id) = element
                                && let Some((_, plane, outer, holes)) = moved_faces.iter().find(|(old, ..)| *old == id)
                            {
                                return mesh
                                    .faces()
                                    .filter_map(|(id, _)| {
                                        if !mesh.face_plane(id)?.same_surface(plane) {
                                            return None;
                                        }
                                        let triangle = *rysmo_mesh::triangulate_face(mesh, id).first()?;
                                        let point =
                                            plane.to_2d(triangle.iter().map(|v| mesh.pos(*v)).sum::<DVec3>() / 3.);
                                        let inside = rysmo_math::point_in_polygon_2d(point, outer, 1e-9)
                                            != rysmo_math::PolygonSide::Outside
                                            && holes.iter().all(|hole| {
                                                rysmo_math::point_in_polygon_2d(point, hole, 1e-9)
                                                    == rysmo_math::PolygonSide::Outside
                                            });
                                        inside.then_some(Element::Face(id))
                                    })
                                    .collect::<Vec<_>>();
                            }
                            vec![element]
                        })
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect();
                }
            }
            Command::Copy { elements, offset, copies } => {
                if copies == 0 {
                    return invalid("zero copies");
                }
                if offset.length() < EPS {
                    return Err(MeshError::ZeroLength.into());
                }
                out.created = copy_items(doc, def, &elements, copies, |k| DMat4::from_translation(offset * k as f64))?;
            }
            Command::Rotate { elements, centre, axis, angle, copies } => {
                validate_rotation(centre, axis, angle)?;
                if !(angle * copies.max(1) as f64).is_finite() {
                    return invalid("rotation array angle exceeds the numeric range");
                }
                let screen_only=!elements.is_empty() && elements.iter().all(|e| matches!(e,Element::Text(id) if doc.def(def).unwrap().texts.get(id).is_some_and(|text| matches!(text.placement,crate::text::TextPlacement::Screen {..}))));
                if angle.abs() < 1e-9 || copies == 0 && screen_only {
                    out.changed = false;
                } else if copies == 0 {
                    transform_items(doc, def, &elements, rotation(centre, axis, angle))?;
                    out.created = elements;
                } else {
                    out.created =
                        copy_items(doc, def, &elements, copies, |k| rotation(centre, axis, angle * k as f64))?;
                }
            }
            Command::SetAxes { origin, x, y } => {
                let axes = rysmo_math::DrawingAxes::new(origin, x, y)
                    .ok_or_else(|| CommandError::Invalid("axes need finite, independent directions".into()))?;
                let d = doc.def_mut(def).expect("checked");
                out.changed = d.axes != axes;
                d.axes = axes;
            }
            Command::ResizeContext { factor } => {
                if !factor.is_finite() || factor <= 1e-9 {
                    return invalid("resize needs a finite positive factor");
                }
                if (factor - 1.0).abs() < 1e-12 {
                    out.changed = false;
                } else {
                    let d = doc.def(def).expect("checked");
                    let elements: Vec<_> = d
                        .mesh
                        .edges()
                        .map(|(id, _)| Element::Edge(id))
                        .chain(d.mesh.faces().map(|(id, _)| Element::Face(id)))
                        .chain(d.instances.keys().copied().map(Element::Instance))
                        .chain(d.guides.keys().copied().map(Element::Guide))
                        .collect();
                    transform_items(doc, def, &elements, DMat4::from_scale(DVec3::splat(factor)))?;
                    let d = doc.def_mut(def).expect("checked");
                    d.axes.origin *= factor;
                    for plane in d.sections.planes.values_mut() {
                        plane.origin *= factor;
                    }
                    for dimension in d.dimensions.values_mut() {
                        *dimension = dimension.clone().scaled(factor);
                    }
                    for text in d.texts.values_mut() {
                        *text = text.clone().scaled(factor);
                    }
                    let faces: Vec<_> = d.mesh.faces().map(|(id, _)| id).collect();
                    for id in faces {
                        let face = d.mesh.face_mut(id).unwrap();
                        let mut uv = face.uv.unwrap_or_default();
                        uv.offset = uv.offset.map(|v| v * factor);
                        uv.scale = uv.scale.map(|v| v * factor);
                        face.uv = Some(uv);
                    }
                    if def.is_none()
                        && let Some(camera) = &mut doc.camera
                    {
                        camera.target *= factor;
                        camera.distance *= factor;
                        camera.eye_base_z *= factor;
                    }
                }
            }
            Command::Scale { elements, origin, factors } | Command::ScaleInAxes { elements, origin, factors, .. } => {
                if !factors.is_finite()
                    || !origin.is_finite()
                    || factors.x.abs() < 1e-9
                    || factors.y.abs() < 1e-9
                    || factors.z.abs() < 1e-9
                {
                    return invalid("a scale factor of zero flattens the geometry");
                }
                let m = DMat4::from_translation(origin)
                    * scale_basis
                    * DMat4::from_scale(factors)
                    * scale_basis.inverse()
                    * DMat4::from_translation(-origin);
                if !elements.is_empty() && elements.iter().all(|e| matches!(e,Element::Text(id) if doc.def(def).unwrap().texts.get(id).is_some_and(|text| matches!(text.placement,crate::text::TextPlacement::Screen {..})))) {out.changed=false;return Ok(out);}
                transform_items(doc, def, &elements, m)?;
                // A mirror (an odd number of negative factors) turns faces inside out.
                if factors.x * factors.y * factors.z < 0.0 {
                    let faces: Vec<FaceId> = elements
                        .iter()
                        .filter_map(|e| if let Element::Face(f) = e { Some(*f) } else { None })
                        .collect();
                    doc.with_mesh(def, |m| m.reverse_faces(&faces));
                }
                out.created = elements;
            }
            Command::OffsetFace { face, distance } => {
                out.created = mesh_op(doc, &mut |m| m.offset_face(face, distance).map(edges))?;
                out.created.extend(new_faces(doc));
            }
            Command::OffsetEdges { edges: chain, normal, distance } => {
                out.created = mesh_op(doc, &mut |m| m.offset_chain(&chain, normal, distance).map(edges))?;
                out.created.extend(new_faces(doc));
            }
            Command::FollowMe { face, path } => {
                out.created = mesh_op(doc, &mut |m| {
                    let (chain, closed) = m.edge_chain(&path).ok_or(MeshError::NotAChain)?;
                    let pts = m.loop_points(&chain);
                    m.follow_me(face, &pts, closed).map(|fs| fs.into_iter().map(Element::Face).collect())
                })?;
            }
            Command::Erase { elements } => {
                let (geo, inst, guides, dimensions) = split(&elements);
                let texts: Vec<_> =
                    elements.iter().filter_map(|e| if let Element::Text(id) = e { Some(*id) } else { None }).collect();
                let d = doc.def(def).expect("checked");
                let sections: Vec<_> = elements
                    .iter()
                    .filter_map(|e| if let Element::Section(id) = e { Some(*id) } else { None })
                    .collect();
                let any = sections.iter().any(|id| d.sections.planes.contains_key(id))
                    || geo.iter().any(|e| d.mesh.contains(*e))
                    || inst.iter().any(|i| d.instances.contains_key(i))
                    || guides.iter().any(|g| d.guides.contains_key(g))
                    || dimensions.iter().any(|id| d.dimensions.contains_key(id))
                    || texts.iter().any(|id| d.texts.contains_key(id));
                if !any {
                    out.changed = false;
                } else {
                    doc.with_mesh(def, |m| m.erase(&geo));
                    let d = doc.def_mut(def).expect("checked");
                    for i in inst {
                        d.instances.remove(&i);
                    }
                    for g in guides {
                        d.guides.remove(&g);
                    }
                    for id in dimensions {
                        d.dimensions.remove(&id);
                    }
                    for id in texts {
                        d.texts.remove(&id);
                    }
                    for id in sections {
                        d.sections.planes.remove(&id);
                        if d.sections.active == Some(id) {
                            d.sections.active = None;
                        }
                    }
                }
            }
            Command::DivideEdges { edges: selected, segments } => {
                out.created = mesh_op(doc, &mut |mesh| mesh.divide_edges(&selected, segments).map(&edges))?;
            }
            Command::WeldEdges { edges: selected } => {
                out.changed = doc.with_mesh(def, |m| m.weld_edges(&selected)).expect("checked")?;
                out.created = edges(selected.into_iter().collect::<BTreeSet<_>>().into_iter().collect());
            }
            Command::ExplodeCurves { edges: selected } => {
                out.created = mesh_op(doc, &mut |m| m.explode_curves(&selected).map(&edges))?;
            }
            Command::OrientFaces { seed } => {
                let flipped = doc.with_mesh(def, |m| m.orient_faces(seed)).expect("checked")?;
                out.changed = !flipped.is_empty();
                out.created = flipped.into_iter().map(Element::Face).collect();
            }
            Command::ReverseFaces { faces } => {
                doc.with_mesh(def, |m| m.reverse_faces(&faces));
            }
            Command::SetEdges { edges, soft, smooth, hidden } => {
                doc.with_mesh(def, |m| m.set_edge_flags(&edges, soft, smooth, hidden));
            }
            Command::SoftenEdges { edges, angle } => {
                doc.with_mesh(def, |m| {
                    m.set_edge_flags(&edges, Some(false), Some(false), None);
                    m.soften_by_angle(&edges, angle);
                });
            }
            Command::MakeGroup { elements } => {
                let i = make_definition(doc, def, &elements, true, None)?;
                out.created = vec![Element::Instance(i)];
            }
            Command::MakeComponent { elements, name } => {
                let i = make_definition(doc, def, &elements, false, name)?;
                out.created = vec![Element::Instance(i)];
            }
            Command::Explode { instance } => {
                out.created = explode(doc, def, instance)?;
            }
            Command::MakeUnique { instance } => {
                make_unique(doc, def, instance)?;
                out.created = vec![Element::Instance(instance)];
            }
            Command::SetGeometryHidden { elements, hidden } => {
                let owner = doc.def_mut(def).unwrap();
                out.changed = false;
                for element in elements {
                    match element {
                        Element::Face(id) => {
                            let face = owner
                                .mesh
                                .face_mut(id)
                                .ok_or_else(|| CommandError::Invalid("face is missing".into()))?;
                            out.changed |= face.hidden != hidden;
                            face.hidden = hidden;
                        }
                        Element::Edge(id) => {
                            let edge =
                                owner.mesh.edge(id).ok_or_else(|| CommandError::Invalid("edge is missing".into()))?;
                            out.changed |= edge.hidden != hidden;
                            owner.mesh.set_edge_flags(&[id], None, None, Some(hidden));
                        }
                        Element::Instance(id) => {
                            let instance = owner
                                .instances
                                .get_mut(&id)
                                .ok_or_else(|| CommandError::Invalid("instance is missing".into()))?;
                            out.changed |= instance.hidden != hidden;
                            instance.hidden = hidden;
                        }
                        Element::Guide(id) => {
                            let guide = owner
                                .guides
                                .get_mut(&id)
                                .ok_or_else(|| CommandError::Invalid("guide is missing".into()))?;
                            out.changed |= guide.hidden() != hidden;
                            guide.set_hidden(hidden);
                        }
                        _ => return invalid("Only geometry, guides and instances can be hidden."),
                    }
                }
            }
            Command::SetInstancesHidden { instances, hidden } => {
                let owner = doc.def_mut(def).unwrap();
                out.changed = false;
                for id in instances {
                    let instance = owner
                        .instances
                        .get_mut(&id)
                        .ok_or_else(|| CommandError::Invalid("instance is missing".into()))?;
                    out.changed |= instance.hidden != hidden;
                    instance.hidden = hidden;
                }
            }
            Command::SetComponentProperties { definition, name, settings } => {
                if settings.cut_opening {
                    if settings.glue == crate::components::GlueMode::None {
                        return invalid("Choose a glue orientation before enabling Cut Opening.");
                    }
                    let mut proposed = doc
                        .def(Some(definition))
                        .ok_or_else(|| CommandError::Invalid("component is missing".into()))?
                        .clone();
                    proposed.component = settings.clone();
                    crate::components::opening_outline(&proposed)?;
                }
                if name.trim().is_empty() || !settings.insertion_axes.valid() {
                    return invalid("invalid component properties");
                }
                let definition = doc
                    .def_mut(Some(definition))
                    .ok_or_else(|| CommandError::Invalid("component is missing".into()))?;
                out.changed = definition.name != name || definition.component != settings;
                definition.name = name;
                definition.component = settings;
            }
            Command::DeleteDefinition { definition } => {
                if doc.instance_count(definition) > 0 {
                    return invalid("delete the component instances first");
                }
                out.changed = doc.definitions.remove(&definition).is_some();
            }
            Command::RenameInstance { instance, name } => {
                let inst = doc
                    .def_mut(def)
                    .expect("checked")
                    .instances
                    .get_mut(&instance)
                    .ok_or_else(|| CommandError::Invalid(format!("no instance {instance}")))?;
                inst.name = name;
            }
            Command::UnglueInstances { instances } => {
                out.changed = false;
                for id in instances {
                    let instance = doc
                        .def_mut(def)
                        .unwrap()
                        .instances
                        .get_mut(&id)
                        .ok_or_else(|| CommandError::Invalid("instance is missing".into()))?;
                    out.changed |= instance.glued_to.take().is_some();
                }
            }
            Command::PlaceGluedInstance { definition, face, origin, rotation, scale } => {
                if Some(definition) == def || def.is_some_and(|d| doc.contains_definition(definition, d)) {
                    return invalid("A component cannot contain itself.");
                }
                let transform =
                    crate::components::glued_transform(doc, def, definition, face, origin, rotation, scale)?;
                let id = InstanceId(doc.reserve_id());
                doc.def_mut(def).unwrap().instances.insert(
                    id,
                    Instance {
                        definition,
                        transform,
                        name: String::new(),
                        tag: None,
                        material: None,
                        hidden: false,
                        glued_to: Some(face),
                    },
                );
                out.created = vec![Element::Instance(id)];
            }
            Command::PlaceInstance { definition, origin, rotation, scale } => {
                if !doc.definitions.contains_key(&definition) {
                    return invalid(format!("no definition {definition}"));
                }
                if !origin.is_finite() || !rotation.is_finite() || !scale.is_finite() || scale.abs() < 1e-9 {
                    return invalid("the placement must be finite and the scale must not be zero");
                }
                if Some(definition) == def || def.is_some_and(|d| doc.contains_definition(definition, d)) {
                    return invalid("a component cannot contain itself");
                }
                let transform = DMat4::from_translation(origin)
                    * DMat4::from_rotation_z(rotation.to_radians())
                    * DMat4::from_scale(DVec3::splat(scale))
                    * doc.definitions[&definition].component.frame().inverse();
                let id = InstanceId(doc.reserve_id());
                doc.def_mut(def).expect("checked").instances.insert(
                    id,
                    Instance {
                        glued_to: None,
                        definition,
                        transform,
                        name: String::new(),
                        tag: None,
                        material: None,
                        hidden: false,
                    },
                );
                out.created = vec![Element::Instance(id)];
            }
            Command::AddGuideLine { origin, dir } => {
                if dir.length() < EPS {
                    return invalid("a guide line needs a direction");
                }
                let id = GuideId(doc.reserve_id());
                doc.def_mut(def)
                    .expect("checked")
                    .guides
                    .insert(id, Guide::Line { origin, dir: dir.normalize(), hidden: false });
                out.created = vec![Element::Guide(id)];
            }
            Command::AddGuidePoint { point } => {
                let id = GuideId(doc.reserve_id());
                doc.def_mut(def)
                    .expect("checked")
                    .guides
                    .insert(id, Guide::Point { p: point, origin: None, hidden: false });
                out.created = vec![Element::Guide(id)];
            }
            Command::AddGuideSegment { start, point } => {
                if !start.is_finite() || !point.is_finite() {
                    return invalid("Invalid guide coordinates");
                }
                let id = GuideId(doc.reserve_id());
                doc.def_mut(def)
                    .expect("checked")
                    .guides
                    .insert(id, Guide::Point { p: point, origin: Some(start), hidden: false });
                out.created = vec![Element::Guide(id)];
            }
            Command::DeleteGuides => {
                let d = doc.def_mut(def).expect("checked");
                out.changed = !d.guides.is_empty();
                d.guides.clear();
            }
            Command::AddMaterial { name, color, texture } => {
                check_texture(doc, texture.as_ref())?;
                let id = MaterialId(doc.reserve_id());
                doc.materials.insert(id, Material { name, color, texture, glow: 0.0, pbr: Default::default() });
                out.material = Some(id);
            }
            Command::AddTexturedMaterial { name, color, image_name, mime, data, size } => {
                if data.is_empty() || !size.iter().all(|v| v.is_finite() && *v > 0.0) {
                    return invalid("a texture needs image data and a finite positive size");
                }
                let existing = doc
                    .images
                    .iter()
                    .find_map(|(id, image)| (image.mime == mime && image.bytes.as_ref() == &data).then_some(*id));
                let image = if let Some(image) = existing {
                    image
                } else {
                    let id = rysmo_mesh::ImageId(doc.reserve_id());
                    doc.images.insert(id, Image { name: image_name, mime, bytes: Arc::new(data) });
                    id
                };
                let material = MaterialId(doc.reserve_id());
                doc.materials.insert(
                    material,
                    Material {
                        name,
                        color,
                        texture: Some(Texture { image, size }),
                        glow: 0.0,
                        pbr: Default::default(),
                    },
                );
                out.image = Some(image);
                out.material = Some(material);
            }
            Command::AddImage { name, mime, data } => {
                if data.is_empty() {
                    return invalid("the image is empty");
                }
                let id = ImageId(doc.reserve_id());
                doc.images.insert(id, Image { name, mime, bytes: Arc::new(data) });
                out.image = Some(id);
            }
            Command::SetMaterialTexture { material, texture } => {
                check_texture(doc, texture.as_ref())?;
                let m = doc
                    .materials
                    .get_mut(&material)
                    .ok_or_else(|| CommandError::Invalid(format!("no material {material}")))?;
                m.texture = texture;
            }
            Command::AddNormalMap { material, name, mime, data } => {
                if !doc.materials.contains_key(&material)
                    || !["image/png", "image/jpeg"].contains(&mime.as_str())
                    || data.is_empty()
                    || data.len() > 32 * 1024 * 1024
                {
                    return invalid("Missing material or invalid normal map image");
                }
                let image = ImageId(doc.reserve_id());
                doc.images.insert(image, Image { name, mime, bytes: Arc::new(data) });
                doc.materials.get_mut(&material).unwrap().pbr.normal_map = Some(image);
            }
            Command::ImportPhoto { name, mime, data, size } => {
                let image = ImageId(doc.reserve_id());
                let setup = crate::photo::Setup::new(image, size);
                if !setup.valid()
                    || !["image/png", "image/jpeg"].contains(&mime.as_str())
                    || data.is_empty()
                    || data.len() > 32 * 1024 * 1024
                {
                    return invalid("Invalid photograph or image dimensions");
                }
                doc.images.insert(image, Image { name, mime, bytes: Arc::new(data) });
                let previous = doc.photo_match.replace(setup).map(|p| p.image);
                drop_unused_image(doc, previous);
            }
            Command::SetPhotoMatch { setup } => {
                if setup.as_ref().is_some_and(|setup| !setup.valid() || !doc.images.contains_key(&setup.image)) {
                    return invalid("Invalid photo matching settings or missing image");
                }
                out.changed = doc.photo_match != setup;
                let previous = doc.photo_match.as_ref().map(|p| p.image);
                doc.photo_match = setup;
                drop_unused_image(doc, previous);
            }
            Command::SetEnvironment { name, mime, data } => {
                if !ENVIRONMENT_MIMES.contains(&mime.as_str()) || data.is_empty() || data.len() > 64 * 1024 * 1024 {
                    return invalid("Environments must be Radiance HDR, PNG or JPEG images up to 64 MB");
                }
                let image = ImageId(doc.reserve_id());
                doc.images.insert(image, Image { name, mime, bytes: Arc::new(data) });
                let previous = doc.style.environment_image.replace(image);
                if doc.style.lighting == crate::Lighting::Flat {
                    doc.style.lighting = crate::Lighting::Studio;
                }
                drop_unused_image(doc, previous);
                out.image = Some(image);
            }
            Command::SetMaterialPbr { material, pbr } => {
                if !pbr.valid() || pbr.images().any(|id| !doc.images.contains_key(&id)) {
                    return invalid("Invalid surface values or missing surface image");
                }
                let m =
                    doc.materials.get_mut(&material).ok_or_else(|| CommandError::Invalid("Missing material".into()))?;
                out.changed = m.pbr != pbr;
                m.pbr = pbr;
            }
            Command::SetMaterialGlow { material, glow } => {
                if !glow.is_finite() || !(0.0..=MAX_GLOW).contains(&glow) {
                    return invalid("a glow must be between 0 and 1000000 cd/m²");
                }
                let m = doc
                    .materials
                    .get_mut(&material)
                    .ok_or_else(|| CommandError::Invalid(format!("no material {material}")))?;
                m.glow = glow;
            }
            Command::SetTexturePlacement { faces, placement } => {
                if let Some(p) = placement
                    && !crate::texture::valid_placement(&p)
                {
                    return invalid("a texture placement needs finite values, nonzero scale and a convex pin layout");
                }
                let d = doc.def_mut(def).expect("checked");
                out.changed = false;
                for f in faces {
                    let face = d.mesh.face_mut(f).ok_or_else(|| CommandError::Invalid(format!("no face {f}")))?;
                    out.changed |= face.uv != placement;
                    face.uv = placement;
                }
            }
            Command::CreateSectionSlice { id } => {
                let plane = doc
                    .def(def)
                    .unwrap()
                    .sections
                    .planes
                    .get(&id)
                    .ok_or_else(|| CommandError::Invalid("section plane is missing".into()))?;
                let lines = crate::sections::slice(doc, def, plane)
                    .map_err(CommandError::Invalid)?
                    .into_iter()
                    .filter(|line| line[0].distance(line[1]) > EPS)
                    .collect::<Vec<_>>();
                if lines.is_empty() {
                    return invalid("section plane does not intersect geometry");
                }
                let mut mesh = Mesh::new();
                mesh.ensure_next_id(doc.next_id);
                mesh.add_lines(&lines.into_iter().map(|line| (line[0], line[1])).collect::<Vec<_>>(), None)?;
                doc.next_id = doc.next_id.max(mesh.next_id());
                let mesh = Mesh::from_parts(
                    mesh.vertices().map(|(id, v)| (id, v.clone())).collect(),
                    mesh.edges().map(|(id, e)| (id, e.clone())).collect(),
                    BTreeMap::new(),
                )?;
                let definition = DefinitionId(doc.reserve_id());
                doc.definitions.insert(
                    definition,
                    Definition { name: "Section Slice".into(), group: true, mesh, ..Default::default() }.into(),
                );
                let instance = InstanceId(doc.reserve_id());
                doc.def_mut(def).unwrap().instances.insert(
                    instance,
                    Instance {
                        glued_to: None,
                        definition,
                        transform: DMat4::IDENTITY,
                        name: "Section Slice".into(),
                        tag: None,
                        material: None,
                        hidden: false,
                    },
                );
                out.created.push(Element::Instance(instance));
            }
            Command::AddSection { plane } => {
                if !plane.valid() {
                    return invalid("invalid section plane");
                }
                let id = doc.reserve_id();
                let sections = &mut doc.def_mut(def).unwrap().sections;
                sections.planes.insert(id, plane);
                sections.active = Some(id);
                out.created.push(Element::Section(id));
            }
            Command::SetSection { id, plane } => {
                let sections = &mut doc.def_mut(def).unwrap().sections;
                if !sections.planes.contains_key(&id) {
                    return invalid("section plane is missing");
                }
                if let Some(plane) = plane {
                    if !plane.valid() {
                        return invalid("invalid section plane");
                    }
                    out.changed = sections.planes.get(&id) != Some(&plane);
                    sections.planes.insert(id, plane);
                } else {
                    sections.planes.remove(&id);
                    if sections.active == Some(id) {
                        sections.active = None;
                    }
                }
            }
            Command::ActivateSection { id } => {
                let sections = &mut doc.def_mut(def).unwrap().sections;
                if id.is_some_and(|id| !sections.planes.contains_key(&id)) {
                    return invalid("section plane is missing");
                }
                out.changed = sections.active != id;
                sections.active = id;
            }
            Command::IntersectFaces { elements, with_model, context } => {
                if doc
                    .resolve(&context)
                    .map_err(|error| CommandError::Invalid(format!("invalid intersection context: {error}")))?
                    != def
                {
                    return invalid("intersection context does not match the editor");
                }
                let lines =
                    crate::intersect::lines(doc, &context, &elements, with_model).map_err(CommandError::Invalid)?;
                let before = doc.def(def).unwrap().mesh.clone();
                if lines.is_empty() {
                    out.changed = false;
                } else {
                    out.created = mesh_op(doc, &mut |m| m.add_lines(&lines, None).map(edges))?;
                    out.created.extend(new_faces(doc));
                    out.changed = before != doc.def(def).unwrap().mesh;
                }
            }
            Command::AddScene { scene } => {
                settings::add_scene(doc, scene)?;
            }
            Command::SetScene { id, scene } => {
                settings::set_scene(doc, &mut out, id, scene)?;
            }
            Command::RestoreScene { id } => {
                settings::restore_scene(doc, id)?;
            }
            Command::ReorderScene { id, index } => {
                settings::reorder_scene(doc, &mut out, id, index)?;
            }
            Command::SetStyle { style } => {
                settings::set_style(doc, &mut out, style)?;
            }
            Command::SetMaterial { material, name, color } => {
                let m = doc
                    .materials
                    .get_mut(&material)
                    .ok_or_else(|| CommandError::Invalid(format!("no material {material}")))?;
                if let Some(n) = name {
                    m.name = n;
                }
                if let Some(c) = color {
                    m.color = c;
                }
            }
            Command::Paint { faces, material, back } => {
                check_material(doc, material)?;
                doc.with_mesh(def, |m| faces.iter().try_for_each(|f| m.paint(*f, material, back))).expect("checked")?;
            }
            Command::PaintWrappedElements { elements, material, back, source, placement } => {
                check_material(doc, material)?;
                let size = material
                    .and_then(|m| doc.materials.get(&m))
                    .and_then(|m| m.texture)
                    .ok_or_else(|| CommandError::Invalid("Wrapping requires a textured material.".into()))?
                    .size;
                let faces = elements
                    .iter()
                    .filter_map(|e| if let Element::Face(f) = e { Some(*f) } else { None })
                    .collect::<Vec<_>>();
                let placements =
                    crate::texture::wrapped_placements(&doc.def(def).unwrap().mesh, source, &faces, size, placement)
                        .map_err(CommandError::Invalid)?;
                let result = Command::PaintElements { elements, material, back }.run(doc, def)?;
                for (face, placement) in placements {
                    doc.def_mut(def).unwrap().mesh.face_mut(face).unwrap().uv = Some(placement);
                }
                return Ok(result);
            }
            Command::PaintPositionedElements { elements, material, back, placement } => {
                let faces =
                    elements.iter().filter_map(|e| if let Element::Face(f) = e { Some(*f) } else { None }).collect();
                let outcome = Command::PaintElements { elements, material, back }.run(doc, def)?;
                Command::SetTexturePlacement { faces, placement: Some(placement) }.run(doc, def)?;
                return Ok(outcome);
            }
            Command::PaintElements { elements, material, back } => {
                check_material(doc, material)?;
                let faces: Vec<_> =
                    elements.iter().filter_map(|e| if let Element::Face(f) = e { Some(*f) } else { None }).collect();
                let instances: Vec<_> = elements
                    .iter()
                    .filter_map(|e| if let Element::Instance(i) = e { Some(*i) } else { None })
                    .collect();
                let edges: Vec<_> =
                    elements.iter().filter_map(|e| if let Element::Edge(id) = e { Some(*id) } else { None }).collect();
                if faces.is_empty() && instances.is_empty() && edges.is_empty() {
                    return invalid("Select edges, faces, groups or components to paint.");
                }
                doc.with_mesh(def, |m| {
                    faces.iter().try_for_each(|f| m.paint(*f, material, back))?;
                    edges.iter().try_for_each(|e| m.paint_edge(*e, material))
                })
                .expect("checked")?;
                let d = doc.def_mut(def).expect("checked");
                for id in instances {
                    d.instances
                        .get_mut(&id)
                        .ok_or_else(|| CommandError::Invalid(format!("no instance {id}")))?
                        .material = material;
                }
            }
            Command::PaintInstances { instances, material } => {
                check_material(doc, material)?;
                let d = doc.def_mut(def).expect("checked");
                for i in instances {
                    d.instances
                        .get_mut(&i)
                        .ok_or_else(|| CommandError::Invalid(format!("no instance {i}")))?
                        .material = material;
                }
            }
            Command::AddTag { name } => {
                if name.trim().is_empty() {
                    return invalid("a tag needs a name");
                }
                let id = TagId(doc.reserve_id());
                doc.tags.insert(id, Tag { name, visible: true });
                out.tag = Some(id);
            }
            Command::SetTag { tag, name, visible } => {
                let t = doc.tags.get_mut(&tag).ok_or_else(|| CommandError::Invalid(format!("no tag {tag}")))?;
                if let Some(n) = name {
                    t.name = n;
                }
                if let Some(v) = visible {
                    t.visible = v;
                }
            }
            Command::DeleteTag { tag } => {
                if doc.tags.remove(&tag).is_none() {
                    return invalid(format!("no tag {tag}"));
                }
                for d in doc.definitions_mut_where(|d| d.instances.values().any(|i| i.tag == Some(tag))) {
                    for i in d.instances.values_mut() {
                        if i.tag == Some(tag) {
                            i.tag = None;
                        }
                    }
                }
            }
            Command::AssignTag { instances, tag } => {
                if let Some(t) = tag
                    && !doc.tags.contains_key(&t)
                {
                    return invalid(format!("no tag {t}"));
                }
                let d = doc.def_mut(def).expect("checked");
                for i in instances {
                    d.instances.get_mut(&i).ok_or_else(|| CommandError::Invalid(format!("no instance {i}")))?.tag = tag;
                }
            }
            Command::SetMeasurementSettings { settings } => {
                settings::set_measurement_settings(doc, &mut out, settings)?;
            }
            Command::SetLengthPrecision { precision } => {
                settings::set_length_precision(doc, &mut out, precision)?;
            }
            Command::SetUnits { units } => {
                settings::set_units(doc, &mut out, units)?;
            }
        }
        crate::components::refresh_glue(doc, &glued_before);
        crate::components::validate_openings(doc)?;
        purge_unused(doc);
        Ok(out)
    }
}
fn check_texture(doc: &Document, texture: Option<&Texture>) -> Result<(), CommandError> {
    match texture {
        Some(t) if !doc.images.contains_key(&t.image) => invalid(format!("no image {}", t.image)),
        Some(t) if !t.size.iter().all(|v| v.is_finite() && *v > 0.0) => {
            invalid("a texture needs a finite positive size")
        }
        _ => Ok(()),
    }
}

fn check_material(doc: &Document, material: Option<MaterialId>) -> Result<(), CommandError> {
    match material {
        Some(m) if !doc.materials.contains_key(&m) => invalid(format!("no material {m}")),
        _ => Ok(()),
    }
}

/// Image types an environment panorama can come in.
pub const ENVIRONMENT_MIMES: [&str; 3] = ["image/vnd.radiance", "image/png", "image/jpeg"];

/// Removes an image nothing uses any more.
pub(super) fn drop_unused_image(doc: &mut Document, previous: Option<ImageId>) {
    if let Some(image) = previous
        && doc.photo_match.as_ref().is_none_or(|p| p.image != image)
        && !doc.materials.values().any(|m| m.images().any(|i| i == image))
        && doc.style.environment_image != Some(image)
    {
        doc.images.remove(&image);
    }
}
