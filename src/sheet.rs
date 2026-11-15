//! The sheet (ISO A0-A4, landscape or portrait): border, reference-zone grid
//! and title block.

use crate::config::{Config, Projection, SheetSpec, TitleBlock};
use crate::geom::*;
use crate::svg::{text_width, Anchor, Prim};
use std::cell::Cell;

pub const TB_W: f64 = 180.0;
/// General tolerance text when the title block doesn't give one.
pub const DEFAULT_GENERAL_TOL: &str = "UNLESS STATED:  X.X ±0.4    X.XX ±0.15    ANGLES ±0.5°";
pub const TB_H: f64 = 72.0;

/// Size of the sheet being drawn.
#[derive(Clone, Copy, Debug)]
pub struct Sheet {
    pub w: f64,
    pub h: f64,
    /// "A3" etc.
    pub name: &'static str,
    /// Outer border (trim) offset and drawing-frame offset from the sheet edge.
    pub outer: f64,
    pub frame: f64,
}

const SIZES: [(&str, f64, f64); 5] = [("A0", 1189.0, 841.0), ("A1", 841.0, 594.0), ("A2", 594.0, 420.0), ("A3", 420.0, 297.0), ("A4", 297.0, 210.0)];

impl Sheet {
    /// The sheet for a spec (default A3 landscape).
    pub fn of(spec: Option<&SheetSpec>) -> Result<Sheet, String> {
        let size = spec.map_or("A3", |s| s.size.as_str()).trim().to_ascii_uppercase();
        let &(name, long, short) = SIZES.iter().find(|s| s.0 == size).ok_or_else(|| format!("unknown sheet size '{}' (A0, A1, A2, A3 or A4)", size))?;
        let portrait = match spec.map_or("landscape", |s| s.orientation.as_str()).trim().to_ascii_lowercase().as_str() {
            "landscape" | "" => false,
            "portrait" => true,
            o => return Err(format!("unknown sheet orientation '{}' (landscape or portrait)", o)),
        };
        let (w, h) = if portrait { (short, long) } else { (long, short) };
        // A4 needs narrower margins to fit the 180 mm title block
        let (outer, frame) = if name == "A4" { (5.0, 10.0) } else { (10.0, 17.0) };
        Ok(Sheet { w, h, name, outer, frame })
    }
}

thread_local! {
    static SHEET: Cell<Sheet> = Cell::new(Sheet::of(None).unwrap());
}

/// Use `s` for the drawing being rendered on this thread.
pub fn set(s: Sheet) {
    SHEET.with(|c| c.set(s));
}

pub fn get() -> Sheet {
    SHEET.with(|c| c.get())
}

/// Sheet width and height (mm).
#[allow(non_snake_case)]
pub fn W() -> f64 {
    get().w
}

#[allow(non_snake_case)]
pub fn H() -> f64 {
    get().h
}

pub fn frame_box() -> BBox2 {
    let s = get();
    BBox2::new(v2(s.frame, s.frame), v2(s.w - s.frame, s.h - s.frame))
}

pub fn title_box() -> BBox2 {
    let s = get();
    BBox2::new(v2(s.w - s.frame - TB_W, s.frame), v2(s.w - s.frame, s.frame + TB_H))
}

pub fn border(cfg: &Config) -> Vec<Prim> {
    let Sheet { w, h: ht, outer, frame, .. } = get();
    let mut v = vec![
        Prim::Rect { b: BBox2::new(v2(outer, outer), v2(w - outer, ht - outer)), cls: "frame" },
        Prim::Rect { b: frame_box(), cls: "border" },
    ];
    let h = cfg.text_height;
    // reference zones about 50 mm, letters across, numbers down
    let letters = ["A", "B", "C", "D", "E", "F", "G", "ht", "J", "K", "L", "M", "N", "P", "R", "S", "T", "U", "V", "w", "X", "Y", "Z"];
    let ncols = (((w - 2.0 * frame) / 55.0).round() as usize).clamp(2, letters.len());
    let cols = &letters[..ncols];
    let fw = (w - 2.0 * frame) / cols.len() as f64;
    let mid = (outer + frame) / 2.0;
    for (i, c) in cols.iter().enumerate() {
        let x0 = frame + i as f64 * fw;
        if i > 0 {
            for (ya, yb) in [(ht - outer, ht - frame), (outer, frame)] {
                v.push(Prim::Line { a: v2(x0, ya), b: v2(x0, yb), cls: "frame" });
            }
        }
        for y in [ht - mid, mid] {
            v.push(Prim::Text { p: v2(x0 + fw / 2.0, y - h * 0.36), h, anchor: Anchor::Middle, rot: 0.0, s: c.to_string(), cls: "zone" });
        }
    }
    let rows = (((ht - 2.0 * frame) / 55.0).round() as usize).max(2);
    let rh = (ht - 2.0 * frame) / rows as f64;
    for i in 0..rows {
        // numbered top to bottom
        let y1 = ht - frame - i as f64 * rh;
        if i > 0 {
            for (xa, xb) in [(outer, frame), (w - frame, w - outer)] {
                v.push(Prim::Line { a: v2(xa, y1), b: v2(xb, y1), cls: "frame" });
            }
        }
        for x in [mid, w - mid] {
            v.push(Prim::Text {
                p: v2(x, y1 - rh / 2.0 - h * 0.36),
                h,
                anchor: Anchor::Middle,
                rot: 0.0,
                s: (i + 1).to_string(),
                cls: "zone",
            });
        }
    }
    v
}

fn wrap(s: &str, h: f64, width: f64) -> Vec<String> {
    let mut out = Vec::new();
    for para in s.split('\n') {
        let mut cur = String::new();
        for word in para.split_whitespace() {
            let cand = if cur.is_empty() { word.to_string() } else { format!("{} {}", cur, word) };
            if text_width(&cand, h) > width && !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
                cur = word.to_string();
            } else {
                cur = cand;
            }
        }
        out.push(cur);
    }
    out
}

pub fn title_block(cfg: &Config, tb: &TitleBlock, scale: &str, proj: Projection) -> Vec<Prim> {
    let b = title_box();
    let (x0, y0) = (b.min.x, b.min.y);
    let lh = cfg.label_height;
    let th = cfg.text_height;
    let mut v = vec![Prim::Rect { b, cls: "border" }];
    // cell: (x, y, w, h, label, value, value height)
    let mut cells: Vec<(f64, f64, f64, f64, &str, String, f64)> = vec![
        (0.0, 0.0, 105.0, 12.0, "DRAWING No.", tb.drawing_number.clone(), 5.0),
        (105.0, 0.0, 25.0, 12.0, "ISSUE", tb.issue.clone(), 5.0),
        (130.0, 0.0, 25.0, 12.0, "REV", tb.revision.clone(), 5.0),
        (155.0, 0.0, 25.0, 12.0, "SIZE", get().name.into(), 5.0),
        (0.0, 12.0, 60.0, 9.0, "DRAWN BY", tb.drawn_by.clone(), th),
        (60.0, 12.0, 45.0, 9.0, "DATE", tb.date.clone(), th),
        (105.0, 12.0, 30.0, 9.0, "SCALE", scale.to_string(), th),
        (135.0, 12.0, 45.0, 9.0, "SHEET", "1 OF 1".into(), th),
        (0.0, 21.0, 180.0, 10.0, "DESCRIPTION", tb.description.clone(), 4.0),
        (0.0, 31.0, 90.0, 9.0, "MATERIAL", tb.material.clone(), th),
        (90.0, 31.0, 90.0, 9.0, "FINISH", tb.finish.clone(), th),
        (0.0, 40.0, 180.0, 10.0, "GENERAL TOLERANCES", String::new(), th),
    ];
    cells.push((0.0, 50.0, 135.0, 22.0, "NOTES", String::new(), th));
    for (cx, cy, w, h, label, value, vh) in &cells {
        let cb = BBox2::new(v2(x0 + cx, y0 + cy), v2(x0 + cx + w, y0 + cy + h));
        v.push(Prim::Rect { b: cb, cls: "thin" });
        v.push(Prim::Text { p: v2(cb.min.x + 1.0, cb.max.y - 1.0 - lh), h: lh, anchor: Anchor::Start, rot: 0.0, s: label.to_string(), cls: "label" });
        if !value.is_empty() {
            v.push(Prim::Text {
                p: v2(cb.min.x + 2.0, cb.min.y + (h - lh - 1.0 - vh) / 2.0 + 0.2 * vh),
                h: *vh,
                anchor: Anchor::Start,
                rot: 0.0,
                s: value.clone(),
                cls: if *label == "DRAWING No." { "title" } else { "value" },
            });
        }
    }
    // general tolerances: parts separated by two or more spaces go in columns
    let gt = tb.general_tolerances.clone().unwrap_or_else(|| DEFAULT_GENERAL_TOL.into());
    let parts: Vec<&str> = gt.split("  ").map(str::trim).filter(|p| !p.is_empty()).collect();
    let gy = y0 + 40.0 + (10.0 - lh - 1.0 - th) / 2.0 + 0.2 * th;
    let first_w = parts.first().map_or(0.0, |p| text_width(p, th));
    let col = if parts.len() > 1 { (TB_W - 6.0 - first_w) / (parts.len() - 1) as f64 } else { 0.0 };
    for (i, part) in parts.iter().enumerate() {
        let x = if i == 0 { x0 + 2.0 } else { x0 + 6.0 + first_w + col * (i - 1) as f64 };
        v.push(Prim::Text { p: v2(x, gy), h: th, anchor: Anchor::Start, rot: 0.0, s: part.to_string(), cls: "value" });
    }
    // notes text (wrapped)
    let nh = 2.5;
    let lines = wrap(&tb.notes, nh, 131.0);
    for (i, l) in lines.iter().take(6).enumerate() {
        v.push(Prim::Text {
            p: v2(x0 + 2.0, y0 + TB_H - 1.0 - lh - 1.5 - nh - i as f64 * nh * 1.35),
            h: nh,
            anchor: Anchor::Start,
            rot: 0.0,
            s: l.clone(),
            cls: "value",
        });
    }
    // projection symbol cell
    let pb = BBox2::new(v2(x0 + 135.0, y0 + 50.0), v2(x0 + 180.0, y0 + TB_H));
    v.push(Prim::Rect { b: pb, cls: "thin" });
    let label = match proj {
        Projection::Third => "THIRD ANGLE",
        Projection::First => "FIRST ANGLE",
    };
    v.push(Prim::Text { p: v2(pb.min.x + 1.0, pb.max.y - 1.0 - lh), h: lh, anchor: Anchor::Start, rot: 0.0, s: label.into(), cls: "label" });
    v.extend(projection_symbol(v2(pb.center().x, pb.min.y + 8.5), proj));
    v
}

/// ISO 128 projection symbol (truncated cone), centred at c.
pub fn projection_symbol(c: V2, proj: Projection) -> Vec<Prim> {
    let (d_big, d_small, len) = (10.0, 5.0, 12.0);
    let gap = 5.0;
    let total = d_big + gap + len;
    let left = c.x - total / 2.0;
    let (cone_x, circ_x) = match proj {
        Projection::Third => (left + d_big + gap, left + d_big / 2.0),
        Projection::First => (left, left + len + gap + d_big / 2.0),
    };
    let cy = c.y;
    let mut v = vec![
        Prim::Poly {
            pts: vec![
                v2(cone_x, cy - d_small / 2.0),
                v2(cone_x + len, cy - d_big / 2.0),
                v2(cone_x + len, cy + d_big / 2.0),
                v2(cone_x, cy + d_small / 2.0),
            ],
            closed: true,
            cls: "thin",
        },
        Prim::Circle { c: v2(circ_x, cy), r: d_big / 2.0, cls: "thin" },
        Prim::Circle { c: v2(circ_x, cy), r: d_small / 2.0, cls: "thin" },
    ];
    // centre lines
    v.push(Prim::Line { a: v2(cone_x - 1.5, cy), b: v2(cone_x + len + 1.5, cy), cls: "center" });
    v.push(Prim::Line { a: v2(circ_x - d_big / 2.0 - 1.5, cy), b: v2(circ_x + d_big / 2.0 + 1.5, cy), cls: "center" });
    v.push(Prim::Line { a: v2(circ_x, cy - d_big / 2.0 - 1.5), b: v2(circ_x, cy + d_big / 2.0 + 1.5), cls: "center" });
    v
}
