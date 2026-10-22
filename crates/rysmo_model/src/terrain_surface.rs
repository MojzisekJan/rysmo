//! Indexed height/material lookup on an existing triangular terrain domain.
use rysmo_math::{DVec2, DVec3};
use rysmo_mesh::{Edge, EdgeId, FaceId, Mesh, Vertex, VertexId};
use spade::{ConstrainedDelaunayTriangulation, HasPosition, Point2, PositionInTriangulation, Triangulation};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Copy)]
struct Sample {
    site: Point2<f64>,
    world: DVec3,
}
impl HasPosition for Sample {
    type Scalar = f64;
    fn position(&self) -> Point2<f64> {
        self.site
    }
}
pub struct TerrainSurface {
    cdt: ConstrainedDelaunayTriangulation<Sample>,
    regions: BTreeMap<usize, FaceId>,
    origin: DVec2,
    scale: f64,
}
// Predicate coordinates use 44 binary fractional bits in normalized terrain
// space. This absorbs roundoff from composed drawing-axes transforms while
// keeping source world coordinates and element identities unchanged.
fn predicate_coordinate(value: f64) -> f64 {
    const GRID: f64 = (1u64 << 44) as f64;
    (value * GRID).round() / GRID
}
fn rings_touch_or_cross(ring: &[DVec2], other: &[DVec2]) -> bool {
    for i in 0..ring.len() {
        for j in 0..other.len() {
            let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
            let (c, d) = (other[j], other[(j + 1) % other.len()]);
            let (ab, cd) = (b - a, d - c);
            let cross = ab.perp_dot(cd);
            if cross != 0. {
                let t = (c - a).perp_dot(cd) / cross;
                let u = (c - a).perp_dot(ab) / cross;
                if (0. ..=1.).contains(&t) && (0. ..=1.).contains(&u) {
                    return true;
                }
            } else if ab.perp_dot(c - a) == 0. {
                let t = (c - a).dot(ab) / ab.length_squared();
                let u = (d - a).dot(ab) / ab.length_squared();
                if t.min(u) <= 1. && t.max(u) >= 0. {
                    return true;
                }
            }
        }
    }
    false
}
impl TerrainSurface {
    pub fn new(mesh: &Mesh) -> Result<Self, String> {
        if mesh.faces().next().is_none() || mesh.faces().any(|(_, f)| f.outer.len() != 3 || !f.holes.is_empty()) {
            return Err("A triangular terrain is required.".into());
        }
        let vertices = mesh.faces().flat_map(|(_, f)| f.outer.iter().copied()).collect::<BTreeSet<_>>();
        let lo = vertices.iter().fold(DVec3::splat(f64::INFINITY), |a, id| a.min(mesh.pos(*id)));
        let hi = vertices.iter().fold(DVec3::splat(f64::NEG_INFINITY), |a, id| a.max(mesh.pos(*id)));
        let scale = (hi.x - lo.x).max(hi.y - lo.y);
        if !scale.is_finite() || scale < 1e-7 {
            return Err("Terrain must span a horizontal area.".into());
        }
        // Binary scaling is exact: arbitrary division can turn a point on an
        // existing diagonal into a microscopic off-edge triangle in predicate space.
        let scale = 2f64.powi(scale.log2().ceil() as i32);
        if !scale.is_finite() {
            return Err("Terrain extent is too large.".into());
        }
        let origin = DVec2::new(lo.x, lo.y);
        let project = |p: DVec3| {
            Point2::new(predicate_coordinate((p.x - lo.x) / scale), predicate_coordinate((p.y - lo.y) / scale))
        };
        let mut cdt = ConstrainedDelaunayTriangulation::<Sample>::new();
        let mut handles = BTreeMap::new();
        let mut sites: BTreeMap<(u64, u64), f64> = BTreeMap::new();
        for id in vertices {
            let world = mesh.pos(id);
            if !world.is_finite() {
                return Err("Terrain contains a non-finite point.".into());
            }
            let site = project(world);
            let bits = |v: f64| if v == 0. { 0f64.to_bits() } else { v.to_bits() };
            let key = (bits(site.x), bits(site.y));
            if sites.insert(key, world.z).is_some() {
                return Err("Terrain vertices share a horizontal position within predicate precision.".into());
            }
            let handle = cdt.insert(Sample { site, world }).map_err(|e| e.to_string())?;
            handles.insert(id, handle);
        }
        let mut source_faces = BTreeMap::new();
        for (id, face) in mesh.faces() {
            let mut key = face.outer.iter().map(|id| handles[id].index()).collect::<Vec<_>>();
            key.sort_unstable();
            if key[0] == key[1] || key[1] == key[2] || source_faces.insert(key, id).is_some() {
                return Err("Terrain faces overlap or collapse horizontally.".into());
            }
            for (a, b) in face.loop_edges() {
                let a = handles[&a];
                let b = handles[&b];
                if !cdt.can_add_constraint(a, b) {
                    return Err("Terrain triangles overlap in projection.".into());
                }
                cdt.add_constraint(a, b);
            }
        }
        let mut regions = BTreeMap::new();
        for face in cdt.inner_faces() {
            let mut key = face.vertices().iter().map(|v| v.fix().index()).collect::<Vec<_>>();
            key.sort_unstable();
            if let Some(id) = source_faces.get(&key) {
                regions.insert(face.fix().index(), *id);
            }
        }
        if regions.len() != source_faces.len() {
            return Err("Terrain triangulation is not conforming.".into());
        }
        Ok(Self { cdt, regions, origin, scale })
    }
    /// Splits the terrain along projected segments, preserving its original domain
    /// and face attributes. Existing geometry retains IDs; new IDs start at
    /// `first_id` or the mesh allocation cursor, whichever is larger. This kernel does not
    /// replace document geometry or update annotation references.
    pub fn imprint(mesh: &Mesh, segments: &[[DVec2; 2]], first_id: u64) -> Result<Mesh, String> {
        let surface = Self::new(mesh)?;
        if segments.iter().flatten().any(|p| !p.is_finite()) {
            return Err("Imprint contains a non-finite point.".into());
        }
        let mut cdt = surface.cdt.clone();
        for [a, b] in segments {
            if a.distance_squared(*b) == 0. {
                return Err("Imprint segment has no length.".into());
            }
            let mut ends = Vec::new();
            for point in [a, b] {
                let site = ((*point - surface.origin) / surface.scale).map(predicate_coordinate);
                if let PositionInTriangulation::OnVertex(handle) = cdt.locate(Point2::new(site.x, site.y)) {
                    ends.push(handle);
                    continue;
                }
                let z = surface.sample(*point).map(|v| v.0).unwrap_or(0.);
                ends.push(
                    cdt.insert(Sample { site: Point2::new(site.x, site.y), world: DVec3::new(point.x, point.y, z) })
                        .map_err(|e| e.to_string())?,
                );
            }
            if ends[0] == ends[1] {
                return Err("Imprint segment is smaller than predicate precision.".into());
            }
            cdt.add_constraint_and_split(ends[0], ends[1], |site| {
                let xy = surface.origin + DVec2::new(site.x, site.y) * surface.scale;
                let z = surface.sample(xy).map(|v| v.0).unwrap_or(0.);
                Sample { site, world: DVec3::new(xy.x, xy.y, z) }
            });
        }
        let mut next = first_id.max(mesh.next_id()).max(1);
        let mut originals = BTreeMap::new();
        for (id, vertex) in mesh.vertices() {
            let xy =
                ((DVec2::new(vertex.pos.x, vertex.pos.y) - surface.origin) / surface.scale).map(predicate_coordinate);
            if let PositionInTriangulation::OnVertex(handle) = cdt.locate(Point2::new(xy.x, xy.y)) {
                originals.entry(handle.index()).or_insert(id);
            }
        }
        let mut vertices = BTreeMap::new();
        let mut mapping = BTreeMap::new();
        let mut faces = BTreeMap::new();
        for face in cdt.inner_faces() {
            let points = face.vertices().map(|v| v.data().world);
            let centre = (points[0] + points[1] + points[2]) / 3.;
            let Some((_, source)) = surface.sample(DVec2::new(centre.x, centre.y)) else { continue };
            let mut value = mesh.face(source).unwrap().clone();
            value.outer.clear();
            for vertex in face.vertices() {
                let index = vertex.fix().index();
                let id = *mapping.entry(index).or_insert_with(|| {
                    let id = originals.get(&index).copied().unwrap_or_else(|| {
                        let id = VertexId(next);
                        next += 1;
                        id
                    });
                    vertices.insert(id, Vertex { pos: vertex.data().world });
                    id
                });
                value.outer.push(id);
            }
            let mut original = mesh.face(source).unwrap().outer.clone();
            let mut replacement = value.outer.clone();
            original.sort_unstable();
            replacement.sort_unstable();
            let id = if original == replacement {
                source
            } else {
                let id = FaceId(next);
                next += 1;
                id
            };
            faces.insert(id, value);
        }
        let mut edges = BTreeMap::new();
        let mut counts = BTreeMap::new();
        for face in faces.values() {
            for (a, b) in face.loop_edges() {
                let key = if a < b { (a, b) } else { (b, a) };
                *counts.entry(key).or_insert(0) += 1;
            }
        }
        for ((a, b), count) in counts {
            let mut edge = Edge::new(a, b);
            edge.soft = count == 2;
            edge.smooth = edge.soft;
            let id = if let Some(id) = mesh.edge_between(a, b) {
                edge = mesh.edge(id).unwrap().clone();
                id
            } else {
                let id = EdgeId(next);
                next += 1;
                id
            };
            edges.insert(id, edge);
        }
        let mut curves = BTreeMap::new();
        for (curve_id, original) in mesh.curves() {
            let source = mesh.curve_edges(curve_id);
            let complete = source.iter().all(|edge| edges.contains_key(edge));
            if !complete {
                for source_id in &source {
                    let source_edge = mesh.edge(*source_id).unwrap();
                    let a = mesh.pos(source_edge.v[0]);
                    let b = mesh.pos(source_edge.v[1]);
                    let direction = b - a;
                    let length_squared = direction.length_squared();
                    if length_squared == 0. {
                        continue;
                    }
                    let on_segment = |p: DVec3| {
                        let t = (p - a).dot(direction) / length_squared;
                        (-1e-8..=1. + 1e-8).contains(&t)
                            && p.distance(a + direction * t) <= 1e-8 * direction.length().max(1.)
                    };
                    for edge in edges.values_mut() {
                        if edge.v.iter().all(|id| on_segment(vertices[id].pos)) {
                            edge.curve = Some(curve_id);
                            edge.soft = source_edge.soft;
                            edge.smooth = source_edge.smooth;
                            edge.hidden = source_edge.hidden;
                        }
                    }
                }
            }
            let count = edges.values().filter(|edge| edge.curve == Some(curve_id)).count();
            if count > 0 {
                let mut curve = *original;
                if !complete {
                    curve.kind = rysmo_mesh::CurveKind::Polyline;
                }
                curve.segments = count as u32;
                curves.insert(curve_id, curve);
            }
        }
        Mesh::from_parts_with_curves(vertices, edges, faces, curves).map_err(|e| e.to_string())
    }

    /// Projects segments onto the original terrain, split at triangle boundaries
    /// and clipped to its domain. Source mesh and terrain heights remain unchanged.
    pub fn drape(mesh: &Mesh, segments: &[[DVec2; 2]]) -> Result<Vec<[DVec3; 2]>, String> {
        let imprinted = Self::imprint(mesh, segments, mesh.next_id())?;
        let on_segment = |p: DVec3, a: DVec2, b: DVec2| {
            let p = DVec2::new(p.x, p.y);
            let direction = b - a;
            let t = (p - a).dot(direction) / direction.length_squared();
            (-1e-8..=1. + 1e-8).contains(&t) && p.distance(a + direction * t) <= 1e-8 * direction.length().max(1.)
        };
        Ok(imprinted
            .edges()
            .filter_map(|(_, edge)| {
                let points = edge.v.map(|id| imprinted.pos(id));
                segments.iter().any(|[a, b]| points.iter().all(|p| on_segment(*p, *a, *b))).then_some(points)
            })
            .collect())
    }

    /// Drape only downwards from actual source heights. Source/terrain crossings
    /// split output pieces; overlapping sources contribute the union of valid parts.
    pub fn drape_downward(mesh: &Mesh, sources: &[[DVec3; 2]]) -> Result<Vec<[DVec3; 2]>, String> {
        if sources.iter().flatten().any(|p| !p.is_finite()) {
            return Err("Drape source is not finite.".into());
        }
        let xy = |p: DVec3| DVec2::new(p.x, p.y);
        let projected = sources
            .iter()
            .map(|line| line.map(xy))
            .filter(|line| line[0].distance_squared(line[1]) > 1e-20)
            .collect::<Vec<_>>();
        if projected.is_empty() {
            return Ok(Vec::new());
        }
        let pieces = Self::drape(mesh, &projected)?;
        let mut result = Vec::new();
        for [a, b] in pieces {
            let mut intervals = Vec::new();
            for [start, end] in sources {
                let direction = xy(*end) - xy(*start);
                let length_squared = direction.length_squared();
                if length_squared <= 1e-20 {
                    continue;
                }
                let parameter = |p: DVec3| (xy(p) - xy(*start)).dot(direction) / length_squared;
                let ta = parameter(a);
                let tb = parameter(b);
                let on = |p: DVec3, t: f64| {
                    (-1e-8..=1. + 1e-8).contains(&t)
                        && xy(p).distance(xy(*start) + direction * t) <= 1e-8 * direction.length().max(1.)
                };
                if !on(a, ta) || !on(b, tb) {
                    continue;
                }
                let da = start.z + (end.z - start.z) * ta - a.z;
                let db = start.z + (end.z - start.z) * tb - b.z;
                match (da >= -1e-8, db >= -1e-8) {
                    (true, true) => intervals.push((0., 1.)),
                    (false, false) => {}
                    (true, false) => intervals.push((0., (da / (da - db)).clamp(0., 1.))),
                    (false, true) => intervals.push(((da / (da - db)).clamp(0., 1.), 1.)),
                }
            }
            intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut merged: Vec<(f64, f64)> = Vec::new();
            for (start, end) in intervals {
                if let Some(last) = merged.last_mut()
                    && start <= last.1 + 1e-10
                {
                    last.1 = last.1.max(end);
                } else {
                    merged.push((start, end));
                }
            }
            for (start, end) in merged {
                let points = [a.lerp(b, start), a.lerp(b, end)];
                if points[0].distance(points[1]) >= 1e-7 {
                    result.push(points);
                }
            }
        }
        Ok(result)
    }

    /// Stamps a simple projected outline at a fixed elevation with a mitred
    /// transition boundary. Use stamp_rings for compound outlines.
    pub fn stamp_outline(
        mesh: &Mesh,
        outline: &[DVec2],
        offset: f64,
        height: f64,
        first_id: u64,
    ) -> Result<Mesh, String> {
        Self::stamp_rings(mesh, &[outline.to_vec()], offset, height, first_id)
    }

    /// Validated mitred transition loops shared by the preview and terrain kernel.
    pub fn transition_rings(outlines: &[Vec<DVec2>], offset: f64) -> Result<Vec<Vec<DVec2>>, String> {
        if outlines.is_empty()
            || !offset.is_finite()
            || offset <= 0.
            || outlines.iter().any(|ring| ring.len() < 3 || ring.iter().any(|p| !p.is_finite()))
        {
            return Err("Stamp requires finite outlines, positive offset and finite height.".into());
        }
        let contains = |point: DVec2, ring: &[DVec2]| {
            let mut inside = false;
            for i in 0..ring.len() {
                let a = ring[i];
                let b = ring[(i + 1) % ring.len()];
                if (a.y > point.y) != (b.y > point.y) && point.x < (b.x - a.x) * (point.y - a.y) / (b.y - a.y) + a.x {
                    inside = !inside;
                }
            }
            inside
        };
        for (index, ring) in outlines.iter().enumerate() {
            if outlines[index + 1..].iter().any(|other| rings_touch_or_cross(ring, other)) {
                return Err("Stamp outlines intersect or touch.".into());
            }
        }
        let mut transitions: Vec<Vec<DVec2>> = Vec::new();
        for (index, outline) in outlines.iter().enumerate() {
            let area = rysmo_math::polygon_area_2d(outline);
            if area.abs() < 1e-14 {
                return Err("Stamp outline has no area.".into());
            }
            let depth = outlines
                .iter()
                .enumerate()
                .filter(|(other, ring)| *other != index && contains(outline[0], ring))
                .count();
            let points = outline.iter().map(|p| DVec3::new(p.x, p.y, 0.)).collect::<Vec<_>>();
            rysmo_mesh::offset_polyline(&points, DVec3::Z, 0., true).ok_or("Stamp outline intersects itself.")?;
            let direction = if depth % 2 == 0 { -1. } else { 1. };
            let Some(expanded) =
                rysmo_mesh::offset_polyline(&points, DVec3::Z, direction * offset * area.signum(), true)
            else {
                return Self::transition_union(outlines, offset);
            };
            transitions.push(expanded.iter().map(|p| DVec2::new(p.x, p.y)).collect());
        }
        for (index, ring) in transitions.iter().enumerate() {
            for (other_index, other) in transitions.iter().enumerate().skip(index + 1) {
                if rings_touch_or_cross(ring, other)
                    || (contains(ring[0], other) && !contains(outlines[index][0], &outlines[other_index]))
                    || (contains(other[0], ring) && !contains(outlines[other_index][0], &outlines[index]))
                {
                    return Self::transition_union(outlines, offset);
                }
            }
        }
        Ok(transitions)
    }

    /// Buffer the filled footprint by uniting outward edge strips and mitre corners.
    /// Unlike offsetting each loop alone, this lets narrow holes close and concave
    /// transition regions merge without creating a self-intersecting boundary.
    fn transition_union(outlines: &[Vec<DVec2>], offset: f64) -> Result<Vec<Vec<DVec2>>, String> {
        let contains = |point: DVec2, ring: &[DVec2]| {
            let mut inside = false;
            for i in 0..ring.len() {
                let a = ring[i];
                let b = ring[(i + 1) % ring.len()];
                if (a.y > point.y) != (b.y > point.y) && point.x < (b.x - a.x) * (point.y - a.y) / (b.y - a.y) + a.x {
                    inside = !inside;
                }
            }
            inside
        };
        let depths = outlines
            .iter()
            .enumerate()
            .map(|(index, ring)| {
                outlines.iter().enumerate().filter(|(other, outer)| *other != index && contains(ring[0], outer)).count()
            })
            .collect::<Vec<_>>();
        let mut polygons = Vec::new();
        for (index, outline) in outlines.iter().enumerate() {
            let points = outline.iter().map(|point| DVec3::new(point.x, point.y, 0.)).collect::<Vec<_>>();
            rysmo_mesh::offset_polyline(&points, DVec3::Z, 0., true).ok_or("Stamp outline intersects itself.")?;
            if depths[index] % 2 == 0 {
                let mut polygon = vec![outline.clone()];
                polygon.extend(
                    outlines
                        .iter()
                        .enumerate()
                        .filter(|(other, hole)| depths[*other] == depths[index] + 1 && contains(hole[0], outline))
                        .map(|(_, hole)| hole.clone()),
                );
                polygons.push(polygon);
            }
            let mut ring = outline.clone();
            let area = rysmo_math::polygon_area_2d(&ring);
            if area.abs() < 1e-14 {
                return Err("Stamp outline has no area.".into());
            }
            if (area > 0.) != (depths[index] % 2 == 0) {
                ring.reverse();
            }
            for i in 0..ring.len() {
                let p = ring[i];
                let next = ring[(i + 1) % ring.len()];
                let direction = (next - p).normalize();
                let normal = DVec2::new(direction.y, -direction.x);
                polygons.push(vec![vec![p, next, next + normal * offset, p + normal * offset]]);
                let previous = (p - ring[(i + ring.len() - 1) % ring.len()]).normalize();
                let turn = previous.perp_dot(direction);
                if turn > 1e-14 {
                    let a = p + DVec2::new(previous.y, -previous.x) * offset;
                    let b = p + normal * offset;
                    let corner = a + previous * ((b - a).perp_dot(direction) / turn);
                    polygons.push(vec![vec![p, a, corner, b]]);
                }
            }
        }
        crate::terrain_footprint::projected_union(&polygons)
    }

    /// Stamps disjoint and nested simple loops using even/odd filling.
    pub fn stamp_rings(
        mesh: &Mesh,
        outlines: &[Vec<DVec2>],
        offset: f64,
        height: f64,
        first_id: u64,
    ) -> Result<Mesh, String> {
        if !height.is_finite() {
            return Err("Stamp height is not finite.".into());
        }
        let transitions = Self::transition_rings(outlines, offset)?;
        let mut segments = Vec::new();
        for ring in outlines.iter().chain(transitions.iter()) {
            for i in 0..ring.len() {
                segments.push([ring[i], ring[(i + 1) % ring.len()]]);
            }
        }
        let mut result = Self::imprint(mesh, &segments, first_id)?;
        result.map_all(|p| {
            let xy = DVec2::new(p.x, p.y);
            let mut inside = false;
            let mut distance = f64::INFINITY;
            for outline in outlines {
                for i in 0..outline.len() {
                    let a = outline[i];
                    let b = outline[(i + 1) % outline.len()];
                    if (a.y > xy.y) != (b.y > xy.y) && xy.x < (b.x - a.x) * (xy.y - a.y) / (b.y - a.y) + a.x {
                        inside = !inside;
                    }
                    let direction = b - a;
                    let t = ((xy - a).dot(direction) / direction.length_squared()).clamp(0., 1.);
                    distance = distance.min(xy.distance(a + t * direction));
                }
            }
            let weight = if inside { 1. } else { (1. - distance / offset).clamp(0., 1.) };
            DVec3::new(p.x, p.y, p.z + (height - p.z) * weight)
        });
        let mut curves = result.curves().map(|(id, curve)| (id, *curve)).collect::<BTreeMap<_, _>>();
        for (id, curve) in &mut curves {
            let members =
                result.curve_edges(*id).iter().flat_map(|edge| result.edge(*edge).unwrap().v).collect::<BTreeSet<_>>();
            let displacements = members
                .iter()
                .filter_map(|id| mesh.vertex(*id).map(|v| result.pos(*id).z - v.pos.z))
                .collect::<Vec<_>>();
            if let Some(first) = displacements.first()
                && displacements.iter().any(|delta| (delta - first).abs() > 1e-7)
            {
                curve.kind = rysmo_mesh::CurveKind::Polyline;
            }
        }
        Mesh::from_parts_with_curves(
            result.vertices().map(|(id, v)| (id, v.clone())).collect(),
            result.edges().map(|(id, e)| (id, e.clone())).collect(),
            result.faces().map(|(id, f)| (id, f.clone())).collect(),
            curves,
        )
        .map_err(|e| e.to_string())
    }

    /// Returns height and original face; gaps/holes and exterior positions return None.
    pub fn sample(&self, point: DVec2) -> Option<(f64, FaceId)> {
        if !point.is_finite() {
            return None;
        }
        let p = ((point - self.origin) / self.scale).map(predicate_coordinate);
        if !p.is_finite() {
            return None;
        }
        let site = Point2::new(p.x, p.y);
        let face = match self.cdt.locate(site) {
            PositionInTriangulation::OnFace(face) => Some(face),
            PositionInTriangulation::OnEdge(edge) => {
                let edge = self.cdt.directed_edge(edge);
                [edge.face().as_inner(), edge.rev().face().as_inner()]
                    .into_iter()
                    .flatten()
                    .find(|f| self.regions.contains_key(&f.fix().index()))
                    .map(|f| f.fix())
            }
            PositionInTriangulation::OnVertex(vertex) => self
                .cdt
                .vertex(vertex)
                .out_edges()
                .filter_map(|e| e.face().as_inner())
                .find(|f| self.regions.contains_key(&f.fix().index()))
                .map(|f| f.fix()),
            _ => None,
        }?;
        let source = *self.regions.get(&face.index())?;
        let vertices = self.cdt.face(face).vertices();
        let [a, b, c] = vertices.map(|v| v.data().world);
        let sites = vertices.map(|v| v.position());
        let ab = DVec2::new(sites[1].x - sites[0].x, sites[1].y - sites[0].y);
        let ac = DVec2::new(sites[2].x - sites[0].x, sites[2].y - sites[0].y);
        let ap = p - DVec2::new(sites[0].x, sites[0].y);
        let denominator = ab.perp_dot(ac);
        if denominator == 0. || !denominator.is_finite() {
            return None;
        }
        let u = ap.perp_dot(ac) / denominator;
        let v = ab.perp_dot(ap) / denominator;
        Some((a.z + (b.z - a.z) * u + (c.z - a.z) * v, source))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stamp_transition_merges_neighbouring_footprints() {
        let first = vec![DVec2::ZERO, DVec2::new(2., 0.), DVec2::splat(2.), DVec2::new(0., 2.)];
        let second = first.iter().map(|p| *p + DVec2::new(4., 0.)).collect::<Vec<_>>();
        for offset in [1., 1.5] {
            let transitions = TerrainSurface::transition_rings(&[first.clone(), second.clone()], offset).unwrap();
            assert_eq!(transitions.len(), 1);
            let expected = (6. + 2. * offset) * (2. + 2. * offset);
            assert!((rysmo_math::polygon_area_2d(&transitions[0]).abs() - expected).abs() < 1e-8);
        }
    }

    #[test]
    fn stamp_transition_unites_a_closed_concave_notch() {
        let outline = vec![
            DVec2::new(2., 2.),
            DVec2::new(8., 2.),
            DVec2::new(8., 8.),
            DVec2::new(6., 8.),
            DVec2::new(6., 4.),
            DVec2::new(4., 4.),
            DVec2::new(4., 8.),
            DVec2::new(2., 8.),
        ];
        for offset in [1., 1.5] {
            let transitions = TerrainSurface::transition_rings(std::slice::from_ref(&outline), offset).unwrap();
            assert_eq!(transitions.len(), 1);
            assert!((rysmo_math::polygon_area_2d(&transitions[0]).abs() - (6. + 2. * offset).powi(2)).abs() < 1e-8);
            let mesh = crate::terrain::grid(10., 10., 1.).unwrap();
            TerrainSurface::stamp_rings(&mesh, std::slice::from_ref(&outline), offset, 2., mesh.next_id())
                .unwrap()
                .validate()
                .unwrap();
        }
    }

    #[test]
    fn stamp_transition_can_close_a_small_hole() {
        let outer = vec![DVec2::ZERO, DVec2::new(6., 0.), DVec2::splat(6.), DVec2::new(0., 6.)];
        let hole = vec![DVec2::splat(2.), DVec2::new(2., 4.), DVec2::splat(4.), DVec2::new(4., 2.)];
        for offset in [1., 1.5, 2.] {
            let transitions = TerrainSurface::transition_rings(&[outer.clone(), hole.clone()], offset).unwrap();
            assert_eq!(transitions.len(), 1);
            let area = rysmo_math::polygon_area_2d(&transitions[0]).abs();
            assert!((area - (6. + 2. * offset).powi(2)).abs() < 1e-8);
            let mesh = crate::terrain::grid(10., 10., 1.).unwrap();
            let stamped =
                TerrainSurface::stamp_rings(&mesh, &[outer.clone(), hole.clone()], offset, 2., mesh.next_id()).unwrap();
            stamped.validate().unwrap();
            let height = TerrainSurface::new(&stamped).unwrap().sample(DVec2::splat(3.)).unwrap().0;
            assert!((height - 2. * (1. - 1. / offset)).abs() < 1e-9);
        }
    }

    #[test]
    fn imprint_rejects_segments_collapsed_by_predicate_precision() {
        let mesh = crate::terrain::grid(4., 4., 1.).unwrap();
        let before = mesh.clone();
        let result =
            TerrainSurface::imprint(&mesh, &[[DVec2::new(1., 1.), DVec2::new(1. + f64::EPSILON, 1.)]], mesh.next_id());
        assert!(result.unwrap_err().contains("predicate precision"));
        assert_eq!(mesh, before);
    }
    #[test]
    fn downward_drape_clips_height_crossings_and_unions_overlapping_sources() {
        let mut mesh = crate::terrain::grid(4., 4., 1.).unwrap();
        mesh.map_all(|p| DVec3::new(p.x, p.y, p.x));
        let line = |height| [DVec3::new(0., 1.5, height), DVec3::new(4., 1.5, height)];
        assert!(TerrainSurface::drape_downward(&mesh, &[line(-1.)]).unwrap().is_empty());
        let lengths = |lines: Vec<[DVec3; 2]>| lines.iter().map(|[a, b]| (b.x - a.x).abs()).sum::<f64>();
        assert!((lengths(TerrainSurface::drape_downward(&mesh, &[line(2.)]).unwrap()) - 2.).abs() < 1e-9);
        assert!((lengths(TerrainSurface::drape_downward(&mesh, &[line(1.), line(3.)]).unwrap()) - 3.).abs() < 1e-9);
        let source = [DVec3::new(0., 1.5, 1.), DVec3::new(4., 1.5, -1.)];
        let lines = TerrainSurface::drape_downward(&mesh, &[source]).unwrap();
        assert!((lengths(lines.clone()) - 2. / 3.).abs() < 1e-9);
        assert!(lines.iter().flatten().all(|p| (p.z - p.x).abs() < 1e-9 && p.x <= 2. / 3. + 1e-9));
    }
    #[test]
    fn source_below_terrain_is_a_command_noop() {
        use crate::{Command, Editor, Element};
        let mut editor = Editor::new();
        let out = editor
            .apply(Command::CreateTerrainGrid { origin: DVec3::ZERO, u: DVec3::X * 4., v: DVec3::Y * 4., spacing: 1. })
            .unwrap();
        let Element::Instance(target) = out.created[0] else { panic!() };
        editor
            .apply(Command::DrawLines {
                lines: vec![[DVec3::new(-1., 1.5, -1.), DVec3::new(5., 1.5, -1.)]],
                facing: None,
            })
            .unwrap();
        let edges = editor.mesh().edges().map(|(id, _)| id).collect();
        let before = editor.doc().clone();
        let label = editor.undo_label().map(str::to_owned);
        let out = editor.apply(Command::DrapeEdges { edges, target }).unwrap();
        assert!(!out.changed);
        assert_eq!(editor.doc(), &before);
        assert_eq!(editor.undo_label().map(str::to_owned), label);
    }

    #[test]
    fn drape_follows_triangle_heights_and_clips_boundary_and_hole() {
        let mut mesh = crate::terrain::grid(4., 4., 1.).unwrap();
        mesh.map_all(|p| DVec3::new(p.x, p.y, p.x * p.x * 0.25));
        let hole = mesh.faces().find(|(id, _)| mesh.face_contains(*id, DVec3::new(1.75, 1.5, 0.8125))).unwrap().0;
        mesh.remove_face(hole);
        let before = mesh.clone();
        let lines = TerrainSurface::drape(&mesh, &[[DVec2::new(-1., 1.5), DVec2::new(5., 1.5)]]).unwrap();
        assert_eq!(mesh, before);
        let surface = TerrainSurface::new(&mesh).unwrap();
        let length = lines.iter().map(|[a, b]| (b.x - a.x).abs()).sum::<f64>();
        assert!((length - 3.5).abs() < 1e-9);
        for point in lines.iter().flatten() {
            assert!(point.x >= 0. && point.x <= 4.);
            assert!((surface.sample(DVec2::new(point.x, point.y)).unwrap().0 - point.z).abs() < 1e-9);
        }
    }
    #[test]
    fn drape_group_source_creates_edges_outside_both_unchanged_groups() {
        use crate::{Command, Editor, Element};
        let mut editor = Editor::new();
        let out = editor
            .apply(Command::CreateTerrainGrid {
                origin: DVec3::new(2., 3., 4.),
                u: DVec3::X * 4.,
                v: DVec3::Y * 4.,
                spacing: 1.,
            })
            .unwrap();
        let Element::Instance(target) = out.created[0] else { panic!() };
        editor
            .apply(Command::DrawLines {
                lines: vec![[DVec3::new(1., 4.5, 10.), DVec3::new(7., 4.5, 10.)]],
                facing: None,
            })
            .unwrap();
        let elements = editor.mesh().edges().map(|(id, _)| Element::Edge(id)).collect();
        let out = editor.apply(Command::MakeGroup { elements }).unwrap();
        let source = out.created[0];
        let before = editor.doc().clone();
        editor.apply(Command::DrapeSources { sources: vec![source], target }).unwrap();
        assert_eq!(editor.doc().definitions, before.definitions);
        assert_eq!(editor.doc().root.instances, before.root.instances);
        assert!(editor.mesh().vertices().all(|(_, v)| (v.pos.z - 4.).abs() < 1e-9));
        assert!(!editor.mesh().edges().collect::<Vec<_>>().is_empty());
        let after = editor.doc().clone();
        assert!(editor.apply(Command::DrapeSources { sources: vec![Element::Instance(target)], target }).is_err());
        assert_eq!(editor.doc(), &after);
        editor.undo();
        assert_eq!(editor.doc(), &before);
    }

    #[test]
    fn drape_command_creates_parent_edges_and_preserves_terrain_and_undo() {
        use crate::{Command, Editor, Element};
        let mut editor = Editor::new();
        let out = editor
            .apply(Command::CreateTerrainGrid {
                origin: DVec3::new(2., 3., 4.),
                u: DVec3::X * 4.,
                v: DVec3::Y * 4.,
                spacing: 1.,
            })
            .unwrap();
        let Element::Instance(target) = out.created[0] else { panic!() };
        editor
            .apply(Command::DrawLines {
                lines: vec![[DVec3::new(1., 4.5, 10.), DVec3::new(7., 4.5, 10.)]],
                facing: None,
            })
            .unwrap();
        let edges = editor.mesh().edges().map(|(id, _)| id).collect::<Vec<_>>();
        let before = editor.doc().clone();
        let definition = editor.active().instances[&target].definition;
        editor.apply(Command::DrapeEdges { edges: edges.clone(), target }).unwrap();
        assert_eq!(&editor.doc().definitions[&definition], &before.definitions[&definition]);
        for id in edges {
            assert_eq!(editor.mesh().edge(id), before.root.mesh.edge(id));
        }
        assert!(editor.mesh().vertices().any(|(_, v)| (v.pos.z - 4.).abs() < 1e-9));
        let after = editor.doc().clone();
        editor.undo();
        assert_eq!(editor.doc(), &before);
        editor.redo();
        assert_eq!(editor.doc(), &after);
    }

    #[test]
    fn stamp_clips_to_sloped_terrain_boundary_and_keeps_holes() {
        let mut mesh = crate::terrain::grid(6., 6., 1.).unwrap();
        mesh.map_all(|p| DVec3::new(p.x, p.y, 4. + p.x * 0.25 + p.y * 0.5));
        let hole_point = DVec3::new(1.75, 1.25, 5.0625);
        let hole = mesh.faces().find(|(id, _)| mesh.face_contains(*id, hole_point)).unwrap().0;
        mesh.remove_face(hole);
        let outline = vec![DVec2::new(-1., -1.), DVec2::new(3., -1.), DVec2::new(3., 3.), DVec2::new(-1., 3.)];
        let original = mesh.clone();
        for offset in [0.5, 0.75, 1., 2.] {
            let stamped = TerrainSurface::stamp_outline(&mesh, &outline, offset, 2., 10000).unwrap();
            assert_eq!(mesh, original);
            stamped.validate().unwrap();
            let area = stamped
                .faces()
                .map(|(_, face)| {
                    let points = face.outer.iter().map(|id| stamped.pos(*id)).collect::<Vec<_>>();
                    (points[1] - points[0]).cross(points[2] - points[0]).z.abs() * 0.5
                })
                .sum::<f64>();
            assert!((area - 35.5).abs() < 1e-9);
            let surface = TerrainSurface::new(&stamped).unwrap();
            assert!(surface.sample(DVec2::new(-0.5, 2.)).is_none());
            assert!(surface.sample(DVec2::new(1.75, 1.25)).is_none());
            assert!((surface.sample(DVec2::ZERO).unwrap().0 - 2.).abs() < 1e-9);
            assert!((surface.sample(DVec2::new(5., 5.)).unwrap().0 - 7.75).abs() < 1e-9);
            assert!(stamped.vertices().all(|(_, v)| v.pos.x >= 0. && v.pos.x <= 6. && v.pos.y >= 0. && v.pos.y <= 6.));
        }
    }

    #[test]
    fn half_cell_transition_has_no_degenerate_triangles() {
        for span in [6., 10., 14.] {
            for origin in [DVec3::ZERO, DVec3::new(10.25, 20.5, 0.)] {
                let mut mesh = crate::terrain::grid(span, span, 1.).unwrap();
                mesh.map_all(|p| p + origin);
                let outline = vec![DVec2::new(2., 2.), DVec2::new(4., 2.), DVec2::new(4., 4.), DVec2::new(2., 4.)]
                    .into_iter()
                    .map(|p| p + DVec2::new(origin.x, origin.y))
                    .collect::<Vec<_>>();
                let result = TerrainSurface::stamp_outline(&mesh, &outline, 0.5, 2., 10000).unwrap();
                result.validate().unwrap();
                let area = result
                    .faces()
                    .map(|(_, face)| {
                        let points = face.outer.iter().map(|id| result.pos(*id)).collect::<Vec<_>>();
                        (points[1] - points[0]).cross(points[2] - points[0]).z.abs() * 0.5
                    })
                    .sum::<f64>();
                assert!((area - span * span).abs() < 1e-8);
                TerrainSurface::new(&result).unwrap();
            }
        }
    }

    #[test]
    fn closed_stamp_updates_parent_annotation_through_instance_path_and_correction() {
        use crate::dimensions::{PointEntity, PointReference};
        use crate::text::{Text, TextAnchor, TextEndpoint, TextLeader, TextPlacement, TextReference};
        use crate::{Command, Editor, Element};
        let mut editor = Editor::new();
        let out = editor
            .apply(Command::CreateTerrainGrid {
                origin: DVec3::new(2., 3., 4.),
                u: DVec3::X * 6.,
                v: DVec3::Y * 6.,
                spacing: 1.,
            })
            .unwrap();
        let Element::Instance(target) = out.created[0] else { panic!() };
        let definition = editor.active().instances[&target].definition;
        let vertex = editor.doc().definitions[&definition].mesh.find_vertex(DVec3::new(3., 3., 0.)).unwrap();
        editor
            .apply(Command::DrawRectangle {
                corner: DVec3::new(4., 5., 8.),
                u: DVec3::X * 2.,
                v: DVec3::Y * 2.,
                facing: Some(DVec3::Z),
            })
            .unwrap();
        let face = editor.mesh().faces().next().unwrap().0;
        let text = Text {
            content: "Terrain point".into(),
            font: Default::default(),
            endpoint: TextEndpoint::ClosedArrow,
            placement: TextPlacement::Attached {
                anchor: TextAnchor {
                    point: DVec3::new(5., 6., 4.),
                    reference: Some(TextReference::Point {
                        reference: PointReference { path: vec![target], entity: PointEntity::Vertex { vertex } },
                    }),
                },
                leader: TextLeader::Pushpin { offset: DVec3::X },
            },
        };
        let id = editor.apply(Command::AddText { text }).unwrap().texts[0];
        let before = editor.doc().clone();
        let command =
            |height| Command::StampTerrainInstance { sources: vec![Element::Face(face)], target, offset: 1., height };
        editor.apply(command(6.)).unwrap();
        editor.amend_last(command(7.5)).unwrap();
        let TextPlacement::Attached { anchor, leader } = &editor.active().texts[&id].placement else { panic!() };
        assert_eq!(anchor.resolve(editor.doc(), editor.active_definition()), (DVec3::new(5., 6., 7.5), true));
        assert_eq!(anchor.point, DVec3::new(5., 6., 7.5));
        assert_eq!(leader, &TextLeader::Pushpin { offset: DVec3::X });
        let corrected = editor.doc().clone();
        editor.undo();
        assert_eq!(editor.doc(), &before);
        editor.redo();
        assert_eq!(editor.doc(), &corrected);
    }

    #[test]
    fn closed_target_stamp_preserves_source_placement_and_history() {
        use crate::{Command, Editor, Element};
        let mut editor = Editor::new();
        let terrain = editor
            .apply(Command::CreateTerrainGrid {
                origin: DVec3::new(10., 20., 0.),
                u: DVec3::X * 6.,
                v: DVec3::Y * 6.,
                spacing: 1.,
            })
            .unwrap();
        let Element::Instance(target) = terrain.created[0] else { panic!() };
        editor
            .apply(Command::DrawRectangle {
                corner: DVec3::new(12., 22., 5.),
                u: DVec3::X * 2.,
                v: DVec3::Y * 2.,
                facing: Some(DVec3::Z),
            })
            .unwrap();
        let source = editor.mesh().faces().next().unwrap().0;
        let source_mesh = editor.mesh().clone();
        let placement = editor.active().instances[&target].transform;
        let before = editor.doc().clone();
        editor
            .apply(Command::StampTerrainInstance {
                sources: vec![Element::Face(source)],
                target,
                offset: 1.,
                height: 2.,
            })
            .unwrap();
        assert_eq!(editor.mesh(), &source_mesh);
        assert_eq!(editor.active().instances[&target].transform, placement);
        assert!(editor.context().is_empty());
        let instance = &editor.active().instances[&target];
        let mesh = &editor.doc().definitions[&instance.definition].mesh;
        assert!((TerrainSurface::new(mesh).unwrap().sample(DVec2::new(3., 3.)).unwrap().0 - 2.).abs() < 1e-9);
        let after = editor.doc().clone();
        assert!(
            editor
                .apply(Command::StampTerrainInstance {
                    sources: vec![Element::Instance(target)],
                    target,
                    offset: 1.,
                    height: 3.,
                })
                .is_err()
        );
        assert_eq!(editor.doc(), &after);
        editor.undo();
        assert_eq!(editor.doc(), &before);
        editor.redo();
        assert_eq!(editor.doc(), &after);
    }

    #[test]
    fn compound_stamp_retains_hole_and_disjoint_plateaus() {
        let mesh = crate::terrain::grid(12., 10., 0.5).unwrap();
        let rectangle = |a: DVec2, b: DVec2| vec![a, DVec2::new(b.x, a.y), b, DVec2::new(a.x, b.y)];
        let outer = rectangle(DVec2::new(1., 1.), DVec2::new(7., 9.));
        let hole = rectangle(DVec2::new(3., 3.), DVec2::new(5., 7.));
        let island = rectangle(DVec2::new(9., 3.), DVec2::new(11., 7.));
        let overlapping = rectangle(DVec2::new(6., 2.), DVec2::new(8., 4.));
        assert!(TerrainSurface::stamp_rings(&mesh, &[outer.clone(), overlapping], 0.5, 2., 10000).is_err());
        for reversed in [false, true] {
            let mut rings = vec![outer.clone(), hole.clone(), island.clone()];
            if reversed {
                for ring in &mut rings {
                    ring.reverse();
                }
            }
            let stamped = TerrainSurface::stamp_rings(&mesh, &rings, 0.5, 2., 10000).unwrap();
            let surface = TerrainSurface::new(&stamped).unwrap();
            for point in [DVec2::new(2., 5.), DVec2::new(10., 5.)] {
                assert!((surface.sample(point).unwrap().0 - 2.).abs() < 1e-8);
            }
            assert!(surface.sample(DVec2::new(4., 5.)).unwrap().0.abs() < 1e-8);
            assert!(surface.sample(DVec2::new(8., 5.)).unwrap().0.abs() < 1e-8);
            let slope = surface.sample(DVec2::new(3.25, 5.)).unwrap().0;
            assert!(slope > 0. && slope < 2.);
        }
    }

    #[test]
    fn imprint_retains_split_curve_membership_and_source_flags() {
        use rysmo_mesh::{Curve, CurveKind};
        let mut mesh = crate::terrain::grid(4., 4., 4.).unwrap();
        let edges = mesh.edges().filter(|(_, edge)| !edge.soft).map(|(id, _)| id).collect::<Vec<_>>();
        assert_eq!(edges.len(), 4);
        mesh.set_edge_flags(&edges, Some(false), Some(false), Some(true));
        let curve = mesh
            .register_curve(
                &edges,
                Curve {
                    kind: CurveKind::Polygon,
                    faceted: true,
                    centre: DVec3::new(2., 2., 0.),
                    normal: DVec3::Z,
                    start: DVec3::new(-1., -1., 0.).normalize(),
                    radius: 8f64.sqrt(),
                    segments: 4,
                    sweep: 360.,
                },
            )
            .unwrap();
        let source = mesh.clone();
        let output = TerrainSurface::imprint(&mesh, &[[DVec2::new(-1., 1.), DVec2::new(5., 1.)]], 10000).unwrap();
        assert_eq!(mesh, source);
        output.validate().unwrap();
        assert_eq!(output.curve(curve).unwrap().kind, CurveKind::Polyline);
        assert_eq!(output.curve(curve).unwrap().segments, 6);
        assert_eq!(output.curve_edges(curve).len(), 6);
        for id in output.curve_edges(curve) {
            let edge = output.edge(id).unwrap();
            assert!(edge.hidden && !edge.soft && !edge.smooth);
        }
        let mut edited = output.clone();
        assert!(edited.set_curve_radius(curve, 3.).is_err());
    }

    #[test]
    fn imprint_retains_existing_vertex_face_and_edge_identity() {
        let mut mesh = crate::terrain::grid(6., 6., 1.).unwrap();
        let a = mesh.find_vertex(DVec3::ZERO).unwrap();
        let b = mesh.find_vertex(DVec3::X).unwrap();
        let edge = mesh.edge_between(a, b).unwrap();
        mesh.set_edge_flags(&[edge], None, None, Some(true));
        let output = TerrainSurface::imprint(&mesh, &[[DVec2::new(2.2, 2.2), DVec2::new(3.8, 3.8)]], 10000).unwrap();
        for (id, vertex) in mesh.vertices() {
            assert_eq!(output.vertex(id), Some(vertex));
        }
        assert_eq!(output.edge(edge), mesh.edge(edge));
        let untouched = mesh.faces().find(|(_, f)| f.outer.contains(&a)).unwrap().0;
        assert!(output.face(untouched).is_some());
        assert!(output.vertices().any(|(id, _)| id.0 >= 10000));
    }

    #[test]
    fn stamped_vertex_text_remains_associated_and_undo_restores_it() {
        use crate::dimensions::{PointEntity, PointReference};
        use crate::text::{Text, TextAnchor, TextEndpoint, TextLeader, TextPlacement, TextReference};
        use crate::{Command, Editor, Element};
        let mut editor = Editor::new();
        let out = editor
            .apply(Command::CreateTerrainGrid { origin: DVec3::ZERO, u: DVec3::X * 6., v: DVec3::Y * 6., spacing: 1. })
            .unwrap();
        let Element::Instance(instance) = out.created[0] else { panic!() };
        editor.enter(instance).unwrap();
        let point = DVec3::new(3., 3., 0.);
        let vertex = editor.mesh().find_vertex(point).unwrap();
        let text = Text {
            content: "Plateau".into(),
            font: Default::default(),
            endpoint: TextEndpoint::ClosedArrow,
            placement: TextPlacement::Attached {
                anchor: TextAnchor {
                    point,
                    reference: Some(TextReference::Point {
                        reference: PointReference { path: vec![], entity: PointEntity::Vertex { vertex } },
                    }),
                },
                leader: TextLeader::Pushpin { offset: DVec3::X },
            },
        };
        let id = editor.apply(Command::AddText { text }).unwrap().texts[0];
        let before = editor.doc().clone();
        editor
            .apply(Command::StampTerrainOutline {
                outline: vec![DVec2::new(2., 2.), DVec2::new(4., 2.), DVec2::new(4., 4.), DVec2::new(2., 4.)],
                offset: 1.,
                height: 2.,
            })
            .unwrap();
        let TextPlacement::Attached { anchor, .. } = &editor.active().texts[&id].placement else { panic!() };
        assert_eq!(anchor.resolve(editor.doc(), editor.active_definition()), (point + DVec3::Z * 2., true));
        editor.undo();
        assert_eq!(editor.doc(), &before);
    }

    #[test]
    fn stamp_command_uses_drawing_axes_and_atomic_history() {
        use crate::{Command, Editor};
        for rotated in [false, true] {
            let mut editor = Editor::new();
            if rotated {
                editor.apply(Command::SetAxes { origin: DVec3::new(10., 20., 30.), x: DVec3::Y, y: DVec3::Z }).unwrap();
            }
            let axes = editor.active().axes;
            let basis = axes.basis();
            editor
                .apply(Command::CreateTerrainGrid {
                    origin: axes.origin,
                    u: basis.transform_vector3(DVec3::X * 6.),
                    v: basis.transform_vector3(DVec3::Y * 6.),
                    spacing: 1.,
                })
                .unwrap();
            let id = *editor.active().instances.keys().next().unwrap();
            editor.enter(id).unwrap();
            // Terrain coordinates are relative to its group placement.
            let before = editor.doc().clone();
            let outline = vec![DVec2::new(2., 2.), DVec2::new(4., 2.), DVec2::new(4., 4.), DVec2::new(2., 4.)];
            editor.apply(Command::StampTerrainOutline { outline: outline.clone(), offset: 1., height: 2. }).unwrap();
            assert!(before.definitions.values().any(|d| !d.mesh.vertices().collect::<Vec<_>>().is_empty()
                && d.mesh.vertices().all(|(id, _)| editor.mesh().vertex(id).is_some())));
            let local_axes = editor.active().axes;
            let mut sampled_mesh = editor.mesh().clone();
            sampled_mesh.map_all(|p| local_axes.basis().inverse().transform_vector3(p - local_axes.origin));
            let height = TerrainSurface::new(&sampled_mesh).unwrap().sample(DVec2::new(3., 3.)).unwrap().0;
            assert!((height - 2.).abs() < 1e-8);
            let after = editor.doc().clone();
            editor.undo();
            assert_eq!(editor.doc(), &before);
            editor.redo();
            assert_eq!(editor.doc(), &after);
            assert!(editor.apply(Command::StampTerrainOutline { outline, offset: -1., height: 2. }).is_err());
            assert_eq!(editor.doc(), &after);
        }
    }

    #[test]
    fn stamp_flattens_outline_and_transitions_to_unchanged_terrain() {
        let mesh = crate::terrain::grid(6., 6., 1.).unwrap();
        let outline = [DVec2::new(2.2, 2.2), DVec2::new(3.8, 2.2), DVec2::new(3.8, 3.8), DVec2::new(2.2, 3.8)];
        let result = TerrainSurface::stamp_outline(&mesh, &outline, 1., 2., 10000).unwrap();
        result.validate().unwrap();
        let surface = TerrainSurface::new(&result).unwrap();
        for p in [DVec2::new(3., 3.), outline[0], outline[2]] {
            assert!((surface.sample(p).unwrap().0 - 2.).abs() < 1e-8);
        }
        assert!(surface.sample(DVec2::new(0.5, 0.5)).unwrap().0.abs() < 1e-8);
        assert!(surface.sample(DVec2::new(1.2, 3.)).unwrap().0.abs() < 1e-8);
        let slope = surface.sample(DVec2::new(1.7, 3.)).unwrap().0;
        assert!(slope > 0. && slope < 2.);
        let reversed = outline.into_iter().rev().collect::<Vec<_>>();
        let reversed = TerrainSurface::stamp_outline(&mesh, &reversed, 1., 2., 20000).unwrap();
        assert!((TerrainSurface::new(&reversed).unwrap().sample(DVec2::new(3., 3.)).unwrap().0 - 2.).abs() < 1e-8);
        assert!(TerrainSurface::stamp_outline(&mesh, &outline, 0., 2., 1).is_err());
    }

    #[test]
    fn imprint_splits_crossing_edges_preserves_heights_attributes_and_holes() {
        let mut mesh = crate::terrain::grid(4., 4., 1.).unwrap();
        mesh.map_all(|p| DVec3::new(p.x, p.y, p.x * 0.25 + p.y * 0.5));
        let hole = mesh.faces().find(|(id, _)| mesh.face_contains(*id, DVec3::new(2.7, 2.2, 1.775))).unwrap().0;
        mesh.remove_face(hole);
        for id in mesh.faces().map(|(id, _)| id).collect::<Vec<_>>() {
            mesh.face_mut(id).unwrap().material = Some(rysmo_mesh::MaterialId(42));
        }
        let original = mesh.clone();
        let output = TerrainSurface::imprint(
            &mesh,
            &[[DVec2::new(-1., 0.4), DVec2::new(5., 0.4)], [DVec2::new(0.4, -1.), DVec2::new(0.4, 5.)]],
            10000,
        )
        .unwrap();
        assert_eq!(mesh, original);
        output.validate().unwrap();
        assert!(output.vertices().all(|(_, v)| (v.pos.z - v.pos.x * 0.25 - v.pos.y * 0.5).abs() < 1e-8));
        assert!(output.find_vertex(DVec3::new(0.4, 0.4, 0.3)).is_some());
        assert!(output.faces().all(|(_, f)| f.material == Some(rysmo_mesh::MaterialId(42))));
        let sampled = TerrainSurface::new(&output).unwrap();
        assert!(sampled.sample(DVec2::new(2.7, 2.2)).is_none());
        assert!(sampled.sample(DVec2::new(-0.5, 0.4)).is_none());
        assert!((sampled.sample(DVec2::new(0.4, 0.4)).unwrap().0 - 0.3).abs() < 1e-9);
        assert!(TerrainSurface::imprint(&mesh, &[[DVec2::ZERO, DVec2::ZERO]], 1).is_err());
    }

    #[test]
    fn small_triangle_samples_use_normalized_coordinates() {
        let mut mesh = crate::terrain::grid(1., 1., 1.).unwrap();
        mesh.map_all(|p| DVec3::new(p.x * 1e-7, p.y * 1e-7, p.x + p.y));
        let surface = TerrainSurface::new(&mesh).unwrap();
        let (height, _) = surface.sample(DVec2::splat(2.5e-8)).unwrap();
        assert!((height - 0.5).abs() < 1e-9);
    }

    #[test]
    fn indexed_samples_interpolate_height_and_preserve_domain_gaps() {
        let mut mesh = crate::terrain::grid(4., 4., 1.).unwrap();
        mesh.map_all(|p| DVec3::new(p.x, p.y, p.x * 0.25 + p.y * 0.5));
        let surface = TerrainSurface::new(&mesh).unwrap();
        for point in [DVec2::ZERO, DVec2::new(0.4, 0.3), DVec2::new(2., 2.), DVec2::new(4., 4.)] {
            let (height, face) = surface.sample(point).unwrap();
            assert!((height - point.x * 0.25 - point.y * 0.5).abs() < 1e-9);
            assert!(mesh.face(face).is_some());
        }
        assert!(surface.sample(DVec2::new(5., 2.)).is_none());
        let removed = mesh.faces().find(|(id, _)| mesh.face_contains(*id, DVec3::new(0.7, 0.2, 0.275))).unwrap().0;
        mesh.remove_face(removed);
        let surface = TerrainSurface::new(&mesh).unwrap();
        assert!(surface.sample(DVec2::new(0.7, 0.2)).is_none());
    }
}
