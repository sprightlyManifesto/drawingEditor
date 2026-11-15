//! Boundary-representation model built from STEP entities.
//!
//! Supports the analytic subset used by most mechanical parts: planes,
//! cylinders, cones, spheres, tori (topology only), lines, circles, ellipses
//! and (rational) B-spline curves.

use crate::geom::*;
use crate::step::{Ent, P, StepFile};
use std::collections::HashMap;
use std::f64::consts::{PI, TAU};

#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub o: V3,
    pub x: V3,
    pub y: V3,
    pub z: V3,
}

impl Frame {
    pub fn local(&self, p: V3) -> V3 {
        let q = p - self.o;
        v3(q.dot(self.x), q.dot(self.y), q.dot(self.z))
    }
}

#[derive(Clone, Debug)]
pub enum Curve {
    Line { dir: V3 },
    Circle { fr: Frame, r: f64 },
    Ellipse { fr: Frame, a: f64, b: f64 },
    Spline { deg: usize, ctrl: Vec<V3>, weights: Option<Vec<f64>>, knots: Vec<f64> },
    Other(String),
}

impl Curve {
    pub fn type_name(&self) -> &str {
        match self {
            Curve::Line { .. } => "LINE",
            Curve::Circle { .. } => "CIRCLE",
            Curve::Ellipse { .. } => "ELLIPSE",
            Curve::Spline { .. } => "BSPLINE",
            Curve::Other(_) => "OTHER",
        }
    }
}

#[derive(Clone, Debug)]
pub enum SurfKind {
    Plane,
    Cylinder { r: f64 },
    Cone { r: f64, semi: f64 },
    Sphere { r: f64 },
    Torus { major: f64, minor: f64 },
    Other(String),
}

#[derive(Clone, Debug)]
pub struct Surface {
    pub kind: SurfKind,
    pub fr: Frame,
}

impl Surface {
    pub fn type_name(&self) -> &str {
        match &self.kind {
            SurfKind::Plane => "PLANE",
            SurfKind::Cylinder { .. } => "CYLINDER",
            SurfKind::Cone { .. } => "CONE",
            SurfKind::Sphere { .. } => "SPHERE",
            SurfKind::Torus { .. } => "TORUS",
            SurfKind::Other(_) => "OTHER",
        }
    }
    /// Unit normal at a point on the surface (either sense), if the surface type is known.
    pub fn normal_at(&self, p: V3) -> Option<V3> {
        let fr = &self.fr;
        let q = fr.local(p);
        let radial = || {
            let l = (q.x * q.x + q.y * q.y).sqrt();
            (l > 1e-12).then(|| (fr.x * q.x + fr.y * q.y) * (1.0 / l))
        };
        let n = match self.kind {
            SurfKind::Plane => fr.z,
            SurfKind::Cylinder { .. } => radial()?,
            SurfKind::Cone { semi, .. } => radial()? - fr.z * semi.tan(),
            SurfKind::Sphere { .. } => p - fr.o,
            SurfKind::Torus { major, .. } => p - (fr.o + radial()? * major),
            SurfKind::Other(_) => return None,
        };
        (n.len() > 1e-12).then(|| n.norm())
    }
    pub fn periodic(&self) -> bool {
        !matches!(self.kind, SurfKind::Plane | SurfKind::Other(_))
    }
    /// Parametric coordinates used for point-in-face tests.
    pub fn uv(&self, p: V3) -> V2 {
        let q = self.fr.local(p);
        if self.periodic() { v2(wrap_2pi(q.y.atan2(q.x)), q.z) } else { v2(q.x, q.y) }
    }
    /// Parameters t of intersections of the line p + t d with the surface.
    pub fn hits(&self, p: V3, d: V3) -> Vec<f64> {
        let fr = &self.fr;
        let w = p - fr.o;
        let solve = |a: f64, b: f64, c: f64| -> Vec<f64> {
            if a.abs() < 1e-14 {
                if b.abs() < 1e-14 {
                    return vec![];
                }
                return vec![-c / b];
            }
            let disc = b * b - 4.0 * a * c;
            if disc < 0.0 {
                return vec![];
            }
            let s = disc.sqrt();
            vec![(-b - s) / (2.0 * a), (-b + s) / (2.0 * a)]
        };
        match self.kind {
            SurfKind::Plane => {
                let den = fr.z.dot(d);
                if den.abs() < 1e-9 {
                    return vec![];
                }
                vec![fr.z.dot(fr.o - p) / den]
            }
            SurfKind::Cylinder { r } => {
                let wp = w - fr.z * w.dot(fr.z);
                let dp = d - fr.z * d.dot(fr.z);
                solve(dp.dot(dp), 2.0 * wp.dot(dp), wp.dot(wp) - r * r)
            }
            SurfKind::Cone { r, semi } => {
                let k = semi.tan();
                let wa = w.dot(fr.z);
                let da = d.dot(fr.z);
                let wp = w - fr.z * wa;
                let dp = d - fr.z * da;
                let r0 = r + wa * k;
                let a = dp.dot(dp) - da * da * k * k;
                let b = 2.0 * (wp.dot(dp) - r0 * da * k);
                let c = wp.dot(wp) - r0 * r0;
                solve(a, b, c).into_iter().filter(|t| r + (wa + t * da) * k >= -1e-9).collect()
            }
            SurfKind::Sphere { r } => solve(d.dot(d), 2.0 * w.dot(d), w.dot(w) - r * r),
            _ => vec![],
        }
    }
}

#[derive(Clone, Debug)]
pub struct Edge {
    pub v0: usize,
    pub v1: usize,
    pub curve: Curve,
    /// Sampled polyline from v0 to v1.
    pub pts: Vec<V3>,
    pub faces: Vec<usize>,
    pub center: V3,
    /// Signed sweep for circles/ellipses (radians), 0 otherwise.
    pub sweep: f64,
}

#[derive(Clone, Debug)]
pub struct Face {
    pub surf: Surface,
    pub same_sense: bool,
    pub loops: Vec<Vec<(usize, bool)>>,
    pub loop_pts: Vec<Vec<V3>>,
    pub uv_loops: Vec<Vec<V2>>,
    pub bbox: BBox3,
    pub center: V3,
    pub edges: Vec<usize>,
}

impl Face {
    /// Outward normal of a planar face.
    pub fn plane_normal(&self) -> Option<V3> {
        match self.surf.kind {
            SurfKind::Plane => Some(if self.same_sense { self.surf.fr.z } else { -self.surf.fr.z }),
            _ => None,
        }
    }

    /// Whether `q` lies within `tol` of the face's boundary (in 3D).
    pub fn near_boundary(&self, q: V3, tol: f64) -> bool {
        for l in &self.loop_pts {
            let n = l.len();
            for i in 0..n {
                let (a, b) = (l[i], l[(i + 1) % n]);
                let ab = b - a;
                let ll = ab.dot(ab);
                let t = if ll > 0.0 { ((q - a).dot(ab) / ll).clamp(0.0, 1.0) } else { 0.0 };
                if q.dist(a + ab * t) < tol {
                    return true;
                }
            }
        }
        false
    }

    /// Unit normal pointing out of the solid at a point of the face, if known.
    pub fn outward_at(&self, p: V3) -> Option<V3> {
        self.surf.normal_at(p).map(|n| if self.same_sense { n } else { -n })
    }

    /// Range of the axial parameter v over the face boundary.
    pub fn v_range(&self) -> (f64, f64) {
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for l in &self.uv_loops {
            for p in l {
                lo = lo.min(p.y);
                hi = hi.max(p.y);
            }
        }
        (lo, hi)
    }

    /// Fraction of the full turn covered by the face boundary (periodic faces).
    pub fn angular_coverage(&self) -> f64 {
        let mut bins = [false; 72];
        for l in &self.uv_loops {
            for w in l.windows(2) {
                let du = wrap_pi(w[1].x - w[0].x);
                let n = ((du.abs() / (TAU / 144.0)).ceil() as usize).max(1);
                for k in 0..=n {
                    let u = wrap_2pi(w[0].x + du * k as f64 / n as f64);
                    bins[((u / TAU * 72.0) as usize).min(71)] = true;
                }
            }
        }
        bins.iter().filter(|b| **b).count() as f64 / 72.0
    }

    pub fn contains_uv(&self, q: V2) -> bool {
        let mut inside = false;
        if self.surf.periodic() {
            // A point exactly at the angle of a boundary vertex (above all on
            // the seam, u = 0) makes the crossing count ambiguous; testing a
            // hair to one side gives the same answer without the tie.
            let q = v2(q.x + 1e-9, q.y);
            // Ray from (u, -inf) up to q: count boundary crossings.
            for l in &self.uv_loops {
                let n = l.len();
                for i in 0..n {
                    let a = l[i];
                    let b = l[(i + 1) % n];
                    let du = wrap_pi(b.x - a.x);
                    if du.abs() < 1e-12 {
                        continue;
                    }
                    let bu = a.x + du;
                    for uc in [q.x, q.x + TAU, q.x - TAU] {
                        let (lo, hi) = if a.x < bu { (a.x, bu) } else { (bu, a.x) };
                        if uc >= lo && uc < hi {
                            let t = (uc - a.x) / du;
                            let v = a.y + t * (b.y - a.y);
                            if v < q.y {
                                inside = !inside;
                            }
                        }
                    }
                }
            }
        } else {
            for l in &self.uv_loops {
                let n = l.len();
                let mut j = n.wrapping_sub(1);
                for i in 0..n {
                    let (a, b) = (l[i], l[j]);
                    if (a.y > q.y) != (b.y > q.y) && q.x < (b.x - a.x) * (q.y - a.y) / (b.y - a.y) + a.x {
                        inside = !inside;
                    }
                    j = i;
                }
            }
        }
        inside
    }

    pub fn contains(&self, p: V3) -> bool {
        self.contains_uv(self.surf.uv(p))
    }
}

pub struct Model {
    pub verts: Vec<V3>,
    pub edges: Vec<Edge>,
    pub faces: Vec<Face>,
    pub bbox: BBox3,
    pub vert_edges: Vec<Vec<usize>>,
}

impl Model {
    pub fn diag(&self) -> f64 {
        self.bbox.diag().max(1e-6)
    }
    /// Whether edge `ei` is a smooth (tangent) join of its two faces, such as
    /// the boundary between a fillet and the face it blends into.
    pub fn smooth_edge(&self, ei: usize) -> bool {
        let e = &self.edges[ei];
        if e.faces.len() != 2 || e.pts.len() < 2 {
            return false;
        }
        let (a, b) = (&self.faces[e.faces[0]].surf, &self.faces[e.faces[1]].surf);
        let n = e.pts.len() - 1;
        [n / 4, n / 2, 3 * n / 4].iter().all(|&i| match (a.normal_at(e.pts[i]), b.normal_at(e.pts[i])) {
            (Some(x), Some(y)) => x.dot(y).abs() > 1.0 - 1e-6,
            _ => false,
        })
    }
}

struct Builder<'a> {
    f: &'a StepFile,
    len: f64,
    ang: f64,
    vmap: HashMap<usize, usize>,
    emap: HashMap<usize, usize>,
    verts: Vec<V3>,
    edges: Vec<Edge>,
}

fn nums(p: &P) -> Vec<f64> {
    p.as_list().map(|l| l.iter().filter_map(|x| x.as_f64()).collect()).unwrap_or_default()
}

impl<'a> Builder<'a> {
    fn ent(&self, id: usize) -> Result<&'a Ent, String> {
        self.f.ents.get(&id).ok_or_else(|| format!("missing STEP entity #{}", id))
    }
    fn refp(&self, p: Option<&P>) -> Result<usize, String> {
        p.and_then(|x| x.as_ref()).ok_or_else(|| "expected entity reference".to_string())
    }
    fn point(&self, id: usize) -> Result<V3, String> {
        let e = self.ent(id)?;
        match e.name() {
            "CARTESIAN_POINT" => {
                let c = nums(&e.params()[1]);
                Ok(v3(
                    c.first().copied().unwrap_or(0.0),
                    c.get(1).copied().unwrap_or(0.0),
                    c.get(2).copied().unwrap_or(0.0),
                ) * self.len)
            }
            "VERTEX_POINT" => self.point(self.refp(e.params().get(1))?),
            n => Err(format!("#{}: expected point, got {}", id, n)),
        }
    }
    fn dir(&self, id: usize) -> Result<V3, String> {
        let e = self.ent(id)?;
        match e.name() {
            "DIRECTION" => {
                let c = nums(&e.params()[1]);
                Ok(v3(
                    c.first().copied().unwrap_or(0.0),
                    c.get(1).copied().unwrap_or(0.0),
                    c.get(2).copied().unwrap_or(0.0),
                )
                .norm())
            }
            "VECTOR" => self.dir(self.refp(e.params().get(1))?),
            n => Err(format!("#{}: expected direction, got {}", id, n)),
        }
    }
    fn placement(&self, id: usize) -> Result<Frame, String> {
        let e = self.ent(id)?;
        let p = e.params();
        let o = self.point(self.refp(p.get(1))?)?;
        let z = match p.get(2).and_then(|x| x.as_ref()) {
            Some(r) => self.dir(r)?,
            None => v3(0.0, 0.0, 1.0),
        };
        let x = match p.get(3).and_then(|x| x.as_ref()) {
            Some(r) => {
                let x = self.dir(r)?;
                let x = x - z * x.dot(z);
                if x.len() < 1e-9 { z.any_perp() } else { x.norm() }
            }
            None => z.any_perp(),
        };
        Ok(Frame { o, x, y: z.cross(x), z })
    }
    fn curve(&self, id: usize) -> Result<Curve, String> {
        let e = self.ent(id)?;
        if e.is_simple() {
            let p = e.params();
            match e.name() {
                "LINE" => {
                    let v = self.ent(self.refp(p.get(2))?)?;
                    let d = if v.name() == "VECTOR" { self.dir(self.refp(v.params().get(1))?)? } else { self.dir(self.refp(p.get(2))?)? };
                    Ok(Curve::Line { dir: d })
                }
                "CIRCLE" => Ok(Curve::Circle {
                    fr: self.placement(self.refp(p.get(1))?)?,
                    r: p.get(2).and_then(|x| x.as_f64()).unwrap_or(0.0) * self.len,
                }),
                "ELLIPSE" => Ok(Curve::Ellipse {
                    fr: self.placement(self.refp(p.get(1))?)?,
                    a: p.get(2).and_then(|x| x.as_f64()).unwrap_or(0.0) * self.len,
                    b: p.get(3).and_then(|x| x.as_f64()).unwrap_or(0.0) * self.len,
                }),
                "SURFACE_CURVE" | "SEAM_CURVE" | "INTERSECTION_CURVE" | "BOUNDED_SURFACE_CURVE" => {
                    self.curve(self.refp(p.get(1))?)
                }
                "TRIMMED_CURVE" => self.curve(self.refp(p.get(1))?),
                "B_SPLINE_CURVE_WITH_KNOTS" => {
                    let deg = p.get(1).and_then(|x| x.as_f64()).unwrap_or(1.0) as usize;
                    let ctrl = self.point_list(p.get(2))?;
                    let knots = expand_knots(p.get(6), p.get(7));
                    Ok(Curve::Spline { deg, ctrl, weights: None, knots })
                }
                n => Ok(Curve::Other(n.to_string())),
            }
        } else if let Some(bs) = e.part("B_SPLINE_CURVE") {
            let deg = bs.first().and_then(|x| x.as_f64()).unwrap_or(1.0) as usize;
            let ctrl = self.point_list(bs.get(1))?;
            let knots = e.part("B_SPLINE_CURVE_WITH_KNOTS").map(|k| expand_knots(k.first(), k.get(1))).unwrap_or_default();
            let weights = e.part("RATIONAL_B_SPLINE_CURVE").map(|w| nums(&w[0]));
            Ok(Curve::Spline { deg, ctrl, weights, knots })
        } else {
            Ok(Curve::Other(e.name().to_string()))
        }
    }
    fn point_list(&self, p: Option<&P>) -> Result<Vec<V3>, String> {
        let mut v = Vec::new();
        if let Some(l) = p.and_then(|x| x.as_list()) {
            for r in l {
                if let Some(id) = r.as_ref() {
                    v.push(self.point(id)?);
                }
            }
        }
        Ok(v)
    }
    fn surface(&self, id: usize) -> Result<Surface, String> {
        let e = self.ent(id)?;
        let p = e.params();
        let num = |i: usize| p.get(i).and_then(|x| x.as_f64()).unwrap_or(0.0);
        let fr = || self.placement(self.refp(p.get(1))?);
        let dummy = Frame { o: V3::default(), x: v3(1.0, 0.0, 0.0), y: v3(0.0, 1.0, 0.0), z: v3(0.0, 0.0, 1.0) };
        if !e.is_simple() {
            return Ok(Surface { kind: SurfKind::Other(e.name().into()), fr: dummy });
        }
        Ok(match e.name() {
            "PLANE" => Surface { kind: SurfKind::Plane, fr: fr()? },
            "CYLINDRICAL_SURFACE" => Surface { kind: SurfKind::Cylinder { r: num(2) * self.len }, fr: fr()? },
            "CONICAL_SURFACE" => {
                Surface { kind: SurfKind::Cone { r: num(2) * self.len, semi: num(3) * self.ang }, fr: fr()? }
            }
            "SPHERICAL_SURFACE" => Surface { kind: SurfKind::Sphere { r: num(2) * self.len }, fr: fr()? },
            "TOROIDAL_SURFACE" => {
                Surface { kind: SurfKind::Torus { major: num(2) * self.len, minor: num(3) * self.len }, fr: fr()? }
            }
            n => Surface { kind: SurfKind::Other(n.into()), fr: dummy },
        })
    }
    fn vertex(&mut self, id: usize) -> Result<usize, String> {
        if let Some(&v) = self.vmap.get(&id) {
            return Ok(v);
        }
        let p = self.point(id)?;
        self.verts.push(p);
        self.vmap.insert(id, self.verts.len() - 1);
        Ok(self.verts.len() - 1)
    }
    fn edge(&mut self, id: usize) -> Result<usize, String> {
        if let Some(&e) = self.emap.get(&id) {
            return Ok(e);
        }
        let ent = self.ent(id)?;
        if ent.name() != "EDGE_CURVE" {
            return Err(format!("#{}: expected EDGE_CURVE, got {}", id, ent.name()));
        }
        let p = ent.params();
        let v0 = self.vertex(self.refp(p.get(1))?)?;
        let v1 = self.vertex(self.refp(p.get(2))?)?;
        let curve = self.curve(self.refp(p.get(3))?)?;
        let same = p.get(4).and_then(|x| x.as_bool()).unwrap_or(true);
        let (pts, sweep) = sample_edge(&curve, self.verts[v0], self.verts[v1], v0 == v1, same);
        let center = polyline_centroid(&pts);
        self.edges.push(Edge { v0, v1, curve, pts, faces: vec![], center, sweep });
        self.emap.insert(id, self.edges.len() - 1);
        Ok(self.edges.len() - 1)
    }
}

fn expand_knots(mults: Option<&P>, knots: Option<&P>) -> Vec<f64> {
    let m = mults.map(nums).unwrap_or_default();
    let k = knots.map(nums).unwrap_or_default();
    let mut out = Vec::new();
    for (mi, ki) in m.iter().zip(k.iter()) {
        for _ in 0..(*mi as usize) {
            out.push(*ki);
        }
    }
    out
}

fn bspline_eval(deg: usize, ctrl: &[V3], w: Option<&[f64]>, knots: &[f64], t: f64) -> V3 {
    let n = ctrl.len();
    let mut k = deg;
    while k < n - 1 && t >= knots[k + 1] {
        k += 1;
    }
    let mut d: Vec<[f64; 4]> = (0..=deg)
        .map(|j| {
            let i = j + k - deg;
            let wi = w.and_then(|w| w.get(i).copied()).unwrap_or(1.0);
            let p = ctrl[i];
            [p.x * wi, p.y * wi, p.z * wi, wi]
        })
        .collect();
    for r in 1..=deg {
        for j in (r..=deg).rev() {
            let i = j + k - deg;
            let den = knots[i + deg + 1 - r] - knots[i];
            let a = if den.abs() < 1e-15 { 0.0 } else { (t - knots[i]) / den };
            for c in 0..4 {
                d[j][c] = (1.0 - a) * d[j - 1][c] + a * d[j][c];
            }
        }
    }
    let h = d[deg];
    v3(h[0], h[1], h[2]) / if h[3].abs() < 1e-15 { 1.0 } else { h[3] }
}

/// Sample an edge from p0 to p1. Returns points and signed sweep (conics).
fn sample_edge(c: &Curve, p0: V3, p1: V3, closed: bool, same: bool) -> (Vec<V3>, f64) {
    let closed = closed || p0.dist(p1) < 1e-9;
    let conic = |fr: &Frame, a: f64, b: f64| -> (Vec<V3>, f64) {
        let ang = |p: V3| {
            let q = fr.local(p);
            (q.y / b).atan2(q.x / a)
        };
        let a0 = ang(p0);
        let a1 = ang(p1);
        let sweep = if closed {
            if same { TAU } else { -TAU }
        } else if same {
            let s = wrap_2pi(a1 - a0);
            if s < 1e-12 { TAU } else { s }
        } else {
            let s = wrap_2pi(a0 - a1);
            -(if s < 1e-12 { TAU } else { s })
        };
        let n = ((sweep.abs() / (TAU / 96.0)).ceil() as usize).max(4);
        let mut pts: Vec<V3> = (0..=n)
            .map(|i| {
                let t = a0 + sweep * i as f64 / n as f64;
                fr.o + fr.x * (a * t.cos()) + fr.y * (b * t.sin())
            })
            .collect();
        pts[0] = p0;
        if !closed {
            *pts.last_mut().unwrap() = p1;
        }
        (pts, sweep)
    };
    match c {
        Curve::Line { .. } => (vec![p0, p1], 0.0),
        Curve::Circle { fr, r } => conic(fr, *r, *r),
        Curve::Ellipse { fr, a, b } => conic(fr, *a, *b),
        Curve::Spline { deg, ctrl, weights, knots } => {
            let n = ctrl.len();
            if n < 2 || knots.len() < n + deg + 1 || *deg < 1 {
                return (vec![p0, p1], 0.0);
            }
            let t0 = knots[*deg];
            let t1 = knots[n];
            let m = (n * 12).clamp(24, 600);
            let mut pts: Vec<V3> = (0..=m)
                .map(|i| bspline_eval(*deg, ctrl, weights.as_deref(), knots, t0 + (t1 - t0) * i as f64 / m as f64))
                .collect();
            // Trim to the vertices: nearest samples to p0 and p1.
            let nearest = |p: V3, pts: &[V3]| {
                pts.iter().enumerate().min_by(|a, b| a.1.dist(p).total_cmp(&b.1.dist(p))).map(|x| x.0).unwrap_or(0)
            };
            if !closed {
                let i0 = nearest(p0, &pts);
                let i1 = nearest(p1, &pts);
                if i0 <= i1 {
                    pts = pts[i0..=i1].to_vec();
                } else {
                    pts = pts[i1..=i0].to_vec();
                    pts.reverse();
                }
            } else if !same {
                pts.reverse();
            }
            if pts.len() < 2 {
                return (vec![p0, p1], 0.0);
            }
            pts[0] = p0;
            *pts.last_mut().unwrap() = p1;
            (pts, 0.0)
        }
        Curve::Other(_) => (vec![p0, p1], 0.0),
    }
}

pub fn polyline_centroid(pts: &[V3]) -> V3 {
    let mut acc = V3::default();
    let mut tot = 0.0;
    for w in pts.windows(2) {
        let l = w[0].dist(w[1]);
        acc = acc + (w[0] + w[1]) * (0.5 * l);
        tot += l;
    }
    if tot < 1e-12 { pts.first().copied().unwrap_or_default() } else { acc / tot }
}

fn units(f: &StepFile) -> (f64, f64) {
    let mut len = None;
    let mut ang = None;
    let mut ids: Vec<_> = f.ents.keys().copied().collect();
    ids.sort();
    for id in ids {
        let e = &f.ents[&id];
        let scale = if let Some(si) = e.part("SI_UNIT") {
            let prefix = si.first().and_then(|p| p.as_enum()).unwrap_or("");
            let pf = match prefix {
                "MILLI" => 1e-3,
                "CENTI" => 1e-2,
                "MICRO" => 1e-6,
                "KILO" => 1e3,
                "DECI" => 1e-1,
                _ => 1.0,
            };
            Some((pf, true))
        } else {
            e.part("CONVERSION_BASED_UNIT").map(|cb| {
                let name = cb.first().and_then(|p| p.as_str()).unwrap_or("").to_ascii_uppercase();
                let v = match name.as_str() {
                    "INCH" => 0.0254,
                    "FOOT" => 0.3048,
                    "DEGREE" | "DEGREES" => PI / 180.0,
                    _ => 1.0,
                };
                (v, false)
            })
        };
        let Some((s, _)) = scale else { continue };
        if e.has("LENGTH_UNIT") && len.is_none() {
            len = Some(s * 1000.0); // to mm
        }
        if e.has("PLANE_ANGLE_UNIT") && ang.is_none() {
            ang = Some(s);
        }
    }
    (len.unwrap_or(1.0), ang.unwrap_or(1.0))
}

pub fn build(f: &StepFile) -> Result<Model, String> {
    let (len, ang) = units(f);
    let mut b = Builder { f, len, ang, vmap: HashMap::new(), emap: HashMap::new(), verts: vec![], edges: vec![] };
    let mut face_ids: Vec<usize> =
        f.ents.iter().filter(|(_, e)| matches!(e.name(), "ADVANCED_FACE" | "FACE_SURFACE")).map(|(k, _)| *k).collect();
    face_ids.sort();
    if face_ids.is_empty() {
        return Err("STEP file contains no B-rep faces (ADVANCED_FACE)".into());
    }
    let mut faces = Vec::new();
    for fid in face_ids {
        let e = b.ent(fid)?;
        let p = e.params();
        let surf = b.surface(b.refp(p.get(2))?)?;
        let same_sense = p.get(3).and_then(|x| x.as_bool()).unwrap_or(true);
        let mut loops = Vec::new();
        for bound in p.get(1).and_then(|x| x.as_list()).unwrap_or(&[]) {
            let Some(bid) = bound.as_ref() else { continue };
            let be = b.ent(bid)?;
            let lid = b.refp(be.params().get(1))?;
            let le = b.ent(lid)?;
            if le.name() != "EDGE_LOOP" {
                continue;
            }
            let mut lp = Vec::new();
            for oe in le.params().get(1).and_then(|x| x.as_list()).unwrap_or(&[]) {
                let Some(oid) = oe.as_ref() else { continue };
                let o = b.ent(oid)?;
                let (eid, fwd) = if o.name() == "ORIENTED_EDGE" {
                    (b.refp(o.params().get(3))?, o.params().get(4).and_then(|x| x.as_bool()).unwrap_or(true))
                } else {
                    (oid, true)
                };
                lp.push((b.edge(eid)?, fwd));
            }
            if !lp.is_empty() {
                loops.push(lp);
            }
        }
        faces.push((surf, same_sense, loops));
    }
    let Builder { verts, mut edges, .. } = b;
    let mut out_faces = Vec::new();
    for (fi, (surf, same_sense, loops)) in faces.into_iter().enumerate() {
        let mut loop_pts = Vec::new();
        let mut bbox = BBox3::default();
        let mut fedges = Vec::new();
        for l in &loops {
            let mut pts: Vec<V3> = Vec::new();
            for &(ei, fwd) in l {
                if !fedges.contains(&ei) {
                    fedges.push(ei);
                }
                if !edges[ei].faces.contains(&fi) {
                    edges[ei].faces.push(fi);
                }
                let mut ep = edges[ei].pts.clone();
                if !fwd {
                    ep.reverse();
                }
                for q in ep {
                    if pts.last().is_none_or(|l: &V3| l.dist(q) > 1e-9) {
                        pts.push(q);
                    }
                }
            }
            if pts.len() > 1 && pts[0].dist(*pts.last().unwrap()) < 1e-9 {
                pts.pop();
            }
            for q in &pts {
                bbox.add(*q);
            }
            loop_pts.push(pts);
        }
        if let SurfKind::Sphere { r } = surf.kind {
            bbox.add(surf.fr.o - v3(r, r, r));
            bbox.add(surf.fr.o + v3(r, r, r));
        }
        let uv_loops: Vec<Vec<V2>> = loop_pts.iter().map(|l| l.iter().map(|p| surf.uv(*p)).collect()).collect();
        let center = face_center(&surf, &loop_pts, &uv_loops);
        out_faces.push(Face { surf, same_sense, loops, loop_pts, uv_loops, bbox, center, edges: fedges });
    }
    let mut bbox = BBox3::default();
    for e in &edges {
        for p in &e.pts {
            bbox.add(*p);
        }
    }
    let mut vert_edges = vec![Vec::new(); verts.len()];
    for (i, e) in edges.iter().enumerate() {
        vert_edges[e.v0].push(i);
        if e.v1 != e.v0 {
            vert_edges[e.v1].push(i);
        }
    }
    Ok(Model { verts, edges, faces: out_faces, bbox, vert_edges })
}

fn face_center(surf: &Surface, loops: &[Vec<V3>], uv: &[Vec<V2>]) -> V3 {
    if matches!(surf.kind, SurfKind::Plane) {
        // Area-weighted centroid: largest loop is the outer boundary.
        let mut parts: Vec<(f64, V2)> = uv
            .iter()
            .map(|l| {
                let n = l.len();
                let mut a = 0.0;
                let mut c = V2::default();
                for i in 0..n {
                    let p = l[i];
                    let q = l[(i + 1) % n];
                    let cr = p.cross(q);
                    a += cr;
                    c = c + (p + q) * cr;
                }
                let a = a * 0.5;
                if a.abs() < 1e-12 { (0.0, V2::default()) } else { (a.abs(), c / (6.0 * a)) }
            })
            .collect();
        parts.sort_by(|a, b| b.0.total_cmp(&a.0));
        if let Some(&(ao, co)) = parts.first() {
            if ao > 1e-12 {
                let mut a = ao;
                let mut m = co * ao;
                for &(ah, ch) in &parts[1..] {
                    a -= ah;
                    m = m - ch * ah;
                }
                let c = if a > 1e-12 { m / a } else { co };
                return surf.fr.o + surf.fr.x * c.x + surf.fr.y * c.y;
            }
        }
    }
    let mut acc = V3::default();
    let mut tot = 0.0;
    for l in loops {
        let n = l.len();
        for i in 0..n {
            let (a, b) = (l[i], l[(i + 1) % n]);
            let w = a.dist(b);
            acc = acc + (a + b) * (0.5 * w);
            tot += w;
        }
    }
    let mut c = if tot > 1e-12 { acc / tot } else { surf.fr.o };
    if let SurfKind::Sphere { .. } = surf.kind {
        c = surf.fr.o;
    }
    c
}
