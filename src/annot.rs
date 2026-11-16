//! Dimensions, GD&T frames, datum symbols and notes for one view.
//!
//! All coordinates are view-local sheet millimetres (y up), origin at the
//! projected model centre.

use crate::brep::{Curve, Model, SurfKind};
use crate::config::*;
use crate::geom::*;
use crate::hlr::Projector;
use crate::selector::{select, Kind};
use crate::svg::{bbox_of, text_width, vec_attr, Anchor, Prim};
use std::f64::consts::PI;

/// Geometric meaning of one selected object.
#[derive(Clone, Debug)]
pub struct RefGeo {
    pub rep: V3,
    pub ext: Vec<V3>,
    pub circ: Option<Circ>,
    pub line: Option<(V3, V3)>,
    pub plane_n: Option<V3>,
}

#[derive(Clone, Debug)]
pub struct Circ {
    pub c: V3,
    pub axis: V3,
    pub r: f64,
    pub pts: Vec<V3>,
    pub v_range: (f64, f64),
    pub full: bool,
}

pub fn resolve(m: &Model, s: &str) -> Result<Vec<RefGeo>, String> {
    let sel = select(m, s)?;
    let mut out = Vec::new();
    for &i in &sel.ids {
        match sel.kind {
            Kind::Vertex => out.push(RefGeo { rep: m.verts[i], ext: vec![m.verts[i]], circ: None, line: None, plane_n: None }),
            Kind::Edge => {
                let e = &m.edges[i];
                let mut g = RefGeo { rep: e.center, ext: e.pts.clone(), circ: None, line: None, plane_n: None };
                match &e.curve {
                    Curve::Circle { fr, r } => {
                        g.rep = fr.o;
                        g.circ = Some(Circ {
                            c: fr.o,
                            axis: fr.z,
                            r: *r,
                            pts: e.pts.clone(),
                            v_range: (0.0, 0.0),
                            full: e.sweep.abs() > 2.0 * PI - 1e-6,
                        });
                    }
                    Curve::Line { .. } => {
                        g.line = Some((m.verts[e.v0], m.verts[e.v1]));
                    }
                    _ => {}
                }
                out.push(g);
            }
            Kind::Face => {
                let f = &m.faces[i];
                let ext: Vec<V3> = f.loop_pts.iter().flatten().copied().collect();
                let mut g = RefGeo { rep: f.center, ext, circ: None, line: None, plane_n: f.plane_normal() };
                if let SurfKind::Cylinder { r } = f.surf.kind {
                    let (v0, v1) = f.v_range();
                    let fr = &f.surf.fr;
                    let c = fr.o + fr.z * ((v0 + v1) * 0.5);
                    g.rep = c;
                    g.circ = Some(Circ {
                        c,
                        axis: fr.z,
                        r,
                        pts: g.ext.clone(),
                        v_range: (v0 - (v0 + v1) * 0.5, v1 - (v0 + v1) * 0.5),
                        full: f.angular_coverage() > 0.97,
                    });
                }
                out.push(g);
            }
        }
    }
    Ok(out)
}

#[derive(Clone, Debug)]
pub struct DimText {
    pub main: String,
    /// stacked upper / lower text after `main` (deviations or limits)
    pub plus: Option<String>,
    pub minus: Option<String>,
    /// the stack holds limits (larger text, sitting on the baseline)
    pub limits: bool,
    /// text after the stack
    pub tail: String,
    /// basic (theoretically exact) dimension: boxed
    pub basic: bool,
    /// feature control frames attached to the dimension: (prims with the
    /// frame's left-middle at the origin, width)
    pub frames: Vec<(Vec<Prim>, f64, Option<String>)>,
    /// datum feature symbol attached to the dimension
    pub datum: Option<String>,
    /// where the dimension (or leader shoulder) line is, across the text,
    /// relative to the baseline; frames go on the other side of the text
    pub line_y: f64,
    /// more lines of text (merged notes, thread callouts), read after `main`:
    /// between it and the line when the line is below the text, else beyond it
    pub extra: Vec<String>,
    /// "flipArrows": the arrows the other way round from where they'd go
    pub flip: bool,
}

/// Copy of `p` with every point mapped by `f` and text turned by `rot` degrees.
fn map_prim(p: &Prim, f: &dyn Fn(V2) -> V2, rot: f64) -> Prim {
    match p {
        Prim::Line { a, b, cls } => Prim::Line { a: f(*a), b: f(*b), cls },
        Prim::Poly { pts, closed, cls } => Prim::Poly { pts: pts.iter().map(|q| f(*q)).collect(), closed: *closed, cls },
        Prim::Fill { pts } => Prim::Fill { pts: pts.iter().map(|q| f(*q)).collect() },
        Prim::Circle { c, r, cls } => Prim::Circle { c: f(*c), r: *r, cls },
        Prim::Rect { b, cls } => Prim::Poly {
            pts: [b.min, v2(b.max.x, b.min.y), b.max, v2(b.min.x, b.max.y)].iter().map(|q| f(*q)).collect(),
            closed: true,
            cls,
        },
        Prim::Text { p, h, anchor, rot: r, s, cls } => Prim::Text { p: f(*p), h: *h, anchor: *anchor, rot: r + rot, s: s.clone(), cls },
        Prim::Area { loops, cls } => Prim::Area { loops: loops.iter().map(|l| l.iter().map(|q| f(*q)).collect()).collect(), cls },
        Prim::Group { attrs, pts, items } => Prim::Group {
            attrs: attrs.clone(),
            pts: pts.iter().map(|(k, q)| (*k, f(*q))).collect(),
            items: items.iter().map(|p| map_prim(p, f, rot)).collect(),
        },
    }
}

/// Part of an annotation group, for the drawing editor: "text" / "sym" move
/// with the annotation, "static" moves with the dimension line, "ext" /
/// "lead" / "dyn" are redrawn from the annotation's data.
fn role(r: &str, items: Vec<Prim>) -> Prim {
    Prim::Group { attrs: vec![("data-role", r.to_string())], pts: vec![], items }
}

/// One annotation as a group carrying what it is attached to.
fn ann(kind: &str, mut attrs: Vec<(&'static str, String)>, pts: Vec<(&'static str, V2)>, items: Vec<Prim>) -> Prim {
    attrs.insert(0, ("class", "ann".into()));
    attrs.insert(1, ("data-kind", kind.into()));
    Prim::Group { attrs, pts, items }
}

/// Decimal places `v` needs (up to `max`).
/// Does `s` look like an ISO 286 tolerance class: one or two letters and a grade (H7, g6, JS9, js6)?
fn is_fit_name(s: &str) -> bool {
    let letters = s.chars().take_while(|c| c.is_ascii_alphabetic()).count();
    let rest = &s[letters..];
    (1..=2).contains(&letters) && !rest.is_empty() && rest.len() <= 2 && rest.chars().all(|c| c.is_ascii_digit())
}

fn decs(v: f64, max: usize) -> usize {
    let s = fmt_num(v, max, true);
    s.find('.').map_or(0, |i| s.len() - i - 1)
}

pub fn fmt_num(v: f64, dec: usize, trim: bool) -> String {
    let s = format!("{:.*}", dec, v);
    let s = if trim && s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s };
    if s == "-0" { "0".into() } else { s }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Side {
    Above = 0,
    Below = 1,
    Left = 2,
    Right = 3,
}

impl Side {
    fn parse(s: &str) -> Option<Side> {
        match s.to_ascii_lowercase().as_str() {
            "above" | "top" => Some(Side::Above),
            "below" | "bottom" => Some(Side::Below),
            "left" => Some(Side::Left),
            "right" => Some(Side::Right),
            _ => None,
        }
    }
    fn sign(self) -> f64 {
        match self {
            Side::Above | Side::Right => 1.0,
            _ => -1.0,
        }
    }
    fn horizontal_dim(self) -> bool {
        matches!(self, Side::Above | Side::Below)
    }
}

struct Lin {
    a: V2,
    b: V2,
    horiz: bool,
    side: Side,
    dt: DimText,
    w: f64,
    level: usize,
    /// JSON pointer of the dimension's spec
    src: String,
}

pub struct Annotator<'a> {
    m: &'a Model,
    pr: Projector,
    cfg: &'a Config,
    pub geom: BBox2,
    segs: Vec<(V2, V2)>,
    pub dims: Vec<Prim>,
    pub gdt: Vec<Prim>,
    /// Hard obstacles (text, frames): nothing may overlap or cross them.
    obst: Vec<BBox2>,
    /// Soft obstacles (dimension and leader lines): symbols may not overlap
    /// them, leaders may cross them at a cost.
    soft: Vec<BBox2>,
    used: [f64; 4],
    pub warnings: Vec<String>,
    tol_warn: std::cell::RefCell<Vec<String>>,
    /// The view draws hidden lines.
    pub hidden: bool,
    /// Section view: the cutting plane.
    pub clip: Option<crate::hlr::Clip>,
}

impl<'a> Annotator<'a> {
    pub fn new(m: &'a Model, pr: Projector, cfg: &'a Config, lines: &[Vec<V2>]) -> Self {
        let mut geom = BBox2::empty();
        let mut segs = Vec::new();
        for l in lines {
            for p in l {
                geom.add(*p);
            }
            for w in l.windows(2) {
                segs.push((w[0], w[1]));
            }
        }
        Annotator { m, pr, cfg, geom, segs, dims: vec![], gdt: vec![], obst: vec![], soft: vec![], used: [0.0; 4], warnings: vec![], tol_warn: Default::default(), hidden: false, clip: None }
    }

    fn h(&self) -> f64 {
        self.cfg.text_height
    }

    fn base(&self, s: Side) -> f64 {
        match s {
            Side::Above => self.geom.max.y,
            Side::Below => self.geom.min.y,
            Side::Left => self.geom.min.x,
            Side::Right => self.geom.max.x,
        }
    }

    fn make_text(&self, spec: &DimSpec, value: f64, default_prefix: &str, deg: bool) -> DimText {
        let mut dt = self.make_text_only(spec, value, default_prefix, deg);
        for g in &spec.gdt {
            let (prims, w, warns) = self.build_frame(g);
            self.tol_warn.borrow_mut().extend(warns);
            dt.frames.push((prims, w, g.datum.clone().filter(|d| !d.is_empty())));
        }
        dt.datum = spec.datum.clone().filter(|d| !d.is_empty());
        dt.extra = spec.extra.clone();
        dt
    }

    fn make_text_only(&self, spec: &DimSpec, value: f64, default_prefix: &str, deg: bool) -> DimText {
        let dec = spec.decimals.unwrap_or(if deg { self.cfg.angle_decimals } else { self.cfg.decimals });
        let trim = self.cfg.trim_zeros;
        let unit = if deg { "°" } else { "" };
        let num = fmt_num(value, dec, trim) + unit;
        let prefix = spec.prefix.clone().unwrap_or_else(|| default_prefix.to_string());
        let suffix = spec.suffix.clone().unwrap_or_default();
        // text before and after the value
        let (pre, post) = match &spec.text {
            Some(t) if t.contains("<>") => {
                let i = t.find("<>").unwrap();
                (t[..i].to_string(), t[i + 2..].to_string())
            }
            Some(t) => (t.clone(), String::new()),
            None => (prefix, suffix),
        };
        let has_value = spec.text.as_ref().map_or(true, |t| t.contains("<>"));
        let whole = |n: &str| if has_value { format!("{}{}{}", pre, n, post) } else { pre.clone() };
        let mut dt = DimText { main: whole(&num), plus: None, minus: None, limits: false, tail: String::new(), basic: false, frames: vec![], datum: None, line_y: -1.0, extra: vec![], flip: spec.flip_arrows };
        // signed deviation text; both deviations get the same decimal places
        let dev = |v: f64, d: usize| {
            if v == 0.0 {
                "0".to_string()
            } else {
                format!("{}{}{}", if v > 0.0 { "+" } else { "-" }, fmt_num(v.abs(), d, false), unit)
            }
        };
        let sym = |dt: &mut DimText, t: f64| {
            let d = if trim { decs(t, 4).max(1) } else { dec.max(1) };
            dt.main = whole(&format!("{} ±{}{}", num, fmt_num(t.abs(), d, false), unit));
        };
        let spec_t = match &spec.tolerance {
            None => return dt,
            Some(Tolerance::Sym(t)) => {
                sym(&mut dt, *t);
                return dt;
            }
            Some(Tolerance::Text(t)) => match t.to_ascii_lowercase().as_str() {
                "basic" => TolSpec { kind: Some("basic".into()), ..Default::default() },
                "reference" | "ref" => TolSpec { kind: Some("reference".into()), ..Default::default() },
                _ => {
                    // a named fit (H7, g6, ...): its deviations for this size after the name
                    let name = t.trim();
                    if !deg && is_fit_name(name) {
                        match self.cfg.fits.lookup(name, value) {
                            Some((up, lo)) => {
                                let d = decs(up, 4).max(decs(lo, 4)).max(3);
                                dt.plus = Some(dev(up, d));
                                dt.minus = Some(dev(lo, d));
                                if has_value {
                                    dt.main = format!("{}{} {}", pre, num, name);
                                    dt.tail = post.trim_start().to_string();
                                } else {
                                    dt.main = format!("{} {}", dt.main, name);
                                }
                                return dt;
                            }
                            None => self.warn_tol(spec, &format!("no limits for fit '{}' at size {} in named_fits.json; shown without them", name, num)),
                        }
                    }
                    dt.main = format!("{} {}", dt.main, t);
                    return dt;
                }
            },
            Some(Tolerance::Spec(t)) => t.clone(),
        };
        let kind = spec_t.kind.clone().unwrap_or_else(|| {
            if spec_t.upper.is_some() || spec_t.lower.is_some() {
                "limits".into()
            } else if spec_t.value.is_some() {
                "symmetric".into()
            } else {
                "asymmetric".into()
            }
        });
        let (plus, minus) = (spec_t.plus.unwrap_or(0.0), spec_t.minus.unwrap_or(0.0));
        match kind.to_ascii_lowercase().as_str() {
            "symmetric" | "sym" | "plusminus" => match spec_t.value.or(spec_t.plus) {
                Some(t) => sym(&mut dt, t),
                None => self.warn_tol(spec, "symmetric tolerance needs \"value\""),
            },
            "asymmetric" | "deviation" | "deviations" => {
                if spec_t.plus.is_none() && spec_t.minus.is_none() {
                    self.warn_tol(spec, "asymmetric tolerance needs \"plus\" and \"minus\"");
                } else if (plus + minus).abs() < 1e-12 && plus > 0.0 {
                    sym(&mut dt, plus);
                } else {
                    let d = if trim { decs(plus, 4).max(decs(minus, 4)).max(1) } else { dec.max(1) };
                    dt.plus = Some(dev(plus, d));
                    dt.minus = Some(dev(minus, d));
                    if has_value {
                        // deviations follow the value, before any trailing text
                        dt.main = format!("{}{}", pre, num);
                        dt.tail = post.trim_start().to_string();
                    }
                }
            }
            "limits" | "limit" => {
                let up = spec_t.upper.unwrap_or(value + plus);
                let lo = spec_t.lower.unwrap_or(value + minus);
                let (up, lo) = if up >= lo { (up, lo) } else { (lo, up) };
                let d = if trim { decs(up, 4).max(decs(lo, 4)) } else { dec.max(decs(up, 4)).max(decs(lo, 4)) };
                dt.main = if has_value { pre.clone() } else { String::new() };
                dt.plus = Some(fmt_num(up, d, false) + unit);
                dt.minus = Some(fmt_num(lo, d, false) + unit);
                dt.limits = true;
                dt.tail = if has_value { post.trim_start().to_string() } else { String::new() };
            }
            "basic" => dt.basic = true,
            "reference" | "ref" => dt.main = format!("({})", dt.main),
            k => self.warn_tol(spec, &format!("unknown tolerance type '{}'", k)),
        }
        dt
    }

    fn warn_tol(&self, spec: &DimSpec, msg: &str) {
        let what = spec.from.as_deref().or(spec.of.as_deref()).unwrap_or(&spec.kind);
        self.tol_warn.borrow_mut().push(format!("dimension {}: {}", what, msg));
    }

    /// Height of the text block above its baseline.
    fn text_h(&self, dt: &DimText) -> f64 {
        self.h() * if dt.limits { 1.75 } else { 1.0 } + self.extra_h(dt)
    }

    /// Height taken by the extra lines of a dimension text.
    fn extra_h(&self, dt: &DimText) -> f64 {
        dt.extra.len() as f64 * 1.4 * self.h()
    }

    /// Height of the whole block (text, attached frames and datum symbol)
    /// beyond the baseline, on the side away from the dimension line.
    fn block_h(&self, dt: &DimText) -> f64 {
        let l = self.block_layout(dt, 1.0);
        let top = l.frames_top.unwrap_or(l.top);
        top.max(self.text_h(dt))
    }

    /// Width of the datum feature symbol box.
    fn datum_w(&self, dt: &DimText) -> f64 {
        dt.datum.as_ref().map_or(0.0, |d| (text_width(d, self.h()) + 2.5).max(self.h() * 1.8))
    }

    /// Vertical layout of the extras around a dimension text; `sgn` is +1 when
    /// the dimension line is below the text.
    fn block_layout(&self, dt: &DimText, sgn: f64) -> BlockLayout {
        let h = self.h();
        let fh = self.cfg.frame_height;
        let mut top = 0.75 * h;
        let mut bot = -0.25 * h;
        if dt.plus.is_some() {
            top = if dt.limits { 1.55 * h } else { 0.95 * h };
            if !dt.limits {
                bot = -0.35 * h;
            }
        }
        if dt.basic {
            top = 0.72 * h + 0.7;
            bot = bot.min(-0.7);
        }
        if sgn > 0.0 {
            top += self.extra_h(dt);
        } else {
            bot -= self.extra_h(dt);
        }
        let line_y = if sgn > 0.0 { dt.line_y.min(bot) } else { dt.line_y.max(top) };
        let tri_h = h * 0.8;
        let s = self.datum_w(dt);
        // datum box: [near edge, far edge] measured along sgn from the line
        let datum = dt.datum.as_ref().map(|_| {
            let near = line_y + sgn * (tri_h + 1.2);
            (near, near + sgn * s)
        });
        // frames start beyond the text (and the datum box)
        let (mut t, mut b) = (top, bot);
        if let Some((n, f)) = datum {
            t = t.max(n.max(f));
            b = b.min(n.min(f));
        }
        let frames_h: f64 = dt.frames.iter().map(|f| fh + f.2.as_deref().map_or(0.0, |l| self.hang_h(l))).sum();
        let frames_top = (!dt.frames.is_empty()).then(|| if sgn > 0.0 { t + 0.8 + frames_h } else { b - 0.8 - frames_h });
        BlockLayout { top: if sgn > 0.0 { t } else { b }, text_far: if sgn > 0.0 { top } else { bot }, line_y, tri_h, datum, frames_top }
    }

    fn stack_h(&self, dt: &DimText) -> f64 {
        self.h() * if dt.limits { 0.8 } else { 0.65 }
    }

    fn text_w(&self, dt: &DimText) -> f64 {
        let h = self.h();
        let mut w = text_width(&dt.main, h);
        if dt.plus.is_some() {
            let th = self.stack_h(dt);
            if !dt.main.is_empty() {
                w += 0.8;
            }
            w += text_width(dt.plus.as_deref().unwrap_or(""), th).max(text_width(dt.minus.as_deref().unwrap_or(""), th));
            if !dt.tail.is_empty() {
                w += 0.8 + text_width(&dt.tail, h);
            }
        }
        if dt.basic {
            w += 1.6;
        }
        if dt.datum.is_some() {
            w += 1.5 + self.datum_w(dt);
        }
        dt.extra.iter().map(|l| text_width(l, h)).fold(w, f64::max)
    }

    /// Width of the first line (main text, tolerance, datum symbol).
    fn main_w(&self, dt: &DimText) -> f64 {
        let d = DimText { extra: vec![], ..dt.clone() };
        self.text_w(&d)
    }

    /// Text prims for a dimension text, baseline origin `o`, rotation `rot` (deg).
    fn text_prims(&self, dt: &DimText, o: V2, rot: f64, anchor: Anchor) -> Vec<Prim> {
        let h = self.h();
        let w = self.text_w(dt);
        let x0 = match anchor {
            Anchor::Start => 0.0,
            Anchor::Middle => -w / 2.0,
            Anchor::End => -w,
        };
        let a = rot.to_radians();
        let at = |dx: f64, dy: f64| o + v2(dx, dy).rot(a);
        let mut v = Vec::new();
        let mut x = x0 + if dt.basic { 0.8 } else { 0.0 };
        let wm = self.main_w(dt);
        let sgn = if dt.line_y <= 0.0 { 1.0 } else { -1.0 };
        // extra lines: under the main text, which moves up, when the line is below
        let lift = if sgn > 0.0 { self.extra_h(dt) } else { 0.0 };
        for (k, line) in dt.extra.iter().enumerate() {
            let y = if sgn > 0.0 { lift - (k as f64 + 1.0) * 1.4 * h } else { -(k as f64 + 1.0) * 1.4 * h };
            v.push(Prim::Text { p: at(x0, y), h, anchor: Anchor::Start, rot, s: line.clone(), cls: "dim" });
        }
        // limits sit on the baseline with the other text centred on the stack
        let ym = lift + if dt.limits { h * 0.45 } else { 0.0 };
        if !dt.main.is_empty() {
            v.push(Prim::Text { p: at(x, ym), h, anchor: Anchor::Start, rot, s: dt.main.clone(), cls: "dim" });
            x += text_width(&dt.main, h);
        }
        if let (Some(p), Some(mi)) = (&dt.plus, &dt.minus) {
            let th = self.stack_h(dt);
            if !dt.main.is_empty() {
                x += 0.8;
            }
            let (yu, yl) = if dt.limits { (h * 0.95, 0.0) } else { (h * 0.45, -h * 0.25) };
            let (yu, yl) = (yu + lift, yl + lift);
            v.push(Prim::Text { p: at(x, yu), h: th, anchor: Anchor::Start, rot, s: p.clone(), cls: "dim" });
            v.push(Prim::Text { p: at(x, yl), h: th, anchor: Anchor::Start, rot, s: mi.clone(), cls: "dim" });
            x += text_width(p, th).max(text_width(mi, th));
            if !dt.tail.is_empty() {
                v.push(Prim::Text { p: at(x + 0.8, ym), h, anchor: Anchor::Start, rot, s: dt.tail.clone(), cls: "dim" });
            }
        }
        if dt.basic {
            let (y0, y1) = (lift - 0.7, lift + h * 0.72 + 0.7);
            let pts = [v2(x0, y0), v2(x0 + wm, y0), v2(x0 + wm, y1), v2(x0, y1)].iter().map(|q| at(q.x, q.y)).collect();
            v.push(Prim::Poly { pts, closed: true, cls: "thin" });
        }
        if dt.datum.is_none() && dt.frames.is_empty() {
            return v;
        }
        let l = self.block_layout(dt, sgn);
        // frames line up with the text (without the datum symbol), extra lines included
        let wt = dt.extra.iter().map(|t| text_width(t, h)).fold(wm - if dt.datum.is_some() { 1.5 + self.datum_w(dt) } else { 0.0 }, f64::max);
        if let (Some(lbl), Some((near, far))) = (&dt.datum, l.datum) {
            // datum feature symbol beside the text, its triangle on the dimension line
            let s = self.datum_w(dt);
            let cx = x0 + wm - s / 2.0;
            let hw = l.tri_h * 0.6;
            let apex = l.line_y + sgn * l.tri_h;
            v.push(Prim::Line { a: at(cx - hw - 1.0, l.line_y), b: at(cx + hw + 1.0, l.line_y), cls: "thin" });
            v.push(Prim::Fill { pts: vec![at(cx - hw, l.line_y), at(cx + hw, l.line_y), at(cx, apex)] });
            v.push(Prim::Line { a: at(cx, apex), b: at(cx, near), cls: "thin" });
            let (y0, y1) = (near.min(far), near.max(far));
            let pts = [v2(cx - s / 2.0, y0), v2(cx + s / 2.0, y0), v2(cx + s / 2.0, y1), v2(cx - s / 2.0, y1)].iter().map(|q| at(q.x, q.y)).collect();
            v.push(Prim::Poly { pts, closed: true, cls: "box" });
            v.push(Prim::Text { p: at(cx, (y0 + y1) / 2.0 - h * 0.35), h, anchor: Anchor::Middle, rot, s: lbl.clone(), cls: "dim" });
        }
        // frames stacked beyond the text, centred on it (or from its start)
        let fh = self.cfg.frame_height;
        let mut y = l.top + sgn * 0.8;
        for (prims, fw, dl) in &dt.frames {
            let left = match anchor {
                Anchor::Start => x0,
                Anchor::End => x0 + wt - fw,
                Anchor::Middle => x0 + wt / 2.0 - fw / 2.0,
            };
            let cy = y + sgn * fh / 2.0;
            let f = move |q: V2| at(left + q.x, cy + q.y);
            v.extend(prims.iter().map(|p| map_prim(p, &f, rot)));
            y += sgn * fh;
            // a datum symbol on the frame hangs off its far side
            if let Some(l) = dl {
                v.extend(self.hang_datum(l, fw / 2.0, sgn * fh / 2.0, sgn).iter().map(|p| map_prim(p, &f, rot)));
                y += sgn * self.hang_h(l);
            }
        }
        let _ = l.text_far;
        v
    }

    fn arrow(&self, tip: V2, toward_tail: V2) -> Prim {
        let d = toward_tail.norm();
        let n = d.perp() * (self.cfg.arrow_width * 0.5);
        let base = tip + d * self.cfg.arrow_length;
        Prim::Fill { pts: vec![tip, base + n, base - n] }
    }

    fn box_free(&self, b: &BBox2) -> bool {
        if self.obst.iter().chain(&self.soft).any(|o| o.overlaps(b)) {
            return false;
        }
        let bb = b.expand(0.5);
        !self.segs.iter().any(|(p, q)| bb.hits_segment(*p, *q))
    }

    fn seg_free(&self, a: V2, b: V2) -> bool {
        !self.obst.iter().any(|o| o.expand(-0.2).hits_segment(a, b))
    }

    fn soft_crossings(&self, a: V2, b: V2) -> usize {
        self.soft.iter().filter(|o| o.expand(-0.2).hits_segment(a, b)).count()
    }

    /// Register externally drawn items (e.g. cutting-plane symbols) as obstacles.
    pub fn block(&mut self, prims: &[Prim]) {
        self.add_obstacles(prims);
    }

    fn add_obstacles(&mut self, prims: &[Prim]) {
        for p in prims {
            match p {
                Prim::Group { items, .. } => self.add_obstacles(items),
                Prim::Text { .. } | Prim::Rect { .. } => self.obst.push(p.bbox().expand(0.6)),
                _ => self.soft.push(p.bbox().expand(0.4)),
            }
        }
    }

    // ---------------------------------------------------------------- refs

    fn refs(&mut self, s: &Option<String>, what: &str) -> Option<Vec<RefGeo>> {
        let Some(s) = s else {
            self.warnings.push(format!("{} dimension is missing its '{}' selector", what, what));
            return None;
        };
        match resolve(self.m, s) {
            Ok(r) => Some(r),
            Err(e) => {
                self.warnings.push(e);
                None
            }
        }
    }

    /// References of a diameter / radius: like `refs`, with a fillet (torus)
    /// face seen in profile standing for its tube circle in this view.
    fn circ_refs(&mut self, s: &Option<String>) -> Option<Vec<RefGeo>> {
        let mut refs = self.refs(s, "of")?;
        if let Ok(sel) = select(self.m, s.as_deref().unwrap_or("")) {
            if sel.kind == Kind::Face {
                for (g, &fi) in refs.iter_mut().zip(&sel.ids) {
                    if g.circ.is_none() {
                        if let Some(c) = self.torus_profile(fi) {
                            g.rep = c.c;
                            g.circ = Some(c);
                        }
                    }
                }
            }
        }
        Some(refs)
    }

    /// The tube circle of a torus face where it shows in profile (its axis
    /// across the view): the arc of it on the face, on the side with more of it.
    fn torus_profile(&self, fi: usize) -> Option<Circ> {
        let f = &self.m.faces[fi];
        let SurfKind::Torus { major, minor } = f.surf.kind else { return None };
        let fr = &f.surf.fr;
        let (z, d) = (fr.z.norm(), self.pr.d.norm());
        if z.dot(d).abs() > 0.01 {
            return None;
        }
        // tube angles the face covers: the shortest arc holding its boundary points
        let tube = |p: V3| {
            let q = fr.local(p);
            q.z.atan2((q.x * q.x + q.y * q.y).sqrt() - major)
        };
        let mut ws: Vec<f64> = f.loop_pts.iter().flatten().map(|p| wrap_2pi(tube(*p))).collect();
        if ws.is_empty() {
            return None;
        }
        ws.sort_by(|a, b| a.total_cmp(b));
        let (mut gap, mut w0) = (ws[0] + 2.0 * PI - ws[ws.len() - 1], ws[0]);
        for k in 1..ws.len() {
            if ws[k] - ws[k - 1] > gap {
                gap = ws[k] - ws[k - 1];
                w0 = ws[k];
            }
        }
        let span = 2.0 * PI - gap;
        let w = z.cross(d).norm();
        let mut best: Option<Circ> = None;
        for side in [w, -w] {
            let c = fr.o + side * major;
            let n = 90;
            let pts: Vec<V3> = (0..=n)
                .map(|k| w0 + span * k as f64 / n as f64)
                .map(|t| c + (side * t.cos() + z * t.sin()) * minor)
                .filter(|p| f.contains(*p))
                .collect();
            if pts.len() > 1 && best.as_ref().is_none_or(|b| pts.len() > b.pts.len()) {
                best = Some(Circ { c, axis: side.cross(z).norm(), r: minor, pts, v_range: (0.0, 0.0), full: span > 2.0 * PI - 1e-3 });
            }
        }
        best
    }

    /// Measure coordinate (2D) of a reference along `axis` (2D unit), and the
    /// extension-line start point extreme toward `toward`.
    fn ref_point(&self, refs: &[RefGeo], axis: V2, toward: V2) -> V2 {
        let reps: Vec<V2> = refs.iter().map(|r| self.pr.p(r.rep)).collect();
        let mc = reps.iter().map(|p| p.dot(axis)).sum::<f64>() / reps.len() as f64;
        let mut best: Option<V2> = None;
        for r in refs {
            for q in r.ext.iter().map(|p| self.pr.p(*p)).chain(std::iter::once(self.pr.p(r.rep))) {
                if (q.dot(axis) - mc).abs() < 0.05 && best.is_none_or(|b| q.dot(toward) > b.dot(toward)) {
                    best = Some(q);
                }
            }
        }
        let b = best.unwrap_or_else(|| reps.iter().copied().max_by(|a, b| a.dot(toward).total_cmp(&b.dot(toward))).unwrap());
        // snap the measure coordinate exactly
        b + axis * (mc - b.dot(axis))
    }

    fn default_side(&self, mid: V2, horiz: bool) -> Side {
        let c = self.geom.center();
        if horiz {
            if mid.y >= c.y { Side::Above } else { Side::Below }
        } else if mid.x >= c.x {
            Side::Right
        } else {
            Side::Left
        }
    }

    // ------------------------------------------------------------ building

    /// Record `src` (a JSON pointer) on the annotation groups emitted since
    /// `dims` / `gdt` had `d0` / `g0` items.
    fn tag_from(&mut self, d0: usize, g0: usize, src: &str) {
        for p in self.dims[d0..].iter_mut().chain(self.gdt[g0..].iter_mut()) {
            if let Prim::Group { attrs, .. } = p
                && !attrs.iter().any(|a| a.0 == "data-src")
            {
                attrs.push(("data-src", src.to_string()));
            }
        }
    }

    /// Emit with `f`, tagging what it emits with `src`.
    fn tagged(&mut self, src: String, f: impl FnOnce(&mut Self)) {
        let (d0, g0) = (self.dims.len(), self.gdt.len());
        f(self);
        self.tag_from(d0, g0, &src);
    }

    /// Build the annotations of a view; `base` is the JSON pointer of the
    /// view's spec.
    pub fn build(&mut self, ann: &Annotations, base: &str) {
        // datums that name a frame go on that frame
        let mut gdts = ann.gdt.clone();
        for t in ann.datums.iter().filter(|t| t.gdt.is_some()) {
            let id = t.gdt.as_deref().unwrap();
            match gdts.iter_mut().find(|g| g.id.as_deref() == Some(id)) {
                Some(g) => g.datum = Some(t.label.clone()),
                None => self.warnings.push(format!("datum {}: no GD&T frame with id '{}'", t.label, id)),
            }
        }
        let src_of = |it: Item| match it {
            Item::Dim(i) => format!("{}/dimensions/{}", base, i),
            Item::Gdt(i) => format!("{}/gdt/{}", base, i),
            Item::Note(i) => format!("{}/notes/{}", base, i),
            Item::Thread(i) => format!("{}/threads/{}", base, i),
        };
        // merged items: frames, notes and thread callouts on the leader of another item
        let mut ids: std::collections::HashMap<&str, Item> = std::collections::HashMap::new();
        let all_items: Vec<(Item, Option<&str>)> = ann.dimensions.iter().enumerate().map(|(i, d)| (Item::Dim(i), d.id.as_deref()))
            .chain(gdts.iter().enumerate().map(|(i, g)| (Item::Gdt(i), g.id.as_deref())))
            .chain(ann.notes.iter().enumerate().map(|(i, n)| (Item::Note(i), n.id.as_deref())))
            .chain(ann.threads.iter().enumerate().map(|(i, t)| (Item::Thread(i), t.id.as_deref())))
            .collect();
        for (it, id) in &all_items {
            if let Some(id) = id.filter(|s| !s.is_empty()) {
                ids.entry(id).or_insert(*it);
            }
        }
        let merge_of = |it: Item| -> Option<&str> {
            match it {
                Item::Dim(_) => None,
                Item::Gdt(i) => gdts[i].merge.as_deref().or(gdts[i].dimension.as_deref()),
                Item::Note(i) => ann.notes[i].merge.as_deref(),
                Item::Thread(i) => ann.threads[i].merge.as_deref(),
            }
            .filter(|s| !s.is_empty())
        };
        let mut head_of: std::collections::HashMap<Item, Item> = std::collections::HashMap::new();
        for (it, _) in &all_items {
            let Some(first) = merge_of(*it) else { continue };
            let mut h = *it;
            let mut next = Some(first);
            let mut ok = true;
            for _ in 0..16 {
                let Some(id) = next else { break };
                match ids.get(id) {
                    Some(t) if *t == *it => {
                        ok = false;
                        break;
                    }
                    Some(t) => {
                        h = *t;
                        next = merge_of(h);
                    }
                    None => {
                        self.warnings.push(format!("{}: nothing with id '{}' to merge with", src_of(*it), id));
                        ok = false;
                        break;
                    }
                }
            }
            if ok && h != *it {
                head_of.insert(*it, h);
            }
        }
        // members of each head, in order: thread callouts, notes, frames
        let mut members: Vec<(Item, Vec<Item>)> = Vec::new();
        let order = |it: &Item| match it {
            Item::Thread(i) => (0, *i),
            Item::Note(i) => (1, *i),
            Item::Gdt(i) => (2, *i),
            Item::Dim(i) => (3, *i),
        };
        let mut ms: Vec<(&Item, &Item)> = head_of.iter().collect();
        ms.sort_by_key(|(m, _)| order(m));
        for (m, h) in ms {
            match members.iter_mut().find(|(hh, _)| hh == h) {
                Some((_, v)) => v.push(*m),
                None => members.push((*h, vec![*m])),
            }
        }
        let members_of = |h: Item| members.iter().find(|(hh, _)| *hh == h).map(|(_, v)| v.clone()).unwrap_or_default();
        let mut extra_attrs: Vec<(String, &'static str, String)> = Vec::new();
        for (h, v) in &members {
            extra_attrs.push((src_of(*h), "data-members", v.iter().map(|m| src_of(*m)).collect::<Vec<_>>().join(" ")));
        }
        // the feature each item is on (for the editor's "merge with")
        for (it, _) in &all_items {
            let sel = match *it {
                Item::Dim(i) => {
                    let d = &ann.dimensions[i];
                    if matches!(d.kind.to_ascii_lowercase().as_str(), "diameter" | "radius") { d.of.clone() } else { None }
                }
                Item::Gdt(i) => Some(gdts[i].attach.clone()),
                Item::Note(i) => ann.notes[i].attach.clone(),
                Item::Thread(i) => Some(ann.threads[i].of.clone()),
            };
            if let Some(k) = sel.filter(|s| !s.is_empty()).and_then(|s| self.feat_key(&s)) {
                extra_attrs.push((src_of(*it), "data-feat", k));
            }
        }
        // threads merged elsewhere: drawn now, their callout text kept
        let mut thread_text: std::collections::HashMap<usize, Option<String>> = std::collections::HashMap::new();
        for (i, t) in ann.threads.iter().enumerate() {
            if head_of.contains_key(&Item::Thread(i)) || members.iter().any(|(h, _)| *h == Item::Thread(i)) {
                let (d0, g0) = (self.dims.len(), self.gdt.len());
                let txt = self.thread(t, true);
                self.tag_from(d0, g0, &src_of(Item::Thread(i)));
                thread_text.insert(i, txt);
            }
        }
        let text_lines = |it: Item, thread_text: &std::collections::HashMap<usize, Option<String>>| -> Vec<String> {
            match it {
                Item::Note(i) => wrap_text(&ann.notes[i].text),
                Item::Thread(i) => thread_text.get(&i).cloned().flatten().into_iter().collect(),
                _ => vec![],
            }
        };
        let mut dims = ann.dimensions.clone();
        for (h, v) in &members {
            if let Item::Dim(i) = h {
                for m in v {
                    match m {
                        Item::Gdt(g) => dims[*i].gdt.push(gdts[*g].clone()),
                        _ => {
                            let l = text_lines(*m, &thread_text);
                            dims[*i].extra.extend(l);
                        }
                    }
                }
            }
        }
        for t in ann.datums.iter().filter(|t| t.dimension.is_some()) {
            let id = t.dimension.as_deref().unwrap();
            match dims.iter_mut().find(|d| d.id.as_deref() == Some(id)) {
                Some(d) => d.datum = Some(t.label.clone()),
                None => self.warnings.push(format!("datum {}: no dimension with id '{}'", t.label, id)),
            }
        }
        for d in &dims {
            if d.gdt.iter().any(|g| g.all_around) {
                self.warnings.push(format!(
                    "GD&T frame on dimension {}: all_around needs the frame on its own leader (\"attach\"); ignored",
                    d.id.as_deref().unwrap_or(&d.kind)
                ));
            }
            if d.kind.eq_ignore_ascii_case("ordinate") && (!d.gdt.is_empty() || d.datum.is_some() || !d.extra.is_empty()) {
                self.warnings.push("GD&T frames, datums and merged notes cannot be attached to ordinate dimensions".into());
            }
        }
        let mut lins: Vec<Lin> = Vec::new();
        let mut later: Vec<(usize, &DimSpec)> = Vec::new();
        let dsrc = |i: usize| format!("{}/dimensions/{}", base, i);
        for (i, d) in dims.iter().enumerate() {
            let n0 = lins.len();
            let (d0, g0) = (self.dims.len(), self.gdt.len());
            match d.kind.to_ascii_lowercase().as_str() {
                "linear" => self.prep_linear(d, &mut lins),
                "diameter" | "radius" => {
                    if !self.prep_diameter_side(d, &mut lins) {
                        later.push((i, d));
                    }
                }
                "angle" | "angular" | "ordinate" => later.push((i, d)),
                k => self.warnings.push(format!("unknown dimension type '{}'", k)),
            }
            for l in &mut lins[n0..] {
                l.src = dsrc(i);
            }
            // aligned dimensions are drawn straight away
            self.tag_from(d0, g0, &dsrc(i));
        }
        self.place_linear(lins);
        for (i, d) in later.iter().filter(|d| d.1.kind.eq_ignore_ascii_case("ordinate")) {
            self.tagged(dsrc(*i), |a| a.ordinate(d));
        }
        for (i, d) in later.iter().filter(|d| !d.1.kind.eq_ignore_ascii_case("ordinate")) {
            match d.kind.to_ascii_lowercase().as_str() {
                "angle" | "angular" => self.tagged(dsrc(*i), |a| a.angle(d)),
                _ => self.tagged(dsrc(*i), |a| a.diameter_face_on(d)),
            }
        }
        for (i, d) in ann.datums.iter().enumerate().filter(|d| d.1.dimension.is_none() && d.1.gdt.is_none()) {
            self.tagged(format!("{}/datums/{}", base, i), |a| a.datum(d));
        }
        for (i, t) in ann.datum_targets.iter().enumerate() {
            self.tagged(format!("{}/datum_targets/{}", base, i), |a| a.datum_target(t));
        }
        // a merged callout on a frame, note or thread: the head's text first,
        // then the members' lines, then the frames
        let callout = |a: &mut Self, h: Item, own: Vec<String>, own_frame: Option<GdtSpec>, attach: &str, around: bool| {
            let mut lines = own;
            let mut frames: Vec<GdtSpec> = own_frame.into_iter().collect();
            for m in members_of(h) {
                match m {
                    Item::Gdt(g) => frames.push(gdts[g].clone()),
                    _ => lines.extend(text_lines(m, &thread_text)),
                }
            }
            a.tagged(src_of(h), |a| a.callout(&src_of(h), attach, lines, &frames, around));
        };
        for (i, g) in gdts.iter().enumerate() {
            let it = Item::Gdt(i);
            // merged elsewhere (or on a dimension id that isn't there: warned above)
            if head_of.contains_key(&it) || (g.merge.is_none() && g.dimension.is_some()) {
                continue;
            }
            if members.iter().any(|(h, _)| *h == it) {
                callout(self, it, vec![], Some(g.clone()), &g.attach, g.all_around);
            } else {
                self.tagged(src_of(it), |a| a.frame(g));
            }
        }
        for w in self.tol_warn.take() {
            if !self.warnings.contains(&w) {
                self.warnings.push(w);
            }
        }
        for (i, t) in ann.threads.iter().enumerate() {
            let it = Item::Thread(i);
            if head_of.contains_key(&it) {
                continue;
            }
            if members.iter().any(|(h, _)| *h == it) {
                let own = thread_text.get(&i).cloned().flatten().into_iter().collect();
                callout(self, it, own, None, &t.of, false);
            } else {
                self.tagged(src_of(it), |a| {
                    a.thread(t, false);
                });
            }
        }
        for (i, n) in ann.notes.iter().enumerate() {
            let it = Item::Note(i);
            if head_of.contains_key(&it) {
                continue;
            }
            match n.attach.as_deref().filter(|s| !s.is_empty()) {
                Some(at) if members.iter().any(|(h, _)| *h == it) => callout(self, it, wrap_text(&n.text), None, at, false),
                _ => {
                    if members.iter().any(|(h, _)| *h == it) {
                        self.warnings.push(format!("note '{}': give it \"attach\" to merge other items onto its leader", n.text));
                    }
                    self.tagged(src_of(it), |a| a.note(n));
                }
            }
        }
        for w in self.tol_warn.take() {
            if !self.warnings.contains(&w) {
                self.warnings.push(w);
            }
        }
        // editor hints on the groups: members merged onto a leader, the feature an item is on
        for p in self.dims.iter_mut().chain(self.gdt.iter_mut()) {
            if let Prim::Group { attrs, .. } = p
                && let Some(src) = attrs.iter().find(|a| a.0 == "data-src").map(|a| a.1.clone())
            {
                for (s, k, v) in &extra_attrs {
                    if *s == src && !attrs.iter().any(|a| a.0 == *k) {
                        attrs.push((k, v.clone()));
                    }
                }
            }
        }
    }

    /// Key of the feature a selector picks: holes / shafts by their axis
    /// (coaxial features match), planes by their plane, others by position.
    fn feat_key(&self, sel: &str) -> Option<String> {
        let r = resolve(self.m, sel).ok()?;
        let g = r.first()?;
        let f = |v: f64| format!("{:.3}", (v * 1000.0).round() / 1000.0 + 0.0);
        let canon = |a: V3| if a.x < -1e-9 || (a.x.abs() <= 1e-9 && (a.y < -1e-9 || (a.y.abs() <= 1e-9 && a.z < 0.0))) { a * -1.0 } else { a };
        let v3s = |a: V3| format!("{},{},{}", f(a.x), f(a.y), f(a.z));
        if let Some(c) = &g.circ {
            let a = canon(c.axis.norm());
            let p = c.c - a * c.c.dot(a);
            return Some(format!("axis:{}:{}", v3s(a), v3s(p)));
        }
        if let Some(n) = g.plane_n {
            let n = canon(n.norm());
            return Some(format!("plane:{}:{}", v3s(n), f(g.rep.dot(n))));
        }
        if let Some((a, b)) = g.line {
            let (a, b) = if v3s(a) <= v3s(b) { (a, b) } else { (b, a) };
            return Some(format!("line:{}:{}", v3s(a), v3s(b)));
        }
        Some(format!("point:{}", v3s(g.rep)))
    }

    /// Text lines and frames stacked on one leader to the feature `attach`.
    fn callout(&mut self, what: &str, attach: &str, lines: Vec<String>, frames: &[GdtSpec], around: bool) {
        let Some(r) = self.attach_ref(attach) else { return };
        let mut it = lines.into_iter();
        let mut dt = DimText {
            main: it.next().unwrap_or_default(),
            plus: None,
            minus: None,
            limits: false,
            tail: String::new(),
            basic: false,
            frames: vec![],
            datum: None,
            line_y: -1.0,
            extra: it.collect(),
            flip: false,
        };
        for g in frames {
            let (prims, w, warns) = self.build_frame(g);
            self.tol_warn.borrow_mut().extend(warns);
            dt.frames.push((prims, w, g.datum.clone().filter(|d| !d.is_empty())));
        }
        let mut prims = self.text_prims(&dt, V2::default(), 0.0, Anchor::Start);
        let b = bbox_of(&prims);
        if b.width() <= 0.0 {
            return;
        }
        // left-middle of the block at the origin, with a little margin
        let d = v2(0.8 - b.min.x, -b.center().y);
        for p in prims.iter_mut() {
            p.translate(d);
        }
        self.emit_leadered(&format!("merged callout {}", what), &r, None, prims, b.width() + 1.6, b.height() + 1.6, Term::Arrow, around);
    }

    /// `fromPoint` / `toPoint`: +1 max, -1 min, 0 center.
    fn end_mode(&mut self, v: &Option<String>, what: &str) -> f64 {
        match v.as_deref().map(|s| s.trim().to_ascii_lowercase()) {
            None => 0.0,
            Some(s) if s == "max" || s == "maximum" || s == "far" => 1.0,
            Some(s) if s == "min" || s == "minimum" || s == "near" => -1.0,
            Some(s) if s == "center" || s == "centre" || s.is_empty() => 0.0,
            Some(s) => {
                self.warnings.push(format!("{} \"{}\": use \"max\", \"min\" or \"center\"", what, s));
                0.0
            }
        }
    }

    /// References at one end of a linear dimension, and the circle there when
    /// the dimension goes to its edge (`mode` max / min) rather than its centre.
    fn lin_end(&mut self, sel: &Option<String>, what: &str, mode: f64) -> Option<(Vec<RefGeo>, Option<Circ>)> {
        if mode == 0.0 {
            return Some((self.refs(sel, what)?, None));
        }
        let refs = self.circ_refs(sel)?;
        let c = refs.iter().find_map(|r| r.circ.clone());
        if c.is_none() {
            self.warnings.push(format!("{}Point: {:?} is not a circle, arc or cylinder; measured to it as it is", what, sel));
        }
        Some((refs, c))
    }

    fn prep_linear(&mut self, d: &DimSpec, lins: &mut Vec<Lin>) {
        let (ma, mb) = (self.end_mode(&d.from_point, "fromPoint"), self.end_mode(&d.to_point, "toPoint"));
        let (Some((ra, ca)), Some((rb, cb))) = (self.lin_end(&d.from, "from", ma), self.lin_end(&d.to, "to", mb)) else { return };
        // a circle end: its centre moved across to its far (max) or near (min)
        // side along the measuring direction `ax3`; returns the two 3D measure
        // points and the offset of each from its centre
        let ends = |ax3: V3| -> (V3, V3, V3, V3) {
            let (a0, b0) = (avg(&ra), avg(&rb));
            let s = if (b0 - a0).dot(ax3) < 0.0 { -1.0 } else { 1.0 };
            let off = |c: &Option<Circ>, k: f64| -> V3 {
                let Some(c) = c else { return V3::default() };
                let w = ax3 - c.axis * c.axis.dot(ax3);
                if w.len() < 1e-9 { V3::default() } else { w.norm() * (c.r * k) }
            };
            let (oa, ob) = (off(&ca, -s * ma), off(&cb, s * mb));
            (a0 + oa, b0 + ob, oa, ob)
        };
        // the extension line start on a circle end: on its edge, at the end of
        // a cylinder nearest the dimension, level with the measure point
        let circ_start = |c: &Option<Circ>, o: V3, m3: V3, axis: V2, toward: V2| -> Option<V2> {
            let c = c.as_ref()?;
            if o.len() < 1e-12 {
                return None;
            }
            let q = [c.v_range.0, c.v_range.1]
                .iter()
                .map(|v| self.pr.p(c.c + c.axis * *v + o))
                .max_by(|p, q| p.dot(toward).total_cmp(&q.dot(toward)))?;
            Some(q + axis * (self.pr.p(m3).dot(axis) - q.dot(axis)))
        };
        let pa = self.pr.p(avg(&ra));
        let pb = self.pr.p(avg(&rb));
        let dir = d.direction.as_deref().unwrap_or("auto").to_ascii_lowercase();
        let horiz = match dir.as_str() {
            "horizontal" | "x" | "h" => true,
            "vertical" | "y" | "v" => false,
            "aligned" | "parallel" => {
                // two parallel lines in the view (faces seen edge-on, straight
                // edges): measure square to them, from their outer ends
                if let (Some((a0, a1)), Some((b0, b1))) = (self.ref_line(&ra), self.ref_line(&rb)) {
                    let ua = (a1 - a0).norm();
                    if ua.cross((b1 - b0).norm()).abs() < 1e-3 {
                        let nrm = ua.perp();
                        let gap = (b0 - a0).dot(nrm);
                        if gap.abs() > 1e-6 {
                            let t = if gap > 0.0 { nrm } else { -nrm };
                            let mid = (a0 + a1 + b0 + b1) * 0.25;
                            let s = if (mid - self.geom.center()).dot(ua) >= 0.0 { 1.0 } else { -1.0 };
                            let pick = |p: V2, q: V2| if p.dot(ua) * s >= q.dot(ua) * s { p } else { q };
                            let value = match ra.iter().find_map(|r| r.plane_n) {
                                Some(n3) => (avg(&rb) - avg(&ra)).dot(n3).abs(),
                                None => gap.abs() / self.pr.scale,
                            };
                            let dt = self.make_text(d, value, "", false);
                            self.aligned(pick(a0, a1), pick(b0, b1), Some(t), dt);
                            return;
                        }
                    }
                }
                let (a3, b3, oa, ob) = ends((avg(&rb) - avg(&ra)).norm());
                let a = if oa.len() > 0.0 { self.pr.p(a3) } else { self.ref_point(&ra, (pb - pa).norm(), V2::default()) };
                let b = if ob.len() > 0.0 { self.pr.p(b3) } else { self.ref_point(&rb, (pb - pa).norm(), V2::default()) };
                let value = a3.dist(b3);
                let dt = self.make_text(d, value, "", false);
                self.aligned(a, b, None, dt);
                return;
            }
            _ => (pb.x - pa.x).abs() >= (pb.y - pa.y).abs(),
        };
        let mid = (pa + pb) * 0.5;
        let side = d
            .side
            .as_deref()
            .and_then(Side::parse)
            .filter(|s| s.horizontal_dim() == horiz)
            .unwrap_or_else(|| self.default_side(mid, horiz));
        let (axis, toward) = if horiz { (v2(1.0, 0.0), v2(0.0, side.sign())) } else { (v2(0.0, 1.0), v2(side.sign(), 0.0)) };
        let ax3 = if horiz { self.pr.r } else { self.pr.u };
        let (a3, b3, oa, ob) = ends(ax3);
        let a = circ_start(&ca, oa, a3, axis, toward).unwrap_or_else(|| self.ref_point(&ra, axis, toward));
        let b = circ_start(&cb, ob, b3, axis, toward).unwrap_or_else(|| self.ref_point(&rb, axis, toward));
        let value = (b3 - a3).dot(ax3).abs();
        let dt = self.make_text(d, value, "", false);
        let w = self.text_w(&dt);
        lins.push(Lin { a, b, horiz, side, dt, w, level: 0, src: String::new() });
    }

    /// Diameter of a cylinder / circle seen side-on, drawn as a linear dimension.
    /// Returns false if the feature is seen face-on (handled later).
    fn prep_diameter_side(&mut self, d: &DimSpec, lins: &mut Vec<Lin>) -> bool {
        let Some(refs) = self.circ_refs(&d.of) else { return true };
        let Some(c) = refs.iter().find_map(|r| r.circ.clone()) else {
            self.warnings.push(format!("{:?}: diameter/radius needs a circle or arc edge, a cylindrical face, or a fillet (torus) face seen in profile", d.of));
            return true;
        };
        if c.axis.dot(self.pr.d).abs() > 0.5 {
            return false;
        }
        let radius = d.kind.eq_ignore_ascii_case("radius");
        let s = self.pr.scale;
        let ax2 = self.pr.dir(c.axis).norm();
        let n2 = ax2.perp();
        let e0 = self.pr.p(c.c + c.axis * c.v_range.0);
        let e1 = self.pr.p(c.c + c.axis * c.v_range.1);
        let value = if radius { c.r } else { 2.0 * c.r };
        let dt = self.make_text(d, value, if radius { "R" } else { "Ø" }, false);
        let w = self.text_w(&dt);
        if n2.x.abs() > 0.999 || n2.y.abs() > 0.999 {
            let horiz = n2.x.abs() > 0.999;
            let mid = (e0 + e1) * 0.5;
            let side = d
                .side
                .as_deref()
                .and_then(Side::parse)
                .filter(|s| s.horizontal_dim() == horiz)
                .unwrap_or_else(|| self.default_side(mid, horiz));
            let toward = if horiz { v2(0.0, side.sign()) } else { v2(side.sign(), 0.0) };
            let e = if e0.dot(toward) >= e1.dot(toward) { e0 } else { e1 };
            let a = if radius { e } else { e - n2 * (c.r * s) };
            let b = e + n2 * (c.r * s);
            lins.push(Lin { a, b, horiz, side, dt, w, level: 0, src: String::new() });
        } else {
            let a = e1 - n2 * (c.r * s);
            let b = e1 + n2 * (c.r * s);
            self.aligned(if radius { e1 } else { a }, b, None, dt);
        }
        true
    }

    fn lin_interval(&self, l: &Lin) -> (f64, f64, f64, f64) {
        // (lo, hi) of whole dim incl. text; (tlo, thi) of text only
        let (xa, xb) = if l.horiz { (l.a.x, l.b.x) } else { (l.a.y, l.b.y) };
        let (lo, hi) = (xa.min(xb), xa.max(xb));
        let al = self.cfg.arrow_length;
        if hi - lo >= l.w + 2.0 * al + 2.0 {
            let m = (lo + hi) / 2.0;
            let out = if l.dt.flip { al + 2.0 } else { 0.0 };
            (lo - 1.0 - out, hi + 1.0 + out, m - l.w / 2.0 - 0.5, m + l.w / 2.0 + 0.5)
        } else {
            let t0 = hi + al + 1.5;
            (lo - al - 2.0, t0 + l.w + 1.0, t0 - 0.5, t0 + l.w + 0.5)
        }
    }

    fn place_linear(&mut self, mut lins: Vec<Lin>) {
        lins.sort_by(|p, q| {
            let sp = |l: &Lin| if l.horiz { (l.a.x - l.b.x).abs() } else { (l.a.y - l.b.y).abs() };
            sp(p).total_cmp(&sp(q))
        });
        // per side: list of (level, lo, hi, tlo, thi, ext a, ext b, levels taken)
        let mut placed: Vec<Vec<(usize, f64, f64, f64, f64, f64, f64, usize)>> = vec![vec![]; 4];
        for l in lins.iter_mut() {
            let (lo, hi, tlo, thi) = self.lin_interval(l);
            // extra levels taken by attached frames / datum symbol
            let bh = self.block_h(&l.dt);
            let span = ((bh - self.h()) / self.cfg.dim_spacing).ceil().max(0.0) as usize;
            let (lo, hi) = if l.dt.frames.is_empty() {
                (lo, hi)
            } else {
                let fw = l.dt.frames.iter().map(|f| f.1).fold(0.0, f64::max);
                let c = (tlo + thi) / 2.0;
                (lo.min(c - fw / 2.0), hi.max(c + fw / 2.0))
            };
            let (xa, xb) = if l.horiz { (l.a.x, l.b.x) } else { (l.a.y, l.b.y) };
            let side = &placed[l.side as usize];
            let clash = |level: usize, strict: bool| {
                side.iter().any(|&(lv, plo, phi, ptlo, pthi, pxa, pxb, psp)| {
                    (lv <= level + span && level <= lv + psp && lo < phi && plo < hi)
                        || (strict && lv < level && ((xa > ptlo && xa < pthi) || (xb > ptlo && xb < pthi)))
                        || (lv > level && ((pxa > tlo && pxa < thi) || (pxb > tlo && pxb < thi)))
                })
            };
            // the dimension line + text band at a level must avoid symbols
            // already on the view (e.g. cutting-plane letters)
            let band = |level: usize| {
                let s = l.side.sign();
                let pos = self.base(l.side) + s * (self.cfg.dim_offset + level as f64 * self.cfg.dim_spacing);
                let (a0, a1) = (lo.min(tlo), hi.max(thi));
                if l.horiz {
                    BBox2::new(v2(a0, pos - 0.5), v2(a1, pos + bh + 1.5))
                } else {
                    BBox2::new(v2(pos - bh - 1.5, a0), v2(pos + 0.5, a1))
                }
            };
            let blocked = |level: usize| self.obst.iter().any(|o| o.overlaps(&band(level)));
            // Prefer a level where nothing crosses; otherwise accept crossing
            // extension lines (cannot be cured by moving outward).
            let level = (0..12)
                .find(|&k| !clash(k, true) && !blocked(k))
                .or_else(|| (0..64).find(|&k| !clash(k, false) && !blocked(k)))
                .or_else(|| (0..64).find(|&k| !clash(k, false)))
                .unwrap_or(0);
            l.level = level;
            placed[l.side as usize].push((level, lo, hi, tlo, thi, xa, xb, span));
        }
        for l in &lins {
            let (d0, g0) = (self.dims.len(), self.gdt.len());
            self.render_lin(l);
            self.tag_from(d0, g0, &l.src);
        }
    }

    fn render_lin(&mut self, l: &Lin) {
        let cfg = self.cfg;
        let s = l.side.sign();
        let off = cfg.dim_offset + l.level as f64 * cfg.dim_spacing;
        let pos = self.base(l.side) + s * off;
        self.used[l.side as usize] = self.used[l.side as usize].max(off + self.block_h(&l.dt) + 1.0);
        let w = |along: f64, across: f64| if l.horiz { v2(along, across) } else { v2(across, along) };
        let (xa, ya) = if l.horiz { (l.a.x, l.a.y) } else { (l.a.y, l.a.x) };
        let (xb, yb) = if l.horiz { (l.b.x, l.b.y) } else { (l.b.y, l.b.x) };
        let mut ext = Vec::new();
        for (x, y) in [(xa, ya), (xb, yb)] {
            if (pos - y) * s > cfg.ext_gap {
                ext.push(Prim::Line { a: w(x, y + s * cfg.ext_gap), b: w(x, pos + s * cfg.ext_overshoot), cls: "thin" });
            }
        }
        let mut prims = Vec::new();
        let (lo, hi) = (xa.min(xb), xa.max(xb));
        let al = cfg.arrow_length;
        // the text fits between the extension lines with the arrows inside
        let inside = hi - lo >= l.w + 2.0 * al + 2.0;
        // arrows inside pointing out, or outside pointing in ("flipArrows" swaps them)
        let arrows_in = inside != l.dt.flip;
        let rot = if l.horiz { 0.0 } else { 90.0 };
        let toff = if l.horiz { 1.0 } else { -1.0 };
        let dir_al = w(1.0, 0.0);
        if arrows_in {
            // the line runs on to the text when it is outside
            prims.push(Prim::Line { a: w(lo, pos), b: w(if inside { hi } else { hi + al + 1.0 }, pos), cls: "thin" });
            prims.push(self.arrow(w(lo, pos), dir_al));
            prims.push(self.arrow(w(hi, pos), -dir_al));
        } else {
            // the line runs on past both arrows (to the text, when it is outside)
            let far = if inside { al + 2.0 } else { al + 1.0 };
            prims.push(Prim::Line { a: w(lo - al - 2.0, pos), b: w(hi + far, pos), cls: "thin" });
            prims.push(Prim::Line { a: w(lo, pos), b: w(hi, pos), cls: "thin" });
            prims.push(self.arrow(w(lo, pos), -dir_al));
            prims.push(self.arrow(w(hi, pos), dir_al));
        }
        let text = if inside {
            self.text_prims(&l.dt, w((lo + hi) / 2.0, pos + toff), rot, Anchor::Middle)
        } else {
            self.text_prims(&l.dt, w(hi + al + 1.5, pos + toff), rot, Anchor::Start)
        };
        self.add_obstacles(&prims.iter().chain(&text).filter(|p| !matches!(p, Prim::Line { .. }) || is_dimline(p, l.horiz)).cloned().collect::<Vec<_>>());
        let g = self.lin_group(w(xa, ya + s * cfg.ext_gap), w(xb, yb + s * cfg.ext_gap), w(xa, pos), w(xb, pos), w(0.0, s), ext, prims, text);
        self.dims.push(g);
    }

    /// Group for a linear dimension: extension lines from `ea`, `eb` to the
    /// dimension line `pa`-`pb`, which moves along `n`.
    #[allow(clippy::too_many_arguments)]
    fn lin_group(&self, ea: V2, eb: V2, pa: V2, pb: V2, n: V2, ext: Vec<Prim>, stat: Vec<Prim>, text: Vec<Prim>) -> Prim {
        let c = self.cfg;
        ann(
            "linear",
            vec![
                ("data-n", vec_attr(n)),
                ("data-ov", c.ext_overshoot.to_string()),
                ("data-arrow", format!("{},{}", c.arrow_length, c.arrow_width)),
            ],
            vec![("data-ea", ea), ("data-eb", eb), ("data-pa", pa), ("data-pb", pb)],
            vec![role("ext", ext), role("static", stat), role("text", text)],
        )
    }

    /// Dimension between `a` and `b` measured along `dir` (default: a to b).
    fn aligned(&mut self, a: V2, b: V2, dir: Option<V2>, mut dt: DimText) {
        let cfg = self.cfg;
        let t = dir.unwrap_or(b - a).norm();
        if (b - a).dot(t).abs() < 1e-9 {
            self.warnings.push("aligned dimension of zero length".into());
            return;
        }
        let mut n = t.perp();
        if (self.geom.center() - (a + b) * 0.5).dot(n) > 0.0 {
            n = -n;
        }
        let g = self.geom;
        let corners = [g.min, g.max, v2(g.min.x, g.max.y), v2(g.max.x, g.min.y)];
        // dimension line level along n, measured from the outer of a, b
        let base = a.dot(n).max(b.dot(n));
        let reach = corners.iter().map(|c| c.dot(n) - base).fold(0.0, f64::max);
        let mut ang = t.angle().to_degrees();
        if ang > 90.0 + 1e-6 {
            ang -= 180.0;
        } else if ang <= -90.0 + 1e-6 {
            ang += 180.0;
        }
        let up = V2::from_angle(ang.to_radians()).perp();
        let th = self.text_h(&dt);
        if up.dot(n) < 0.0 {
            dt.line_y = 1.0 + th; // the text is on the far side of the line
        }
        // nearest clear level inside the view's reach (the dimension line crosses
        // no geometry, text, frame or dimension line, the text is in free space),
        // otherwise outside the view (text and dimension line clear of text)
        let mut levels: Vec<(f64, bool)> = Vec::new();
        let mut d = cfg.dim_offset;
        while d < reach {
            levels.push((d, true));
            d += cfg.dim_spacing * 0.5;
        }
        levels.extend((0..8).map(|k| (reach + cfg.dim_offset + k as f64 * cfg.dim_spacing, false)));
        let first_outer = levels.iter().position(|l| !l.1).unwrap();
        let geo = |la: V2, lb: V2| self.segs.iter().any(|(p, q)| segments_intersect(la, lb, *p, *q));
        let at = |d: f64| {
            let (pa, pb) = (a + n * (base + d - a.dot(n)), b + n * (base + d - b.dot(n)));
            let mid = (pa + pb) * 0.5;
            let o = if up.dot(n) >= 0.0 { mid + n * 1.0 } else { mid + n * (1.0 + th) };
            let tb = self.text_prims(&dt, o, ang, Anchor::Middle).iter().fold(BBox2::empty(), |mut bb, p| {
                let q = p.bbox();
                bb.add(q.min);
                bb.add(q.max);
                bb
            });
            (pa, pb, o, tb)
        };
        let pick = levels
            .iter()
            .position(|&(d, near)| {
                let (pa, pb, _, tb) = at(d);
                let (la, lb) = (pa - t * cfg.arrow_length, pb + t * cfg.arrow_length);
                self.box_free(&tb)
                    && self.seg_free(la, lb)
                    && (!near
                        || (self.soft_crossings(la, lb) == 0
                            && !geo(la, lb)
                            && self.seg_free(a + n * cfg.ext_gap, pa)
                            && self.seg_free(b + n * cfg.ext_gap, pb)))
            })
            .unwrap_or(first_outer);
        {
            let (pa, pb, o, _) = at(levels[pick].0);
            let ext = vec![
                Prim::Line { a: a + n * cfg.ext_gap, b: pa + n * cfg.ext_overshoot, cls: "thin" },
                Prim::Line { a: b + n * cfg.ext_gap, b: pb + n * cfg.ext_overshoot, cls: "thin" },
            ];
            let stat = if dt.flip {
                // arrows outside pointing in, the line running on past them
                let al = cfg.arrow_length + 2.0;
                vec![Prim::Line { a: pa - t * al, b: pb + t * al, cls: "thin" }, self.arrow(pa, -t), self.arrow(pb, t)]
            } else {
                vec![Prim::Line { a: pa, b: pb, cls: "thin" }, self.arrow(pa, t), self.arrow(pb, -t)]
            };
            let text = self.text_prims(&dt, o, ang, Anchor::Middle);
            self.add_obstacles(&stat);
            self.add_obstacles(&text);
            let g = self.lin_group(a + n * cfg.ext_gap, b + n * cfg.ext_gap, pa, pb, n, ext, stat, text);
            self.dims.push(g);
            return;
        }
    }

    fn ordinate(&mut self, d: &DimSpec) {
        let Some(origin) = self.refs(&d.origin, "origin") else { return };
        let mut targets = vec![(origin.clone(), true)];
        for t in &d.targets {
            match resolve(self.m, t) {
                Ok(r) => targets.push((r, false)),
                Err(e) => self.warnings.push(e),
            }
        }
        let horiz = !matches!(d.direction.as_deref().map(|s| s.to_ascii_lowercase()), Some(ref s) if s == "y" || s == "vertical");
        let side = d
            .side
            .as_deref()
            .and_then(Side::parse)
            .filter(|s| s.horizontal_dim() == horiz)
            .unwrap_or(if horiz { Side::Below } else { Side::Left });
        let s = side.sign();
        let (axis, toward) = if horiz { (v2(1.0, 0.0), v2(0.0, s)) } else { (v2(0.0, 1.0), v2(s, 0.0)) };
        let ax3 = if horiz { self.pr.r } else { self.pr.u };
        let o3 = avg(&origin);
        let h = self.h();
        let cfg = self.cfg;
        let mut items: Vec<(f64, f64, DimText)> = Vec::new(); // (measure coord, start across, text)
        let mut seen: Vec<f64> = Vec::new();
        for (r, is_origin) in &targets {
            let p = self.ref_point(r, axis, toward);
            let x = p.dot(axis);
            if seen.iter().any(|q| (q - x).abs() < 0.01) {
                continue;
            }
            seen.push(x);
            let v = if *is_origin { 0.0 } else { (avg(r) - o3).dot(ax3).abs() };
            let dt = self.make_text(&DimSpec { text: None, gdt: vec![], datum: None, ..d.clone() }, v, "", false);
            items.push((x, p.dot(toward.norm()) * s, dt));
        }
        items.sort_by(|a, b| a.0.total_cmp(&b.0));
        // spread label positions
        let sp = h + 1.5;
        let mut t: Vec<f64> = items.iter().map(|i| i.0).collect();
        for _ in 0..200 {
            let mut moved = false;
            for k in 1..t.len() {
                let gap = t[k] - t[k - 1];
                if gap < sp - 1e-6 {
                    let c = (sp - gap) / 2.0;
                    t[k - 1] -= c;
                    t[k] += c;
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
        let jog = if t.iter().zip(&items).any(|(a, b)| (a - b.0).abs() > 0.01) { 4.0 } else { 0.0 };
        let off = if self.used[side as usize] > 0.0 { self.used[side as usize] + 3.0 } else { cfg.dim_offset * 0.6 };
        let band = self.base(side) + s * off;
        let w = |along: f64, across: f64| if horiz { v2(along, across) } else { v2(across, along) };
        let mut prims = Vec::new();
        let mut groups = Vec::new();
        let mut maxw: f64 = 0.0;
        for ((x, st, dt), ti) in items.iter().zip(&t) {
            let start = st * s; // across coordinate
            let lp = vec![w(*x, start + s * cfg.ext_gap), w(*x, band), w(*ti, band + s * jog), w(*ti, band + s * (jog + 2.0))];
            let lead = Prim::Poly { pts: lp.clone(), closed: false, cls: "thin" };
            prims.push(lead.clone());
            let tw = self.text_w(dt);
            maxw = maxw.max(tw);
            let (o, rot, anchor) = if horiz {
                (v2(ti + 0.35 * h, band + s * (jog + 3.0)), 90.0, if s > 0.0 { Anchor::Start } else { Anchor::End })
            } else {
                (v2(band + s * (jog + 3.0), ti - 0.35 * h), 0.0, if s > 0.0 { Anchor::Start } else { Anchor::End })
            };
            let text = self.text_prims(dt, o, rot, anchor);
            prims.extend(text.clone());
            groups.push(ann(
                "ordinate",
                vec![],
                vec![("data-p0", lp[0]), ("data-p1", lp[1]), ("data-p2", lp[2]), ("data-p3", lp[3])],
                vec![role("lead", vec![lead]), role("text", text)],
            ));
        }
        self.used[side as usize] = off + jog + 3.0 + maxw + 1.0;
        let obst: Vec<Prim> = prims.iter().filter(|p| matches!(p, Prim::Text { .. })).cloned().collect();
        self.add_obstacles(&obst);
        let band_box = {
            let mut b = BBox2::empty();
            for x in &t {
                b.add(w(*x, band));
                b.add(w(*x, band + s * (jog + 2.0)));
            }
            b.expand(0.5)
        };
        self.soft.push(band_box);
        self.dims.extend(groups);
    }

    fn diameter_face_on(&mut self, d: &DimSpec) {
        let Some(refs) = self.circ_refs(&d.of) else { return };
        let Some(c) = refs.iter().find_map(|r| r.circ.clone()) else { return };
        let radius = d.kind.eq_ignore_ascii_case("radius");
        let c2 = self.pr.p(c.c);
        let r2 = c.r * self.pr.scale;
        let value = if radius { c.r } else { 2.0 * c.r };
        let dt = self.make_text(d, value, if radius { "R" } else { "Ø" }, false);
        let tw = self.text_w(&dt);
        // candidate angles: those covered by the arc
        let covered: Vec<f64> = c.pts.iter().map(|p| (self.pr.p(*p) - c2).angle()).collect();
        let on_arc = |a: f64| c.full || covered.iter().any(|q| wrap_pi(q - a).abs() < 4f64.to_radians());
        let mut best: Option<(f64, Vec<Prim>, BBox2)> = None;
        for k in 0..24 {
            let a = (45.0 + 15.0 * k as f64).to_radians();
            if !on_arc(a) {
                continue;
            }
            let pref = wrap_pi(a - 45f64.to_radians()).abs() * 4.0;
            for &len in &[6.0, 10.0, 15.0, 22.0, 30.0, 40.0, 52.0, 66.0] {
                let dir = V2::from_angle(a);
                let p = c2 + dir * r2;
                let k2 = c2 + dir * (r2 + len);
                let right = dir.x >= -1e-9;
                let sh = if right { tw + 1.0 } else { -(tw + 1.0) };
                let end = k2 + v2(sh, 0.0);
                // frames wider than the text stick out on the side away from the leader
                let fw = dt.frames.iter().map(|f| f.1).fold(0.0, f64::max);
                let far = if right { end.x.max(k2.x + 0.5 + fw) } else { end.x.min(k2.x - 0.5 - fw) };
                let tb = BBox2::new(k2, v2(far, k2.y + self.block_h(&dt) + 1.0)).expand(0.3);
                let inside_geom = self.geom.overlaps(&tb);
                if !self.box_free(&tb) || !self.seg_free(p, k2) {
                    continue;
                }
                let score = len + pref + if inside_geom { 40.0 } else { 0.0 };
                if best.as_ref().is_some_and(|b| b.0 <= score) {
                    continue;
                }
                let mut prims = Vec::new();
                if radius {
                    prims.push(Prim::Line { a: c2, b: k2, cls: "thin" });
                    prims.push(self.arrow(p, -dir));
                } else {
                    prims.push(Prim::Line { a: p, b: k2, cls: "thin" });
                    prims.push(self.arrow(p, dir));
                    if r2 > 4.0 && on_arc(a + PI) {
                        // through-line across the circle with the far arrow
                        prims.push(Prim::Line { a: c2 - dir * r2, b: p, cls: "thin" });
                        prims.push(self.arrow(c2 - dir * r2, dir));
                    } else if !c.full {
                        // a diameter of an arc: one arrow, the line running on past the centre
                        prims.push(Prim::Line { a: c2 - dir * (r2 * 0.25).clamp(1.5, 4.0), b: p, cls: "thin" });
                    }
                }
                prims.push(Prim::Line { a: k2, b: end, cls: "thin" });
                let text = if right {
                    self.text_prims(&dt, v2(k2.x + 0.5, k2.y + 0.8), 0.0, Anchor::Start)
                } else {
                    self.text_prims(&dt, v2(k2.x - 0.5, k2.y + 0.8), 0.0, Anchor::End)
                };
                let cfg = self.cfg;
                let mut attrs = vec![
                    ("data-term", "arrow".to_string()),
                    ("data-arrow", format!("{},{}", cfg.arrow_length, cfg.arrow_width)),
                    ("data-mode", "radial".into()),
                    ("data-r", r2.to_string()),
                    ("data-shoulder", "under".into()),
                ];
                if radius {
                    attrs.push(("data-radius", "1".into()));
                } else if r2 > 4.0 && on_arc(a + PI) {
                    attrs.push(("data-through", "1".into()));
                }
                let g = ann(
                    "leader",
                    attrs,
                    vec![("data-tip", p), ("data-s0", end), ("data-s1", k2), ("data-sc", (k2 + end) * 0.5), ("data-c", c2)],
                    vec![role("static", vec![]), role("sym", text), role("lead", prims)],
                );
                best = Some((score, vec![g], tb));
            }
        }
        match best {
            Some((_, prims, tb)) => {
                self.obst.push(tb);
                self.dims.extend(prims);
            }
            None => self.warnings.push(format!("could not place {} dimension for {:?}", d.kind, d.of)),
        }
    }

    /// 2D line (two endpoints) of a reference: a line edge or a planar face seen edge-on.
    fn ref_line(&self, refs: &[RefGeo]) -> Option<(V2, V2)> {
        for r in refs {
            if let Some((a, b)) = r.line {
                let (a, b) = (self.pr.p(a), self.pr.p(b));
                if a.dist(b) > 1e-6 {
                    return Some((a, b));
                }
            }
            if let Some(n) = r.plane_n {
                let t = self.pr.dir(n.cross(self.pr.d)).norm();
                if t.len() < 0.5 {
                    continue;
                }
                let pts: Vec<V2> = r.ext.iter().map(|p| self.pr.p(*p)).collect();
                let mn = pts.iter().min_by(|a, b| a.dot(t).total_cmp(&b.dot(t)))?;
                let mx = pts.iter().max_by(|a, b| a.dot(t).total_cmp(&b.dot(t)))?;
                return Some((*mn, *mx));
            }
        }
        None
    }

    fn angle(&mut self, d: &DimSpec) {
        let (Some(ra), Some(rb)) = (self.refs(&d.from, "from"), self.refs(&d.to, "to")) else { return };
        let (Some((a0, a1)), Some((b0, b1))) = (self.ref_line(&ra), self.ref_line(&rb)) else {
            self.warnings.push("angle dimension needs two straight edges or planar faces seen edge-on".into());
            return;
        };
        let (ta, tb) = ((a1 - a0).norm(), (b1 - b0).norm());
        let den = ta.cross(tb);
        if den.abs() < 1e-9 {
            self.warnings.push("angle dimension between parallel lines".into());
            return;
        }
        let s = (b0 - a0).cross(tb) / den;
        let ip = a0 + ta * s;
        let far = |p: V2, q: V2| if p.dist(ip) > q.dist(ip) { p } else { q };
        let (fa, fb) = (far(a0, a1), far(b0, b1));
        let (da, db) = ((fa - ip).norm(), (fb - ip).norm());
        let ang = da.dot(db).clamp(-1.0, 1.0).acos();
        let la = fa.dist(ip);
        let lb = fb.dist(ip);
        let cfg = self.cfg;
        let rr = (la.min(lb) * 0.6).max(cfg.dim_offset * 1.2);
        let (s0, mut sweep) = (da.angle(), wrap_pi(db.angle() - da.angle()));
        if sweep.abs() < 1e-9 {
            sweep = ang;
        }
        let n = 48;
        let arc: Vec<V2> = (0..=n).map(|i| ip + V2::from_angle(s0 + sweep * i as f64 / n as f64) * rr).collect();
        let dt = self.make_text(d, ang.to_degrees(), "", true);
        let mut prims = vec![Prim::Poly { pts: arc.clone(), closed: false, cls: "thin" }];
        let tdir0 = V2::from_angle(s0).perp() * sweep.signum();
        prims.push(self.arrow(arc[0], tdir0));
        let tdir1 = -V2::from_angle(s0 + sweep).perp() * sweep.signum();
        prims.push(self.arrow(arc[n], tdir1));
        for (f, l, dd) in [(fa, la, da), (fb, lb, db)] {
            if rr > l {
                prims.push(Prim::Line { a: f + dd * cfg.ext_gap, b: ip + dd * (rr + cfg.ext_overshoot), cls: "thin" });
            }
        }
        let bis = V2::from_angle(s0 + sweep / 2.0);
        let tw = self.text_w(&dt);
        let th = self.block_h(&dt);
        let c = ip + bis * (rr + 1.5 + (th / 2.0).max((tw / 2.0) * bis.x.abs()));
        let text = self.text_prims(&dt, c - v2(0.0, th * 0.4), 0.0, Anchor::Middle);
        self.add_obstacles(&prims);
        self.add_obstacles(&text);
        self.dims.push(ann(
            "angle",
            vec![
                ("data-r", rr.to_string()),
                ("data-sweep", (-sweep).to_string()),
                ("data-gap", cfg.ext_gap.to_string()),
                ("data-ov", cfg.ext_overshoot.to_string()),
                ("data-arrow", format!("{},{}", cfg.arrow_length, cfg.arrow_width)),
            ],
            vec![("data-ip", ip), ("data-fa", fa), ("data-fb", fb)],
            vec![role("dyn", prims), role("text", text)],
        ));
    }

    // ------------------------------------------------------- GD&T / notes

    fn attach_ref(&mut self, s: &str) -> Option<RefGeo> {
        match resolve(self.m, s) {
            Ok(mut r) => Some(r.remove(0)),
            Err(e) => {
                self.warnings.push(e);
                None
            }
        }
    }

    /// Possible landing points of a leader on a feature, each with the outward
    /// normal of the edge there. If `fixed` is given the leader must land there.
    fn attach_options(&self, r: &RefGeo, fixed: Option<V2>) -> Vec<Att> {
        let mut out: Vec<Att> = Vec::new();
        let s = self.pr.scale;
        let d = self.pr.d;
        let free_dirs = |p: V2, dot: bool, out: &mut Vec<Att>| {
            for k in 0..8 {
                let a = (45.0 + 45.0 * k as f64).to_radians();
                out.push(Att { p, n: V2::from_angle(a), pen: (k % 4) as f64, dot, seg: None, circ: None });
            }
        };
        // straight edge a-b seen in the view; normals: the given ones
        let seg_opts = |a: V2, b: V2, normals: &[(V2, f64)], out: &mut Vec<Att>| {
            if let Some(q) = fixed {
                for &(n, pen) in normals {
                    out.push(Att { p: q, n, pen, dot: false, seg: Some((a, b)), circ: None });
                }
                return;
            }
            for f in [0.5, 0.35, 0.65, 0.2, 0.8, 0.1, 0.9] {
                let p = a.lerp(b, f);
                for &(n, pen) in normals {
                    out.push(Att { p, n, pen: pen + (f - 0.5f64).abs() * 12.0, dot: false, seg: Some((a, b)), circ: None });
                }
            }
        };
        if let Some(c) = &r.circ {
            let c2 = self.pr.p(c.c);
            let r2 = c.r * s;
            if c.axis.dot(d).abs() > 0.999 {
                // circle seen face-on: leader is radial
                if let Some(q) = fixed {
                    let n = (q - c2).norm();
                    out.push(Att { p: c2 + n * r2, n, pen: 0.0, dot: false, seg: None, circ: Some((c2, r2)) });
                    return out;
                }
                let covered: Vec<f64> = c.pts.iter().map(|p| (self.pr.p(*p) - c2).angle()).collect();
                for k in 0..24 {
                    let a = (45.0 + 15.0 * k as f64).to_radians();
                    if c.full || covered.iter().any(|q| wrap_pi(q - a).abs() < 4f64.to_radians()) {
                        let n = V2::from_angle(a);
                        out.push(Att { p: c2 + n * r2, n, pen: wrap_pi(a - 45f64.to_radians()).abs() * 2.0, dot: false, seg: None, circ: Some((c2, r2)) });
                    }
                }
                return out;
            }
            let ax2 = self.pr.dir(c.axis).norm();
            let n2 = ax2.perp();
            let (e0, e1) = (self.pr.p(c.c + c.axis * c.v_range.0), self.pr.p(c.c + c.axis * c.v_range.1));
            if e0.dist(e1) < 1e-6 {
                // a circle seen edge-on is a line across the axis
                seg_opts(c2 - n2 * r2, c2 + n2 * r2, &[(ax2, 0.0), (-ax2, 0.0)], &mut out);
            } else {
                // silhouette lines of the cylinder
                for sg in [1.0, -1.0] {
                    seg_opts(e0 + n2 * (sg * r2), e1 + n2 * (sg * r2), &[(n2 * sg, 0.0)], &mut out);
                }
            }
            return out;
        }
        if let Some(pn) = r.plane_n {
            if pn.dot(d).abs() > 0.999 {
                free_dirs(fixed.unwrap_or(self.pr.p(r.rep)), true, &mut out);
                return out;
            }
            let t = self.pr.dir(pn.cross(d)).norm();
            let pts: Vec<V2> = r.ext.iter().map(|p| self.pr.p(*p)).collect();
            let mn = pts.iter().copied().min_by(|a, b| a.dot(t).total_cmp(&b.dot(t))).unwrap_or_default();
            let mx = pts.iter().copied().max_by(|a, b| a.dot(t).total_cmp(&b.dot(t))).unwrap_or_default();
            // outward normal of the face, as seen in this view
            let mut n = self.pr.dir(pn).norm();
            n = (n - t * n.dot(t)).norm();
            // the edge-on line passes through the face's own points
            let base = self.pr.p(r.rep);
            let off = base - t * base.dot(t);
            let (a, b) = (off + t * mn.dot(t), off + t * mx.dot(t));
            seg_opts(a, b, &[(n, 0.0)], &mut out);
            return out;
        }
        if let Some((a, b)) = r.line {
            let (a, b) = (self.pr.p(a), self.pr.p(b));
            if a.dist(b) > 1e-6 {
                let n = (b - a).norm().perp();
                let mid = (a + b) * 0.5;
                let outward = if (mid - self.geom.center()).dot(n) >= 0.0 { n } else { -n };
                seg_opts(a, b, &[(outward, 0.0), (-outward, 12.0)], &mut out);
                return out;
            }
        }
        free_dirs(fixed.unwrap_or(self.pr.p(r.rep)), false, &mut out);
        out
    }

    fn geom_crossings(&self, a: V2, b: V2) -> usize {
        self.segs.iter().filter(|(p, q)| segments_intersect(a, b, *p, *q)).count()
    }

    fn on_visible(&self, p: V2) -> bool {
        self.segs.iter().any(|(a, b)| point_seg_dist(p, *a, *b) < 0.15)
    }

    /// Choose a landing point, leader length and side for a w x h symbol.
    /// The leader leaves the symbol horizontally (shoulder), then runs along
    /// the edge normal to the landing point.
    fn place_leadered(&self, opts: &[Att], w: f64, h: f64) -> Option<Placed> {
        self.place_leadered_pass(opts, w, h, false).or_else(|| self.place_leadered_pass(opts, w, h, true))
    }

    /// `relax`: leaders may cross text (heavily penalised) when nothing else fits.
    fn place_leadered_pass(&self, opts: &[Att], w: f64, h: f64, relax: bool) -> Option<Placed> {
        let sh = self.cfg.leader_shoulder;
        let g = self.geom.expand(3.0);
        let mut best: Option<(f64, Placed)> = None;
        for att in opts {
            let vis_pen = if self.on_visible(att.p) || att.dot { 0.0 } else { 25.0 };
            for li in 0..40 {
                let len = 4.0 + li as f64 * 2.5;
                for side in [1.0, -1.0] {
                    // the shoulder must not fold back over the leader
                    if att.n.x * side < -0.3 {
                        continue;
                    }
                    let q = att.p + att.n * len;
                    let a = q + v2(side * sh, 0.0);
                    let b = if side > 0.0 {
                        BBox2::new(a - v2(0.0, h / 2.0), a + v2(w, h / 2.0))
                    } else {
                        BBox2::new(a - v2(w, h / 2.0), a + v2(0.0, h / 2.0))
                    };
                    let mut score = len + att.pen + vis_pen;
                    if best.as_ref().is_some_and(|(s, _)| *s <= score) {
                        continue;
                    }
                    if b.overlaps(&g) || !self.box_free(&b.expand(1.0)) {
                        continue;
                    }
                    let start = att.p + att.n * 0.3;
                    if !self.seg_free(start, q) || !self.seg_free(q, a) {
                        if !relax {
                            continue;
                        }
                        score += 100.0;
                    }
                    score += 15.0 * (self.geom_crossings(start, q) + self.geom_crossings(q, a)) as f64
                        + 6.0 * (self.soft_crossings(start, q) + self.soft_crossings(q, a)) as f64;
                    if best.as_ref().is_some_and(|(s, _)| *s <= score) {
                        continue;
                    }
                    best = Some((score, Placed { b, q, a, att: att.clone() }));
                }
            }
        }
        best.map(|b| b.1)
    }

    fn draw_leader(&self, pl: &Placed, term: Term, prims: &mut Vec<Prim>) {
        let Placed { q, a, att, .. } = pl;
        let (p, n) = (att.p, att.n);
        prims.push(Prim::Line { a: *a, b: *q, cls: "thin" });
        let term = if att.dot && term == Term::Arrow { Term::Dot } else { term };
        match term {
            Term::Arrow => {
                prims.push(Prim::Line { a: *q, b: p, cls: "thin" });
                prims.push(self.arrow(p, n));
            }
            Term::Dot => {
                prims.push(Prim::Line { a: *q, b: p, cls: "thin" });
                prims.push(Prim::Fill { pts: (0..12).map(|i| p + V2::from_angle(i as f64 * PI / 6.0) * 0.6).collect() });
            }
            Term::Triangle => {
                let th = 2.6;
                let t = n.perp() * (th * 0.6);
                prims.push(Prim::Line { a: *q, b: p + n * th, cls: "thin" });
                prims.push(Prim::Fill { pts: vec![p + t, p - t, p + n * th] });
            }
            Term::Cross => {
                prims.push(Prim::Line { a: *q, b: p, cls: "thin" });
                let k = 1.6;
                prims.push(Prim::Line { a: p + v2(-k, -k), b: p + v2(k, k), cls: "visible" });
                prims.push(Prim::Line { a: p + v2(-k, k), b: p + v2(k, -k), cls: "visible" });
            }
            Term::None => prims.push(Prim::Line { a: *q, b: p, cls: "thin" }),
        }
    }

    /// Place a symbol (built at the origin, size w x h, with its left-middle
    /// at (0,0)) on a leader to the feature, and emit it.
    fn emit_leadered(&mut self, what: &str, r: &RefGeo, fixed: Option<V2>, mut sym: Vec<Prim>, w: f64, h: f64, term: Term, around: bool) -> Option<Placed> {
        let opts = self.attach_options(r, fixed);
        let Some(pl) = self.place_leadered(&opts, w, h) else {
            self.warnings.push(format!("could not place {}", what));
            return None;
        };
        let o = v2(pl.b.min.x, pl.b.center().y);
        for p in sym.iter_mut() {
            p.translate(o);
        }
        self.obst.push(pl.b.expand(0.5));
        // keep later symbols off this leader
        self.soft.push(BBox2::new(pl.q, pl.a).expand(0.3));
        self.soft.push(BBox2::new(pl.q, pl.att.p).expand(0.3));
        let g = self.leader_group(&pl, term, sym, vec![], None, around);
        self.gdt.push(g);
        Some(pl)
    }

    /// Group for a symbol on a leader: the symbol and its shoulder move, the
    /// leader stays on the feature (sliding along an edge seen edge-on,
    /// radial to a circle, or fixed on a face seen face-on).
    fn leader_group(&self, pl: &Placed, term: Term, sym: Vec<Prim>, stat: Vec<Prim>, circ: Option<(V2, f64)>, around: bool) -> Prim {
        let mut lead = Vec::new();
        self.draw_leader(pl, term, &mut lead);
        let around_r = self.cfg.frame_height * 0.22;
        if around {
            // all-around symbol on the bend
            lead.push(Prim::Circle { c: pl.q, r: around_r, cls: "thin" });
        }
        let term = if pl.att.dot && term == Term::Arrow { Term::Dot } else { term };
        let tname = match term {
            Term::Arrow => "arrow",
            Term::Dot => "dot",
            Term::Triangle => "triangle",
            Term::Cross => "cross",
            Term::None => "none",
        };
        let c = self.cfg;
        let mut attrs = vec![("data-term", tname.to_string()), ("data-arrow", format!("{},{}", c.arrow_length, c.arrow_width))];
        if around {
            attrs.push(("data-around", around_r.to_string()));
        }
        let mut pts = vec![("data-tip", pl.att.p), ("data-s0", pl.a), ("data-s1", pl.q), ("data-sc", pl.b.center())];
        match (circ.or(pl.att.circ), pl.att.seg) {
            (Some((cc, r)), _) => {
                attrs.push(("data-mode", "radial".into()));
                attrs.push(("data-r", r.to_string()));
                pts.push(("data-c", cc));
            }
            (None, Some((a, b))) if !pl.att.dot => {
                attrs.push(("data-mode", "edge".into()));
                pts.push(("data-e0", a));
                pts.push(("data-e1", b));
            }
            _ => attrs.push(("data-mode", "fixed".into())),
        }
        ann("leader", attrs, pts, vec![role("static", stat), role("sym", sym), role("lead", lead)])
    }

    fn frame(&mut self, g: &GdtSpec) {
        let Some(r) = self.attach_ref(&g.attach) else { return };
        let (mut prims, total, warns) = self.build_frame(g);
        self.warnings.extend(warns);
        let fh = self.cfg.frame_height;
        // a datum symbol hangs below the frame; the same room is kept above so
        // the leader still meets the middle of the frame
        let mut h = fh;
        if let Some(l) = g.datum.as_deref().filter(|d| !d.is_empty()) {
            prims.extend(self.hang_datum(l, total / 2.0, -fh / 2.0, -1.0));
            h += 2.0 * self.hang_h(l);
        }
        self.emit_leadered(&format!("GD&T frame for {}", g.attach), &r, None, prims, total, h, Term::Arrow, g.all_around);
    }

    /// Size of a datum feature symbol (box side) for `label`.
    fn datum_box(&self, label: &str) -> f64 {
        let h = self.h();
        (text_width(label, h) + 2.5).max(h * 1.8)
    }

    /// How far a datum symbol hung off a frame reaches beyond the frame.
    fn hang_h(&self, label: &str) -> f64 {
        self.h() * 0.8 + 1.2 + self.datum_box(label)
    }

    /// Datum feature symbol whose triangle sits on a frame edge at height
    /// `base_y`, centred at `cx`, pointing away from the frame towards `sgn`
    /// (+1 up, -1 down).
    fn hang_datum(&self, label: &str, cx: f64, base_y: f64, sgn: f64) -> Vec<Prim> {
        let h = self.h();
        let (tri_h, s) = (h * 0.8, self.datum_box(label));
        let hw = tri_h * 0.6;
        let apex = base_y + sgn * tri_h;
        let near = base_y + sgn * (tri_h + 1.2);
        let far = near + sgn * s;
        let (y0, y1) = (near.min(far), near.max(far));
        vec![
            Prim::Fill { pts: vec![v2(cx - hw, base_y), v2(cx + hw, base_y), v2(cx, apex)] },
            Prim::Line { a: v2(cx, apex), b: v2(cx, near), cls: "thin" },
            Prim::Rect { b: BBox2::new(v2(cx - s / 2.0, y0), v2(cx + s / 2.0, y1)), cls: "box" },
            Prim::Text { p: v2(cx, (y0 + y1) / 2.0 - h * 0.35), h, anchor: Anchor::Middle, rot: 0.0, s: label.to_string(), cls: "dim" },
        ]
    }

    /// A feature control frame with its left-middle at the origin, its width
    /// and any warnings.
    fn build_frame(&self, g: &GdtSpec) -> (Vec<Prim>, f64, Vec<String>) {
        let mut warnings = Vec::new();
        let what = g.dimension.as_ref().map_or_else(|| g.attach.clone(), |d| format!("dimension {}", d));
        let fh = self.cfg.frame_height;
        let h = self.h();
        let tol = match &g.tolerance {
            serde_json::Value::Number(n) => fmt_num(n.as_f64().unwrap_or(0.0), 3, true),
            serde_json::Value::String(s) => s.clone(),
            _ => String::new(),
        };
        let tol_text = format!("{}{}", if g.diameter { "Ø" } else { "" }, tol);
        let mods = g.modifier.as_ref().map(|m| m.list()).unwrap_or_default();
        let (tol_prims, tw) = frame_cell(&tol_text, &mods, h);
        let tol_w = tw + 2.5;
        let datums: Vec<(String, Vec<String>)> = g.datums.iter().map(|d| parse_datum_ref(d)).collect();
        let datum_cells: Vec<(Vec<Prim>, f64)> = datums.iter().map(|(l, m)| frame_cell(l, m, h)).collect();
        let datum_ws: Vec<f64> = datum_cells.iter().map(|(_, w)| (w + 2.5).max(fh)).collect();
        let total = fh + tol_w + datum_ws.iter().sum::<f64>();
        if gdt_symbol(&g.characteristic, V2::default(), 1.0).is_empty() {
            warnings.push(format!("unknown GD&T characteristic '{}'", g.characteristic));
        }
        for m in mods.iter().chain(datums.iter().flat_map(|(_, m)| m)) {
            if !known_modifier(m) {
                warnings.push(format!("GD&T frame for {}: modifier '{}' drawn as plain text", what, m));
            }
        }
        // frame built with its left-middle at the origin
        let y0 = -fh / 2.0;
        let mut prims = vec![Prim::Rect { b: BBox2::new(v2(0.0, y0), v2(total, -y0)), cls: "box" }];
        prims.extend(gdt_symbol(&g.characteristic, v2(fh / 2.0, 0.0), fh * 0.62));
        let mut x = fh;
        prims.push(Prim::Line { a: v2(x, y0), b: v2(x, -y0), cls: "thin" });
        for mut p in tol_prims {
            p.translate(v2(x + 1.25, 0.0));
            prims.push(p);
        }
        x += tol_w;
        for ((cell, cw), dw) in datum_cells.into_iter().zip(&datum_ws) {
            prims.push(Prim::Line { a: v2(x, y0), b: v2(x, -y0), cls: "thin" });
            for mut p in cell {
                p.translate(v2(x + (dw - cw) / 2.0, 0.0));
                prims.push(p);
            }
            x += dw;
        }
        (prims, total, warnings)
    }

    fn datum(&mut self, d: &DatumSpec) {
        let Some(r) = self.attach_ref(&d.attach) else { return };
        let h = self.h();
        let s = (text_width(&d.label, h) + 2.5).max(h * 1.8);
        let prims = vec![
            Prim::Rect { b: BBox2::new(v2(0.0, -s / 2.0), v2(s, s / 2.0)), cls: "box" },
            Prim::Text { p: v2(s / 2.0, -h * 0.35), h, anchor: Anchor::Middle, rot: 0.0, s: d.label.clone(), cls: "dim" },
        ];
        self.emit_leadered(&format!("datum {}", d.label), &r, None, prims, s, s, Term::Triangle, false);
    }

    fn datum_target(&mut self, t: &DatumTargetSpec) {
        let Some(r) = self.attach_ref(&t.attach) else { return };
        let h = self.h();
        let th = h * 0.85;
        let size_txt = t.size.clone().unwrap_or_default();
        let dia = (text_width(&size_txt, th).max(text_width(&t.label, th)) + 3.0).max(h * 2.8);
        let rad = dia / 2.0;
        let c = v2(rad, 0.0);
        let mut prims = vec![
            Prim::Circle { c, r: rad, cls: "box" },
            Prim::Line { a: v2(0.0, 0.0), b: v2(dia, 0.0), cls: "thin" },
            Prim::Text { p: v2(rad, -th * 1.15), h: th, anchor: Anchor::Middle, rot: 0.0, s: t.label.clone(), cls: "dim" },
        ];
        if !size_txt.is_empty() {
            prims.push(Prim::Text { p: v2(rad, th * 0.3), h: th, anchor: Anchor::Middle, rot: 0.0, s: size_txt.clone(), cls: "dim" });
        }
        let fixed = t.point.map(|p| self.pr.p(v3(p[0], p[1], p[2])));
        let kind = t.kind.to_ascii_lowercase();
        // size of a line / area target in sheet mm
        let target_len = size_txt
            .chars()
            .filter(|c| c.is_ascii_digit() || *c == '.')
            .collect::<String>()
            .parse::<f64>()
            .map(|v| v * self.pr.scale)
            .unwrap_or(10.0);
        let opts = self.attach_options(&r, fixed);
        let face_on = opts.first().is_some_and(|a| a.dot);
        let term = match kind.as_str() {
            "area" if face_on => Term::None,
            _ => Term::Cross,
        };
        let Some(pl) = self.place_leadered(&opts, dia, dia) else {
            self.warnings.push(format!("could not place datum target {}", t.label));
            return;
        };
        let o = v2(pl.b.min.x, pl.b.center().y);
        for p in prims.iter_mut() {
            p.translate(o);
        }
        let p = pl.att.p;
        let mut leader_pl = pl.clone();
        let mut stat = Vec::new();
        let mut circ = None;
        match kind.as_str() {
            "area" if face_on => {
                // hatched circular area; the leader stops on its boundary
                let ar = target_len / 2.0;
                circ = Some((p, ar));
                stat.push(Prim::Circle { c: p, r: ar, cls: "phantom" });
                let mut k = -ar + 0.8;
                while k < ar {
                    let half = (ar * ar - k * k).max(0.0).sqrt();
                    let u = v2(1.0, -1.0).norm() * k;
                    let dir = v2(1.0, 1.0).norm();
                    stat.push(Prim::Line { a: p + u - dir * half, b: p + u + dir * half, cls: "thin" });
                    k += 1.0;
                }
                leader_pl.att.p = p + pl.att.n * ar;
            }
            "line" | "area" => {
                let t2 = pl.att.n.perp();
                stat.push(Prim::Line { a: p - t2 * (target_len / 2.0), b: p + t2 * (target_len / 2.0), cls: "phantom" });
            }
            _ => {}
        }
        self.obst.push(pl.b.expand(0.5));
        self.soft.push(BBox2::new(pl.q, pl.a).expand(0.3));
        self.soft.push(BBox2::new(pl.q, pl.att.p).expand(0.3));
        let g = self.leader_group(&leader_pl, term, prims, stat, circ, false);
        self.gdt.push(g);
    }

    /// Cosmetic thread: ISO 6410 conventions. End-on, a thin 3/4 circle (at the
    /// major diameter in a hole, the minor on a shaft); side-on, thin lines
    /// along the thread and a thick line where it ends. Hidden internal
    /// threads are dashed (when the view draws hidden lines).
    /// Draws the thread and its callout on its own leader (unless
    /// `merged`); returns the callout text (None when there is no callout).
    fn thread(&mut self, t: &ThreadSpec, merged: bool) -> Option<String> {
        let sel = match select(self.m, &t.of) {
            Ok(s) => s,
            Err(e) => {
                self.warnings.push(format!("thread: {}", e));
                return None;
            }
        };
        // the cylindrical faces picked (an edge stands for its cylinders)
        let all = cylinders(self.m);
        let faces: Vec<usize> = match sel.kind {
            Kind::Face => sel.ids.clone(),
            Kind::Edge => sel.ids.iter().flat_map(|&e| edge_cyl_faces(self.m, &all, e)).collect(),
            Kind::Vertex => vec![],
        };
        let mut picked: Vec<usize> = (0..all.len()).filter(|&i| all[i].faces.iter().any(|f| faces.contains(f))).collect();
        if picked.is_empty() {
            self.warnings.push(format!("thread: {} has no cylindrical face", t.of));
            return None;
        }
        if t.all_alike {
            let c0 = &all[picked[0]];
            picked = (0..all.len())
                .filter(|&i| {
                    let c = &all[i];
                    (c.r - c0.r).abs() < 1e-3 && c.internal == c0.internal && c.z.dot(c0.z).abs() > 0.9999 && ((c.t1 - c.t0) - (c0.t1 - c0.t0)).abs() < 1e-2
                })
                .collect();
        }
        let c0 = all[picked[0]].clone();
        let internal = match t.kind.as_deref().map(|k| k.to_ascii_lowercase()) {
            Some(k) if k.starts_with("int") => true,
            Some(k) if k.starts_with("ext") => false,
            _ => c0.internal,
        };
        // major diameter and pitch
        let (size_major, size_pitch) = t.size.as_deref().and_then(parse_thread_size).map_or((None, None), |(d, p)| (Some(d), p));
        let major = t.major.or(size_major).unwrap_or(if internal { 0.0 } else { 2.0 * c0.r });
        if major <= 0.0 {
            self.warnings.push(format!("thread on {}: give \"size\" (e.g. \"M6\") or \"major\"", t.of));
            return None;
        }
        let pitch = t.pitch.or(size_pitch).or_else(|| coarse_pitch(major)).unwrap_or(major * 0.15);
        if internal && 2.0 * c0.r > major - 1e-6 {
            self.warnings.push(format!("thread on {}: the hole (Ø{}) is not smaller than the thread (Ø{}); model tapped holes at the tap drill size", t.of, fmt_num(2.0 * c0.r, 3, true), fmt_num(major, 3, true)));
        }
        // thin line: major diameter in a hole, minor on a shaft
        let r_thin = if internal { major / 2.0 } else { (major - 1.2269 * pitch) / 2.0 };
        let r_major = major / 2.0;
        let s = self.pr.scale;
        let d = self.pr.d;
        let mut prims = Vec::new();
        for &i in &picked {
            let c = &all[i];
            // the thread starts at the open end; through: the end higher in the model (+Z, then +Y, +X)
            let (open0, open1) = (cyl_end_open(self.m, c, false), cyl_end_open(self.m, c, true));
            let up = |v: V3| if v.z.abs() > 1e-6 { v.z } else if v.y.abs() > 1e-6 { v.y } else { v.x };
            let from_top = if open0 && open1 { up(c.z) > 0.0 } else { open1 || !open0 };
            let len = c.t1 - c.t0;
            let depth = t.depth.unwrap_or(len).clamp(0.0, len);
            let (ta, tb) = if from_top { (c.t1, c.t1 - depth) } else { (c.t0, c.t0 + depth) };
            let full = depth >= len - 1e-6;
            let zd = c.z.dot(d);
            if zd.abs() > 0.999 {
                // end-on: drawn when the thread's open end faces the viewer
                let faces_viewer = if from_top { zd > 0.0 } else { zd < 0.0 };
                if !faces_viewer && !(full && open0 && open1) {
                    continue;
                }
                let cc = self.pr.p(c.o);
                let r2 = r_thin * s;
                // 3/4 circle, open in the upper right quadrant (as seen on the sheet)
                let pts: Vec<V2> = (0..=54).map(|k| cc + V2::from_angle((90.0 + 270.0 * k as f64 / 54.0).to_radians()) * r2).collect();
                prims.push(Prim::Poly { pts, closed: false, cls: "thin" });
            } else if zd.abs() < 0.01 {
                // side-on
                let a = self.pr.p(c.o + c.z * ta);
                let b = self.pr.p(c.o + c.z * tb);
                if a.dist(b) < 1e-6 {
                    continue;
                }
                let n = (b - a).norm().perp();
                let cut = self.clip.as_ref().is_some_and(|cl| cl.d.dot(c.z).abs() < 1e-6 && (c.o - cl.c).dot(cl.d).abs() < c.r);
                let (cls, end_cls) = if !internal || cut {
                    ("thin", "visible")
                } else if self.hidden {
                    ("hidden", "hidden")
                } else {
                    continue;
                };
                for k in [-1.0, 1.0] {
                    prims.push(Prim::Line { a: a + n * (k * r_thin * s), b: b + n * (k * r_thin * s), cls });
                }
                if !full || !internal {
                    // thread end, across the major diameter
                    let rr = if internal { r_major } else { c.r.max(r_major) };
                    prims.push(Prim::Line { a: b + n * (-rr * s), b: b + n * (rr * s), cls: end_cls });
                }
            }
        }
        if !prims.is_empty() {
            self.dims.push(ann("thread", vec![], vec![], vec![role("static", prims)]));
        }
        // callout
        if t.callout.unwrap_or(true) {
            let n = picked.len();
            let desig = match &t.size {
                Some(sz) => sz.trim().to_string(),
                None => format!("Ø{}", fmt_num(major, 3, true)),
            };
            let through = cyl_end_open(self.m, &c0, false) && cyl_end_open(self.m, &c0, true);
            let len = c0.t1 - c0.t0;
            let tail = match t.depth {
                Some(dp) if dp < len - 1e-6 => format!(" x {} DEEP", fmt_num(dp, 2, true)),
                _ if internal && through => " THRU".to_string(),
                _ if internal => format!(" x {} DEEP", fmt_num(len, 2, true)),
                _ => String::new(),
            };
            let text = t.text.clone().unwrap_or_else(|| format!("{}{}{}", if n > 1 { format!("{}X ", n) } else { String::new() }, desig, tail));
            if !merged {
                let note = NoteSpec { text: text.clone(), attach: Some(t.of.clone()), boxed: false, ..Default::default() };
                self.note(&note);
            }
            return Some(text);
        }
        None
    }

    fn note(&mut self, n: &NoteSpec) {
        let h = self.h();
        let lines = wrap_text(&n.text);
        let text = lines.join("\n");
        let w = lines.iter().map(|l| text_width(l, h)).fold(0.0, f64::max) + 3.0;
        let ht = h + (lines.len() as f64 - 1.0) * 1.4 * h + 3.0;
        let mut prims = Vec::new();
        if n.boxed {
            prims.push(Prim::Rect { b: BBox2::new(v2(0.0, -ht / 2.0), v2(w, ht / 2.0)), cls: "box" });
        }
        prims.push(Prim::Text { p: v2(1.5, ht / 2.0 - 1.5 - h), h, anchor: Anchor::Start, rot: 0.0, s: text, cls: "dim" });
        if let Some(r) = n.attach.as_deref().and_then(|s| self.attach_ref(s)) {
            self.emit_leadered("note", &r, None, prims, w, ht, Term::Arrow, false);
            return;
        }
        // free note: nearest free spot below/right of the view
        let target = v2(self.geom.max.x, self.geom.min.y);
        let g = self.geom.expand(3.0);
        let mut best: Option<(f64, BBox2)> = None;
        for i in 0..80 {
            for j in 0..80 {
                let c = target + v2(-100.0 + i as f64 * 2.5, -100.0 + j as f64 * 2.5);
                let b = BBox2::from_center(c, w, ht);
                let sc = b.clamp(target).dist(target);
                if best.as_ref().is_some_and(|x| x.0 <= sc) || b.overlaps(&g) || !self.box_free(&b.expand(1.0)) {
                    continue;
                }
                best = Some((sc, b));
            }
        }
        let Some((_, b)) = best else {
            self.warnings.push("could not place note".into());
            return;
        };
        let o = v2(b.min.x, b.center().y);
        for p in prims.iter_mut() {
            p.translate(o);
        }
        self.obst.push(b.expand(0.5));
        self.gdt.push(ann("free", vec![], vec![], vec![role("sym", prims)]));
    }
}

/// An annotation of a view, by list and index.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Item {
    Dim(usize),
    Gdt(usize),
    Note(usize),
    Thread(usize),
}

/// Note text in lines of up to 48 characters (breaking at spaces and at "\n").
fn wrap_text(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for para in text.split('\n') {
        let mut cur = String::new();
        for word in para.split_whitespace() {
            if !cur.is_empty() && cur.len() + word.len() + 1 > 48 {
                lines.push(std::mem::take(&mut cur));
            }
            if !cur.is_empty() {
                cur.push(' ');
            }
            cur.push_str(word);
        }
        lines.push(cur);
    }
    lines
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Term {
    Arrow,
    Dot,
    Triangle,
    Cross,
    None,
}

/// A leader landing point with the outward normal of the edge there.
#[derive(Clone, Debug)]
struct Att {
    p: V2,
    n: V2,
    pen: f64,
    /// Feature seen face-on (leader ends in a dot).
    dot: bool,
    seg: Option<(V2, V2)>,
    /// circle seen face-on (centre, radius): the leader is radial
    circ: Option<(V2, f64)>,
}

#[derive(Clone, Debug)]
struct Placed {
    /// Symbol box.
    b: BBox2,
    /// Elbow between the normal leader and the horizontal shoulder.
    q: V2,
    /// Shoulder end on the symbol.
    a: V2,
    att: Att,
}

fn is_dimline(p: &Prim, horiz: bool) -> bool {
    if let Prim::Line { a, b, .. } = p {
        if horiz { (a.y - b.y).abs() < 1e-9 } else { (a.x - b.x).abs() < 1e-9 }
    } else {
        false
    }
}

fn avg(r: &[RefGeo]) -> V3 {
    r.iter().fold(V3::default(), |a, g| a + g.rep) / r.len().max(1) as f64
}

/// Geometric characteristic symbols drawn with primitives (size s, centred at c).
/// Single-letter modifiers drawn in a circle (MMC, LMC, free state,
/// projected zone, tangent plane, unequal profile, RFS, envelope).
const CIRCLED: &str = "MLFPTUSE";

fn circled_split(m: &str) -> Option<(char, &str)> {
    let mut it = m.chars();
    let c = it.next()?;
    let rest = it.as_str().trim_start();
    let ok = CIRCLED.contains(c) && (rest.is_empty() || rest.starts_with(|r: char| r.is_ascii_digit() || r == '.' || r == '+' || r == '-'));
    ok.then_some((c, rest))
}

fn known_modifier(m: &str) -> bool {
    let u = m.to_uppercase();
    circled_split(&u).is_some() || matches!(u.as_str(), "ST" | "CZ" | "SZ" | "UZ" | "ACS" | "LD" | "MD" | "PD" | "CF" | "<>" | "><")
}

/// "B(M)", "A-B (M)", "C M" -> ("B", ["M"])
struct BlockLayout {
    /// far edge of the text and datum symbol (frames start beyond it)
    top: f64,
    text_far: f64,
    line_y: f64,
    tri_h: f64,
    /// datum box near / far edge
    datum: Option<(f64, f64)>,
    /// far edge of the last frame
    frames_top: Option<f64>,
}

pub fn parse_datum_ref(d: &str) -> (String, Vec<String>) {
    let d = d.trim();
    let i = d.find(|c: char| c == '(' || c.is_whitespace()).unwrap_or(d.len());
    let mods = d[i..].split(|c: char| c == '(' || c == ')' || c.is_whitespace()).filter(|t| !t.is_empty()).map(String::from).collect();
    (d[..i].to_string(), mods)
}

/// Contents of a frame cell: text followed by modifier symbols, laid out from
/// x = 0 on the frame's centre line. Returns the prims and the width.
fn frame_cell(text: &str, mods: &[String], h: f64) -> (Vec<Prim>, f64) {
    let ty = -0.35 * h;
    let mut v = Vec::new();
    let mut x = 0.0;
    if !text.is_empty() {
        v.push(Prim::Text { p: v2(0.0, ty), h, anchor: Anchor::Start, rot: 0.0, s: text.to_string(), cls: "dim" });
        x = text_width(text, h);
    }
    for m in mods {
        let u = m.to_uppercase();
        if x > 0.0 {
            x += 0.4;
        }
        if let Some((c, rest)) = circled_split(&u) {
            let r = h * 0.5;
            v.push(Prim::Circle { c: v2(x + r, 0.0), r, cls: "thin" });
            v.push(Prim::Text { p: v2(x + r, -h * 0.28), h: h * 0.75, anchor: Anchor::Middle, rot: 0.0, s: c.to_string(), cls: "dim" });
            x += 2.0 * r;
            if !rest.is_empty() {
                x += 0.4;
                v.push(Prim::Text { p: v2(x, ty), h, anchor: Anchor::Start, rot: 0.0, s: rest.to_string(), cls: "dim" });
                x += text_width(rest, h);
            }
        } else if u == "ST" {
            // statistical tolerance: ST in a hexagon
            let (w, hh) = (h * 1.7, h * 0.55);
            let pts = vec![v2(x, 0.0), v2(x + hh * 0.55, hh), v2(x + w - hh * 0.55, hh), v2(x + w, 0.0), v2(x + w - hh * 0.55, -hh), v2(x + hh * 0.55, -hh)];
            v.push(Prim::Poly { pts, closed: true, cls: "thin" });
            v.push(Prim::Text { p: v2(x + w / 2.0, -h * 0.26), h: h * 0.7, anchor: Anchor::Middle, rot: 0.0, s: "ST".into(), cls: "dim" });
            x += w;
        } else {
            v.push(Prim::Text { p: v2(x, ty), h, anchor: Anchor::Start, rot: 0.0, s: m.clone(), cls: "dim" });
            x += text_width(m, h);
        }
    }
    (v, x)
}

pub fn gdt_symbol(name: &str, c: V2, s: f64) -> Vec<Prim> {
    let l = |a: V2, b: V2| Prim::Line { a: c + a * s, b: c + b * s, cls: "thin" };
    let circ = |r: f64| Prim::Circle { c, r: r * s, cls: "thin" };
    let arc = |r: f64, a0: f64, a1: f64, dy: f64, closed: bool| Prim::Poly {
        pts: (0..=24).map(|i| c + (V2::from_angle(a0 + (a1 - a0) * i as f64 / 24.0) * r + v2(0.0, dy)) * s).collect(),
        closed,
        cls: "thin",
    };
    let n = name.to_ascii_lowercase().replace([' ', '-'], "_");
    match n.as_str() {
        "straightness" => vec![l(v2(-0.5, 0.0), v2(0.5, 0.0))],
        "flatness" => vec![Prim::Poly {
            pts: [v2(-0.5, -0.25), v2(0.25, -0.25), v2(0.5, 0.25), v2(-0.25, 0.25)].iter().map(|p| c + *p * s).collect(),
            closed: true,
            cls: "thin",
        }],
        "circularity" | "roundness" => vec![circ(0.42)],
        "cylindricity" => vec![circ(0.32), l(v2(-0.2, -0.5), v2(0.05, 0.5)), l(v2(0.12, -0.5), v2(0.37, 0.5))],
        "profile_line" | "profile_of_a_line" => vec![arc(0.45, 0.0, PI, -0.2, false)],
        "profile_surface" | "profile_of_a_surface" => vec![arc(0.45, 0.0, PI, -0.2, true)],
        "angularity" => vec![l(v2(-0.5, -0.35), v2(0.5, -0.35)), l(v2(-0.5, -0.35), v2(0.4, 0.4))],
        "perpendicularity" => vec![l(v2(-0.5, -0.4), v2(0.5, -0.4)), l(v2(0.0, -0.4), v2(0.0, 0.5))],
        "parallelism" => vec![l(v2(-0.35, -0.45), v2(0.0, 0.45)), l(v2(0.0, -0.45), v2(0.35, 0.45))],
        "position" | "true_position" => vec![circ(0.3), l(v2(-0.5, 0.0), v2(0.5, 0.0)), l(v2(0.0, -0.5), v2(0.0, 0.5))],
        "concentricity" | "coaxiality" => vec![circ(0.22), circ(0.45)],
        "symmetry" => vec![l(v2(-0.5, 0.0), v2(0.5, 0.0)), l(v2(-0.3, 0.28), v2(0.3, 0.28)), l(v2(-0.3, -0.28), v2(0.3, -0.28))],
        "circular_runout" | "runout" => {
            let tip = c + v2(0.35, 0.45) * s;
            let d = (v2(-0.35, -0.45) - v2(0.35, 0.45)).norm();
            let n = d.perp() * (0.15 * s);
            vec![l(v2(-0.35, -0.45), v2(0.35, 0.45)), Prim::Fill { pts: vec![tip, tip + d * (0.4 * s) + n, tip + d * (0.4 * s) - n] }]
        }
        "total_runout" => {
            let mut v = vec![l(v2(-0.5, -0.45), v2(0.5, -0.45))];
            for x0 in [-0.45, 0.05] {
                let a = v2(x0, -0.45);
                let b = v2(x0 + 0.4, 0.45);
                let d = (a - b).norm();
                let tip = c + b * s;
                let n = d.perp() * (0.13 * s);
                v.push(l(a, b));
                v.push(Prim::Fill { pts: vec![tip, tip + d * (0.35 * s) + n, tip + d * (0.35 * s) - n] });
            }
            v
        }
        _ => vec![],
    }
}

/// A cylinder of the model: its faces (a hole is often split in two), axis,
/// radius, extent along the axis and whether it is a hole.
#[derive(Clone, Debug)]
struct Cyl {
    faces: Vec<usize>,
    o: V3,
    z: V3,
    r: f64,
    t0: f64,
    t1: f64,
    internal: bool,
}

/// The model's cylinders, coaxial faces of the same radius merged.
fn cylinders(m: &Model) -> Vec<Cyl> {
    let mut out: Vec<Cyl> = Vec::new();
    for (fi, f) in m.faces.iter().enumerate() {
        let SurfKind::Cylinder { r } = f.surf.kind else { continue };
        let (o, z) = (f.surf.fr.o, f.surf.fr.z.norm());
        let ts: Vec<f64> = f.loop_pts.iter().flatten().map(|p| (*p - o).dot(z)).collect();
        if ts.is_empty() {
            continue;
        }
        let (t0, t1) = (ts.iter().copied().fold(f64::MAX, f64::min), ts.iter().copied().fold(f64::MIN, f64::max));
        // material outside the surface (normal towards the axis): a hole
        let internal = !f.same_sense;
        // the same surface: coaxial, same radius, and touching along the axis
        // (two shafts of the same diameter either side of a collar stay apart)
        let near = |c: &Cyl| {
            let w = o - c.o;
            let (a, b) = ((o + z * t0 - c.o).dot(c.z), (o + z * t1 - c.o).dot(c.z));
            (c.r - r).abs() < 1e-4
                && c.z.cross(z).len() < 1e-6
                && (w - c.z * w.dot(c.z)).len() < 1e-4
                && c.internal == internal
                && a.max(b) > c.t0 - 1e-4
                && a.min(b) < c.t1 + 1e-4
        };
        if let Some(c) = out.iter_mut().find(|c| near(c)) {
            let (a, b) = ((o + z * t0 - c.o).dot(c.z), (o + z * t1 - c.o).dot(c.z));
            c.t0 = c.t0.min(a.min(b));
            c.t1 = c.t1.max(a.max(b));
            c.faces.push(fi);
        } else {
            out.push(Cyl { faces: vec![fi], o, z, r, t0, t1, internal });
        }
    }
    out
}

/// The cylindrical faces an edge stands for: the cylinders it bounds, or for a
/// circle away from any cylinder (the rim of a chamfer or countersink) the
/// coaxial cylinder whose end is nearest it.
fn edge_cyl_faces(m: &Model, all: &[Cyl], ei: usize) -> Vec<usize> {
    let e = &m.edges[ei];
    let own: Vec<usize> = e.faces.iter().copied().filter(|f| all.iter().any(|c| c.faces.contains(f))).collect();
    if !own.is_empty() {
        return own;
    }
    let Curve::Circle { fr, r } = &e.curve else { return vec![] };
    let (o, z) = (fr.o, fr.z.norm());
    all.iter()
        .filter(|c| {
            let w = o - c.o;
            c.z.cross(z).len() < 1e-6 && (w - c.z * w.dot(c.z)).len() < 1e-4
        })
        .map(|c| {
            let t = (o - c.o).dot(c.z);
            let gap = if t < c.t0 { c.t0 - t } else if t > c.t1 { t - c.t1 } else { 0.0 };
            (gap + (c.r - r).abs(), c)
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, c)| c.faces.clone())
        .unwrap_or_default()
}

/// Is the cylinder open at its +axis end (`top`) or its -axis end: does it
/// end on a face looking outwards there, widen into a countersink, or break
/// out through a face across its axis (a slanted or curved wall)?
fn cyl_end_open(m: &Model, c: &Cyl, top: bool) -> bool {
    let (t, s) = if top { (c.t1, c.z) } else { (c.t0, -c.z) };
    for &fi in &c.faces {
        for &ei in &m.faces[fi].edges {
            let e = &m.edges[ei];
            // an edge reaching this end (a slanted exit's edge only touches it)
            if !e.pts.iter().any(|p| ((*p - c.o).dot(c.z) - t).abs() < 1e-3) {
                continue;
            }
            for &g in e.faces.iter().filter(|g| !c.faces.contains(g)) {
                let gf = &m.faces[g];
                // a round face on another axis: the hole / shaft runs out through it
                let coaxial = || {
                    let w = gf.surf.fr.o - c.o;
                    gf.surf.fr.z.norm().cross(c.z).len() < 1e-6 && (w - c.z * w.dot(c.z)).len() < 1e-4
                };
                match gf.surf.kind {
                    SurfKind::Plane => {
                        if gf.plane_normal().is_some_and(|n| n.dot(s) > 0.5 || (n.dot(c.z).abs() < 0.999 && n.dot(s) > 1e-6)) {
                            return true;
                        }
                    }
                    SurfKind::Cylinder { .. } | SurfKind::Sphere { .. } | SurfKind::Torus { .. } if !coaxial() => return true,
                    SurfKind::Cone { .. } if !coaxial() => return true,
                    SurfKind::Cone { .. } => {
                        // a chamfer at the mouth: widening into a hole's countersink,
                        // narrowing to a shaft's end
                        let rad: Vec<f64> = gf
                            .loop_pts
                            .iter()
                            .flatten()
                            .map(|p| {
                                let w = *p - c.o;
                                (w - c.z * w.dot(c.z)).len()
                            })
                            .collect();
                        let open = if c.internal {
                            rad.iter().copied().fold(0.0, f64::max) > c.r * 1.01
                        } else {
                            rad.iter().copied().fold(f64::MAX, f64::min) < c.r * 0.99 && rad.iter().all(|&q| q < c.r * 1.001)
                        };
                        if open {
                            return true;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    false
}

/// "M10x1.25" -> (10, Some(1.25)); "M6" -> (6, None).
fn parse_thread_size(s: &str) -> Option<(f64, Option<f64>)> {
    let t = s.trim().trim_start_matches(['M', 'm']);
    let mut it = t.splitn(2, ['x', 'X', '×']);
    let d: f64 = it.next()?.trim().parse().ok()?;
    let p = it.next().and_then(|p| p.trim().split_whitespace().next()?.parse().ok());
    Some((d, p))
}

/// ISO coarse pitch for a metric major diameter.
fn coarse_pitch(d: f64) -> Option<f64> {
    const T: [(f64, f64); 26] = [
        (1.0, 0.25), (1.2, 0.25), (1.6, 0.35), (2.0, 0.4), (2.5, 0.45), (3.0, 0.5), (4.0, 0.7), (5.0, 0.8), (6.0, 1.0), (8.0, 1.25),
        (10.0, 1.5), (12.0, 1.75), (14.0, 2.0), (16.0, 2.0), (18.0, 2.5), (20.0, 2.5), (22.0, 2.5), (24.0, 3.0), (27.0, 3.0), (30.0, 3.5),
        (33.0, 3.5), (36.0, 4.0), (42.0, 4.5), (48.0, 5.0), (56.0, 5.5), (64.0, 6.0),
    ];
    T.iter().find(|t| (t.0 - d).abs() < 1e-6).map(|t| t.1)
}
