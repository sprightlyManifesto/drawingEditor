//! stepdraw – headless generator of engineering drawings (SVG) from STEP files.
//!
//! Usage: stepdraw <drawing.json> [part.step] [-o out.svg] [-c config.json]

#![allow(dead_code)] // some parsed STEP data is kept for future use

mod annot;
mod autodraw;
mod prof;
mod brep;
mod config;
mod geom;
mod hlr;
mod section;
mod server;
mod selector;
mod sheet;
mod step;
mod svg;
mod view;

use config::*;
use geom::*;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use view::{Place, View};

fn usage() -> ! {
    eprintln!(
        "usage: stepdraw <drawing.json> [part.step] [-o output.svg] [-c config.json]\n\
         \x20      stepdraw serve [folder] [--port 8080] [-c config.json]\n\
         \x20      stepdraw auto <part.step> [--base] [-o drawing.json]\n\
         \n\
         auto: writes a first drawing JSON for a STEP file (views, overall\n\
         dimensions, hole diameters and positions) next to it; with --base\n\
         just a base view.\n\
         \n\
         serve: runs the drawing editor at http://127.0.0.1:<port>/ for the drawing\n\
         JSON files in the folder (default: the current directory).\n\
         \n\
         The STEP file may also be given as \"step_file\" in the drawing JSON.\n\
         config.json is looked up in the current directory, then next to the\n\
         executable, unless given with -c."
    );
    std::process::exit(2)
}

pub fn load_config(explicit: Option<&str>) -> Result<Config, String> {
    let candidates: Vec<PathBuf> = match explicit {
        Some(p) => vec![PathBuf::from(p)],
        None => {
            let mut v = vec![PathBuf::from("config.json")];
            if let Ok(exe) = std::env::current_exe() {
                if let Some(d) = exe.parent() {
                    v.push(d.join("config.json"));
                }
            }
            v
        }
    };
    let mut cfg = None;
    for p in &candidates {
        if p.exists() {
            let s = std::fs::read_to_string(p).map_err(|e| format!("{}: {}", p.display(), e))?;
            cfg = Some(serde_json::from_str::<Config>(&s).map_err(|e| format!("{}: {}", p.display(), e))?);
            break;
        }
    }
    if cfg.is_none() && explicit.is_some() {
        return Err(format!("config file {} not found", candidates[0].display()));
    }
    let mut cfg = cfg.unwrap_or_default();
    // named fits: named_fits.json beside the config file(s), else the built-in copy
    for p in &candidates {
        let f = p.parent().map(|d| if d.as_os_str().is_empty() { Path::new(".") } else { d }).unwrap_or(Path::new(".")).join("named_fits.json");
        if f.exists() {
            let s = std::fs::read_to_string(&f).map_err(|e| format!("{}: {}", f.display(), e))?;
            let fits: config::NamedFits = serde_json::from_str(&s).map_err(|e| format!("{}: {}", f.display(), e))?;
            cfg.fits = std::sync::Arc::new(fits);
            break;
        }
    }
    Ok(cfg)
}

/// The label of a view not at the sheet scale: "SCALE: 2:1".
fn scale_note(scale: f64) -> Option<String> {
    ((scale - sheet::get().scale).abs() > 1e-9).then(|| format!("SCALE: {}", scale_text(scale)))
}

fn scale_text(s: f64) -> String {
    let f = |v: f64| annot::fmt_num(v, 2, true);
    if s >= 1.0 { format!("{}:1", f(s)) } else { format!("1:{}", f(1.0 / s)) }
}

fn run() -> Result<Vec<String>, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut pos = Vec::new();
    let mut out: Option<String> = None;
    let mut cfg_path: Option<String> = None;
    let mut port: u16 = 8080;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-o" | "--output" => out = Some(it.next().cloned().unwrap_or_else(|| usage())),
            "-p" | "--port" => port = it.next().and_then(|p| p.parse().ok()).unwrap_or_else(|| usage()),
            "-c" | "--config" => cfg_path = Some(it.next().cloned().unwrap_or_else(|| usage())),
            "-h" | "--help" => usage(),
            _ => pos.push(a.clone()),
        }
    }
    if pos.first().map(String::as_str) == Some("serve") {
        if pos.len() > 2 {
            usage();
        }
        let dir = PathBuf::from(pos.get(1).map(String::as_str).unwrap_or("."));
        server::serve(&dir, port, cfg_path.as_deref())?;
        return Ok(vec![]);
    }
    if pos.first().map(String::as_str) == Some("auto") {
        // a first drawing JSON for a STEP file (--base: just a base view)
        let base_only = pos.iter().any(|a| a == "--base");
        let rest: Vec<&String> = pos.iter().skip(1).filter(|a| *a != "--base").collect();
        let Some(step) = rest.first().filter(|_| rest.len() == 1) else { usage() };
        let step = Path::new(step.as_str());
        let model = load_model(step)?;
        let (path, notes) = autodraw::write_for(&model, step, out.as_deref().map(Path::new), base_only)?;
        eprintln!("wrote {}", path.display());
        return Ok(notes);
    }
    if pos.is_empty() || pos.len() > 2 {
        usage();
    }
    let cfg = load_config(cfg_path.as_deref())?;
    let spec_path = Path::new(&pos[0]);
    let spec_txt = std::fs::read_to_string(spec_path).map_err(|e| format!("{}: {}", spec_path.display(), e))?;
    let spec: DrawingSpec = serde_json::from_str(&spec_txt).map_err(|e| format!("{}: {}", spec_path.display(), e))?;
    let step_path = match (pos.get(1), &spec.step_file) {
        (Some(p), _) => PathBuf::from(p),
        (None, Some(p)) => spec_path.parent().unwrap_or(Path::new(".")).join(p),
        (None, None) => return Err("no STEP file given (argument or \"step_file\" in the JSON)".into()),
    };
    let out_path = out.map(PathBuf::from).unwrap_or_else(|| spec_path.with_extension("svg"));
    let (svg, warnings) = render(&spec, &step_path, &cfg)?;
    std::fs::write(&out_path, svg).map_err(|e| format!("{}: {}", out_path.display(), e))?;
    eprintln!("wrote {}", out_path.display());
    Ok(warnings)
}

/// Projected views of the view `parent` (named `pname`), and recursively the
/// views projected from them, each aligned with its own parent.
#[allow(clippy::too_many_arguments)]
fn add_projected(
    model: &brep::Model,
    cfg: &Config,
    list: &[config::ProjectedSpec],
    pname: &str,
    parent: &Parent,
    proj: config::Projection,
    views: &mut Vec<View>,
    cl: &mut Vec<usize>,
    details: &mut Vec<usize>,
    warnings: &mut Vec<String>,
) -> Result<(), String> {
    for (pj, p) in list.iter().enumerate() {
        let psrc = format!("{}/projected/{}", parent.src, pj);
        let (pf, place) = view::projected_frame(parent.frame, &p.position, proj).map_err(|e| format!("view '{}': {}", pname, e))?;
        let name = p.name.clone().unwrap_or_else(|| format!("{}_{}", pname, p.position));
        let hidden = p.hidden_lines.unwrap_or(parent.hidden);
        let psecs = plan_sections(model, pf, &p.sections, proj).map_err(|e| format!("view '{}': {}", name, e))?;
        let pdets = plan_details(model, &p.details).map_err(|e| format!("view '{}': {}", name, e))?;
        let opts = view::ViewOpts {
            name: name.clone(),
            frame: pf,
            scale: parent.scale,
            hidden,
            ann: &p.ann,
            src: psrc.clone(),
            cuts: cuts_of(&psecs, &psrc),
            section: None,
            title: scale_note(parent.scale),
            title_offset: p.title_offset.map_or(V2::default(), |t| v2(t[0], t[1])),
            marks: marks_of(model, &pdets, &psrc),
            detail: None,
        };
        let mut v = view::build_view(model, cfg, opts, warnings);
        v.place = place;
        v.parent = Some(parent.index);
        v.dist = p.distance;
        let vi = views.len();
        cl.push(vi);
        views.push(v);
        let ctx = Parent { index: vi, frame: pf, scale: parent.scale, hidden, section: None, src: psrc };
        add_details(model, cfg, &pdets, &ctx, views, details, warnings);
        add_sections(model, cfg, &psecs, parent.scale, vi, &ctx.src, proj, views, cl, details, warnings);
        add_projected(model, cfg, &p.projected, &name, &ctx, proj, views, cl, details, warnings)?;
    }
    Ok(())
}

/// Read a STEP file into a B-rep model.
pub fn load_model(step_path: &Path) -> Result<brep::Model, String> {
    let step_txt = std::fs::read(step_path).map_err(|e| format!("{}: {}", step_path.display(), e))?;
    let sf = step::parse(&String::from_utf8_lossy(&step_txt))?;
    let mut m = brep::build(&sf)?;
    m.key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        step_txt.hash(&mut h);
        h.finish()
    };
    Ok(m)
}

/// Render a drawing: the SVG text and the warnings.
pub fn render(spec: &DrawingSpec, step_path: &Path, cfg: &Config) -> Result<(String, Vec<String>), String> {
    let mut cfg = cfg.clone();
    // the drawing's own line widths
    if let Some(w) = &spec.line_weights {
        let ok = |v: Option<f64>| v.filter(|x| *x > 0.0 && x.is_finite());
        cfg.line_visible = ok(w.visible).unwrap_or(cfg.line_visible);
        cfg.line_hidden = ok(w.hidden).unwrap_or(cfg.line_hidden);
        cfg.line_thin = ok(w.thin).unwrap_or(cfg.line_thin);
        cfg.line_border = ok(w.border).unwrap_or(cfg.line_border);
    }
    let cfg = cfg;
    let model = load_model(step_path)?;
    eprintln!(
        "{}: {} faces, {} edges, {} vertices",
        step_path.display(),
        model.faces.len(),
        model.edges.len(),
        model.verts.len()
    );

    let mut sh = sheet::Sheet::of(spec.sheet.as_ref())?;
    sh.scale = spec.views.first().map_or(1.0, |v| v.scale);
    sheet::set(sh);
    let proj = spec.projection.unwrap_or(cfg.projection);
    let mut warnings = Vec::new();
    let mut views: Vec<View> = Vec::new();
    let mut clusters: Vec<Vec<usize>> = Vec::new();
    let mut details: Vec<usize> = Vec::new();
    if spec.views.is_empty() {
        return Err("the drawing has no views".into());
    }
    let frame = sheet::frame_box();
    for (ci, vs) in spec.views.iter().enumerate() {
        let fr = view::base_frame(vs.direction, vs.up).map_err(|e| format!("view '{}': {}", vs.name, e))?;
        let hidden = vs.hidden_lines.unwrap_or(cfg.hidden_lines);
        let secs = plan_sections(&model, fr, &vs.sections, proj).map_err(|e| format!("view '{}': {}", vs.name, e))?;
        let dets = plan_details(&model, &vs.details).map_err(|e| format!("view '{}': {}", vs.name, e))?;
        let opts = view::ViewOpts {
            name: vs.name.clone(),
            frame: fr,
            scale: vs.scale,
            hidden,
            ann: &vs.ann,
            src: format!("/views/{}", ci),
            cuts: cuts_of(&secs, &format!("/views/{}", ci)),
            section: None,
            title: scale_note(vs.scale),
            title_offset: vs.title_offset.map_or(V2::default(), |t| v2(t[0], t[1])),
            marks: marks_of(&model, &dets, &format!("/views/{}", ci)),
            detail: None,
        };
        let mut base = view::build_view(&model, &cfg, opts, &mut warnings);
        match &vs.location {
            Some(l) if l.len() >= 2 => {
                base.pos = v2(l[0], l[1]);
                base.fixed = true;
            }
            _ => {
                base.pos = v2(frame.min.x + frame.width() * (0.35 + 0.3 * ci as f64), frame.min.y + frame.height() * 0.55);
            }
        }
        let base_i = views.len();
        let mut cl = vec![base_i];
        views.push(base);
        let ctx = Parent { index: base_i, frame: fr, scale: vs.scale, hidden, section: None, src: format!("/views/{}", ci) };
        add_details(&model, &cfg, &dets, &ctx, &mut views, &mut details, &mut warnings);
        add_sections(&model, &cfg, &secs, vs.scale, base_i, &ctx.src, proj, &mut views, &mut cl, &mut details, &mut warnings);
        let ctx = Parent { index: base_i, frame: fr, scale: vs.scale, hidden, section: None, src: format!("/views/{}", ci) };
        add_projected(&model, &cfg, &vs.projected, &vs.name, &ctx, proj, &mut views, &mut cl, &mut details, &mut warnings)?;
        clusters.push(cl);
    }
    // unique group ids
    let mut names: Vec<String> = Vec::new();
    for v in views.iter_mut() {
        let mut n = v.name.clone();
        let mut k = 2;
        while names.contains(&n) {
            n = format!("{}_{}", v.name, k);
            k += 1;
        }
        v.name = n.clone();
        names.push(n);
    }
    view::layout(&mut views, &clusters, cfg.view_gap, &mut warnings);
    view::place_details(&mut views, &details, cfg.view_gap, &mut warnings);

    // ------------------------------------------------------------ SVG
    let mut s = String::new();
    let _ = writeln!(s, r#"<?xml version="1.0" encoding="UTF-8" standalone="no"?>"#);
    let _ = writeln!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:inkscape="http://www.inkscape.org/namespaces/inkscape" width="{}mm" height="{}mm" viewBox="0 0 {} {}">"#,
        sheet::W(),
        sheet::H(),
        sheet::W(),
        sheet::H()
    );
    let _ = writeln!(s, "<title>{}</title>", xml(&format!("{} {}", spec.title.drawing_number, spec.title.description).trim().to_string()));
    let _ = writeln!(s, "{}", svg::style(&cfg));
    let _ = writeln!(s, r#"<rect x="0" y="0" width="{}" height="{}" style="fill:#fff;stroke:none"/>"#, sheet::W(), sheet::H());
    let _ = writeln!(s, r#"<g id="sheet" inkscape:label="sheet" transform="translate(0 {})">"#, sheet::H());
    if sheet::get().border {
        let _ = writeln!(s, r#"  <g id="border" inkscape:label="border">"#);
        svg::emit(&mut s, &sheet::border(&cfg), "    ");
        let _ = writeln!(s, "  </g>");
        let _ = writeln!(s, r#"  <g id="title_block" inkscape:label="title_block">"#);
        let base_scale = spec.views[0].scale;
        svg::emit(&mut s, &sheet::title_block(&cfg, &spec.title, &scale_text(base_scale), proj), "    ");
        let _ = writeln!(s, "  </g>");
    } else if cfg.pick_layer {
        // no border or title block: for the editor, where the title block would be (click to edit it)
        let _ = writeln!(s, r#"  <g id="title_block" class="tbhit">"#);
        svg::emit(&mut s, &[svg::Prim::Rect { b: sheet::title_box_drawn(), cls: "tbhit" }], "    ");
        let _ = writeln!(s, "  </g>");
    }
    let _ = writeln!(s, "</g>");
    for v in &views {
        let kind = if v.anchor.is_some() {
            "detail"
        } else if v.place == Place::Base {
            "base"
        } else {
            "projected"
        };
        // for the editor: an aligned view slides only along its projection axis
        // and follows its parent across it
        let align = match (v.parent, v.place) {
            (Some(p), Place::Left | Place::Right | Place::FarRight) => format!(r#" data-parent="{}" data-axis="x""#, views[p].name),
            (Some(p), Place::Above | Place::Below) => format!(r#" data-parent="{}" data-axis="y""#, views[p].name),
            _ => String::new(),
        };
        // for the editor to record a move in the JSON: the distance from the
        // parent along the axis (and which sheet direction, y down, makes it
        // grow), or the sheet position the view's `location` refers to
        let place_attrs = match (v.parent, v.place) {
            (Some(p), Place::Right | Place::FarRight | Place::Left | Place::Above | Place::Below) => {
                let (pp, q) = (views[p].pos, v.pos);
                let (d, dir) = match v.place {
                    Place::Left => (pp.x - q.x, "-1,0"),
                    Place::Above => (q.y - pp.y, "0,-1"),
                    Place::Below => (pp.y - q.y, "0,1"),
                    _ => (q.x - pp.x, "1,0"),
                };
                format!(r#" data-dist="{:.3}" data-dir="{}""#, d, dir)
            }
            _ => {
                let l = v.pos + v.loc_ref;
                format!(r#" data-loc="{:.3},{:.3}""#, l.x, l.y)
            }
        };
        let _ = writeln!(
            s,
            r#"<g id="{n}" class="view {k}" inkscape:label="{n}" data-src="{src}" data-model="{md}" data-hidden="{hd}"{al}{pa} transform="translate({x:.3} {y:.3})">"#,
            n = v.name,
            k = kind,
            src = v.src,
            md = {
                let (r, u, _) = v.frame;
                let o = v.origin;
                format!("{},{},{};{},{},{};{},{},{};{}", r.x, r.y, r.z, u.x, u.y, u.z, o.x, o.y, o.z, v.scale)
            },
            hd = v.hidden as u8,
            al = align,
            pa = place_attrs,
            x = v.pos.x,
            y = sheet::H() - v.pos.y
        );
        for (gname, prims) in [("geometry", &v.geometry), ("GDandT", &v.gdt), ("dimensions", &v.dims)] {
            let _ = writeln!(s, r#"  <g id="{}-{}" class="{}" inkscape:label="{}">"#, v.name, gname, gname, gname);
            svg::emit(&mut s, prims, "    ");
            let _ = writeln!(s, "  </g>");
        }
        if !v.pick.is_empty() {
            let _ = writeln!(s, r#"  <g id="{}-pick" class="pick">"#, v.name);
            svg::emit(&mut s, &v.pick, "    ");
            let _ = writeln!(s, "  </g>");
        }
        let _ = writeln!(s, "</g>");
    }
    s.push_str("</svg>\n");
    crate::prof::report();
    Ok((s, warnings))
}

/// A section to be cut from a parent view: frame, placement and plane point.
struct PlannedSection<'a> {
    spec: &'a SectionSpec,
    frame: (V3, V3, V3),
    place: Place,
    c: V3,
    /// index in the parent's "sections"
    idx: usize,
}

fn plan_sections<'a>(
    m: &brep::Model,
    parent: (V3, V3, V3),
    secs: &'a [SectionSpec],
    proj: Projection,
) -> Result<Vec<PlannedSection<'a>>, String> {
    let mut out = Vec::new();
    for (idx, s) in secs.iter().enumerate() {
        let (frame, place) = view::projected_frame(parent, &s.position, proj)?;
        let c = match (&s.through, s.point) {
            (_, Some(p)) => v3(p[0], p[1], p[2]),
            (Some(sel), None) => {
                let r = annot::resolve(m, sel).map_err(|e| format!("section {}: {}", s.label, e))?;
                r.iter().fold(V3::default(), |a, g| a + g.rep) / r.len() as f64
            }
            // through the centre of the part
            (None, None) => m.bbox.center(),
        };
        out.push(PlannedSection { spec: s, frame, place, c, idx });
    }
    Ok(out)
}

fn cuts_of(secs: &[PlannedSection], owner: &str) -> Vec<view::Cut> {
    secs.iter().map(|s| view::Cut { label: s.spec.label.clone(), c: s.c, d: s.frame.2, src: format!("{}/sections/{}", owner, s.idx) }).collect()
}

#[allow(clippy::too_many_arguments)]
fn add_sections(
    m: &brep::Model,
    cfg: &Config,
    secs: &[PlannedSection],
    scale: f64,
    parent: usize,
    owner: &str,
    proj: config::Projection,
    views: &mut Vec<View>,
    cl: &mut Vec<usize>,
    details: &mut Vec<usize>,
    warnings: &mut Vec<String>,
) {
    for s in secs {
        let sp = s.spec;
        let src = format!("{}/sections/{}", owner, s.idx);
        let dets = match plan_details(m, &sp.details) {
            Ok(d) => d,
            Err(e) => {
                warnings.push(format!("section {}: {}", sp.label, e));
                vec![]
            }
        };
        let clip = hlr::Clip { c: s.c, d: s.frame.2 };
        let hatch = (sp.hatch_angle.unwrap_or(cfg.hatch_angle), sp.hatch_spacing.unwrap_or(cfg.hatch_spacing));
        let hidden = sp.hidden_lines.unwrap_or(false);
        let opts = view::ViewOpts {
            name: sp.name.clone().unwrap_or_else(|| format!("section_{}", sp.label)),
            frame: s.frame,
            scale,
            hidden,
            ann: &sp.ann,
            src: src.clone(),
            cuts: vec![],
            section: Some((clip, hatch.0, hatch.1)),
            // a second line with the scale when not at the sheet scale
            title: Some(format!("SECTION {}-{}", sp.label, sp.label) + &scale_note(scale).map(|n| format!("\n{}", n)).unwrap_or_default()),
            title_offset: sp.title_offset.map_or(V2::default(), |t| v2(t[0], t[1])),
            marks: marks_of(m, &dets, &src),
            detail: None,
        };
        let mut v = view::build_view(m, cfg, opts, warnings);
        v.place = s.place;
        v.parent = Some(parent);
        v.dist = sp.distance;
        let vi = views.len();
        cl.push(vi);
        views.push(v);
        let ctx = Parent { index: vi, frame: s.frame, scale, hidden, section: Some((clip, hatch.0, hatch.1)), src };
        add_details(m, cfg, &dets, &ctx, views, details, warnings);
        // views projected from the section are whole (not cut)
        let pctx = Parent { section: None, hidden: cfg.hidden_lines, ..ctx };
        let name = sp.name.clone().unwrap_or_else(|| format!("section_{}", sp.label));
        if let Err(e) = add_projected(m, cfg, &sp.projected, &name, &pctx, proj, views, cl, details, warnings) {
            warnings.push(e);
        }
    }
}

/// The view a detail is taken from.
struct Parent {
    index: usize,
    frame: (V3, V3, V3),
    scale: f64,
    hidden: bool,
    section: Option<(hlr::Clip, f64, f64)>,
    /// JSON pointer of the view's spec
    src: String,
}

/// Detail specs with their resolved centre.
fn plan_details<'a>(m: &brep::Model, dets: &'a [DetailSpec]) -> Result<Vec<(&'a DetailSpec, V3, usize)>, String> {
    let mut out = Vec::new();
    for (i, d) in dets.iter().enumerate() {
        let c = match (&d.center, d.point) {
            (_, Some(p)) => v3(p[0], p[1], p[2]),
            (Some(sel), None) => {
                let r = annot::resolve(m, sel).map_err(|e| format!("detail {}: {}", d.label, e))?;
                r.iter().fold(V3::default(), |a, g| a + g.rep) / r.len() as f64
            }
            (None, None) => m.bbox.center(),
        };
        out.push((d, c, i));
    }
    Ok(out)
}

fn marks_of(m: &brep::Model, dets: &[(&DetailSpec, V3, usize)], owner: &str) -> Vec<view::DetailMark> {
    dets.iter()
        .map(|(d, c, i)| view::DetailMark { label: d.label.clone(), c: *c, shape: det_shape(m, d), src: format!("{}/details/{}", owner, i) })
        .collect()
}

/// Boundary of a detail in model units.
fn det_shape(m: &brep::Model, d: &DetailSpec) -> view::DetShape {
    let r = if d.radius > 0.0 { d.radius } else { (m.diag() * 0.12).max(1.0) };
    if d.shape.as_deref().is_some_and(|s| s.eq_ignore_ascii_case("ellipse")) {
        let r2 = d.radius2.filter(|x| *x > 0.0).unwrap_or(r * 0.6);
        view::DetShape { r, r2, ang: d.angle.unwrap_or(0.0) }
    } else {
        view::DetShape { r, r2: r, ang: 0.0 }
    }
}

fn add_details(
    m: &brep::Model,
    cfg: &Config,
    dets: &[(&DetailSpec, V3, usize)],
    p: &Parent,
    views: &mut Vec<View>,
    details: &mut Vec<usize>,
    warnings: &mut Vec<String>,
) {
    for (d, c, di) in dets {
        let scale = d.scale.unwrap_or(p.scale * 2.0);
        let opts = view::ViewOpts {
            name: d.name.clone().unwrap_or_else(|| format!("detail_{}", d.label)),
            frame: p.frame,
            scale,
            hidden: d.hidden_lines.unwrap_or(p.hidden),
            ann: &d.ann,
            src: format!("{}/details/{}", p.src, di),
            cuts: vec![],
            section: p.section,
            title: Some(format!("DETAIL {} ({})", d.label, scale_text(scale))),
            title_offset: d.title_offset.map_or(V2::default(), |t| v2(t[0], t[1])),
            marks: vec![],
            detail: Some((*c, det_shape(m, d), d.border.unwrap_or(true))),
        };
        let mut v = view::build_view(m, cfg, opts, warnings);
        let pr = hlr::Projector { o: m.bbox.center(), r: p.frame.0, u: p.frame.1, d: p.frame.2, scale: p.scale };
        v.anchor = Some((p.index, pr.p(*c)));
        v.loc_ref = hlr::Projector { scale, ..pr }.p(*c);
        match &d.location {
            Some(l) if l.len() >= 2 => {
                // location is the detail centre on the sheet
                let pr2 = hlr::Projector { scale, ..pr };
                v.pos = v2(l[0], l[1]) - pr2.p(*c);
                v.fixed = true;
            }
            _ => {}
        }
        details.push(views.len());
        views.push(v);
    }
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn main() {
    match run() {
        Ok(w) => {
            for x in &w {
                eprintln!("warning: {}", x);
            }
        }
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
    }
}
