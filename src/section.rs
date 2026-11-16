//! Section views: plane/solid intersection, hatching and cutting-plane symbols.

use crate::brep::{Face, Model, SurfKind};
use crate::geom::*;
use crate::hlr::Projector;
use crate::svg::{Anchor, Prim};
use std::f64::consts::TAU;

/// Trace the curve g(t), t in [t0, t1], keeping the parts inside the face.
fn trace(f: &Face, g: &dyn Fn(f64) -> Option<V3>, t0: f64, t1: f64, n: usize, periodic: bool) -> Vec<Vec<V3>> {
    let inside = |t: f64| g(t).is_some_and(|p| f.contains(p));
    let ts: Vec<f64> = (0..=n).map(|i| t0 + (t1 - t0) * i as f64 / n as f64).collect();
    let ins: Vec<bool> = ts.iter().map(|t| inside(*t)).collect();
    let edge = |a: f64, b: f64, a_in: bool| {
        let (mut a, mut b) = (a, b);
        for _ in 0..30 {
            let m = 0.5 * (a + b);
            if inside(m) == a_in {
                a = m;
            } else {
                b = m;
            }
        }
        0.5 * (a + b)
    };
    let mut runs: Vec<Vec<f64>> = Vec::new();
    let mut cur: Vec<f64> = Vec::new();
    for i in 0..ts.len() {
        if ins[i] {
            if cur.is_empty() && i > 0 {
                cur.push(edge(ts[i - 1], ts[i], false));
            }
            cur.push(ts[i]);
        } else if !cur.is_empty() {
            cur.push(edge(ts[i - 1], ts[i], true));
            runs.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        runs.push(cur);
    }
    // a periodic curve whose run crosses the seam is joined
    if periodic && runs.len() > 1 && ins[0] && ins[n] {
        let first = runs.remove(0);
        runs.last_mut().unwrap().extend(first.into_iter().skip(1));
    }
    runs.into_iter()
        .map(|r| r.into_iter().filter_map(|t| g(t)).collect::<Vec<V3>>())
        .filter(|r| r.len() > 1)
        .collect()
}

/// Intersection curves of the plane (point c, normal n) with the model's faces.
pub fn plane_cut(m: &Model, c: V3, n: V3) -> Vec<Vec<V3>> {
    let mut out = Vec::new();
    for (fi, f) in m.faces.iter().enumerate() {
        // quick reject: whole face on one side
        let b = f.bbox;
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for i in 0..8 {
            let p = v3(
                if i & 1 == 0 { b.min.x } else { b.max.x },
                if i & 2 == 0 { b.min.y } else { b.max.y },
                if i & 4 == 0 { b.min.z } else { b.max.z },
            );
            let s = (p - c).dot(n);
            lo = lo.min(s);
            hi = hi.max(s);
        }
        let tol = m.diag() * 1e-7;
        if lo > tol || hi < -tol {
            continue;
        }
        let fr = f.surf.fr;
        let k = (c - fr.o).dot(n);
        match f.surf.kind {
            SurfKind::Plane => {
                let dir = fr.z.cross(n);
                if dir.len() < 1e-9 {
                    continue; // face lies in (or parallel to) the cutting plane
                }
                let (h1, h2) = (fr.z.dot(fr.o), n.dot(c));
                let p0 = (n.cross(dir) * h1 + dir.cross(fr.z) * h2) / dir.dot(dir);
                let dir = dir.norm();
                let mut t0 = f64::INFINITY;
                let mut t1 = f64::NEG_INFINITY;
                for i in 0..8 {
                    let p = v3(
                        if i & 1 == 0 { b.min.x } else { b.max.x },
                        if i & 2 == 0 { b.min.y } else { b.max.y },
                        if i & 4 == 0 { b.min.z } else { b.max.z },
                    );
                    let t = (p - p0).dot(dir);
                    t0 = t0.min(t);
                    t1 = t1.max(t);
                }
                let g = move |t: f64| Some(p0 + dir * t);
                out.extend(trace(f, &g, t0 - 1e-3, t1 + 1e-3, 400, false));
            }
            SurfKind::Cylinder { r } | SurfKind::Cone { r, .. } => {
                let kk = if let SurfKind::Cone { semi, .. } = f.surf.kind { semi.tan() } else { 0.0 };
                let bz = fr.z.dot(n);
                let a_of = move |t: f64| (fr.x * t.cos() + fr.y * t.sin()).dot(n);
                let at = move |t: f64, v: f64| {
                    let rr = r + v * kk;
                    fr.o + fr.x * (rr * t.cos()) + fr.y * (rr * t.sin()) + fr.z * v
                };
                if bz.abs() > 1e-6 {
                    let g = move |t: f64| {
                        let a = a_of(t);
                        let den = kk * a + bz;
                        if den.abs() < 1e-9 {
                            return None;
                        }
                        let v = (k - r * a) / den;
                        if r + v * kk < 0.0 {
                            return None;
                        }
                        Some(at(t, v))
                    };
                    out.extend(trace(f, &g, 0.0, TAU, 360, true));
                } else {
                    // plane parallel to the axis: at height v the circle of radius
                    // rr meets it where cos(t - phi) = k / (rr m): two straight
                    // lines on a cylinder (or on a cone cut through its axis),
                    // otherwise the two halves of a hyperbola
                    let (ax, ay) = (fr.x.dot(n), fr.y.dot(n));
                    let mm = ax.hypot(ay);
                    if mm < 1e-12 {
                        continue;
                    }
                    let phi = ay.atan2(ax);
                    let (v0, v1) = f.v_range();
                    for sg in [1.0, -1.0] {
                        let t_at = move |v: f64| {
                            let rr = r + v * kk;
                            let q = k / (rr * mm);
                            (rr > 0.0 && q.abs() <= 1.0).then(|| phi + sg * q.acos())
                        };
                        let g = move |v: f64| t_at(v).map(|t| at(t, v));
                        let mut runs = trace(f, &g, v0 - 1e-3, v1 + 1e-3, 100, false);
                        // a line lying on the face's seam (or boundary) can fail the
                        // in-face test; retry just either side of it
                        for eps in [1e-6, -1e-6] {
                            if runs.is_empty() {
                                let g = move |v: f64| t_at(v).map(|t| at(t + eps, v));
                                runs = trace(f, &g, v0 - 1e-3, v1 + 1e-3, 100, false);
                            }
                        }
                        out.extend(runs);
                    }
                }
            }
            SurfKind::Sphere { r } => {
                let dd = (fr.o - c).dot(n);
                if dd.abs() >= r {
                    continue;
                }
                let q0 = fr.o - n * dd;
                let rr = (r * r - dd * dd).sqrt();
                let a = n.any_perp();
                let bb = n.cross(a);
                let g = move |t: f64| Some(q0 + a * (rr * t.cos()) + bb * (rr * t.sin()));
                out.extend(trace(f, &g, 0.0, TAU, 240, true));
            }
            SurfKind::Torus { major, minor } => {
                // point at angle u around the axis and w around the tube, kept
                // to the arc of tube angles the face covers (its (u, v) test
                // can't tell the inside of the tube from the outside)
                let (w0, span) = tube_span(f, major);
                let on = move |w: f64| wrap_2pi(w - w0) <= span + 1e-6;
                let e = move |u: f64| fr.x * u.cos() + fr.y * u.sin();
                let at = move |u: f64, w: f64| fr.o + e(u) * (major + minor * w.cos()) + fr.z * (minor * w.sin());
                let bz = fr.z.dot(n);
                let (ax, ay) = (fr.x.dot(n), fr.y.dot(n));
                if bz.abs() < 1e-6 {
                    // plane along the axis: for each tube angle w the ring of
                    // radius rho meets it where cos(u - phi) = k / (rho m)
                    // (a plane through the axis: two tube circles)
                    let mm = ax.hypot(ay);
                    if mm < 1e-12 {
                        continue;
                    }
                    let phi = ay.atan2(ax);
                    for sg in [1.0, -1.0] {
                        let g = move |w: f64| {
                            if !on(w) {
                                return None;
                            }
                            let q = k / ((major + minor * w.cos()) * mm);
                            if q.abs() > 1.0 {
                                return None;
                            }
                            Some(at(phi + sg * q.acos(), w))
                        };
                        out.extend(trace(f, &g, 0.0, TAU, 360, true));
                    }
                } else {
                    // for each angle u: the tube circle there meets the plane
                    // where A cos w + B sin w = C
                    for sg in [1.0, -1.0] {
                        let g = move |u: f64| {
                            let en = e(u).dot(n);
                            let (aa, bb, cc) = (en * minor, bz * minor, k - en * major);
                            let h = aa.hypot(bb);
                            if h < 1e-12 || (cc / h).abs() > 1.0 {
                                return None;
                            }
                            let w = bb.atan2(aa) + sg * (cc / h).acos();
                            if !on(w) {
                                return None;
                            }
                            Some(at(u, w))
                        };
                        out.extend(trace(f, &g, 0.0, TAU, 720, true));
                    }
                }
            }
            SurfKind::Other(_) => out.extend(other_cut(m, fi, c, n)),
        }
    }
    out
}

/// Cut of a face whose surface can't be evaluated (a B-spline fillet): where
/// its boundary edges cross the plane, nearby crossings on different edges
/// are joined by a curve leaving each tangent to the face on the far side of
/// that edge (a straight line when that face's normal isn't known).
fn other_cut(m: &Model, fi: usize, c: V3, n: V3) -> Vec<Vec<V3>> {
    let f = &m.faces[fi];
    let side = |p: V3| (p - c).dot(n);
    // (edge, point, tangent of the cut on the neighbouring face)
    let mut xs: Vec<(usize, V3, Option<V3>)> = Vec::new();
    for &ei in &f.edges {
        let e = &m.edges[ei];
        for w in e.pts.windows(2) {
            let (sa, sb) = (side(w[0]), side(w[1]));
            if (sa < 0.0) == (sb < 0.0) || sa == sb {
                continue;
            }
            let p = w[0] + (w[1] - w[0]) * (sa / (sa - sb));
            if xs.iter().any(|x| x.1.dist(p) < 1e-6) {
                continue;
            }
            let t = e.faces.iter().filter(|&&g| g != fi).find_map(|&g| m.faces[g].surf.normal_at(p)).map(|nb| n.cross(nb)).filter(|t| t.len() > 1e-9).map(|t| t.norm());
            xs.push((ei, p, t));
        }
    }
    let reach = f.bbox.max.dist(f.bbox.min) * 0.25;
    let mut used = vec![false; xs.len()];
    let mut out = Vec::new();
    loop {
        // the closest pair left on different edges
        let mut best: Option<(f64, usize, usize)> = None;
        for i in 0..xs.len() {
            for j in i + 1..xs.len() {
                if used[i] || used[j] || xs[i].0 == xs[j].0 {
                    continue;
                }
                let dd = xs[i].1.dist(xs[j].1);
                if dd < reach && best.is_none_or(|b| dd < b.0) {
                    best = Some((dd, i, j));
                }
            }
        }
        let Some((dd, i, j)) = best else { break };
        used[i] = true;
        used[j] = true;
        let (p0, p1) = (xs[i].1, xs[j].1);
        // control point: where the two tangent lines meet
        let ctrl = match (xs[i].2, xs[j].2) {
            (Some(t0), Some(t1)) => {
                let t0 = if t0.dot(p1 - p0) < 0.0 { -t0 } else { t0 };
                let t1 = if t1.dot(p0 - p1) < 0.0 { -t1 } else { t1 };
                // p0 + s t0 = p1 + u t1, solved in the plane
                let den = t0.cross(t1).dot(n);
                if den.abs() < 1e-9 {
                    None
                } else {
                    let s = (p1 - p0).cross(t1).dot(n) / den;
                    let q = p0 + t0 * s;
                    (s > 0.0 && s < 2.0 * dd && (q - p1).dot(t1) > 0.0).then_some(q)
                }
            }
            _ => None,
        };
        let pts: Vec<V3> = match ctrl {
            Some(q) => (0..=16)
                .map(|k| {
                    let t = k as f64 / 16.0;
                    p0 * ((1.0 - t) * (1.0 - t)) + q * (2.0 * t * (1.0 - t)) + p1 * (t * t)
                })
                .collect(),
            None => vec![p0, p1],
        };
        out.push(pts);
    }
    out
}

/// The arc of tube angles a torus face covers: (start, length), the shortest
/// arc holding all its boundary points.
fn tube_span(f: &Face, major: f64) -> (f64, f64) {
    let fr = f.surf.fr;
    let mut ws: Vec<f64> = f
        .loop_pts
        .iter()
        .flatten()
        .map(|p| {
            let q = fr.local(*p);
            wrap_2pi(q.z.atan2((q.x * q.x + q.y * q.y).sqrt() - major))
        })
        .collect();
    if ws.is_empty() {
        return (0.0, TAU);
    }
    ws.sort_by(|a, b| a.total_cmp(b));
    let (mut gap, mut w0) = (ws[0] + TAU - ws[ws.len() - 1], ws[0]);
    for i in 1..ws.len() {
        if ws[i] - ws[i - 1] > gap {
            gap = ws[i] - ws[i - 1];
            w0 = ws[i];
        }
    }
    (w0, (TAU - gap).max(1e-9))
}

/// Boundary segments of the region bounded by `loops` (open polylines which
/// together form closed loops); nearly coincident end points are snapped.
pub fn closed_segments(loops: &[Vec<V2>]) -> Vec<(V2, V2)> {
    let mut loops: Vec<Vec<V2>> = loops.to_vec();
    let mut ends: Vec<V2> = Vec::new();
    for l in &loops {
        ends.push(l[0]);
        ends.push(*l.last().unwrap());
    }
    let snap = |p: V2| -> V2 {
        let near: Vec<V2> = ends.iter().copied().filter(|q| q.dist(p) < 0.05).collect();
        near.iter().fold(V2::default(), |a, q| a + *q) / near.len().max(1) as f64
    };
    for l in loops.iter_mut() {
        let n = l.len();
        l[0] = snap(l[0]);
        l[n - 1] = snap(l[n - 1]);
    }
    loops
        .iter()
        .flat_map(|l| {
            let mut v: Vec<(V2, V2)> = l.windows(2).map(|w| (w[0], w[1])).collect();
            if l.len() > 2 && l[0].dist(*l.last().unwrap()) < 0.05 && l[0] != *l.last().unwrap() {
                v.push((*l.last().unwrap(), l[0]));
            }
            v
        })
        .collect()
}

/// Even-odd point-in-region test against boundary segments.
pub fn inside(segs: &[(V2, V2)], p: V2) -> bool {
    let mut ins = false;
    for (a, b) in segs {
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            ins = !ins;
        }
    }
    ins
}

/// Even-odd hatching of the region bounded by `loops` (open polylines which
/// together form closed loops).
pub fn hatch(loops: &[Vec<V2>], angle_deg: f64, spacing: f64) -> Vec<(V2, V2)> {
    let a = angle_deg.to_radians();
    let segs: Vec<(V2, V2)> = closed_segments(loops).into_iter().map(|(p, q)| (p.rot(-a), q.rot(-a))).collect();
    let mut bb = BBox2::empty();
    for (p, q) in &segs {
        bb.add(*p);
        bb.add(*q);
    }
    let mut out = Vec::new();
    if bb.is_empty() {
        return out;
    }
    let mut y = bb.min.y + spacing * 0.37;
    while y < bb.max.y {
        let mut xs: Vec<f64> = segs
            .iter()
            .filter(|(p, q)| (p.y <= y && y < q.y) || (q.y <= y && y < p.y))
            .map(|(p, q)| p.x + (y - p.y) / (q.y - p.y) * (q.x - p.x))
            .collect();
        xs.sort_by(|a, b| a.total_cmp(b));
        for pair in xs.chunks(2) {
            if pair.len() == 2 && pair[1] - pair[0] > 0.05 {
                out.push((v2(pair[0], y).rot(a), v2(pair[1], y).rot(a)));
            }
        }
        y += spacing;
    }
    out
}

/// Cutting-plane symbol in the parent view: chain line with thick ends,
/// arrows in the viewing direction and the section letter at each end.
pub fn cut_line(pr: &Projector, geom: BBox2, c: V3, dsec: V3, label: &str, h: f64, arrow_len: f64, arrow_w: f64) -> Option<Vec<Prim>> {
    let sight = -pr.dir(dsec);
    if sight.len() < 0.99 {
        return None; // the cutting plane is not seen edge-on in this view
    }
    let sight = sight.norm();
    let t = sight.perp();
    let p0 = pr.p(c);
    // span of the line across the geometry
    let (mut lo, mut hi) = (f64::NEG_INFINITY, f64::INFINITY);
    for (o, d, a, b) in [(p0.x, t.x, geom.min.x, geom.max.x), (p0.y, t.y, geom.min.y, geom.max.y)] {
        if d.abs() < 1e-12 {
            continue;
        }
        let (u, v) = ((a - o) / d, (b - o) / d);
        lo = lo.max(u.min(v));
        hi = hi.min(u.max(v));
    }
    if !lo.is_finite() || !hi.is_finite() || lo > hi {
        lo = -20.0;
        hi = 20.0;
    }
    let ov = 6.0;
    let (e0, e1) = (p0 + t * (lo - ov), p0 + t * (hi + ov));
    let thick = 7.0;
    let mut v = vec![
        Prim::Line { a: e0, b: e1, cls: "cutplane" },
        Prim::Line { a: e0, b: e0 + t * thick, cls: "cutthick" },
        Prim::Line { a: e1 - t * thick, b: e1, cls: "cutthick" },
    ];
    for (e, inward) in [(e0, t), (e1, -t)] {
        let base = e + inward * 1.5;
        let tip = base + sight * 8.0;
        let n = sight.perp() * (arrow_w * 0.5);
        let back = tip - sight * arrow_len;
        v.push(Prim::Line { a: base, b: back, cls: "thin" });
        v.push(Prim::Fill { pts: vec![tip, back + n, back - n] });
        let lp = tip + sight * (1.0 + h * 0.6) - inward * 0.0;
        v.push(Prim::Text { p: v2(lp.x, lp.y - h * 0.4), h: h * 1.2, anchor: Anchor::Middle, rot: 0.0, s: label.to_string(), cls: "title" });
    }
    Some(v)
}
