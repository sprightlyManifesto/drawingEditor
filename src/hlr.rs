//! Orthographic projection with analytic hidden-line removal.
//!
//! Every model edge (plus silhouettes of curved faces) is sampled; each sample
//! casts a ray towards the viewer and is hidden if the ray meets a face.

use crate::brep::{Curve, Model, SurfKind};
use crate::geom::*;
use std::collections::HashMap;
use std::f64::consts::PI;

#[derive(Clone, Copy, Debug)]
pub struct Projector {
    pub o: V3,
    pub r: V3,
    pub u: V3,
    pub d: V3,
    pub scale: f64,
}

impl Projector {
    pub fn p(&self, q: V3) -> V2 {
        let w = q - self.o;
        v2(w.dot(self.r), w.dot(self.u)) * self.scale
    }
    /// 2D direction of a 3D vector.
    pub fn dir(&self, v: V3) -> V2 {
        v2(v.dot(self.r), v.dot(self.u))
    }
}

pub struct ViewLines {
    pub visible: Vec<Vec<V2>>,
    pub hidden: Vec<Vec<V2>>,
    pub center: Vec<(V2, V2)>,
    /// Outline of the cut faces (section views only), for hatching.
    pub cut: Vec<Vec<V2>>,
    /// Visible runs of model edges (edge index) and of silhouettes (face
    /// index), for picking.
    pub edge_runs: Vec<(usize, Vec<V2>)>,
    pub sil_runs: Vec<(usize, Vec<V2>)>,
    /// Hidden runs of model edges (edge index), when hidden lines are drawn.
    pub hidden_runs: Vec<(usize, Vec<V2>)>,
}

/// Cutting plane through `c`; material on the +`d` side is removed.
#[derive(Clone, Copy, Debug)]
pub struct Clip {
    pub c: V3,
    pub d: V3,
}

/// The cut face as seen in the view: anything behind the plane that projects
/// inside it is hidden.
struct CutFace<'a> {
    clip: Clip,
    pr: &'a Projector,
    segs: Vec<(V2, V2)>,
}

impl Clip {
    pub fn removed(&self, p: V3, eps: f64) -> bool {
        (p - self.c).dot(self.d) > eps
    }
}

fn occluded(m: &Model, p: V3, d: V3, eps: f64, clip: Option<&Clip>) -> bool {
    for f in &m.faces {
        if !f.bbox.expand(eps * 10.0).ray_hits(p, d) {
            continue;
        }
        for t in f.surf.hits(p, d) {
            let q = p + d * t;
            if t > eps && !clip.is_some_and(|c| c.removed(q, eps)) && f.contains(q) {
                return true;
            }
        }
    }
    false
}

/// 0 = cut away, 1 = visible, 2 = hidden
fn state(m: &Model, p: V3, d: V3, eps: f64, cf: Option<&CutFace>) -> u8 {
    let clip = cf.map(|c| &c.clip);
    if clip.is_some_and(|c| c.removed(p, eps)) {
        0
    } else if cf.is_some_and(|c| (p - c.clip.c).dot(c.clip.d) < -eps * 10.0 && crate::section::inside(&c.segs, c.pr.p(p))) {
        2
    } else if occluded(m, p, d, eps, clip) {
        2
    } else {
        1
    }
}

fn resample(pts: &[V3], step: f64) -> Vec<V3> {
    let mut out = vec![pts[0]];
    for w in pts.windows(2) {
        let l = w[0].dist(w[1]);
        let n = ((l / step).ceil() as usize).max(1);
        for k in 1..=n {
            out.push(w[0].lerp(w[1], k as f64 / n as f64));
        }
    }
    out
}

/// Split a 3D polyline into runs by state (see `state`).
fn classify(m: &Model, pts: &[V3], d: V3, step: f64, eps: f64, clip: Option<&CutFace>) -> Vec<(Vec<V3>, u8)> {
    let s = resample(pts, step);
    let vis: Vec<u8> = s.iter().map(|p| state(m, *p, d, eps, clip)).collect();
    let mut runs: Vec<(Vec<V3>, u8)> = vec![(vec![s[0]], vis[0])];
    for i in 1..s.len() {
        if vis[i] != vis[i - 1] {
            // bisect to locate the transition
            let (mut a, mut b) = (s[i - 1], s[i]);
            for _ in 0..10 {
                let mid = a.lerp(b, 0.5);
                if state(m, mid, d, eps, clip) == vis[i - 1] {
                    a = mid;
                } else {
                    b = mid;
                }
            }
            let t = a.lerp(b, 0.5);
            runs.last_mut().unwrap().0.push(t);
            runs.push((vec![t], vis[i]));
        }
        runs.last_mut().unwrap().0.push(s[i]);
    }
    // The first/last sample sits on a vertex where visibility is ambiguous;
    // merge single-sample runs into their neighbour.
    runs
}

fn simplify(pts: &[V2], tol: f64) -> Vec<V2> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let mut keep = vec![false; pts.len()];
    keep[0] = true;
    *keep.last_mut().unwrap() = true;
    let mut stack = vec![(0usize, pts.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let mut best = (0.0, 0);
        for i in a + 1..b {
            let dd = point_seg_dist(pts[i], pts[a], pts[b]);
            if dd > best.0 {
                best = (dd, i);
            }
        }
        if best.0 > tol {
            keep[best.1] = true;
            stack.push((a, best.1));
            stack.push((best.1, b));
        }
    }
    pts.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect()
}

fn poly_len(p: &[V2]) -> f64 {
    p.windows(2).map(|w| w[0].dist(w[1])).sum()
}

/// Silhouette curves of curved faces for view direction d.
fn silhouettes(m: &Model, d: V3) -> Vec<(usize, Vec<V3>)> {
    let mut out = Vec::new();
    for (fi, f) in m.faces.iter().enumerate() {
        let fr = &f.surf.fr;
        let (dx, dy, da) = (d.dot(fr.x), d.dot(fr.y), d.dot(fr.z));
        let (v0, v1) = f.v_range();
        if !(v1 > v0) {
            continue;
        }
        let lines: Vec<(f64, Box<dyn Fn(f64) -> V3>)> = match f.surf.kind {
            SurfKind::Cylinder { r } => {
                if da.abs() > 1.0 - 1e-6 {
                    continue;
                }
                let th = dx.atan2(-dy);
                [th, th + PI]
                    .into_iter()
                    .map(|t| {
                        let fr = *fr;
                        let f: Box<dyn Fn(f64) -> V3> =
                            Box::new(move |v| fr.o + fr.x * (r * t.cos()) + fr.y * (r * t.sin()) + fr.z * v);
                        (wrap_2pi(t), f)
                    })
                    .collect()
            }
            SurfKind::Cone { r, semi } => {
                let k = semi.tan();
                let mm = (dx * dx + dy * dy).sqrt();
                if mm < 1e-9 || (k * da / mm).abs() >= 1.0 {
                    continue;
                }
                let phi = dy.atan2(dx);
                let a = (k * da / mm).acos();
                [phi + a, phi - a]
                    .into_iter()
                    .map(|t| {
                        let fr = *fr;
                        let f: Box<dyn Fn(f64) -> V3> = Box::new(move |v| {
                            let rr = r + v * k;
                            fr.o + fr.x * (rr * t.cos()) + fr.y * (rr * t.sin()) + fr.z * v
                        });
                        (wrap_2pi(t), f)
                    })
                    .collect()
            }
            SurfKind::Sphere { r } => {
                // silhouette circle in the plane through the centre normal to d
                let a = d.any_perp();
                let b = d.cross(a);
                let n = 96;
                let mut cur: Vec<V3> = Vec::new();
                for i in 0..=n {
                    let t = i as f64 / n as f64 * std::f64::consts::TAU;
                    let p = fr.o + a * (r * t.cos()) + b * (r * t.sin());
                    if f.contains(p) {
                        cur.push(p);
                    } else if cur.len() > 1 {
                        out.push((fi, std::mem::take(&mut cur)));
                    } else {
                        cur.clear();
                    }
                }
                if cur.len() > 1 {
                    out.push((fi, cur));
                }
                continue;
            }
            _ => continue,
        };
        for (u, g) in lines {
            let n = 64;
            let mut cur: Vec<V3> = Vec::new();
            for i in 0..=n {
                let v = v0 + (v1 - v0) * i as f64 / n as f64;
                if f.contains_uv(v2(u, v)) || f.contains_uv(v2(u, v + (v1 - v0) * 1e-4)) && i == 0 || f.contains_uv(v2(u, v - (v1 - v0) * 1e-4)) && i == n {
                    cur.push(g(v));
                } else if cur.len() > 1 {
                    out.push((fi, std::mem::take(&mut cur)));
                } else {
                    cur.clear();
                }
            }
            if cur.len() > 1 {
                out.push((fi, cur));
            }
        }
    }
    out
}

struct SegGrid {
    cell: f64,
    map: HashMap<(i64, i64), Vec<(V2, V2)>>,
}

impl SegGrid {
    fn new(cell: f64) -> Self {
        SegGrid { cell, map: HashMap::new() }
    }
    fn key(&self, p: V2) -> (i64, i64) {
        ((p.x / self.cell).floor() as i64, (p.y / self.cell).floor() as i64)
    }
    fn insert(&mut self, a: V2, b: V2) {
        let bb = BBox2::new(a, b);
        let (k0, k1) = (self.key(bb.min), self.key(bb.max));
        for i in k0.0..=k1.0 {
            for j in k0.1..=k1.1 {
                self.map.entry((i, j)).or_default().push((a, b));
            }
        }
    }
    fn near(&self, p: V2, tol: f64) -> bool {
        let k = self.key(p);
        for i in k.0 - 1..=k.0 + 1 {
            for j in k.1 - 1..=k.1 + 1 {
                if let Some(v) = self.map.get(&(i, j)) {
                    if v.iter().any(|(a, b)| point_seg_dist(p, *a, *b) < tol) {
                        return true;
                    }
                }
            }
        }
        false
    }
}

/// Remove parts of `lines` lying on top of lines in `grid` (runs longer than min_len).
fn remove_covered(lines: Vec<Vec<V2>>, grid: &SegGrid, tol: f64, min_len: f64) -> Vec<Vec<V2>> {
    let mut out = Vec::new();
    for l in lines {
        // resample in 2D
        let mut s = vec![l[0]];
        for w in l.windows(2) {
            let n = ((w[0].dist(w[1]) / 0.25).ceil() as usize).max(1);
            for k in 1..=n {
                s.push(w[0].lerp(w[1], k as f64 / n as f64));
            }
        }
        let mut cov: Vec<bool> = s.iter().map(|p| grid.near(*p, tol)).collect();
        // un-cover short covered runs (crossings)
        let mut i = 0;
        while i < s.len() {
            if cov[i] {
                let st = i;
                while i < s.len() && cov[i] {
                    i += 1;
                }
                let len = poly_len(&s[st..i]);
                if len < min_len && !(st == 0 && i == s.len()) {
                    for c in cov.iter_mut().take(i).skip(st) {
                        *c = false;
                    }
                }
            } else {
                i += 1;
            }
        }
        let mut cur = Vec::new();
        for (p, c) in s.iter().zip(&cov) {
            if !c {
                cur.push(*p);
            } else if !cur.is_empty() {
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
    out.into_iter().map(|l| simplify(&l, 0.005)).filter(|l| poly_len(l) > 0.2).collect()
}

pub fn compute(m: &Model, pr: &Projector, hidden: bool, centre: bool, clip: Option<Clip>) -> ViewLines {
    let diag = m.diag();
    let step = diag / 300.0;
    let eps = diag * 1e-5;
    // seam edges (both sides on the same periodic face) are not real edges
    let mut curves: Vec<Vec<V3>> = m.edges.iter().map(|e| if e.faces.len() < 2 { Vec::new() } else { e.pts.clone() }).collect();
    let n_edges = curves.len();
    let sil = silhouettes(m, pr.d);
    let sil_face: Vec<usize> = sil.iter().map(|s| s.0).collect();
    curves.extend(sil.into_iter().map(|s| s.1));
    let (mut edge_runs, mut sil_runs, mut hidden_runs) = (Vec::new(), Vec::new(), Vec::new());
    // the cut outline lies on the plane, nearest the viewer: always visible
    let mut cut = Vec::new();
    if let Some(cl) = &clip {
        for c in crate::section::plane_cut(m, cl.c, cl.d) {
            let p2 = simplify(&c.iter().map(|p| pr.p(*p)).collect::<Vec<_>>(), 0.005);
            if poly_len(&p2) > 0.05 {
                cut.push(p2);
            }
        }
    }
    let cf = clip.map(|c| CutFace { clip: c, pr, segs: crate::section::closed_segments(&cut) });
    let mut vis2: Vec<Vec<V2>> = Vec::new();
    let mut sil_vis: Vec<Vec<V2>> = Vec::new();
    let mut hid2: Vec<Vec<V2>> = Vec::new();
    for (ci, c) in curves.iter().enumerate() {
        if c.len() < 2 {
            continue;
        }
        let proj: Vec<V2> = c.iter().map(|p| pr.p(*p)).collect();
        if poly_len(&proj) < 1e-3 {
            continue; // seen end-on
        }
        for (run, v) in classify(m, c, pr.d, step, eps, cf.as_ref()) {
            let p2: Vec<V2> = run.iter().map(|p| pr.p(*p)).collect();
            let p2 = simplify(&p2, 0.005);
            if poly_len(&p2) < 0.05 {
                continue;
            }
            match v {
                1 if ci < n_edges => {
                    edge_runs.push((ci, p2.clone()));
                    vis2.push(p2)
                }
                1 => {
                    sil_runs.push((sil_face[ci - n_edges], p2.clone()));
                    sil_vis.push(p2)
                }
                2 if hidden => {
                    if ci < n_edges {
                        hidden_runs.push((ci, p2.clone()));
                    }
                    hid2.push(p2)
                }
                _ => {}
            }
        }
    }
    let mut grid = SegGrid::new(2.0);
    for l in &cut {
        for w in l.windows(2) {
            grid.insert(w[0], w[1]);
        }
    }
    // model edges lying in the cutting plane duplicate the outline
    let mut vis2 = remove_covered(vis2, &grid, 0.05, 0.6);
    vis2.extend(cut.iter().cloned());
    let mut grid = SegGrid::new(2.0);
    for l in &vis2 {
        for w in l.windows(2) {
            grid.insert(w[0], w[1]);
        }
    }
    // silhouettes coinciding with real edges are dropped
    let sil_vis = remove_covered(sil_vis, &grid, 0.05, 0.6);
    for l in &sil_vis {
        for w in l.windows(2) {
            grid.insert(w[0], w[1]);
        }
    }
    vis2.extend(sil_vis);
    let hid2 = remove_covered(hid2, &grid, 0.08, 0.6);
    // de-duplicate hidden lines against each other
    let mut hgrid = SegGrid::new(2.0);
    let mut hid_out = Vec::new();
    for l in hid2 {
        let kept = remove_covered(vec![l], &hgrid, 0.05, 0.6);
        for k in kept {
            for w in k.windows(2) {
                hgrid.insert(w[0], w[1]);
            }
            hid_out.push(k);
        }
    }
    let center = if centre { centre_lines(m, pr, clip.as_ref(), eps) } else { vec![] };
    ViewLines { visible: vis2, hidden: hid_out, center, cut, edge_runs, sil_runs, hidden_runs }
}

fn centre_lines(m: &Model, pr: &Projector, clip: Option<&Clip>, eps: f64) -> Vec<(V2, V2)> {
    let ext = 2.5;
    let mut out: Vec<(V2, V2)> = Vec::new();
    let mut seen_c: Vec<(V2, f64)> = Vec::new();
    let mut push_circle = |c: V2, r: f64, out: &mut Vec<(V2, V2)>| {
        if seen_c.iter().any(|(q, rr)| q.dist(c) < 0.01 && (rr - r).abs() < 0.01) {
            return;
        }
        // a smaller concentric circle may already provide a mark; keep largest
        seen_c.retain(|(q, rr)| !(q.dist(c) < 0.01 && *rr < r));
        out.retain(|(a, b)| !(((*a + *b) * 0.5).dist(c) < 0.01));
        seen_c.push((c, r));
        out.push((c - v2(r + ext, 0.0), c + v2(r + ext, 0.0)));
        out.push((c - v2(0.0, r + ext), c + v2(0.0, r + ext)));
    };
    for e in &m.edges {
        if let Curve::Circle { fr, r } = &e.curve {
            if fr.z.dot(pr.d).abs() > 1.0 - 1e-6 && e.sweep.abs() >= PI - 1e-6 && !clip.is_some_and(|c| c.removed(fr.o, eps)) {
                push_circle(pr.p(fr.o), r * pr.scale, &mut out);
            }
        }
    }
    let mut axes: Vec<(V2, V2)> = Vec::new();
    for f in &m.faces {
        if let SurfKind::Cylinder { .. } | SurfKind::Cone { .. } = f.surf.kind {
            let fr = &f.surf.fr;
            if fr.z.dot(pr.d).abs() > 1e-6 || f.angular_coverage() < 0.5 {
                continue;
            }
            let (v0, v1) = f.v_range();
            if clip.is_some_and(|c| c.removed(fr.o + fr.z * ((v0 + v1) * 0.5), eps)) {
                continue;
            }
            let a = pr.p(fr.o + fr.z * v0);
            let b = pr.p(fr.o + fr.z * v1);
            let dir = (b - a).norm();
            let (a, b) = (a - dir * ext, b + dir * ext);
            // merge collinear overlapping axes
            if let Some(ax) = axes.iter_mut().find(|(p, q)| {
                point_seg_dist(a, *p - (*q - *p) * 100.0, *q + (*q - *p) * 100.0) < 0.01
                    && point_seg_dist(b, *p - (*q - *p) * 100.0, *q + (*q - *p) * 100.0) < 0.01
            }) {
                let pts = [ax.0, ax.1, a, b];
                let d0 = (ax.1 - ax.0).norm();
                let mn = pts.iter().min_by(|x, y| x.dot(d0).total_cmp(&y.dot(d0))).unwrap();
                let mx = pts.iter().max_by(|x, y| x.dot(d0).total_cmp(&y.dot(d0))).unwrap();
                *ax = (*mn, *mx);
            } else {
                axes.push((a, b));
            }
        }
    }
    out.extend(axes);
    out
}
