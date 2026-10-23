//! A Tesla Cybertruck built from closed faceted solids, after photographs of production trucks:
//! 5.68 m long, 2.03 m wide, 1.79 m tall, 3.64 m wheelbase, 35-inch tyres.
//!
//! Local axes: X to the right, Y from the nose (0) to the tailgate (5.68), Z up. `build` places
//! the truck with its nose towards -Y at `at` (the middle of the wheelbase, on the ground).
use rysmo_math::DVec3;
use rysmo_model::{Command, Editor, Element, InstanceId, MaterialId};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub struct Paints {
    steel: MaterialId,
    glass: MaterialId,
    trim: MaterialId,
    cladding: MaterialId,
    rubber: MaterialId,
    rim: MaterialId,
    led: MaterialId,
    red: MaterialId,
}

fn surface(e: &mut Editor, name: &str, color: [f32; 4], roughness: f32, metallic: f32) -> Result<MaterialId> {
    let m = e.apply(Command::AddMaterial { name: name.into(), color, texture: None })?.material.ok_or("no material")?;
    let pbr = rysmo_model::Pbr { roughness, metallic, ..Default::default() };
    e.apply(Command::SetMaterialPbr { material: m, pbr })?;
    Ok(m)
}

impl Paints {
    pub fn new(e: &mut Editor) -> Result<Self> {
        Ok(Self {
            steel: surface(e, "Brushed stainless steel", [0.66, 0.67, 0.68, 1.], 0.42, 1.)?,
            glass: surface(e, "Dark truck glass", [0.04, 0.045, 0.05, 0.96], 0.04, 0.)?,
            trim: surface(e, "Satin black trim", [0.05, 0.05, 0.055, 1.], 0.55, 0.)?,
            cladding: surface(e, "Textured arch cladding", [0.16, 0.165, 0.17, 1.], 0.85, 0.)?,
            rubber: surface(e, "Tyre rubber", [0.035, 0.035, 0.035, 1.], 0.92, 0.)?,
            rim: surface(e, "Gloss black wheel", [0.06, 0.06, 0.065, 1.], 0.3, 0.2)?,
            led: surface(e, "White light bar", [0.95, 0.96, 0.98, 1.], 0.15, 0.)?,
            red: surface(e, "Red tail light", [0.62, 0.03, 0.03, 1.], 0.2, 0.)?,
        })
    }
}

/// Front and rear axle positions along local Y, and the tyre radius.
const AXLES: [f64; 2] = [1.0, 4.62];
const TYRE: f64 = 0.445;

fn group_loose(e: &mut Editor) -> Result<InstanceId> {
    let mesh = e.mesh();
    let mut elements: Vec<Element> = mesh.faces().map(|(f, _)| Element::Face(f)).collect();
    elements.extend(mesh.edges().map(|(edge, _)| Element::Edge(edge)));
    match e.apply(Command::MakeGroup { elements })?.created.first() {
        Some(Element::Instance(i)) => Ok(*i),
        _ => Err("no group".into()),
    }
}

fn paint(e: &mut Editor, group: InstanceId, material: MaterialId) -> Result<InstanceId> {
    e.apply(Command::PaintInstances { instances: vec![group], material: Some(material) })?;
    Ok(group)
}

/// Whether four points lie in one plane.
fn planar(q: &[DVec3]) -> bool {
    let n = (q[1] - q[0]).cross(q[2] - q[0]);
    n.length() > 1e-12 && (q[3] - q[0]).dot(n.normalize()).abs() < 1e-7
}

/// A closed solid from face loops. Non-planar quads are split into triangles. Drawing edges
/// also fills any other closed planar loop (internal cross-sections), so faces that were not
/// asked for are erased again.
fn solid(e: &mut Editor, faces: &[Vec<DVec3>], material: MaterialId) -> Result<InstanceId> {
    let mut loops: Vec<Vec<DVec3>> = Vec::new();
    for face in faces {
        if face.len() == 4 && !planar(face) {
            loops.push(vec![face[0], face[1], face[2]]);
            loops.push(vec![face[0], face[2], face[3]]);
        } else {
            loops.push(face.clone());
        }
    }
    let centre = |l: &[DVec3]| l.iter().copied().sum::<DVec3>() / l.len() as f64;
    let wanted: Vec<DVec3> = loops.iter().map(|l| centre(l)).collect();
    let lines = loops.iter().flat_map(|l| (0..l.len()).map(move |i| [l[i], l[(i + 1) % l.len()]])).collect();
    e.apply(Command::DrawLines { lines, facing: None })?;
    let mesh = e.mesh();
    let extra: Vec<Element> = mesh
        .faces()
        .filter(|(_, f)| {
            let c = centre(&f.outer.iter().map(|v| mesh.pos(*v)).collect::<Vec<_>>());
            !wanted.iter().any(|w| w.distance(c) < 1e-6)
        })
        .map(|(id, _)| Element::Face(id))
        .collect();
    if !extra.is_empty() {
        e.apply(Command::Erase { elements: extra })?;
    }
    // Turn every face to point away from the solid's centre (the shapes here are convex
    // enough around their centre for that to be outward).
    let mesh = e.mesh();
    let points: Vec<DVec3> = mesh.vertices().map(|(_, v)| v.pos).collect();
    let middle = points.iter().copied().sum::<DVec3>() / points.len().max(1) as f64;
    let inward: Vec<_> = mesh
        .faces()
        .filter(|(id, f)| {
            let c = centre(&f.outer.iter().map(|v| mesh.pos(*v)).collect::<Vec<_>>());
            mesh.face_normal(*id).is_some_and(|n| n.dot(c - middle) < 0.)
        })
        .map(|(id, _)| id)
        .collect();
    if !inward.is_empty() {
        e.apply(Command::ReverseFaces { faces: inward })?;
    }
    let group = group_loose(e)?;
    paint(e, group, material)
}

/// A prism: a planar outline pushed along `direction` by its length.
fn prism(e: &mut Editor, outline: &[DVec3], direction: DVec3, material: MaterialId) -> Result<InstanceId> {
    let far: Vec<DVec3> = outline.iter().map(|p| *p + direction).collect();
    let mut faces = vec![outline.to_vec(), far.clone()];
    for i in 0..outline.len() {
        let j = (i + 1) % outline.len();
        faces.push(vec![outline[i], outline[j], far[j], far[i]]);
    }
    solid(e, &faces, material)
}

/// A thin plate on a planar outline, raised `lift` off the surface and `thickness` deep.
fn plate(e: &mut Editor, outline: &[DVec3], normal: DVec3, lift: f64, thickness: f64, m: MaterialId) -> Result<()> {
    let n = normal.normalize();
    let base: Vec<DVec3> = outline.iter().map(|p| *p + n * lift).collect();
    prism(e, &base, n * thickness, m)?;
    Ok(())
}

/// Marks edges of a group soft and smooth when `smooth` says so for their direction.
fn smooth_edges(e: &mut Editor, group: InstanceId, smooth: impl Fn(DVec3) -> bool) -> Result<()> {
    e.enter(group)?;
    let mesh = e.mesh();
    let edges = mesh
        .edges()
        .filter(|(id, _)| {
            let (a, b) = mesh.edge_points(*id);
            smooth((b - a).normalize())
        })
        .map(|(id, _)| id)
        .collect();
    e.apply(Command::SetEdges { edges, soft: Some(true), smooth: Some(true), hidden: None })?;
    e.exit();
    Ok(())
}

/// A body cross-section: lower edge, shoulder (belt) and roof edge, as half-widths and heights.
#[derive(Clone, Copy)]
struct Station {
    y: f64,
    low: (f64, f64),
    belt: (f64, f64),
    top: (f64, f64),
}

impl Station {
    fn points(&self, side: f64) -> [DVec3; 3] {
        [
            DVec3::new(side * self.low.0, self.y, self.low.1),
            DVec3::new(side * self.belt.0, self.y, self.belt.1),
            DVec3::new(side * self.top.0, self.y, self.top.1),
        ]
    }
}

/// Moves the X of `d` so that `a, b, c, d` are coplanar (keeping Y and Z).
fn flatten_x(a: DVec3, b: DVec3, c: DVec3, d: DVec3) -> f64 {
    let n = (b - a).cross(c - a);
    if n.x.abs() < 1e-12 {
        return d.x;
    }
    d.x - (d - a).dot(n) / n.x
}

/// The key sections from nose to tailgate, read from side and three-quarter photographs.
fn key_stations() -> Vec<Station> {
    let mut s = vec![
        // Nose: a narrow vertical fascia under the full-width light bar.
        Station { y: 0.0, low: (0.74, 0.6), belt: (0.74, 1.0), top: (0.7, 1.02) },
        // The long chamfered front corners end. Hood and windscreen then rise along one line.
        Station { y: 0.5, low: (0.98, 0.6), belt: (0.98, 1.1), top: (0.92, 1.17) },
        // Windscreen base, where the hood meets the side glass plane.
        Station { y: 1.3, low: (0.98, 0.45), belt: (0.98, 1.25), top: (0.86, 1.43) },
        // Roof apex, above the front seat backs.
        Station { y: 2.5, low: (0.98, 0.45), belt: (0.98, 1.31), top: (0.75, 1.8) },
        // Rear end of the vault, before the tailgate chamfer.
        Station { y: 5.42, low: (0.98, 0.72), belt: (0.98, 1.36), top: (0.93, 1.4) },
        // Tailgate.
        Station { y: 5.68, low: (0.94, 0.72), belt: (0.94, 1.36), top: (0.92, 1.39) },
    ];
    // The side glass sits on one flat plane from the windscreen base to the vault: keep the
    // shoulder line straight and move the roof edge onto the plane through the apex.
    let (a, b) = (s[2].belt.1, s[4].belt.1);
    s[3].belt.1 = a + (b - a) * (s[3].y - s[2].y) / (s[4].y - s[2].y);
    let [_, b0, _] = s[2].points(1.);
    let [_, b1, t1] = s[3].points(1.);
    let [_, b2, _] = s[4].points(1.);
    for k in [2, 4] {
        let [_, _, t] = s[k].points(1.);
        s[k].top.0 = flatten_x(b0, b2, t1, t);
    }
    let _ = b1;
    s
}

/// Half-length of a wheel opening at the bottom and at its flat top, and the top height.
const ARCH: (f64, f64, f64) = (0.62, 0.38, 1.07);

/// All sections: the key ones plus those where the lower edge climbs over each wheel.
fn stations() -> Vec<Station> {
    let keys = key_stations();
    let lerp = |a: f64, b: f64, t: f64| a + (b - a) * t;
    let at = |y: f64| -> Station {
        let k = keys.windows(2).find(|w| y <= w[1].y).unwrap_or(&keys[keys.len() - 2..]);
        let t = ((y - k[0].y) / (k[1].y - k[0].y)).clamp(0., 1.);
        let mix = |a: (f64, f64), b: (f64, f64)| (lerp(a.0, b.0, t), lerp(a.1, b.1, t));
        Station { y, low: mix(k[0].low, k[1].low), belt: mix(k[0].belt, k[1].belt), top: mix(k[0].top, k[1].top) }
    };
    let mut ys: Vec<f64> = keys.iter().map(|k| k.y).collect();
    for c in AXLES {
        ys.extend([c - ARCH.0, c - ARCH.1, c + ARCH.1, c + ARCH.0]);
    }
    ys.sort_by(f64::total_cmp);
    ys.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    ys.into_iter()
        .map(|y| {
            let mut s = keys.iter().find(|k| (k.y - y).abs() < 1e-9).copied().unwrap_or_else(|| at(y));
            // Behind the rear wheel the body ends higher, above the rear bumper.
            if y > AXLES[1] + ARCH.0 - 1e-9 {
                s.low.1 = 0.72;
            } else if y > keys[2].y {
                s.low.1 = 0.45;
            }
            // Over a wheel the lower edge rises to the top of the opening.
            if AXLES.iter().any(|c| (y - c).abs() < ARCH.1 + 1e-9) {
                s.low.1 = ARCH.2;
            }
            s
        })
        .collect()
}

/// The truck body: one faceted stainless shell with wheel wells cut out.
fn body(e: &mut Editor, p: &Paints) -> Result<InstanceId> {
    let st = stations();
    let mut faces = Vec::new();
    for w in st.windows(2) {
        let (a, b) = (w[0], w[1]);
        for side in [-1., 1.] {
            let [la, ba, ta] = a.points(side);
            let [lb, bb, tb] = b.points(side);
            faces.push(vec![la, ba, bb, lb]);
            faces.push(vec![ba, ta, tb, bb]);
        }
        let [la, _, ta] = a.points(-1.);
        let [ra_l, _, ra_t] = a.points(1.);
        let [lb, _, tb] = b.points(-1.);
        let [rb_l, _, rb_t] = b.points(1.);
        faces.push(vec![ta, ra_t, rb_t, tb]);
        faces.push(vec![la, lb, rb_l, ra_l]);
    }
    for (station, _) in [(st[0], -1.), (st[st.len() - 1], 1.)] {
        let [l0, l1, l2] = station.points(-1.);
        let [r0, r1, r2] = station.points(1.);
        faces.push(vec![l0, l1, l2, r2, r1, r0]);
    }
    let shell = solid(e, &faces, p.steel)?;
    let body = shell;
    Ok(body)
}

/// A point on the planar quad between two stations' facet edges.
fn on_facet(a0: DVec3, a1: DVec3, b0: DVec3, b1: DVec3, t: f64, u: f64) -> DVec3 {
    a0.lerp(b0, t).lerp(a1.lerp(b1, t), u)
}

fn facet_patch(f: [DVec3; 4], (t0, t1): (f64, f64), (u0, u1): (f64, f64)) -> Vec<DVec3> {
    let [a0, a1, b0, b1] = f;
    vec![
        on_facet(a0, a1, b0, b1, t0, u0),
        on_facet(a0, a1, b0, b1, t1, u0),
        on_facet(a0, a1, b0, b1, t1, u1),
        on_facet(a0, a1, b0, b1, t0, u1),
    ]
}

fn outward(f: [DVec3; 4], hint: DVec3) -> DVec3 {
    let n = (f[2] - f[0]).cross(f[1] - f[0]).normalize();
    if n.dot(hint) < 0. { -n } else { n }
}

fn glass_and_lights(e: &mut Editor, p: &Paints) -> Result<()> {
    let st = key_stations();
    let (nose, hood, screen, apex, vault, gate) = (st[0], st[1], st[2], st[3], st[4], st[5]);
    let top = |s: Station, side: f64| s.points(side)[2];
    let belt = |s: Station, side: f64| s.points(side)[1];
    // Windscreen: almost the whole top facet between its base and the apex.
    let f = [top(screen, -1.), top(screen, 1.), top(apex, -1.), top(apex, 1.)];
    plate(e, &facet_patch(f, (0.03, 0.97), (0.03, 0.97)), outward(f, DVec3::Z), 0.002, 0.004, p.glass)?;
    // A single long wiper parked along the base.
    let f2 = [top(screen, -1.), top(screen, 1.), top(apex, -1.), top(apex, 1.)];
    plate(e, &facet_patch(f2, (0.06, 0.08), (0.12, 0.82)), outward(f2, DVec3::Z), 0.006, 0.012, p.trim)?;
    // Glass roof over the cabin, then the slatted vault cover.
    let r = [top(apex, -1.), top(apex, 1.), top(vault, -1.), top(vault, 1.)];
    let up = outward(r, DVec3::Z);
    plate(e, &facet_patch(r, (0.015, 0.33), (0.06, 0.94)), up, 0.002, 0.004, p.glass)?;
    for i in 0..22 {
        let t = 0.43 + i as f64 * 0.025;
        plate(e, &facet_patch(r, (t, t + 0.003), (0.02, 0.98)), up, 0.001, 0.003, p.trim)?;
    }
    for side in [-1., 1.] {
        let out = DVec3::X * side;
        // Side glass on the one flat facet, following the roof edge over the apex: front door
        // glass to the B-pillar, rear door glass behind it, and the black belt trim.
        let n = outward([belt(screen, side), top(screen, side), belt(vault, side), top(apex, side)], out);
        let at = |y: f64, u: f64| {
            let (a, b) = if y <= apex.y { (screen, apex) } else { (apex, vault) };
            let t = (y - a.y) / (b.y - a.y);
            let lo = belt(a, side).lerp(belt(b, side), t);
            let hi = top(a, side).lerp(top(b, side), t);
            lo.lerp(hi, u)
        };
        let window = |y0: f64, y1: f64, u0: f64, u1: f64| {
            let mut w = vec![at(y0, u0), at(y1, u0), at(y1, u1)];
            if y0 < apex.y && apex.y < y1 {
                w.push(at(apex.y, u1));
            }
            w.push(at(y0, u1));
            w
        };
        plate(e, &window(1.45, 2.92, 0.1, 0.92), n, 0.002, 0.004, p.glass)?;
        plate(e, &window(2.98, 3.95, 0.1, 0.9), n, 0.002, 0.004, p.glass)?;
        plate(e, &window(1.4, 3.97, 0.0, 0.1), n, 0.001, 0.003, p.trim)?;
    }
    // Light bars: white across the top of the nose, red across the top of the tailgate.
    let n = [belt(nose, -1.), top(nose, -1.), belt(nose, 1.), top(nose, 1.)];
    let front = vec![
        DVec3::new(-0.72, 0., 0.97),
        DVec3::new(0.72, 0., 0.97),
        DVec3::new(0.72, 0., 1.0),
        DVec3::new(-0.72, 0., 1.0),
    ];
    let _ = (n, hood);
    plate(e, &front, -DVec3::Y, 0.001, 0.006, p.led)?;
    let rear = vec![
        DVec3::new(-0.9, 5.68, 1.31),
        DVec3::new(0.9, 5.68, 1.31),
        DVec3::new(0.9, 5.68, 1.335),
        DVec3::new(-0.9, 5.68, 1.335),
    ];
    let _ = gate;
    plate(e, &rear, DVec3::Y, 0.001, 0.006, p.red)?;
    Ok(())
}

fn trim(e: &mut Editor, p: &Paints) -> Result<()> {
    let bx = |e: &mut Editor, a: DVec3, b: DVec3, m: MaterialId| -> Result<()> {
        let (lo, hi) = (a.min(b), a.max(b));
        let outline = [
            DVec3::new(lo.x, lo.y, lo.z),
            DVec3::new(hi.x, lo.y, lo.z),
            DVec3::new(hi.x, hi.y, lo.z),
            DVec3::new(lo.x, hi.y, lo.z),
        ];
        prism(e, &outline, DVec3::Z * (hi.z - lo.z), m)?;
        Ok(())
    };
    // Black front bumper with a lower grille slot and chamfered ends, and a skid plate.
    let bumper = [
        DVec3::new(-0.72, 0.02, 0.36),
        DVec3::new(0.72, 0.02, 0.36),
        DVec3::new(0.97, 0.5, 0.36),
        DVec3::new(0.97, 0.6, 0.36),
        DVec3::new(-0.97, 0.6, 0.36),
        DVec3::new(-0.97, 0.5, 0.36),
    ];
    prism(e, &bumper, DVec3::Z * 0.24, p.trim)?;
    // Grille: horizontal slats in a recessed opening.
    bx(e, DVec3::new(-0.55, 0.005, 0.4), DVec3::new(0.55, 0.02, 0.52), p.rubber)?;
    for k in 0..3 {
        let z = 0.415 + k as f64 * 0.035;
        bx(e, DVec3::new(-0.53, -0.005, z), DVec3::new(0.53, 0.005, z + 0.012), p.trim)?;
    }
    // Rear bumper with a hitch receiver.
    let rear = [
        DVec3::new(-0.97, 5.25, 0.38),
        DVec3::new(0.97, 5.25, 0.38),
        DVec3::new(0.97, 5.62, 0.38),
        DVec3::new(0.85, 5.74, 0.38),
        DVec3::new(-0.85, 5.74, 0.38),
        DVec3::new(-0.97, 5.62, 0.38),
    ];
    prism(e, &rear, DVec3::Z * 0.29, p.trim)?;
    bx(e, DVec3::new(-0.05, 5.74, 0.42), DVec3::new(0.05, 5.82, 0.5), p.cladding)?;
    // Underbody between the wheels.
    bx(e, DVec3::new(-0.92, 0.5, 0.36), DVec3::new(0.92, 5.3, 0.5), p.trim)?;
    for side in [-1., 1.] {
        let x = side * 0.98;
        let out = DVec3::X * side;
        // Rocker panel between the wheel wells.
        let rocker = vec![
            DVec3::new(x, AXLES[0] + ARCH.0, 0.3),
            DVec3::new(x, AXLES[1] - ARCH.0, 0.3),
            DVec3::new(x, AXLES[1] - ARCH.0, 0.47),
            DVec3::new(x, AXLES[0] + ARCH.0, 0.47),
        ];
        plate(e, &rocker, out, 0., 0.035, p.trim)?;
        // Angular arch flares: front leg, flat top, rear leg, standing proud of the side.
        for y in AXLES {
            let (w, h, (o, t)) = (0.1, ARCH.2, (ARCH.0, ARCH.1));
            let legs = [
                [
                    DVec3::new(x, y - o, 0.32),
                    DVec3::new(x, y - t, h),
                    DVec3::new(x, y - t - w, h + w),
                    DVec3::new(x, y - o - w, 0.32),
                ],
                [
                    DVec3::new(x, y - t, h),
                    DVec3::new(x, y + t, h),
                    DVec3::new(x, y + t + w, h + w),
                    DVec3::new(x, y - t - w, h + w),
                ],
                [
                    DVec3::new(x, y + t, h),
                    DVec3::new(x, y + o, 0.32),
                    DVec3::new(x, y + o + w, 0.32),
                    DVec3::new(x, y + t + w, h + w),
                ],
            ];
            for leg in legs {
                plate(e, &leg, out, 0., 0.045, p.cladding)?;
            }
        }
        // Panel gaps: front door, between the doors and behind the rear door.
        for (y, top) in [(1.72, 1.08), (2.98, 1.13), (4.02, 1.16)] {
            let gap = vec![
                DVec3::new(x, y, 0.6),
                DVec3::new(x, y + 0.006, 0.6),
                DVec3::new(x, y + 0.006, top),
                DVec3::new(x, y, top),
            ];
            plate(e, &gap, out, 0.0005, 0.002, p.trim)?;
        }
        // Wedge-shaped mirror on a short arm at the base of the front glass.
        let arm = side * 0.95;
        bx(e, DVec3::new(arm, 1.58, 1.3), DVec3::new(arm + side * 0.12, 1.63, 1.33), p.trim)?;
        let m = [
            DVec3::new(arm + side * 0.1, 1.52, 1.29),
            DVec3::new(arm + side * 0.27, 1.56, 1.29),
            DVec3::new(arm + side * 0.27, 1.56, 1.43),
            DVec3::new(arm + side * 0.1, 1.52, 1.41),
        ];
        prism(e, &m, DVec3::Y * 0.12, p.trim)?;
    }
    Ok(())
}

/// A tyre with rounded shoulders and a black multi-spoke wheel, on the outer side `side`.
fn wheel(e: &mut Editor, y: f64, side: f64, p: &Paints) -> Result<()> {
    let n = 32;
    let (inner, outer) = (side * 0.67, side * 0.99);
    let ring = |x: f64, r: f64| -> Vec<DVec3> {
        (0..n)
            .map(|i| {
                let a = i as f64 * std::f64::consts::TAU / n as f64;
                DVec3::new(x, y + r * a.cos(), TYRE + r * a.sin())
            })
            .collect()
    };
    let w = outer - inner;
    let rings = [ring(inner, 0.4), ring(inner + w * 0.12, TYRE), ring(outer - w * 0.12, TYRE), ring(outer, 0.405)];
    let mut faces = vec![rings[0].clone(), rings[3].clone()];
    for k in 0..3 {
        for i in 0..n {
            let j = (i + 1) % n;
            faces.push(vec![rings[k][i], rings[k][j], rings[k + 1][j], rings[k + 1][i]]);
        }
    }
    let tyre = solid(e, &faces, p.rubber)?;
    smooth_edges(e, tyre, |d| d.x.abs() > 0.2)?;
    // Tread blocks across the crown.
    for i in 0..36 {
        let a = i as f64 * std::f64::consts::TAU / 36.;
        let (c, s) = (a.cos(), a.sin());
        let r = TYRE + 0.004;
        let along = DVec3::new(0., -s, c) * 0.035;
        let at = DVec3::new(0., y + r * c, TYRE + r * s);
        let x0 = inner + w * (if i % 2 == 0 { 0.1 } else { 0.18 });
        let block = vec![
            DVec3::new(x0, at.y, at.z) - along,
            DVec3::new(x0 + w * 0.72, at.y, at.z) - along,
            DVec3::new(x0 + w * 0.72, at.y, at.z) + along,
            DVec3::new(x0, at.y, at.z) + along,
        ];
        plate(e, &block, DVec3::new(0., c, s), 0., 0.012, p.rubber)?;
    }
    // Wheel: a recessed dish, six spokes and a centre cap.
    let face_x = outer + side * 0.001;
    let dish = ring(face_x - side * 0.03, 0.3);
    prism(e, &dish, DVec3::X * side * 0.03, p.rim)?;
    for k in 0..6 {
        let a = k as f64 * std::f64::consts::TAU / 6. + 0.3;
        let (c, s) = (a.cos(), a.sin());
        let (c2, s2) = ((a + 0.32).cos(), (a + 0.32).sin());
        let spoke = vec![
            DVec3::new(face_x, y + 0.07 * c, TYRE + 0.07 * s),
            DVec3::new(face_x, y + 0.29 * c, TYRE + 0.29 * s),
            DVec3::new(face_x, y + 0.29 * c2, TYRE + 0.29 * s2),
            DVec3::new(face_x, y + 0.09 * c2, TYRE + 0.09 * s2),
        ];
        plate(e, &spoke, DVec3::X * side, 0., 0.015, p.trim)?;
    }
    let cap: Vec<DVec3> = ring(face_x, 0.07);
    plate(e, &cap, DVec3::X * side, 0.015, 0.01, p.rim)?;
    Ok(())
}

/// Builds the truck around the local origin and moves it into place: nose to -Y, its
/// wheelbase centred on `at`.
pub fn build(e: &mut Editor, at: DVec3, p: &Paints) -> Result<()> {
    let before: std::collections::BTreeSet<InstanceId> = e.active().instances.keys().copied().collect();
    body(e, p)?;
    glass_and_lights(e, p)?;
    trim(e, p)?;
    for y in AXLES {
        for side in [-1., 1.] {
            wheel(e, y, side, p)?;
        }
    }
    let parts: Vec<Element> =
        e.active().instances.keys().filter(|i| !before.contains(i)).map(|i| Element::Instance(*i)).collect();
    let centre = (AXLES[0] + AXLES[1]) / 2.;
    e.apply(Command::Move { elements: parts, offset: at - DVec3::new(0., centre, 0.) })?;
    Ok(())
}
