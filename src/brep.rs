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
    BSpline(Box<BSurf>),
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
            SurfKind::BSpline(_) => "BSPLINE",
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
        let n = match &self.kind {
            SurfKind::Plane => fr.z,
            SurfKind::Cylinder { .. } => radial()?,
            SurfKind::Cone { semi, .. } => radial()? - fr.z * semi.tan(),
            SurfKind::Sphere { .. } => p - fr.o,
            SurfKind::Torus { major, .. } => p - (fr.o + radial()? * *major),
            SurfKind::BSpline(b) => {
                let (u, v) = b.param(p);
                b.normal(u, v)?
            }
            SurfKind::Other(_) => return None,
        };
        (n.len() > 1e-12).then(|| n.norm())
    }
    /// The point of the surface nearest p (for points meant to be on it).
    pub fn project(&self, p: V3) -> V3 {
        let fr = &self.fr;
        let q = fr.local(p);
        let rho = (q.x * q.x + q.y * q.y).sqrt();
        let er = if rho > 1e-12 { (fr.x * q.x + fr.y * q.y) / rho } else { fr.x };
        match &self.kind {
            SurfKind::Plane => p - fr.z * (p - fr.o).dot(fr.z),
            SurfKind::Cylinder { r } => fr.o + er * *r + fr.z * q.z,
            SurfKind::Cone { r, semi } => {
                // nearest point on the line rho = r + z tan(semi) in the (rho, z) half-plane
                let dir = v2(semi.sin(), semi.cos());
                let t = (v2(rho, q.z) - v2(*r, 0.0)).dot(dir);
                let (rr, z) = (r + dir.x * t, dir.y * t);
                fr.o + er * rr + fr.z * z
            }
            SurfKind::Sphere { r } => {
                let w = p - fr.o;
                if w.len() > 1e-12 { fr.o + w.norm() * *r } else { p }
            }
            SurfKind::Torus { major, minor } => {
                let t = v2(rho - major, q.z);
                let t = if t.len() > 1e-12 { t.norm() * *minor } else { v2(*minor, 0.0) };
                fr.o + er * (major + t.x) + fr.z * t.y
            }
            SurfKind::BSpline(b) => {
                let (u, v) = b.param(p);
                b.eval(u, v)
            }
            SurfKind::Other(_) => p,
        }
    }
    /// A point of the silhouette for view direction d near p (p near it):
    /// on the surface, with the normal square to d.
    pub fn onto_silhouette(&self, p: V3, d: V3) -> V3 {
        let p = self.project(p);
        let fr = &self.fr;
        match &self.kind {
            SurfKind::Sphere { r } => {
                let w = p - fr.o;
                let w = w - d * w.dot(d);
                if w.len() > 1e-12 { fr.o + w.norm() * *r } else { p }
            }
            SurfKind::Torus { major, minor } => {
                // at the point's tube angle w, the silhouette is at m cos(u - phi) = -da tan w
                let q = fr.local(p);
                let w = q.z.atan2((q.x * q.x + q.y * q.y).sqrt() - major);
                let (dx, dy, da) = (d.dot(fr.x), d.dot(fr.y), d.dot(fr.z));
                let m = (dx * dx + dy * dy).sqrt();
                let c = if w.cos().abs() < 1e-12 || m < 1e-12 { 2.0 } else { -da * w.tan() / m };
                if c.abs() > 1.0 {
                    return p;
                }
                let phi = dy.atan2(dx);
                let u0 = q.y.atan2(q.x);
                let u = [phi + c.acos(), phi - c.acos()].into_iter().min_by(|a, b| wrap_pi(a - u0).abs().total_cmp(&wrap_pi(b - u0).abs())).unwrap();
                fr.o + (fr.x * u.cos() + fr.y * u.sin()) * (major + minor * w.cos()) + fr.z * (minor * w.sin())
            }
            SurfKind::BSpline(b) => {
                // Newton on g = n . d along its gradient in (u, v)
                let (mut u, mut v) = b.param(p);
                let ((u0, u1), (v0, v1)) = (b.urange(), b.vrange());
                let (hu, hv) = ((u1 - u0) * 1e-5, (v1 - v0) * 1e-5);
                let g = |u: f64, v: f64| b.normal(u, v).map_or(0.0, |n| n.dot(d));
                for _ in 0..6 {
                    let g0 = g(u, v);
                    if g0.abs() < 1e-10 {
                        break;
                    }
                    let (gu, gv) = ((g(u + hu, v) - g(u - hu, v)) / (2.0 * hu), (g(u, v + hv) - g(u, v - hv)) / (2.0 * hv));
                    let n2 = gu * gu + gv * gv;
                    if n2 < 1e-300 {
                        break;
                    }
                    (u, v) = b.wrap(u - g0 * gu / n2, v - g0 * gv / n2);
                }
                b.eval(u, v)
            }
            _ => p,
        }
    }
    pub fn periodic(&self) -> bool {
        match &self.kind {
            SurfKind::Plane | SurfKind::Other(_) => false,
            SurfKind::BSpline(b) => b.closed_u || b.closed_v,
            _ => true,
        }
    }
    /// Parametric coordinates used for point-in-face tests.
    pub fn uv(&self, p: V3) -> V2 {
        if let SurfKind::BSpline(b) = &self.kind {
            return b.uv(p);
        }
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
            SurfKind::Torus { major, minor } => {
                // within the bounding sphere, sign changes of the implicit
                // (|q_xy| - R)^2 + q_z^2 - r^2, refined by bisection
                let big = major + minor;
                let dd = d.dot(d);
                let Some(&[t0, t1]) = solve(dd, 2.0 * w.dot(d), w.dot(w) - big * big).get(..2).and_then(|s| <&[f64; 2]>::try_from(s).ok()) else {
                    return vec![];
                };
                let f = |t: f64| {
                    let q = fr.local(p + d * t);
                    let a = (q.x * q.x + q.y * q.y).sqrt() - major;
                    a * a + q.z * q.z - minor * minor
                };
                let n = 400;
                let mut out = Vec::new();
                let mut ta = t0;
                let mut fa = f(ta);
                for i in 1..=n {
                    let tb = t0 + (t1 - t0) * i as f64 / n as f64;
                    let fb = f(tb);
                    if (fa < 0.0) != (fb < 0.0) {
                        let (mut lo, mut hi, flo) = (ta, tb, fa);
                        for _ in 0..50 {
                            let mid = 0.5 * (lo + hi);
                            if (f(mid) < 0.0) == (flo < 0.0) {
                                lo = mid;
                            } else {
                                hi = mid;
                            }
                        }
                        out.push(0.5 * (lo + hi));
                    }
                    ta = tb;
                    fa = fb;
                }
                out
            }
            SurfKind::BSpline(ref b) => b.hits(p, d),
            _ => vec![],
        }
    }
}

/// A B-spline surface (rational or not) of a STEP file, with a grid of
/// points on it for finding parameters and intersecting rays.
#[derive(Clone)]
pub struct BSurf {
    du: usize,
    dv: usize,
    nu: usize,
    nv: usize,
    /// control points times their weights, and the weight; [i * nv + j]
    cw: Vec<[f64; 4]>,
    ku: Vec<f64>,
    kv: Vec<f64>,
    pub closed_u: bool,
    pub closed_v: bool,
    gu: usize,
    gv: usize,
    grid: Vec<V3>,
    /// Blocks of grid cells with their bounding boxes, so a ray skips most
    /// of the surface: (cell rows i0..i1, columns j0..j1, box).
    blocks: Vec<(usize, usize, usize, usize, BBox3)>,
}

/// de Boor on homogeneous points: `d` holds the deg + 1 points of span `k`.
fn deboor_h(deg: usize, mut d: Vec<[f64; 4]>, knots: &[f64], k: usize, t: f64) -> [f64; 4] {
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
    d[deg]
}

fn knot_span(deg: usize, n: usize, knots: &[f64], t: f64) -> usize {
    let mut k = deg;
    while k < n - 1 && t >= knots[k + 1] {
        k += 1;
    }
    k
}

impl std::fmt::Debug for BSurf {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "BSurf {{ degree: {}x{}, control points: {}x{}{} }}", self.du, self.dv, self.nu, self.nv, if self.closed_u || self.closed_v { ", closed" } else { "" })
    }
}

impl BSurf {
    pub fn new(du: usize, dv: usize, ctrl: Vec<Vec<V3>>, w: Option<Vec<Vec<f64>>>, ku: Vec<f64>, kv: Vec<f64>) -> Option<BSurf> {
        let nu = ctrl.len();
        let nv = ctrl.first()?.len();
        if du == 0 || dv == 0 || nu <= du || nv <= dv || ctrl.iter().any(|r| r.len() != nv) || ku.len() != nu + du + 1 || kv.len() != nv + dv + 1 {
            return None;
        }
        let mut cw = Vec::with_capacity(nu * nv);
        for i in 0..nu {
            for j in 0..nv {
                let wt = w.as_ref().and_then(|w| w.get(i)?.get(j).copied()).unwrap_or(1.0);
                let p = ctrl[i][j];
                cw.push([p.x * wt, p.y * wt, p.z * wt, wt]);
            }
        }
        let (gu, gv) = ((4 * nu).clamp(16, 96), (4 * nv).clamp(16, 96));
        let mut b = BSurf { du, dv, nu, nv, cw, ku, kv, closed_u: false, closed_v: false, gu, gv, grid: vec![], blocks: vec![] };
        let (u0, u1) = b.urange();
        let (v0, v1) = b.vrange();
        b.grid = (0..=gu).flat_map(|i| (0..=gv).map(move |j| (u0 + (u1 - u0) * i as f64 / gu as f64, v0 + (v1 - v0) * j as f64 / gv as f64))).map(|(u, v)| b.eval(u, v)).collect();
        let size = b.grid.iter().fold(BBox3::default(), |mut bb, p| {
            bb.add(*p);
            bb
        });
        let tol = size.max.dist(size.min) * 1e-6 + 1e-9;
        const BLOCK: usize = 8;
        for i0 in (0..gu).step_by(BLOCK) {
            for j0 in (0..gv).step_by(BLOCK) {
                let (i1, j1) = ((i0 + BLOCK).min(gu), (j0 + BLOCK).min(gv));
                let mut bb = BBox3::default();
                for i in i0..=i1 {
                    for j in j0..=j1 {
                        bb.add(b.grid[i * (gv + 1) + j]);
                    }
                }
                b.blocks.push((i0, i1, j0, j1, bb.expand(tol * 10.0)));
            }
        }
        b.closed_u = (0..=4).all(|k| {
            let v = v0 + (v1 - v0) * k as f64 / 4.0;
            b.eval(u0, v).dist(b.eval(u1, v)) < tol
        });
        b.closed_v = !b.closed_u
            && (0..=4).all(|k| {
                let u = u0 + (u1 - u0) * k as f64 / 4.0;
                b.eval(u, v0).dist(b.eval(u, v1)) < tol
            });
        Some(b)
    }
    pub fn urange(&self) -> (f64, f64) {
        (self.ku[self.du], self.ku[self.nu])
    }
    pub fn vrange(&self) -> (f64, f64) {
        (self.kv[self.dv], self.kv[self.nv])
    }
    pub fn eval(&self, u: f64, v: f64) -> V3 {
        let ((u0, u1), (v0, v1)) = (self.urange(), self.vrange());
        let (u, v) = (u.clamp(u0, u1), v.clamp(v0, v1));
        let ks = knot_span(self.du, self.nu, &self.ku, u);
        let kt = knot_span(self.dv, self.nv, &self.kv, v);
        let col: Vec<[f64; 4]> = (ks - self.du..=ks)
            .map(|i| deboor_h(self.dv, (kt - self.dv..=kt).map(|j| self.cw[i * self.nv + j]).collect(), &self.kv, kt, v))
            .collect();
        let h = deboor_h(self.du, col, &self.ku, ks, u);
        let w = if h[3].abs() < 1e-15 { 1.0 } else { h[3] };
        v3(h[0] / w, h[1] / w, h[2] / w)
    }
    /// The point and its derivatives along u and v (by differences).
    fn derivs(&self, u: f64, v: f64) -> (V3, V3, V3) {
        let ((u0, u1), (v0, v1)) = (self.urange(), self.vrange());
        let (hu, hv) = ((u1 - u0) * 1e-6, (v1 - v0) * 1e-6);
        let (ua, ub) = ((u - hu).max(u0), (u + hu).min(u1));
        let (va, vb) = ((v - hv).max(v0), (v + hv).min(v1));
        let su = (self.eval(ub, v) - self.eval(ua, v)) / (ub - ua).max(1e-300);
        let sv = (self.eval(u, vb) - self.eval(u, va)) / (vb - va).max(1e-300);
        (self.eval(u, v), su, sv)
    }
    /// Unit normal at (u, v) (either sense).
    pub fn normal(&self, u: f64, v: f64) -> Option<V3> {
        let (_, su, sv) = self.derivs(u, v);
        let n = su.cross(sv);
        if n.len() > 1e-12 {
            return Some(n.norm());
        }
        // a degenerate point (a pole): a hair inside the domain
        let ((u0, u1), (v0, v1)) = (self.urange(), self.vrange());
        let (u2, v2) = (u + (0.5 * (u0 + u1) - u) * 1e-4, v + (0.5 * (v0 + v1) - v) * 1e-4);
        let (_, su, sv) = self.derivs(u2, v2);
        let n = su.cross(sv);
        (n.len() > 1e-12).then(|| n.norm())
    }
    pub fn wrap(&self, u: f64, v: f64) -> (f64, f64) {
        let ((u0, u1), (v0, v1)) = (self.urange(), self.vrange());
        let u = if self.closed_u { u0 + (u - u0).rem_euclid(u1 - u0) } else { u.clamp(u0, u1) };
        let v = if self.closed_v { v0 + (v - v0).rem_euclid(v1 - v0) } else { v.clamp(v0, v1) };
        (u, v)
    }
    /// Parameters of the point of the surface nearest p.
    pub fn param(&self, p: V3) -> (f64, f64) {
        let ((u0, u1), (v0, v1)) = (self.urange(), self.vrange());
        // the nearest grid point: on every 4th point first, then around the best of those
        let at = |i: usize, j: usize| self.grid[i * (self.gv + 1) + j].dist(p);
        let st = 4;
        let mut best = (f64::MAX, 0, 0);
        for i in (0..=self.gu).step_by(st) {
            for j in (0..=self.gv).step_by(st) {
                let d = at(i, j);
                if d < best.0 {
                    best = (d, i, j);
                }
            }
        }
        let (bi, bj) = (best.1, best.2);
        for i in bi.saturating_sub(st)..=(bi + st).min(self.gu) {
            for j in bj.saturating_sub(st)..=(bj + st).min(self.gv) {
                let d = at(i, j);
                if d < best.0 {
                    best = (d, i, j);
                }
            }
        }
        let (i, j) = (best.1, best.2);
        let (mut u, mut v) = (u0 + (u1 - u0) * i as f64 / self.gu as f64, v0 + (v1 - v0) * j as f64 / self.gv as f64);
        for _ in 0..12 {
            let (s, su, sv) = self.derivs(u, v);
            let r = p - s;
            let (a, b, c) = (su.dot(su), su.dot(sv), sv.dot(sv));
            let (e, f) = (su.dot(r), sv.dot(r));
            let det = a * c - b * b;
            if det.abs() < 1e-300 {
                break;
            }
            let (du, dv) = ((c * e - b * f) / det, (a * f - b * e) / det);
            (u, v) = self.wrap(u + du, v + dv);
            if du.abs() < (u1 - u0) * 1e-12 && dv.abs() < (v1 - v0) * 1e-12 {
                break;
            }
        }
        (u, v)
    }
    /// Face-test coordinates: the closed direction (if any) as an angle in x.
    pub fn uv(&self, p: V3) -> V2 {
        let (u, v) = self.param(p);
        self.to_uv(u, v)
    }
    pub fn to_uv(&self, u: f64, v: f64) -> V2 {
        let ((u0, u1), (v0, v1)) = (self.urange(), self.vrange());
        if self.closed_u {
            v2(TAU * (u - u0) / (u1 - u0), v)
        } else if self.closed_v {
            v2(TAU * (v - v0) / (v1 - v0), u)
        } else {
            v2(u, v)
        }
    }
    /// Parameters t of the intersections of the line p + t d with the
    /// surface: triangles of the grid, then Newton on S(u, v) = p + t d.
    pub fn hits(&self, p: V3, d: V3) -> Vec<f64> {
        let ((u0, u1), (v0, v1)) = (self.urange(), self.vrange());
        let g = |i: usize, j: usize| self.grid[i * (self.gv + 1) + j];
        let mut out: Vec<f64> = Vec::new();
        for &(i0, i1, j0, j1, ref bb) in &self.blocks {
            if !bb.ray_hits(p, d) {
                continue;
            }
            for i in i0..i1 {
                for j in j0..j1 {
                    let (a, b, c, e) = (g(i, j), g(i + 1, j), g(i + 1, j + 1), g(i, j + 1));
                    for (t0, t1, t2, uv) in [(a, b, c, [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)]), (a, c, e, [(0.0, 0.0), (1.0, 1.0), (0.0, 1.0)])] {
                        let Some((t, bu, bv)) = tri_hit(p, d, t0, t1, t2) else { continue };
                        // start Newton from the hit's place in the cell
                        let fu = uv[0].0 * (1.0 - bu - bv) + uv[1].0 * bu + uv[2].0 * bv;
                        let fv = uv[0].1 * (1.0 - bu - bv) + uv[1].1 * bu + uv[2].1 * bv;
                        let mut u = u0 + (u1 - u0) * (i as f64 + fu) / self.gu as f64;
                        let mut v = v0 + (v1 - v0) * (j as f64 + fv) / self.gv as f64;
                        let mut t = t;
                        for _ in 0..8 {
                            let (s, su, sv) = self.derivs(u, v);
                            let r = s - (p + d * t);
                            // [su sv -d] (du dv dt) = -r
                            let m = [[su.x, sv.x, -d.x], [su.y, sv.y, -d.y], [su.z, sv.z, -d.z]];
                            let Some(x) = solve3(m, [-r.x, -r.y, -r.z]) else { break };
                            (u, v) = self.wrap(u + x[0], v + x[1]);
                            t += x[2];
                            if x[2].abs() < 1e-12 {
                                break;
                            }
                        }
                        if !out.iter().any(|o| (o - t).abs() < 1e-7) {
                            out.push(t);
                        }
                    }
                }
            }
        }
        out
    }
}

/// Ray / triangle (Moller-Trumbore): (t, barycentric b1, b2).
fn tri_hit(p: V3, d: V3, a: V3, b: V3, c: V3) -> Option<(f64, f64, f64)> {
    let (e1, e2) = (b - a, c - a);
    let h = d.cross(e2);
    let det = e1.dot(h);
    if det.abs() < 1e-14 {
        return None;
    }
    let s = p - a;
    let bu = s.dot(h) / det;
    if !(-1e-6..=1.0 + 1e-6).contains(&bu) {
        return None;
    }
    let q = s.cross(e1);
    let bv = d.dot(q) / det;
    if bv < -1e-6 || bu + bv > 1.0 + 1e-6 {
        return None;
    }
    Some((e2.dot(q) / det, bu, bv))
}

fn solve3(m: [[f64; 3]; 3], b: [f64; 3]) -> Option<[f64; 3]> {
    let det = |m: [[f64; 3]; 3]| {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0]) + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    let d = det(m);
    if d.abs() < 1e-300 {
        return None;
    }
    let mut x = [0.0; 3];
    for (k, xk) in x.iter_mut().enumerate() {
        let mut mk = m;
        for r in 0..3 {
            mk[r][k] = b[r];
        }
        *xk = det(mk) / d;
    }
    Some(x)
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
    /// Torus faces: the arc of tube angles the face covers (start, length);
    /// (u, v) alone can't tell the inside of the tube from the outside.
    pub tube: Option<(f64, f64)>,
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
        if let (Some((w0, span)), SurfKind::Torus { major, .. }) = (self.tube, &self.surf.kind) {
            let q = self.surf.fr.local(p);
            let w = q.z.atan2((q.x * q.x + q.y * q.y).sqrt() - major);
            if wrap_2pi(w - w0) > span + 1e-6 {
                return false;
            }
        }
        self.contains_uv(self.surf.uv(p))
    }

    /// The arc of tube angles a torus face covers: the shortest arc holding
    /// all its boundary points.
    fn tube_span(surf: &Surface, loops: &[Vec<V3>]) -> Option<(f64, f64)> {
        let SurfKind::Torus { major, .. } = surf.kind else { return None };
        let mut ws: Vec<f64> = loops
            .iter()
            .flatten()
            .map(|p| {
                let q = surf.fr.local(*p);
                wrap_2pi(q.z.atan2((q.x * q.x + q.y * q.y).sqrt() - major))
            })
            .collect();
        if ws.is_empty() {
            return None;
        }
        ws.sort_by(|a, b| a.total_cmp(b));
        let (mut gap, mut w0) = (ws[0] + TAU - ws[ws.len() - 1], ws[0]);
        for i in 1..ws.len() {
            if ws[i] - ws[i - 1] > gap {
                gap = ws[i] - ws[i - 1];
                w0 = ws[i];
            }
        }
        // a whole tube: no limit
        (gap > 1e-6).then_some((w0, TAU - gap))
    }

    /// The curves on a B-spline face where g(u, v) = 0 (a silhouette, a
    /// plane cut), traced over a grid of its parameters (marching squares),
    /// kept inside the face and joined into polylines.
    pub fn bspline_contour(&self, g: &dyn Fn(f64, f64) -> f64) -> Vec<Vec<V3>> {
        let SurfKind::BSpline(b) = &self.surf.kind else { return vec![] };
        let ((u0, u1), (v0, v1)) = (b.urange(), b.vrange());
        let (nu, nv) = (b.gu * 2, b.gv * 2);
        // in a closed direction the grid starts off the seam (a curve along
        // the seam would sit on the grid's edge) and wraps round
        let (pu, pv) = (if b.closed_u { 0.37 } else { 0.0 }, if b.closed_v { 0.37 } else { 0.0 });
        let uu = |i: usize| u0 + (u1 - u0) * (i as f64 + pu) / nu as f64;
        let vv = |j: usize| v0 + (v1 - v0) * (j as f64 + pv) / nv as f64;
        let g = |u: f64, v: f64| {
            let (u, v) = b.wrap(u, v);
            g(u, v)
        };
        let val: Vec<f64> = (0..=nu).flat_map(|i| (0..=nv).map(move |j| (i, j))).map(|(i, j)| g(uu(i), vv(j))).collect();
        let at = |i: usize, j: usize| val[i * (nv + 1) + j];
        // a crossing on a grid edge: (i, j, along u?) -> its (u, v)
        let cross = |i: usize, j: usize, along_u: bool| -> (f64, f64) {
            let (a, bb) = if along_u { (at(i, j), at(i + 1, j)) } else { (at(i, j), at(i, j + 1)) };
            let t = if (a - bb).abs() < 1e-300 { 0.5 } else { a / (a - bb) };
            let (u, v) = if along_u { (uu(i) + (uu(i + 1) - uu(i)) * t, vv(j)) } else { (uu(i), vv(j) + (vv(j + 1) - vv(j)) * t) };
            b.wrap(u, v)
        };
        type Key = (usize, usize, bool);
        let mut segs: Vec<(Key, Key)> = Vec::new();
        for i in 0..nu {
            for j in 0..nv {
                let c = [at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)];
                let s: Vec<bool> = c.iter().map(|x| *x < 0.0).collect();
                // the cell's edges: bottom, right, top, left
                let edges: [Key; 4] = [(i, j, true), (i + 1, j, false), (i, j + 1, true), (i, j, false)];
                let crossing: Vec<usize> = (0..4).filter(|&k| s[k] != s[(k + 1) % 4]).collect();
                match crossing.len() {
                    2 => segs.push((edges[crossing[0]], edges[crossing[1]])),
                    4 => {
                        // a saddle: pair by the centre's sign
                        let mid = c.iter().sum::<f64>() / 4.0;
                        if (mid < 0.0) == s[0] {
                            segs.push((edges[0], edges[1]));
                            segs.push((edges[2], edges[3]));
                        } else {
                            segs.push((edges[0], edges[3]));
                            segs.push((edges[1], edges[2]));
                        }
                    }
                    _ => {}
                }
            }
        }
        // join segments sharing a crossing into chains
        let mut by_key: HashMap<Key, Vec<usize>> = HashMap::new();
        for (k, (a, bb)) in segs.iter().enumerate() {
            by_key.entry(*a).or_default().push(k);
            by_key.entry(*bb).or_default().push(k);
        }
        let mut used = vec![false; segs.len()];
        let mut chains: Vec<Vec<Key>> = Vec::new();
        for start in 0..segs.len() {
            if used[start] {
                continue;
            }
            used[start] = true;
            let mut chain = vec![segs[start].0, segs[start].1];
            for dir in 0..2 {
                loop {
                    let end = if dir == 0 { *chain.last().unwrap() } else { chain[0] };
                    let Some(&k) = by_key.get(&end).and_then(|v| v.iter().find(|&&k| !used[k])) else { break };
                    used[k] = true;
                    let next = if segs[k].0 == end { segs[k].1 } else { segs[k].0 };
                    if dir == 0 { chain.push(next) } else { chain.insert(0, next) }
                }
            }
            chains.push(chain);
        }
        // to the surface, split where the curve leaves the face
        let inside = |(u, v): (f64, f64)| self.contains_uv(b.to_uv(u, v));
        // where the segment from a (inside or not) to c crosses the face's
        // boundary, by bisection along it
        let edge = |a: (f64, f64), c: (f64, f64)| {
            let a_in = inside(a);
            let (mut lo, mut hi) = (0.0, 1.0);
            let at = |t: f64| (a.0 + (c.0 - a.0) * t, a.1 + (c.1 - a.1) * t);
            for _ in 0..40 {
                let mid = 0.5 * (lo + hi);
                if inside(at(mid)) == a_in {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            at(if a_in { lo } else { hi })
        };
        let mut out = Vec::new();
        for chain in chains {
            let pts: Vec<(f64, f64)> = chain.iter().map(|k| cross(k.0, k.1, k.2)).collect();
            let ins: Vec<bool> = pts.iter().map(|p| inside(*p)).collect();
            let mut cur: Vec<V3> = Vec::new();
            for k in 0..pts.len() {
                // a step across a seam of the parameters is not a step along the curve
                let jump = k > 0 && ((pts[k].0 - pts[k - 1].0).abs() > 0.5 * (u1 - u0) || (pts[k].1 - pts[k - 1].1).abs() > 0.5 * (v1 - v0));
                if ins[k] {
                    if cur.is_empty() && k > 0 && !ins[k - 1] && !jump {
                        let (u, v) = edge(pts[k - 1], pts[k]);
                        cur.push(b.eval(u, v));
                    }
                    cur.push(b.eval(pts[k].0, pts[k].1));
                } else {
                    if !cur.is_empty() && !jump {
                        let (u, v) = edge(pts[k - 1], pts[k]);
                        cur.push(b.eval(u, v));
                    }
                    if cur.len() > 1 {
                        out.push(std::mem::take(&mut cur));
                    } else {
                        cur.clear();
                    }
                }
            }
            if cur.len() > 1 {
                out.push(cur);
            }
        }
        out
    }
}

pub struct Model {
    pub verts: Vec<V3>,
    pub edges: Vec<Edge>,
    pub faces: Vec<Face>,
    pub bbox: BBox3,
    pub vert_edges: Vec<Vec<usize>>,
    /// Identifies the model's STEP text (a hash), for caching what is computed from it.
    pub key: u64,
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
            // a rational B-spline surface is a complex entity
            if let (Some(bs), Some(kn)) = (e.part("B_SPLINE_SURFACE"), e.part("B_SPLINE_SURFACE_WITH_KNOTS")) {
                let w = e.part("RATIONAL_B_SPLINE_SURFACE").and_then(|w| w.first()).map(|w| grid_nums(w));
                if let Some(b) = self.bsurf(bs.first(), bs.get(1), bs.get(2), w, kn.first(), kn.get(1), kn.get(2), kn.get(3))? {
                    return Ok(Surface { kind: SurfKind::BSpline(Box::new(b)), fr: dummy });
                }
            }
            return Ok(Surface { kind: SurfKind::Other(e.name().into()), fr: dummy });
        }
        if e.name() == "B_SPLINE_SURFACE_WITH_KNOTS"
            && let Some(b) = self.bsurf(p.get(1), p.get(2), p.get(3), None, p.get(8), p.get(9), p.get(10), p.get(11))?
        {
            return Ok(Surface { kind: SurfKind::BSpline(Box::new(b)), fr: dummy });
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
    #[allow(clippy::too_many_arguments)]
    fn bsurf(&self, du: Option<&P>, dv: Option<&P>, ctrl: Option<&P>, w: Option<Vec<Vec<f64>>>, mu: Option<&P>, mv: Option<&P>, ku: Option<&P>, kv: Option<&P>) -> Result<Option<BSurf>, String> {
        let deg = |x: Option<&P>| x.and_then(|x| x.as_f64()).unwrap_or(0.0) as usize;
        let mut rows = Vec::new();
        for row in ctrl.and_then(|c| c.as_list()).unwrap_or(&[]) {
            rows.push(self.point_list(Some(row))?);
        }
        Ok(BSurf::new(deg(du), deg(dv), rows, w, expand_knots(mu, ku), expand_knots(mv, kv)))
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

fn grid_nums(p: &P) -> Vec<Vec<f64>> {
    p.as_list().map(|l| l.iter().map(nums).collect()).unwrap_or_default()
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
        let tube = Face::tube_span(&surf, &loop_pts);
        out_faces.push(Face { surf, same_sense, loops, loop_pts, uv_loops, bbox, center, edges: fedges, tube });
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
    Ok(Model { verts, edges, faces: out_faces, bbox, vert_edges, key: 0 })
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
