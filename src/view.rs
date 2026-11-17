//! View construction (projection + annotation) and sheet layout.

use crate::annot::Annotator;
use crate::brep::Model;
use crate::config::*;
use crate::geom::*;
use crate::hlr::{self, Projector};
use crate::sheet;
use crate::svg::{bbox_of, text_box, Anchor, Prim};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Base,
    Right,
    Left,
    Above,
    Below,
    FarRight,
}

pub struct View {
    pub name: String,
    pub geometry: Vec<Prim>,
    pub gdt: Vec<Prim>,
    pub dims: Vec<Prim>,
    /// Invisible pickable faces / edges / vertices (editor only).
    pub pick: Vec<Prim>,
    pub bbox: BBox2,
    pub pos: V2,
    pub place: Place,
    pub fixed: bool,
    pub frame: (V3, V3, V3),
    /// The view this one is projected from (aligned with).
    pub parent: Option<usize>,
    /// Detail view: (parent view, detail centre in the parent's local coords).
    pub anchor: Option<(usize, V2)>,
    /// Aligned view placed at this distance from its parent along the axis (from the JSON).
    pub dist: Option<f64>,
    /// Point of the view (local coords) its `location` refers to: the origin,
    /// or a detail's centre.
    pub loc_ref: V2,
    /// JSON pointer of the view's spec, e.g. "/views/0/projected/1" (editor).
    pub src: String,
    /// Model point at the view's origin and the view's scale (editor: sheet to model).
    pub origin: V3,
    pub scale: f64,
    /// Hidden lines drawn in this view.
    pub hidden: bool,
}

impl View {
    pub fn world_bbox(&self) -> BBox2 {
        self.bbox.translate(self.pos)
    }
    /// Union of the boxes of this view's items that overlap `r` (world coords).
    pub fn hits(&self, r: &BBox2) -> BBox2 {
        self.geometry
            .iter()
            .chain(&self.gdt)
            .chain(&self.dims)
            .map(|p| p.bbox().translate(self.pos))
            .filter(|b| b.overlaps(r))
            .fold(BBox2::empty(), |a, b| a.union(&b))
    }
}

fn fmt_mm(v: f64) -> String {
    crate::annot::fmt_num(v, 3, true)
}

fn face_info(f: &crate::brep::Face) -> String {
    use crate::brep::SurfKind::*;
    match &f.surf.kind {
        Plane => "PLANE".into(),
        Cylinder { r } => format!("CYLINDER Ø{}", fmt_mm(2.0 * r)),
        Cone { r, semi } => format!("CONE Ø{} {}°", fmt_mm(2.0 * r), fmt_mm(semi.to_degrees())),
        Sphere { r } => format!("SPHERE Ø{}", fmt_mm(2.0 * r)),
        Torus { major, minor } => format!("TORUS R{} r{}", fmt_mm(*major), fmt_mm(*minor)),
        BSpline(_) => "BSPLINE".into(),
        Other(s) => s.to_uppercase(),
    }
}

fn vec3_attr(v: V3) -> String {
    format!("{:.5} {:.5} {:.5}", v.x, v.y, v.z)
}

/// Pick attributes of a face: index, description and (planes) the normal.
fn face_attrs(fi: usize, f: &crate::brep::Face) -> Vec<(&'static str, String)> {
    let mut a = vec![("data-face", fi.to_string()), ("data-info", face_info(f))];
    if let Some(n) = f.plane_normal() {
        a.push(("data-n", vec3_attr(n)));
    }
    a
}

fn edge_info(m: &Model, e: &crate::brep::Edge) -> String {
    use crate::brep::Curve::*;
    match &e.curve {
        Line { .. } => format!("LINE {}", fmt_mm(m.verts[e.v0].dist(m.verts[e.v1]))),
        Circle { r, .. } if e.v0 == e.v1 => format!("CIRCLE Ø{}", fmt_mm(2.0 * r)),
        Circle { r, .. } => format!("ARC R{}", fmt_mm(*r)),
        c => c.type_name().to_string(),
    }
}

/// The region on the sheet of a hole's cylinder or cone (seen side-on) that a
/// section cuts: the part of it behind the cutting plane, and its mean depth.
fn cut_round_region(f: &crate::brep::Face, pr: &Projector, c: &hlr::Clip, eps: f64) -> Option<(Vec<V2>, f64)> {
    use crate::brep::SurfKind;
    if f.same_sense || !matches!(f.surf.kind, SurfKind::Cylinder { .. } | SurfKind::Cone { .. }) {
        return None;
    }
    let (o, z) = (f.surf.fr.o, f.surf.fr.z.norm());
    if z.dot(pr.d).abs() > 0.01 {
        return None;
    }
    let pts: Vec<V3> = f.loop_pts.iter().flatten().copied().collect();
    let t = |p: V3| (p - o).dot(z);
    let rad = |p: V3| {
        let w = p - o;
        (w - z * w.dot(z)).len()
    };
    let t0 = pts.iter().map(|p| t(*p)).fold(f64::MAX, f64::min);
    let t1 = pts.iter().map(|p| t(*p)).fold(f64::MIN, f64::max);
    if t1 - t0 < eps {
        return None;
    }
    // radius at each end (a cone's differ)
    let r_at = |te: f64| pts.iter().filter(|p| (t(**p) - te).abs() < (t1 - t0) * 1e-3 + eps).map(|p| rad(*p)).fold(0.0, f64::max);
    let (r0, r1) = (r_at(t0), r_at(t1));
    // the kept arc of each end circle: its extent across the axis on the sheet
    let u = z.cross(if z.x.abs() < 0.9 { v3(1.0, 0.0, 0.0) } else { v3(0.0, 1.0, 0.0) }).norm();
    let v = z.cross(u);
    let a2 = pr.dir(z).norm();
    let n2 = a2.perp();
    let mut sum = 0.0;
    let mut cnt = 0.0;
    let mut span = |te: f64, r: f64| -> Option<(f64, f64)> {
        let mut lo = f64::MAX;
        let mut hi = f64::MIN;
        for k in 0..360 {
            let th = (k as f64).to_radians();
            let p = o + z * te + (u * th.cos() + v * th.sin()) * r;
            if c.removed(p, eps) || !f.contains(p) {
                continue;
            }
            let q = pr.p(p).dot(n2);
            lo = lo.min(q);
            hi = hi.max(q);
            sum += p.dot(pr.d);
            cnt += 1.0;
        }
        (hi > lo).then_some((lo, hi))
    };
    let (s0, s1) = (span(t0 + (t1 - t0) * 1e-3, r0)?, span(t1 - (t1 - t0) * 1e-3, r1)?);
    let (e0, e1) = (pr.p(o + z * t0).dot(a2), pr.p(o + z * t1).dot(a2));
    let at = |along: f64, across: f64| a2 * along + n2 * across;
    let quad = vec![at(e0, s0.0), at(e0, s0.1), at(e1, s1.1), at(e1, s1.0)];
    Some((quad, sum / cnt))
}

/// Invisible pickable layer for the editor: face regions (painted far to
/// near, so the nearest face wins), visible edge runs and silhouettes, and
/// visible vertices, each tagged with its model index.
fn pick_layer(m: &Model, pr: &Projector, lines: &hlr::ViewLines, clip: Option<&hlr::Clip>, circle: Option<Ell>) -> Vec<Prim> {
    use crate::brep::SurfKind;
    let d = pr.d;
    let eps = m.diag() * 1e-5;
    let clip_runs = |l: &Vec<V2>| -> Vec<Vec<V2>> {
        match &circle {
            Some(e) => clip_poly_ell(l, e),
            None => vec![l.clone()],
        }
    };
    // faces
    let mut faces: Vec<(f64, Prim)> = Vec::new();
    for (fi, f) in m.faces.iter().enumerate() {
        match f.surf.kind {
            SurfKind::Plane => {
                // seen edge-on or from behind
                if f.plane_normal().is_none_or(|n| n.dot(d) < 1e-3) {
                    continue;
                }
            }
            SurfKind::Cylinder { .. } | SurfKind::Cone { .. } if f.surf.fr.z.dot(d).abs() > 0.999 => continue,
            _ => {}
        }
        let mut depth = f.center.dot(d);
        let mut loops: Vec<Vec<V2>> = match clip {
            Some(c) if f.loop_pts.iter().flatten().any(|p| c.removed(*p, eps)) => {
                // cut by the section: a hole's cylinder / cone seen side-on picks
                // by the part left behind the cutting plane
                match cut_round_region(f, pr, c, eps) {
                    Some((l, z)) => {
                        depth = z;
                        vec![l]
                    }
                    None => continue,
                }
            }
            _ => f.loop_pts.iter().map(|l| l.iter().map(|p| pr.p(*p)).collect()).collect(),
        };
        if let Some(e) = &circle {
            loops = loops.iter().map(|l| clip_region_ell(l, e)).collect();
        }
        loops.retain(|l| l.len() >= 3);
        if loops.iter().map(|l| poly_area(l).abs()).fold(0.0, f64::max) < 0.3 {
            continue;
        }
        let g = Prim::Group {
            attrs: face_attrs(fi, f),
            pts: vec![],
            items: vec![Prim::Area { loops, cls: "pk" }],
        };
        faces.push((depth, g));
    }
    faces.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<Prim> = faces.into_iter().map(|f| f.1).collect();
    // silhouettes pick their face
    for (fi, run) in &lines.sil_runs {
        let items: Vec<Prim> = clip_runs(run).into_iter().map(|pts| Prim::Poly { pts, closed: false, cls: "pk" }).collect();
        if !items.is_empty() {
            out.push(Prim::Group { attrs: face_attrs(*fi, &m.faces[*fi]), pts: vec![], items });
        }
    }
    // edges, one group per edge
    let mut ends: Vec<(usize, V2)> = Vec::new();
    // visible and hidden runs both pick their edge
    let runs: Vec<&(usize, Vec<V2>)> = lines.edge_runs.iter().chain(&lines.hidden_runs).collect();
    let mut ids: Vec<usize> = runs.iter().map(|e| e.0).collect();
    ids.sort();
    ids.dedup();
    for ei in ids {
        let mut items = Vec::new();
        for (_, run) in runs.iter().filter(|e| e.0 == ei) {
            for pts in clip_runs(run) {
                ends.push((ei, pts[0]));
                ends.push((ei, *pts.last().unwrap()));
                items.push(Prim::Poly { pts, closed: false, cls: "pk" });
            }
        }
        if !items.is_empty() {
            let e = &m.edges[ei];
            let mut attrs = vec![("data-edge", ei.to_string()), ("data-info", edge_info(m, e)), ("data-v", format!("{} {}", e.v0, e.v1))];
            if let crate::brep::Curve::Line { dir } = &e.curve {
                attrs.push(("data-d", vec3_attr(*dir)));
            }
            out.push(Prim::Group { attrs, pts: vec![], items });
        }
    }
    // vertices at the visible ends of their edges
    for (vi, v) in m.verts.iter().enumerate() {
        let p = pr.p(*v);
        if ends.iter().any(|(ei, q)| q.dist(p) < 0.05 && m.vert_edges[vi].contains(ei)) {
            out.push(Prim::Group {
                attrs: vec![("data-vertex", vi.to_string()), ("data-info", format!("VERTEX ({}, {}, {})", fmt_mm(v.x), fmt_mm(v.y), fmt_mm(v.z)))],
                pts: vec![],
                items: vec![Prim::Circle { c: p, r: 0.8, cls: "pk" }],
            });
        }
    }
    out
}

/// (right, up, dir) frame for a base view.
pub fn base_frame(dir: [f64; 3], up: Option<[f64; 3]>) -> Result<(V3, V3, V3), String> {
    let d = v3(dir[0], dir[1], dir[2]);
    if d.len() < 1e-12 {
        return Err("view direction must be non-zero".into());
    }
    let d = d.norm();
    let u = match up {
        Some(u) => v3(u[0], u[1], u[2]),
        None => {
            if d.z.abs() < 0.99 {
                v3(0.0, 0.0, 1.0)
            } else {
                v3(0.0, 1.0, 0.0)
            }
        }
    };
    let u = u - d * u.dot(d);
    if u.len() < 1e-9 {
        return Err("view 'up' vector is parallel to the view direction".into());
    }
    let u = u.norm();
    Ok((u.cross(d), u, d))
}

/// Frame and placement of a projected view seen from `position` of the parent.
pub fn projected_frame(parent: (V3, V3, V3), position: &str, proj: Projection) -> Result<((V3, V3, V3), Place), String> {
    let (r, u, d) = parent;
    let (nd, nu, place3, place1) = match position.to_ascii_lowercase().as_str() {
        "right" => (r, u, Place::Right, Place::Left),
        "left" => (-r, u, Place::Left, Place::Right),
        "top" | "above" | "plan" => (u, -d, Place::Above, Place::Below),
        "bottom" | "below" | "underside" => (-u, d, Place::Below, Place::Above),
        "rear" | "back" => (-d, u, Place::FarRight, Place::FarRight),
        p => return Err(format!("unknown projected view position '{}'", p)),
    };
    let place = if proj == Projection::Third { place3 } else { place1 };
    Ok(((nu.cross(nd), nu, nd), place))
}

pub fn sanitize(s: &str) -> String {
    let mut o: String = s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    if !o.starts_with(|c: char| c.is_ascii_alphabetic()) {
        o.insert(0, 'v');
    }
    o
}

/// A detail circle (or ellipse) to draw on a view.
pub struct DetailMark {
    pub label: String,
    pub c: V3,
    pub shape: DetShape,
    /// JSON pointer of the detail's spec.
    pub src: String,
}

/// Boundary of a detail: semi-axes in model units (equal for a circle), the
/// first along `ang` degrees on the sheet.
#[derive(Clone, Copy, Debug)]
pub struct DetShape {
    pub r: f64,
    pub r2: f64,
    pub ang: f64,
}

impl DetShape {
    /// The boundary on the sheet around `c` (sheet mm) at `scale`.
    pub fn ell(&self, c: V2, scale: f64) -> Ell {
        Ell { c, a: self.r * scale, b: self.r2 * scale, ang: self.ang.to_radians() }
    }
}

/// The prims with groups opened up.
fn flat(prims: &[Prim]) -> Vec<Prim> {
    prims.iter().flat_map(|p| match p {
        Prim::Group { items, .. } => flat(items),
        p => vec![p.clone()],
    }).collect()
}

/// A cutting plane to draw on a view.
pub struct Cut {
    pub label: String,
    pub c: V3,
    pub d: V3,
    /// JSON pointer of the section's spec.
    pub src: String,
}

pub struct ViewOpts<'a> {
    pub name: String,
    pub frame: (V3, V3, V3),
    pub scale: f64,
    pub hidden: bool,
    pub ann: &'a Annotations,
    /// JSON pointer of the view's spec (annotations record where they came from)
    pub src: String,
    /// Cutting-plane symbols to show on this view.
    pub cuts: Vec<Cut>,
    /// Section view: the cutting plane and hatch (angle, spacing).
    pub section: Option<(hlr::Clip, f64, f64)>,
    /// Title shown under the view ("SECTION A-A", "DETAIL B (2:1)").
    pub title: Option<String>,
    /// Move of the title from its default place (sheet mm).
    pub title_offset: V2,
    /// Detail circles to show on this view.
    pub marks: Vec<DetailMark>,
    /// Detail view: only the part inside this circle (centre, model radius,
    /// draw the boundary).
    pub detail: Option<(V3, DetShape, bool)>,
}

pub fn build_view(m: &Model, cfg: &Config, o: ViewOpts, warnings: &mut Vec<String>) -> View {
    let frame = o.frame;
    let name = o.name.as_str();
    let pr = Projector { o: m.bbox.center(), r: frame.0, u: frame.1, d: frame.2, scale: o.scale };
    let clip = o.section.as_ref().map(|s| s.0);
    let mut lines = hlr::compute(m, &pr, o.hidden, cfg.centre_lines, cfg.tangent_edges, clip);
    let pick = if cfg.pick_layer { pick_layer(m, &pr, &lines, clip.as_ref(), o.detail.map(|(c, sh, _)| sh.ell(pr.p(c), o.scale))) } else { vec![] };
    let mut hatch = Vec::new();
    if let Some((_, ang, sp)) = &o.section {
        if lines.cut.is_empty() {
            warnings.push(format!("view '{}': the cutting plane does not cut the part", name));
        }
        hatch = crate::section::hatch(&lines.cut, *ang, *sp);
    }
    // detail view: keep what lies inside the detail circle
    let mut boundary: Vec<V2> = Vec::new();
    if let Some((c, sh, border)) = o.detail {
        let e = sh.ell(pr.p(c), o.scale);
        let clip_all = |ls: &[Vec<V2>]| ls.iter().flat_map(|l| clip_poly_ell(l, &e)).collect::<Vec<_>>();
        let clip_segs = |ss: &[(V2, V2)]| {
            ss.iter().flat_map(|(a, b)| clip_poly_ell(&[*a, *b], &e)).map(|l| (l[0], *l.last().unwrap())).collect::<Vec<_>>()
        };
        lines.visible = clip_all(&lines.visible);
        lines.hidden = clip_all(&lines.hidden);
        lines.tangent = clip_all(&lines.tangent);
        lines.center = clip_segs(&lines.center);
        hatch = clip_segs(&hatch);
        if lines.visible.is_empty() {
            warnings.push(format!("view '{}': no geometry inside the detail circle", name));
        }
        if border {
            boundary = e.ring(96);
        }
    }
    let mut geometry = Vec::new();
    for (a, b) in hatch {
        geometry.push(Prim::Line { a, b, cls: "hatch" });
    }
    if !boundary.is_empty() {
        geometry.push(Prim::Poly { pts: boundary.clone(), closed: true, cls: "thin" });
    }
    for l in &lines.tangent {
        geometry.push(Prim::Poly { pts: l.clone(), closed: false, cls: "tangent" });
    }
    for l in &lines.visible {
        geometry.push(Prim::Poly { pts: l.clone(), closed: false, cls: "visible" });
    }
    for l in &lines.hidden {
        geometry.push(Prim::Poly { pts: l.clone(), closed: false, cls: "hidden" });
    }
    for (a, b) in &lines.center {
        geometry.push(Prim::Line { a: *a, b: *b, cls: "center" });
    }
    let mut geom_bb = BBox2::empty();
    for l in lines.visible.iter().chain(std::iter::once(&boundary)) {
        for p in l {
            geom_bb.add(*p);
        }
    }
    let mut cut_prims = Vec::new();
    for c in &o.cuts {
        match crate::section::cut_line(&pr, geom_bb, c.c, c.d, &c.label, cfg.text_height, cfg.arrow_length * 1.3, cfg.arrow_width * 1.3) {
            // one group per cutting plane, so the editor can pick and drag it
            Some(p) => cut_prims.push(Prim::Group {
                attrs: vec![
                    ("class", "cut".into()),
                    ("data-section", c.src.clone()),
                    ("data-label", c.label.clone()),
                    ("data-n", crate::svg::vec_attr(pr.dir(c.d).norm())),
                    ("data-c", format!("{},{},{}", c.c.x, c.c.y, c.c.z)),
                    ("data-d", format!("{},{},{}", c.d.x, c.d.y, c.d.z)),
                    ("data-scale", o.scale.to_string()),
                ],
                pts: vec![],
                items: p,
            }),
            None => warnings.push(format!("view '{}': section {} cutting plane is not edge-on in this view", name, c.label)),
        }
    }
    // detail circles / ellipses with their letter, placed clear of the geometry;
    // one group each, so the editor can pick and drag them
    let mut mark_block = Vec::new();
    let mut mark_prims: Vec<Prim> = Vec::new();
    for mk in &o.marks {
        let c2 = pr.p(mk.c);
        let e = mk.shape.ell(c2, o.scale);
        let ring = e.ring(if mk.shape.r == mk.shape.r2 { 24 } else { 48 });
        for w in ring.windows(2) {
            mark_block.push(Prim::Line { a: w[0], b: w[1], cls: "thin" });
        }
        let outline = if mk.shape.r == mk.shape.r2 { Prim::Circle { c: c2, r: e.a, cls: "thin" } } else { Prim::Poly { pts: e.ring(96), closed: true, cls: "thin" } };
        let h = cfg.text_height * 1.2;
        let free = |b: &BBox2| {
            !lines.visible.iter().any(|l| l.windows(2).any(|w| b.hits_segment(w[0], w[1])))
                && !flat(&cut_prims).iter().chain(flat(&mark_prims).iter()).any(|p| matches!(p, Prim::Text { .. }) && p.bbox().overlaps(b))
        };
        let at = |dir: V2| c2 + dir * (e.reach(dir) + h * 0.9) - v2(0.0, h * 0.35);
        let mut best = None;
        for k in [1, 3, 0, 2, 7, 5, 4, 6] {
            let p = at(V2::from_angle(k as f64 * std::f64::consts::FRAC_PI_4));
            let b = text_box(p, h, Anchor::Middle, 0.0, &mk.label);
            if free(&b) {
                best = Some(p);
                break;
            }
        }
        let p = best.unwrap_or(at(V2::from_angle(std::f64::consts::FRAC_PI_4)));
        let label = Prim::Text { p, h, anchor: Anchor::Middle, rot: 0.0, s: mk.label.clone(), cls: "title" };
        mark_block.push(label.clone());
        let v3s = |v: V3| format!("{},{},{}", v.x, v.y, v.z);
        mark_prims.push(Prim::Group {
            attrs: vec![
                ("class", "dmark".into()),
                ("data-detail", mk.src.clone()),
                ("data-label", mk.label.clone()),
                ("data-c", v3s(mk.c)),
                ("data-r", v3s(pr.r)),
                ("data-u", v3s(pr.u)),
                ("data-scale", o.scale.to_string()),
            ],
            pts: vec![],
            items: vec![outline, label],
        });
    }
    let mut an_lines = lines.visible.clone();
    if !boundary.is_empty() {
        an_lines.push(boundary.clone());
    }
    let mut an = Annotator::new(m, pr, cfg, &an_lines);
    an.hidden = o.hidden;
    an.clip = o.section.as_ref().map(|s| s.0);
    an.block(&flat(&cut_prims).into_iter().filter(|p| !matches!(p, Prim::Circle { .. })).collect::<Vec<_>>());
    an.block(&mark_block);
    an.build(o.ann, &o.src);
    for w in an.warnings.drain(..) {
        warnings.push(format!("view '{}': {}", name, w));
    }
    geometry.extend(cut_prims);
    geometry.extend(mark_prims);
    let mut bbox = bbox_of(&geometry).union(&bbox_of(&an.dims)).union(&bbox_of(&an.gdt));
    if let Some(title) = &o.title {
        let h = cfg.text_height * 1.4;
        let p = v2(geom_bb.center().x, bbox.min.y - 4.0 - h) + o.title_offset;
        // a group, so the editor can pick and move the label
        geometry.push(Prim::Group {
            attrs: vec![
                ("class", "vtitle".into()),
                ("data-view", o.src.clone()),
                ("data-off", format!("{},{}", o.title_offset.x, o.title_offset.y)),
            ],
            pts: vec![],
            items: vec![Prim::Text { p, h, anchor: crate::svg::Anchor::Middle, rot: 0.0, s: title.clone(), cls: "title" }],
        });
        bbox = bbox_of(&geometry).union(&bbox_of(&an.dims)).union(&bbox_of(&an.gdt));
    }
    View {
        name: sanitize(name),
        geometry,
        gdt: std::mem::take(&mut an.gdt),
        dims: std::mem::take(&mut an.dims),
        pick,
        bbox,
        pos: V2::default(),
        place: Place::Base,
        fixed: false,
        frame,
        parent: None,
        anchor: None,
        dist: None,
        loc_ref: V2::default(),
        src: o.src.clone(),
        origin: pr.o,
        scale: o.scale,
        hidden: o.hidden,
    }
}

/// Put detail views (those with an anchor and not fixed) in the free space
/// nearest their detail circle.
pub fn place_details(views: &mut [View], details: &[usize], gap: f64, warnings: &mut Vec<String>) {
    let frame = sheet::frame_box().expand(-4.0);
    let title = sheet::title_box();
    for (k, &i) in details.iter().enumerate() {
        let others: Vec<usize> = (0..views.len()).filter(|j| *j != i && !details[k + 1..].contains(j)).collect();
        if !views[i].fixed {
            let Some((pi, local)) = views[i].anchor else { continue };
            let target = views[pi].pos + local;
            let bb = views[i].bbox;
            let mut best: Option<(f64, V2)> = None;
            for g in [gap / 2.0, gap / 4.0, 1.0] {
                let mut y = frame.min.y - bb.min.y;
                while y + bb.max.y <= frame.max.y {
                    let mut x = frame.min.x - bb.min.x;
                    while x + bb.max.x <= frame.max.x {
                        let pos = v2(x, y);
                        let wb = bb.translate(pos);
                        let clear = !wb.overlaps(&title.expand(g))
                            && others.iter().all(|&j| {
                                let ob = views[j].world_bbox().expand(g);
                                !ob.overlaps(&wb) || views[j].hits(&wb.expand(g)).is_empty()
                            });
                        if clear {
                            let dist = wb.center().dist(target);
                            if best.is_none_or(|b| dist < b.0) {
                                best = Some((dist, pos));
                            }
                        }
                        x += 2.0;
                    }
                    y += 2.0;
                }
                if best.is_some() {
                    break;
                }
            }
            match best {
                Some((_, p)) => views[i].pos = p,
                None => {
                    warnings.push(format!("no free space on the sheet for detail view '{}' (try a smaller detail scale)", views[i].name));
                    views[i].pos = target;
                }
            }
        }
        let b = views[i].world_bbox();
        if !views[i].hits(&title).is_empty() {
            warnings.push(format!("view '{}' overlaps the title block", views[i].name));
        }
        for &j in &others {
            if b.overlaps(&views[j].world_bbox()) && !views[j].hits(&b).is_empty() {
                warnings.push(format!("views '{}' and '{}' overlap", views[i].name, views[j].name));
            }
        }
    }
}

/// Arrange clusters (base view + its projected views) on the sheet.
pub fn layout(views: &mut [View], clusters: &[Vec<usize>], gap: f64, warnings: &mut Vec<String>) {
    // 1. children around their parent, pushed apart along their own axis only
    for cl in clusters {
        let mut push = vec![0.0; cl.len()];
        for _ in 0..200 {
            position_children(views, cl, &push, gap);
            let mut moved = false;
            for a in 0..cl.len() {
                for b in a + 1..cl.len() {
                    let (va, vb) = (&views[cl[a]], &views[cl[b]]);
                    if vb.parent == Some(cl[a]) && vb.place == Place::Base {
                        continue;
                    }
                    // a view placed by the JSON stays where it was put
                    if vb.dist.is_some() {
                        continue;
                    }
                    let (ba, bb) = (va.world_bbox().expand(gap / 2.0), vb.world_bbox().expand(gap / 2.0));
                    if ba.overlaps(&bb) {
                        // the later view moves outward along its projection axis
                        let horiz = matches!(vb.place, Place::Left | Place::Right | Place::FarRight);
                        let d = if horiz {
                            (ba.max.x - bb.min.x).min(bb.max.x - ba.min.x)
                        } else {
                            (ba.max.y - bb.min.y).min(bb.max.y - ba.min.y)
                        };
                        push[b] += d + 0.5;
                        moved = true;
                    }
                }
            }
            if !moved {
                break;
            }
        }
    }
    // 2. move whole clusters to clear each other and the title block
    let frame = sheet::frame_box().expand(-4.0);
    let title = sheet::title_box();
    let cbox = |views: &[View], cl: &Vec<usize>| cl.iter().fold(BBox2::empty(), |a, &i| a.union(&views[i].world_bbox()));
    let shift = |views: &mut [View], cl: &Vec<usize>, d: V2| {
        for &i in cl {
            views[i].pos = views[i].pos + d;
        }
    };
    for _ in 0..300 {
        let mut moved = false;
        for i in 0..clusters.len() {
            for j in i + 1..clusters.len() {
                let (a, b) = (cbox(views, &clusters[i]).expand(gap / 2.0), cbox(views, &clusters[j]).expand(gap / 2.0));
                if !a.overlaps(&b) {
                    continue;
                }
                let dx = (a.max.x - b.min.x).min(b.max.x - a.min.x);
                let dy = (a.max.y - b.min.y).min(b.max.y - a.min.y);
                let dir = if dx < dy {
                    v2(if b.center().x >= a.center().x { dx } else { -dx }, 0.0)
                } else {
                    v2(0.0, if b.center().y >= a.center().y { dy } else { -dy })
                };
                let (fi, fj) = (views[clusters[i][0]].fixed, views[clusters[j][0]].fixed);
                let (mi, mj) = match (fi, fj) {
                    (false, true) => (1.0, 0.0),
                    (true, false) => (0.0, 1.0),
                    (true, true) => continue,     // both placed by the JSON
                    _ => (0.5, 0.5),
                };
                shift(views, &clusters[i], -dir * (mi + 0.01));
                shift(views, &clusters[j], dir * (mj + 0.01));
                moved = true;
            }
        }
        for cl in clusters {
            // a base view placed by the JSON (and the views aligned with it) stays put
            if views[cl[0]].fixed {
                continue;
            }
            let tz = title.expand(gap / 4.0);
            let b = cl.iter().fold(BBox2::empty(), |a, &i| a.union(&views[i].hits(&tz)));
            if !b.is_empty() {
                let dx = b.max.x - tz.min.x;
                let dy = tz.max.y - b.min.y;
                let cb = cbox(views, cl);
                let fits = |d: V2| {
                    let m = cb.translate(d);
                    m.min.x >= frame.min.x - 1e-6 && m.max.y <= frame.max.y + 1e-6
                };
                let up = v2(0.0, dy + 0.01);
                let left = v2(-dx - 0.01, 0.0);
                let mut cands = [up, left];
                cands.sort_by(|a, b| a.len().total_cmp(&b.len()));
                let d = cands.iter().copied().find(|d| fits(*d)).unwrap_or(cands[0]);
                shift(views, cl, d);
                moved = true;
            }
            let b = cbox(views, cl);
            let mut d = V2::default();
            if b.width() <= frame.width() {
                if b.min.x < frame.min.x {
                    d.x = frame.min.x - b.min.x;
                } else if b.max.x > frame.max.x {
                    d.x = frame.max.x - b.max.x;
                }
            }
            if b.height() <= frame.height() {
                if b.min.y < frame.min.y {
                    d.y = frame.min.y - b.min.y;
                } else if b.max.y > frame.max.y {
                    d.y = frame.max.y - b.max.y;
                }
            }
            if d.len() > 1e-6 {
                shift(views, cl, d);
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
    // report problems
    let all: Vec<usize> = clusters.iter().flatten().copied().collect();
    for (k, &i) in all.iter().enumerate() {
        let b = views[i].world_bbox();
        if !(frame.expand(4.0).contains(b.min) && frame.expand(4.0).contains(b.max)) {
            warnings.push(format!("view '{}' does not fit inside the drawing frame (try a smaller scale)", views[i].name));
        }
        if !views[i].hits(&title).is_empty() {
            warnings.push(format!("view '{}' overlaps the title block", views[i].name));
        }
        for &j in &all[k + 1..] {
            if b.overlaps(&views[j].world_bbox()) {
                warnings.push(format!("views '{}' and '{}' overlap", views[i].name, views[j].name));
            }
        }
    }
}

fn position_children(views: &mut [View], cl: &[usize], push: &[f64], gap: f64) {
    // cluster order lists parents before their children
    for (pk, &pi) in cl.iter().enumerate() {
        let (bp, bb) = (views[pi].pos, views[pi].bbox);
        let mut right = bp.x + bb.max.x + gap;
        let mut left = bp.x + bb.min.x - gap;
        let mut up = bp.y + bb.max.y + gap;
        let mut down = bp.y + bb.min.y - gap;
        // FarRight goes after all Right views
        let mut order: Vec<usize> = (pk + 1..cl.len()).filter(|&k| views[cl[k]].parent == Some(pi)).collect();
        order.sort_by_key(|&k| (views[cl[k]].place == Place::FarRight) as u8);
        for k in order {
            let v = &mut views[cl[k]];
            let p = push[k];
            if let Some(d) = v.dist {
                // placed by the JSON: only the distance along the axis is given
                match v.place {
                    Place::Right | Place::FarRight => {
                        v.pos = v2(bp.x + d, bp.y);
                        right = right.max(v.pos.x + v.bbox.max.x + gap);
                    }
                    Place::Left => {
                        v.pos = v2(bp.x - d, bp.y);
                        left = left.min(v.pos.x + v.bbox.min.x - gap);
                    }
                    Place::Above => {
                        v.pos = v2(bp.x, bp.y + d);
                        up = up.max(v.pos.y + v.bbox.max.y + gap);
                    }
                    Place::Below => {
                        v.pos = v2(bp.x, bp.y - d);
                        down = down.min(v.pos.y + v.bbox.min.y - gap);
                    }
                    Place::Base => {}
                }
                continue;
            }
            match v.place {
                Place::Right | Place::FarRight => {
                    v.pos = v2(right + p - v.bbox.min.x, bp.y);
                    right = v.pos.x + v.bbox.max.x + gap;
                }
                Place::Left => {
                    v.pos = v2(left - p - v.bbox.max.x, bp.y);
                    left = v.pos.x + v.bbox.min.x - gap;
                }
                Place::Above => {
                    v.pos = v2(bp.x, up + p - v.bbox.min.y);
                    up = v.pos.y + v.bbox.max.y + gap;
                }
                Place::Below => {
                    v.pos = v2(bp.x, down - p - v.bbox.max.y);
                    down = v.pos.y + v.bbox.min.y - gap;
                }
                Place::Base => {}
            }
        }
    }
}
