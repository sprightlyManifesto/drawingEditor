//! Small vector / bounding-box toolkit.

use std::ops::{Add, Div, Mul, Neg, Sub};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct V3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

pub const fn v3(x: f64, y: f64, z: f64) -> V3 {
    V3 { x, y, z }
}

impl V3 {
    pub fn dot(self, o: V3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn cross(self, o: V3) -> V3 {
        v3(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }
    pub fn len(self) -> f64 {
        self.dot(self).sqrt()
    }
    pub fn norm(self) -> V3 {
        let l = self.len();
        if l < 1e-300 { self } else { self / l }
    }
    pub fn dist(self, o: V3) -> f64 {
        (self - o).len()
    }
    pub fn lerp(self, o: V3, t: f64) -> V3 {
        self + (o - self) * t
    }
    /// Any unit vector perpendicular to self.
    pub fn any_perp(self) -> V3 {
        let a = if self.x.abs() < 0.9 { v3(1.0, 0.0, 0.0) } else { v3(0.0, 1.0, 0.0) };
        (a - self * a.dot(self)).norm()
    }
}

impl Add for V3 {
    type Output = V3;
    fn add(self, o: V3) -> V3 {
        v3(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}
impl Sub for V3 {
    type Output = V3;
    fn sub(self, o: V3) -> V3 {
        v3(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}
impl Mul<f64> for V3 {
    type Output = V3;
    fn mul(self, s: f64) -> V3 {
        v3(self.x * s, self.y * s, self.z * s)
    }
}
impl Div<f64> for V3 {
    type Output = V3;
    fn div(self, s: f64) -> V3 {
        v3(self.x / s, self.y / s, self.z / s)
    }
}
impl Neg for V3 {
    type Output = V3;
    fn neg(self) -> V3 {
        v3(-self.x, -self.y, -self.z)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct V2 {
    pub x: f64,
    pub y: f64,
}

pub const fn v2(x: f64, y: f64) -> V2 {
    V2 { x, y }
}

impl V2 {
    pub fn dot(self, o: V2) -> f64 {
        self.x * o.x + self.y * o.y
    }
    pub fn cross(self, o: V2) -> f64 {
        self.x * o.y - self.y * o.x
    }
    pub fn len(self) -> f64 {
        self.dot(self).sqrt()
    }
    pub fn norm(self) -> V2 {
        let l = self.len();
        if l < 1e-300 { self } else { self / l }
    }
    pub fn dist(self, o: V2) -> f64 {
        (self - o).len()
    }
    /// Rotate +90 degrees.
    pub fn perp(self) -> V2 {
        v2(-self.y, self.x)
    }
    pub fn angle(self) -> f64 {
        self.y.atan2(self.x)
    }
    pub fn from_angle(a: f64) -> V2 {
        v2(a.cos(), a.sin())
    }
    pub fn rot(self, a: f64) -> V2 {
        let (s, c) = a.sin_cos();
        v2(self.x * c - self.y * s, self.x * s + self.y * c)
    }
    pub fn lerp(self, o: V2, t: f64) -> V2 {
        self + (o - self) * t
    }
}

impl Add for V2 {
    type Output = V2;
    fn add(self, o: V2) -> V2 {
        v2(self.x + o.x, self.y + o.y)
    }
}
impl Sub for V2 {
    type Output = V2;
    fn sub(self, o: V2) -> V2 {
        v2(self.x - o.x, self.y - o.y)
    }
}
impl Mul<f64> for V2 {
    type Output = V2;
    fn mul(self, s: f64) -> V2 {
        v2(self.x * s, self.y * s)
    }
}
impl Div<f64> for V2 {
    type Output = V2;
    fn div(self, s: f64) -> V2 {
        v2(self.x / s, self.y / s)
    }
}
impl Neg for V2 {
    type Output = V2;
    fn neg(self) -> V2 {
        v2(-self.x, -self.y)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BBox2 {
    pub min: V2,
    pub max: V2,
}

impl Default for BBox2 {
    fn default() -> Self {
        Self::empty()
    }
}

impl BBox2 {
    pub fn empty() -> Self {
        BBox2 { min: v2(f64::INFINITY, f64::INFINITY), max: v2(f64::NEG_INFINITY, f64::NEG_INFINITY) }
    }
    pub fn new(a: V2, b: V2) -> Self {
        BBox2 { min: v2(a.x.min(b.x), a.y.min(b.y)), max: v2(a.x.max(b.x), a.y.max(b.y)) }
    }
    pub fn from_center(c: V2, w: f64, h: f64) -> Self {
        BBox2 { min: v2(c.x - w / 2.0, c.y - h / 2.0), max: v2(c.x + w / 2.0, c.y + h / 2.0) }
    }
    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x || self.min.y > self.max.y
    }
    pub fn add(&mut self, p: V2) {
        self.min.x = self.min.x.min(p.x);
        self.min.y = self.min.y.min(p.y);
        self.max.x = self.max.x.max(p.x);
        self.max.y = self.max.y.max(p.y);
    }
    pub fn union(&self, o: &BBox2) -> BBox2 {
        if o.is_empty() {
            return *self;
        }
        if self.is_empty() {
            return *o;
        }
        BBox2 {
            min: v2(self.min.x.min(o.min.x), self.min.y.min(o.min.y)),
            max: v2(self.max.x.max(o.max.x), self.max.y.max(o.max.y)),
        }
    }
    pub fn expand(&self, d: f64) -> BBox2 {
        if self.is_empty() {
            return *self;
        }
        BBox2 { min: self.min - v2(d, d), max: self.max + v2(d, d) }
    }
    pub fn translate(&self, d: V2) -> BBox2 {
        if self.is_empty() {
            return *self;
        }
        BBox2 { min: self.min + d, max: self.max + d }
    }
    pub fn overlaps(&self, o: &BBox2) -> bool {
        !self.is_empty()
            && !o.is_empty()
            && self.min.x < o.max.x
            && o.min.x < self.max.x
            && self.min.y < o.max.y
            && o.min.y < self.max.y
    }
    pub fn contains(&self, p: V2) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }
    pub fn center(&self) -> V2 {
        (self.min + self.max) * 0.5
    }
    pub fn width(&self) -> f64 {
        self.max.x - self.min.x
    }
    pub fn height(&self) -> f64 {
        self.max.y - self.min.y
    }
    /// Closest point of the box to p.
    pub fn clamp(&self, p: V2) -> V2 {
        v2(p.x.clamp(self.min.x, self.max.x), p.y.clamp(self.min.y, self.max.y))
    }
    /// Does the segment a-b touch the box?
    pub fn hits_segment(&self, a: V2, b: V2) -> bool {
        if self.contains(a) || self.contains(b) {
            return true;
        }
        let corners = [self.min, v2(self.max.x, self.min.y), self.max, v2(self.min.x, self.max.y)];
        (0..4).any(|i| segments_intersect(a, b, corners[i], corners[(i + 1) % 4]))
    }
}

pub fn segments_intersect(a: V2, b: V2, c: V2, d: V2) -> bool {
    let d1 = (b - a).cross(c - a);
    let d2 = (b - a).cross(d - a);
    let d3 = (d - c).cross(a - c);
    let d4 = (d - c).cross(b - c);
    (d1 * d2 < 0.0) && (d3 * d4 < 0.0)
}

pub fn point_seg_dist(p: V2, a: V2, b: V2) -> f64 {
    let ab = b - a;
    let l2 = ab.dot(ab);
    if l2 < 1e-24 {
        return p.dist(a);
    }
    let t = ((p - a).dot(ab) / l2).clamp(0.0, 1.0);
    p.dist(a + ab * t)
}

#[derive(Clone, Copy, Debug)]
pub struct BBox3 {
    pub min: V3,
    pub max: V3,
}

impl Default for BBox3 {
    fn default() -> Self {
        BBox3 {
            min: v3(f64::INFINITY, f64::INFINITY, f64::INFINITY),
            max: v3(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
        }
    }
}

impl BBox3 {
    pub fn add(&mut self, p: V3) {
        self.min = v3(self.min.x.min(p.x), self.min.y.min(p.y), self.min.z.min(p.z));
        self.max = v3(self.max.x.max(p.x), self.max.y.max(p.y), self.max.z.max(p.z));
    }
    pub fn center(&self) -> V3 {
        (self.min + self.max) * 0.5
    }
    pub fn diag(&self) -> f64 {
        (self.max - self.min).len()
    }
    pub fn expand(&self, d: f64) -> BBox3 {
        BBox3 { min: self.min - v3(d, d, d), max: self.max + v3(d, d, d) }
    }
    /// Does the ray p + t*d (t > 0) pass through the box?
    pub fn ray_hits(&self, p: V3, d: V3) -> bool {
        let mut t0 = 0.0f64;
        let mut t1 = f64::INFINITY;
        for (o, dir, lo, hi) in [
            (p.x, d.x, self.min.x, self.max.x),
            (p.y, d.y, self.min.y, self.max.y),
            (p.z, d.z, self.min.z, self.max.z),
        ] {
            if dir.abs() < 1e-15 {
                if o < lo || o > hi {
                    return false;
                }
            } else {
                let mut a = (lo - o) / dir;
                let mut b = (hi - o) / dir;
                if a > b {
                    std::mem::swap(&mut a, &mut b);
                }
                t0 = t0.max(a);
                t1 = t1.min(b);
                if t0 > t1 {
                    return false;
                }
            }
        }
        true
    }
}

/// Wrap an angle into [0, 2pi).
pub fn wrap_2pi(a: f64) -> f64 {
    let t = std::f64::consts::TAU;
    let r = a % t;
    if r < 0.0 { r + t } else { r }
}

/// Wrap an angle difference into (-pi, pi].
pub fn wrap_pi(a: f64) -> f64 {
    let mut r = wrap_2pi(a);
    if r > std::f64::consts::PI {
        r -= std::f64::consts::TAU;
    }
    r
}

/// Parts of the polyline `pts` inside the circle (centre `c`, radius `r`).
/// Signed area of a closed polygon.
pub fn poly_area(p: &[V2]) -> f64 {
    let n = p.len();
    (0..n).map(|i| p[i].cross(p[(i + 1) % n])).sum::<f64>() * 0.5
}

/// Polygon `p` clipped to the circle (c, r) (Sutherland–Hodgman against a
/// 64-gon), for regions of detail views.
pub fn clip_region_circle(p: &[V2], c: V2, r: f64) -> Vec<V2> {
    let n = 64;
    let ring: Vec<V2> = (0..n).map(|i| c + V2::from_angle(i as f64 / n as f64 * std::f64::consts::TAU) * r).collect();
    let mut out = p.to_vec();
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        let inside = |q: V2| (b - a).cross(q - a) >= 0.0;
        let input = std::mem::take(&mut out);
        for k in 0..input.len() {
            let (cur, prev) = (input[k], input[(k + input.len() - 1) % input.len()]);
            match (inside(prev), inside(cur)) {
                (true, true) => out.push(cur),
                (true, false) => out.push(seg_line(prev, cur, a, b)),
                (false, true) => {
                    out.push(seg_line(prev, cur, a, b));
                    out.push(cur);
                }
                _ => {}
            }
        }
        if out.is_empty() {
            break;
        }
    }
    out
}

/// Intersection of segment p0-p1 with the line through a, b.
fn seg_line(p0: V2, p1: V2, a: V2, b: V2) -> V2 {
    let d = p1 - p0;
    let e = b - a;
    let den = e.cross(d);
    if den.abs() < 1e-15 {
        return p0;
    }
    let t = e.cross(a - p0) / den;
    p0 + d * t
}

pub fn clip_poly_circle(pts: &[V2], c: V2, r: f64) -> Vec<Vec<V2>> {
    let mut out: Vec<Vec<V2>> = Vec::new();
    let mut cur: Vec<V2> = Vec::new();
    for w in pts.windows(2) {
        let (p, q) = (w[0], w[1]);
        let d = q - p;
        let a = d.dot(d);
        if a < 1e-18 {
            continue;
        }
        let f = p - c;
        let b = 2.0 * f.dot(d);
        let disc = b * b - 4.0 * a * (f.dot(f) - r * r);
        let mut ts = vec![0.0];
        if disc > 0.0 {
            let s = disc.sqrt();
            for t in [(-b - s) / (2.0 * a), (-b + s) / (2.0 * a)] {
                if t > 0.0 && t < 1.0 {
                    ts.push(t);
                }
            }
        }
        ts.push(1.0);
        for k in 0..ts.len() - 1 {
            let (a0, a1) = (p + d * ts[k], p + d * ts[k + 1]);
            if (p + d * (0.5 * (ts[k] + ts[k + 1]))).dist(c) <= r {
                if cur.last().is_none_or(|l| l.dist(a0) > 1e-9) {
                    if cur.len() > 1 {
                        out.push(std::mem::take(&mut cur));
                    }
                    cur.clear();
                    cur.push(a0);
                }
                cur.push(a1);
            } else if !cur.is_empty() {
                if cur.len() > 1 {
                    out.push(std::mem::take(&mut cur));
                }
                cur.clear();
            }
        }
    }
    if cur.len() > 1 {
        out.push(cur);
    }
    out
}
