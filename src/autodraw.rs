//! A first drawing for a STEP file: a base view looking at the side with the
//! most holes, a projected view for every other side with holes (and one to
//! show the depth), overall dimensions, hole diameters and positions, and the
//! diameters of shafts and bosses.

use crate::annot::{Cyl, cyl_end_open, cylinders, fmt_num, resolve};
use crate::brep::Model;
use crate::config::Projection;
use crate::geom::*;
use crate::selector::{Kind, find_selector};
use crate::view::{base_frame, projected_frame};
use serde_json::{Value, json};

/// The six sides of the part, in the order they are preferred when a through
/// hole could be shown from either end: top, front, right, bottom, back, left.
const SIDES: [(f64, f64, f64); 6] = [(0.0, 0.0, 1.0), (0.0, -1.0, 0.0), (1.0, 0.0, 0.0), (0.0, 0.0, -1.0), (0.0, 1.0, 0.0), (-1.0, 0.0, 0.0)];

fn side_vec(i: usize) -> V3 {
    let (x, y, z) = SIDES[i];
    v3(x, y, z)
}

/// The side a direction points to, if it is along an axis.
fn side_of(v: V3) -> Option<usize> {
    (0..6).find(|&i| side_vec(i).dot(v.norm()) > 0.9999)
}

/// `>X`, `<Z`, ... for an axis direction.
fn axis_op(v: V3) -> String {
    let (l, c) = if v.x.abs() > 0.5 { ("X", v.x) } else if v.y.abs() > 0.5 { ("Y", v.y) } else { ("Z", v.z) };
    format!("{}{}", if c > 0.0 { ">" } else { "<" }, l)
}

struct Hole {
    cyl: Cyl,
    /// sides it opens onto
    sides: Vec<usize>,
    through: bool,
    /// side it is shown from
    side: usize,
}

/// One view of the plan: where it is (None: the base view), its frame and its dimensions.
struct ViewPlan {
    pos: Option<&'static str>,
    side: usize,
    r: V3,
    u: V3,
    d: V3,
    dims: Vec<Value>,
}

fn coverage(m: &Model, c: &Cyl) -> f64 {
    c.faces.iter().map(|&f| m.faces[f].angular_coverage()).sum()
}

/// The drawing JSON for the model `m` of `step_file`, and notes on what was
/// left out. `base_only`: just the base view, with nothing on it.
pub fn generate(m: &Model, step_file: &str, stem: &str, base_only: bool) -> (Value, Vec<String>) {
    let mut notes = Vec::new();
    let diag = m.diag();
    let tol = diag * 1e-4 + 1e-6;
    let cyls = cylinders(m);
    // ---- holes: full internal cylinders along an axis, open at one end at least
    let mut holes: Vec<Hole> = Vec::new();
    let mut skipped = 0;
    for c in cyls.iter().filter(|c| c.internal && coverage(m, c) > 0.95) {
        if side_of(c.z).is_none() && side_of(-c.z).is_none() {
            skipped += 1;
            continue;
        }
        let mut sides = Vec::new();
        if cyl_end_open(m, c, true) {
            sides.extend(side_of(c.z));
        }
        if cyl_end_open(m, c, false) {
            sides.extend(side_of(-c.z));
        }
        if sides.is_empty() {
            continue;
        }
        let through = sides.len() == 2;
        holes.push(Hole { cyl: c.clone(), sides, through, side: 0 });
    }
    if skipped > 0 {
        notes.push(format!("{} hole(s) not along X, Y or Z are not dimensioned", skipped));
    }
    // sides by how many holes open onto them, then by preference
    let mut order: Vec<usize> = (0..6).collect();
    let count = |s: usize| holes.iter().filter(|h| h.sides.contains(&s)).count();
    order.sort_by_key(|&s| (std::cmp::Reverse(count(s)), s));
    for h in &mut holes {
        h.side = *order.iter().find(|s| h.sides.contains(s)).unwrap();
    }
    // ---- views
    let base_side = holes.first().map(|_| order[0]).unwrap_or(1);
    let dir = side_vec(base_side);
    let up = if dir.z > 0.5 {
        v3(0.0, 1.0, 0.0)
    } else if dir.z < -0.5 {
        v3(0.0, -1.0, 0.0)
    } else {
        v3(0.0, 0.0, 1.0)
    };
    let (r, u, d) = base_frame([dir.x, dir.y, dir.z], Some([up.x, up.y, up.z])).expect("axis frame");
    let mut views = vec![ViewPlan { pos: None, side: base_side, r, u, d, dims: vec![] }];
    let mut sides_used: Vec<usize> = holes.iter().map(|h| h.side).filter(|&s| s != base_side).collect();
    sides_used.sort();
    sides_used.dedup();
    let place = |s: usize| -> &'static str {
        let v = side_vec(s);
        if v.dot(r) > 0.5 {
            "right"
        } else if v.dot(r) < -0.5 {
            "left"
        } else if v.dot(u) > 0.5 {
            "top"
        } else if v.dot(u) < -0.5 {
            "bottom"
        } else {
            "back"
        }
    };
    let mut positions: Vec<&'static str> = sides_used.iter().map(|&s| place(s)).collect();
    // a view across the base view's direction shows the depth
    if !positions.iter().any(|p| *p != "back") {
        positions.push("right");
    }
    // in a fixed order: right, top, left, bottom, back
    positions.sort_by_key(|p| ["right", "top", "left", "bottom", "back"].iter().position(|q| q == p));
    for p in positions {
        let ((pr, pu, pd), _) = projected_frame((r, u, d), p, Projection::Third).expect("position");
        views.push(ViewPlan { pos: Some(p), side: side_of(pd).unwrap_or(0), r: pr, u: pu, d: pd, dims: vec![] });
    }
    // ---- helpers
    let bb = m.bbox;
    let corners: Vec<V3> = (0..8)
        .map(|i| v3(if i & 1 == 0 { bb.min.x } else { bb.max.x }, if i & 2 == 0 { bb.min.y } else { bb.max.y }, if i & 4 == 0 { bb.min.z } else { bb.max.z }))
        .collect();
    let extreme = |v: V3| corners.iter().map(|p| p.dot(v)).fold(f64::MIN, f64::max);
    // a selector whose measuring point is at the part's extreme along v
    let ext_sel = |v: V3| -> Option<String> {
        let op = axis_op(v);
        let e = extreme(v);
        [format!("faces('%PLANE').faces('{}')", op), format!("faces('{}')", op), format!("edges('{}')", op), format!("vertices('{}')", op)]
            .into_iter()
            .find(|s| {
                resolve(m, s).is_ok_and(|refs| {
                    !refs.is_empty() && {
                        let c = refs.iter().fold(V3::default(), |a, g| a + g.rep) / refs.len() as f64;
                        (c.dot(v) - e).abs() < tol
                    }
                })
            })
    };
    let face_sel = |f: usize| find_selector(m, Kind::Face, &[f]);
    // external round features: shafts and bosses
    let shafts: Vec<&Cyl> = cyls.iter().filter(|c| !c.internal && coverage(m, c) > 0.95).collect();
    let mut dimensioned: Vec<usize> = Vec::new(); // first face of each cylinder given a diameter
    // a view where a cylinder shows side-on (preferred) or end-on
    let view_for = |views: &[ViewPlan], c: &Cyl| -> usize {
        views.iter().position(|v| v.d.dot(c.z).abs() < 1e-6).or_else(|| views.iter().position(|v| v.d.dot(c.z).abs() > 0.9999)).unwrap_or(0)
    };
    // ---- overall dimensions: width and height in the base view, depth in the first side view
    let mut overall = |views: &mut Vec<ViewPlan>, vi: usize, axis: V3, horizontal: bool| {
        match (ext_sel(-axis), ext_sel(axis)) {
            (Some(a), Some(b)) => {
                let (dir, side) = if horizontal { ("horizontal", "below") } else { ("vertical", "left") };
                views[vi].dims.push(json!({ "type": "linear", "from": a, "to": b, "direction": dir, "side": side }));
            }
            _ => {
                // a round outline: the diameter of the cylinder reaching both ends
                let c = shafts.iter().find(|c| c.z.dot(axis).abs() < 1e-6 && (c.o.dot(axis) + c.r - extreme(axis)).abs() < tol * 10.0 && (-c.o.dot(axis) + c.r - extreme(-axis)).abs() < tol * 10.0);
                match c {
                    Some(c) if !dimensioned.contains(&c.faces[0]) => {
                        dimensioned.push(c.faces[0]);
                        let wv = if views[vi].d.dot(c.z).abs() < 1e-6 || views[vi].d.dot(c.z).abs() > 0.9999 { vi } else { view_for(views, c) };
                        views[wv].dims.push(json!({ "type": "diameter", "of": face_sel(c.faces[0]) }));
                    }
                    Some(_) => {}
                    None => notes.push(format!("no feature found at the part's extremes along {}", axis_op(axis).trim_start_matches(['<', '>']))),
                }
            }
        }
    };
    overall(&mut views, 0, r, true);
    overall(&mut views, 0, u, false);
    if let Some(vi) = views.iter().position(|v| matches!(v.pos, Some("right" | "left" | "top" | "bottom"))) {
        let horiz = matches!(views[vi].pos, Some("right" | "left"));
        overall(&mut views, vi, d, horiz);
    }
    // ---- holes: a diameter per set of alike holes and their positions, in the view they open towards
    for vi in 0..views.len() {
        let side = views[vi].side;
        let (vr, vu, vd) = (views[vi].r, views[vi].u, views[vi].d);
        let mine: Vec<&Hole> = holes.iter().filter(|h| h.side == side).collect();
        if mine.is_empty() {
            continue;
        }
        // diameters
        let mut groups: Vec<(&Hole, usize)> = Vec::new();
        for h in &mine {
            let alike = |g: &Hole| (g.cyl.r - h.cyl.r).abs() < 1e-3 && g.through == h.through && ((g.cyl.t1 - g.cyl.t0) - (h.cyl.t1 - h.cyl.t0)).abs() < 1e-2;
            match groups.iter_mut().find(|(g, _)| alike(g)) {
                Some(g) => g.1 += 1,
                None => groups.push((h, 1)),
            }
        }
        for (h, n) in groups {
            let mut text = if n > 1 { format!("{}X Ø<>", n) } else { "Ø<>".to_string() };
            text += &if h.through { " THRU".to_string() } else { format!(" x {} DEEP", fmt_num(h.cyl.t1 - h.cyl.t0, 2, true)) };
            dimensioned.push(h.cyl.faces[0]);
            views[vi].dims.push(json!({ "type": "diameter", "of": face_sel(h.cyl.faces[0]), "text": text }));
        }
        // positions: from the axis of a round part seen end-on, else from its left and bottom
        let round = shafts
            .iter()
            .filter(|c| c.z.dot(vd).abs() > 0.9999)
            .max_by(|a, b| a.r.total_cmp(&b.r))
            .filter(|c| 2.0 * c.r > 0.9 * (extreme(vr) + extreme(-vr)).min(extreme(vu) + extreme(-vu)));
        let (ox, oy, centre) = match round {
            Some(c) => {
                let s = face_sel(c.faces[0]);
                (Some(s.clone()), Some(s), Some(c.o))
            }
            None => {
                // a round outline seen side-on: measure across it from its axis
                let across = |axis: V3| {
                    shafts.iter().filter(|c| c.z.dot(vd).abs() < 1e-6 && c.z.dot(axis).abs() < 1e-6).max_by(|a, b| a.r.total_cmp(&b.r)).map(|c| face_sel(c.faces[0]))
                };
                (ext_sel(-vr).or_else(|| across(vr)), ext_sel(-vu).or_else(|| across(vu)), None)
            }
        };
        // holes on the origin's axis need no position
        let placed: Vec<&&Hole> = mine
            .iter()
            .filter(|h| {
                centre.is_none_or(|o| {
                    let w = h.cyl.o - o;
                    (w - vd * w.dot(vd)).len() > tol
                })
            })
            .collect();
        for (origin, axis, dir, side) in [(ox, vr, "x", "below"), (oy, vu, "y", "left")] {
            let Some(origin) = origin else {
                notes.push(format!("hole positions along the view's {} are left out (no edge to measure from)", dir));
                continue;
            };
            // one target per distinct position
            let mut seen: Vec<f64> = Vec::new();
            let mut targets = Vec::new();
            for h in &placed {
                let x = h.cyl.o.dot(axis);
                if seen.iter().any(|s| (s - x).abs() < 1e-3) {
                    continue;
                }
                seen.push(x);
                targets.push(face_sel(h.cyl.faces[0]));
            }
            if !targets.is_empty() {
                views[vi].dims.push(json!({ "type": "ordinate", "origin": origin, "targets": targets, "direction": dir, "side": side }));
            }
        }
    }
    // ---- shaft and boss diameters
    let mut done_r: Vec<(f64, V3)> = Vec::new();
    for c in &shafts {
        if dimensioned.contains(&c.faces[0]) || done_r.iter().any(|(r0, z)| (r0 - c.r).abs() < 1e-3 && z.cross(c.z).len() < 1e-6) {
            continue;
        }
        done_r.push((c.r, c.z));
        let vi = view_for(&views, c);
        views[vi].dims.push(json!({ "type": "diameter", "of": face_sel(c.faces[0]) }));
    }
    if base_only {
        views.truncate(1);
        views[0].dims.clear();
        notes.clear();
    }
    // ---- scale to fit an A3 sheet
    let ext = |v: V3| extreme(v) + extreme(-v);
    let has = |p: &str| views.iter().any(|v| v.pos == Some(p));
    let horiz = ext(r) + if has("left") { ext(d) } else { 0.0 } + if has("right") { ext(d) } else { 0.0 } + if has("back") { ext(r) } else { 0.0 };
    let vert = ext(u) + if has("top") { ext(d) } else { 0.0 } + if has("bottom") { ext(d) } else { 0.0 };
    let fit = (280.0 / horiz.max(1e-6)).min(170.0 / vert.max(1e-6));
    let scale = [10.0, 5.0, 4.0, 3.0, 2.5, 2.0, 1.5, 1.0, 0.75, 0.5, 0.4, 0.25, 0.2, 0.1, 0.05, 0.02].into_iter().find(|s| *s <= fit).unwrap_or(0.01);
    // ---- the JSON
    let names = ["top", "front", "right", "bottom", "back", "left"];
    let view_json = |v: &ViewPlan| -> Value {
        let mut o = serde_json::Map::new();
        if let Some(p) = v.pos {
            o.insert("position".into(), json!(p));
        }
        o.insert("dimensions".into(), Value::Array(v.dims.clone()));
        Value::Object(o)
    };
    let projected: Vec<Value> = views[1..].iter().map(view_json).collect();
    let b = &views[0];
    let base = json!({
        "name": names[base_side],
        "direction": [dir.x as i64, dir.y as i64, dir.z as i64],
        "up": [up.x as i64, up.y as i64, up.z as i64],
        "scale": scale,
        "dimensions": b.dims,
        "projected": projected,
    });
    let spec = json!({
        "step_file": step_file,
        "title": {
            "drawing_number": stem.to_uppercase(),
            "description": stem.replace(['_', '-'], " ").to_uppercase(),
            "material": "",
            "finish": "",
            "notes": "REMOVE ALL BURRS AND SHARP EDGES.",
            "revision": "A",
            "drawn_by": "",
            "date": today(),
        },
        "views": [base],
    });
    (spec, notes)
}

/// Today's date as YYYY-MM-DD (UTC).
fn today() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    // days since 1970-01-01 to a civil date (Howard Hinnant's algorithm)
    let z = secs.div_euclid(86400) + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    format!("{:04}-{:02}-{:02}", year, month, day)
}

/// Write the generated drawing (or with `base_only` just its base view) next to `step` (as `<stem>.json`, or
/// `<stem>-auto.json`, `<stem>-auto-2.json`, ... when that exists, unless
/// `out` names the file); returns the file written and the notes.
pub fn write_for(m: &Model, step: &std::path::Path, out: Option<&std::path::Path>, base_only: bool) -> Result<(std::path::PathBuf, Vec<String>), String> {
    let stem = step.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "part".into());
    let dir = step.parent().unwrap_or(std::path::Path::new("."));
    let path = match out {
        Some(p) => p.to_path_buf(),
        None => {
            let mut p = dir.join(format!("{}.json", stem));
            let mut k = 1;
            while p.exists() {
                p = dir.join(if k == 1 { format!("{}-auto.json", stem) } else { format!("{}-auto-{}.json", stem, k) });
                k += 1;
            }
            p
        }
    };
    // the STEP file relative to the drawing's folder
    let step_rel = match (path.parent().and_then(|d| d.canonicalize().ok()), step.canonicalize().ok()) {
        (Some(d), Some(s)) if s.parent() == Some(d.as_path()) => s.file_name().unwrap().to_string_lossy().to_string(),
        (_, Some(s)) => s.to_string_lossy().to_string(),
        _ => step.to_string_lossy().to_string(),
    };
    let (mut spec, notes) = generate(m, &step_rel, &stem, base_only);
    // the one time the views are fitted to the sheet: their places go in the
    // JSON, so later renders keep them where they are
    fit_layout(&mut spec, step)?;
    let text = to_text(&spec)?;
    std::fs::write(&path, text).map_err(|e| format!("{}: {}", path.display(), e))?;
    Ok((path, notes))
}

/// Arrays of numbers on one line: `[0, 0, 1]` rather than one number per line.
fn compact_arrays(t: &str) -> String {
    let mut out = String::with_capacity(t.len());
    let mut rest = t;
    while let Some(i) = rest.find('[') {
        out.push_str(&rest[..=i]);
        rest = &rest[i + 1..];
        if let Some(j) = rest.find(']') {
            let inner = &rest[..j];
            if inner.trim().is_empty() || inner.chars().all(|c| c.is_ascii_digit() || " \n.,-+eE".contains(c)) {
                let nums: Vec<&str> = inner.split(',').map(str::trim).filter(|x| !x.is_empty()).collect();
                out.push_str(&nums.join(", "));
                rest = &rest[j..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Lay the drawing's views out to fit the sheet and write where each went
/// ("location" of base and detail views, "distance" of projected and section
/// views) into `spec`.
pub fn fit_layout(spec: &mut Value, step: &std::path::Path) -> Result<(), String> {
    let ds: crate::config::DrawingSpec = serde_json::from_value(spec.clone()).map_err(|e| e.to_string())?;
    let cfg = crate::load_config(None)?;
    let (_, _, places) = crate::render_with(&ds, step, &cfg, true)?;
    for (src, key, val) in places {
        if let Some(Value::Object(o)) = spec.pointer_mut(&src) {
            o.insert(key.to_string(), val);
        }
    }
    Ok(())
}

/// A drawing JSON as text: pretty, with arrays of numbers on one line.
pub fn to_text(spec: &Value) -> Result<String, String> {
    Ok(compact_arrays(&serde_json::to_string_pretty(spec).map_err(|e| e.to_string())?) + "\n")
}
