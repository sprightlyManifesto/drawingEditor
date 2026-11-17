//! Drawing primitives in sheet millimetres (y up) and their SVG output.

use crate::geom::*;
use std::fmt::Write;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Anchor {
    Start,
    Middle,
    End,
}

#[derive(Clone, Debug)]
pub enum Prim {
    Line { a: V2, b: V2, cls: &'static str },
    Poly { pts: Vec<V2>, closed: bool, cls: &'static str },
    Circle { c: V2, r: f64, cls: &'static str },
    Fill { pts: Vec<V2> },
    Rect { b: BBox2, cls: &'static str },
    Text { p: V2, h: f64, anchor: Anchor, rot: f64, s: String, cls: &'static str },
    /// `<g>` with data attributes; `pts` are points (moved with the group,
    /// written with y flipped like everything else).
    Group { attrs: Vec<(&'static str, String)>, pts: Vec<(&'static str, V2)>, items: Vec<Prim> },
    /// Region bounded by closed loops (even-odd: inner loops are holes).
    Area { loops: Vec<Vec<V2>>, cls: &'static str },
}

/// A direction or other vector as an attribute value in SVG coordinates (y down).
pub fn vec_attr(v: V2) -> String {
    format!("{},{}", f(v.x), f(-v.y))
}

/// Approximate advance width of a string at text height h.
pub fn text_width(s: &str, h: f64) -> f64 {
    s.chars()
        .map(|c| match c {
            'i' | 'l' | 'I' | '.' | ',' | ':' | ';' | '!' | '|' | '\'' => 0.3,
            ' ' | 'f' | 't' | 'j' | 'r' | '(' | ')' | '-' => 0.42,
            'm' | 'w' | 'M' | 'W' | 'Ø' | '±' | '°' => 0.85,
            c if c.is_uppercase() || c.is_ascii_digit() => 0.66,
            _ => 0.58,
        })
        .sum::<f64>()
        * h
}

impl Prim {
    pub fn bbox(&self) -> BBox2 {
        let mut b = BBox2::empty();
        match self {
            Prim::Line { a, b: q, .. } => {
                b.add(*a);
                b.add(*q);
            }
            Prim::Poly { pts, .. } | Prim::Fill { pts } => pts.iter().for_each(|p| b.add(*p)),
            Prim::Circle { c, r, .. } => {
                b.add(*c - v2(*r, *r));
                b.add(*c + v2(*r, *r));
            }
            Prim::Rect { b: r, .. } => b = *r,
            Prim::Text { p, h, anchor, rot, s, .. } => {
                return text_box(*p, *h, *anchor, *rot, s);
            }
            Prim::Group { items, .. } => return bbox_of(items),
            Prim::Area { loops, .. } => loops.iter().flatten().for_each(|p| b.add(*p)),
        }
        b
    }
    pub fn translate(&mut self, d: V2) {
        match self {
            Prim::Line { a, b, .. } => {
                *a = *a + d;
                *b = *b + d;
            }
            Prim::Poly { pts, .. } | Prim::Fill { pts } => pts.iter_mut().for_each(|p| *p = *p + d),
            Prim::Circle { c, .. } => *c = *c + d,
            Prim::Rect { b, .. } => *b = b.translate(d),
            Prim::Text { p, .. } => *p = *p + d,
            Prim::Group { pts, items, .. } => {
                pts.iter_mut().for_each(|(_, p)| *p = *p + d);
                items.iter_mut().for_each(|p| p.translate(d));
            }
            Prim::Area { loops, .. } => loops.iter_mut().flatten().for_each(|p| *p = *p + d),
        }
    }
}

pub fn text_box(p: V2, h: f64, anchor: Anchor, rot: f64, s: &str) -> BBox2 {
    let lines: Vec<&str> = s.split('\n').collect();
    let w = lines.iter().map(|l| text_width(l, h)).fold(0.0, f64::max);
    let x0 = match anchor {
        Anchor::Start => 0.0,
        Anchor::Middle => -w / 2.0,
        Anchor::End => -w,
    };
    let nl = lines.len() as f64;
    let corners = [v2(x0, -0.25 * h - (nl - 1.0) * 1.4 * h), v2(x0 + w, -0.25 * h - (nl - 1.0) * 1.4 * h), v2(x0 + w, h), v2(x0, h)];
    let mut b = BBox2::empty();
    let a = rot.to_radians();
    for c in corners {
        b.add(p + c.rot(a));
    }
    b
}

pub fn bbox_of(prims: &[Prim]) -> BBox2 {
    prims.iter().fold(BBox2::empty(), |a, p| a.union(&p.bbox()))
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn f(x: f64) -> String {
    let s = format!("{:.3}", x);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.to_string() }
}

/// Emit primitives. Coordinates are flipped (y up -> SVG y down).
pub fn emit(out: &mut String, prims: &[Prim], indent: &str) {
    for p in prims {
        match p {
            Prim::Group { attrs, pts, items } => {
                let mut a = String::new();
                for (k, v) in attrs {
                    let _ = write!(a, r#" {}="{}""#, k, esc(v));
                }
                for (k, v) in pts {
                    let _ = write!(a, r#" {}="{},{}""#, k, f(v.x), f(-v.y));
                }
                let _ = writeln!(out, "{}<g{}>", indent, a);
                emit(out, items, &format!("{}  ", indent));
                let _ = writeln!(out, "{}</g>", indent);
            }
            Prim::Area { loops, cls } => {
                let mut d = String::new();
                for l in loops {
                    for (i, q) in l.iter().enumerate() {
                        let _ = write!(d, "{}{},{} ", if i == 0 { "M" } else { "L" }, f(q.x), f(-q.y));
                    }
                    d.push_str("Z ");
                }
                let _ = writeln!(out, r#"{}<path class="{}" fill-rule="evenodd" d="{}"/>"#, indent, cls, d.trim_end());
            }
            Prim::Line { a, b, cls } => {
                let _ = writeln!(
                    out,
                    r#"{}<line class="{}" x1="{}" y1="{}" x2="{}" y2="{}"/>"#,
                    indent, cls, f(a.x), f(-a.y), f(b.x), f(-b.y)
                );
            }
            Prim::Poly { pts, closed, cls } => {
                if pts.len() < 2 {
                    continue;
                }
                let mut d = String::new();
                for (i, q) in pts.iter().enumerate() {
                    let _ = write!(d, "{}{},{} ", if i == 0 { "M" } else { "L" }, f(q.x), f(-q.y));
                }
                if *closed {
                    d.push('Z');
                }
                let _ = writeln!(out, r#"{}<path class="{}" d="{}"/>"#, indent, cls, d.trim_end());
            }
            Prim::Circle { c, r, cls } => {
                let _ = writeln!(out, r#"{}<circle class="{}" cx="{}" cy="{}" r="{}"/>"#, indent, cls, f(c.x), f(-c.y), f(*r));
            }
            Prim::Fill { pts } => {
                let s: Vec<String> = pts.iter().map(|q| format!("{},{}", f(q.x), f(-q.y))).collect();
                let _ = writeln!(out, r#"{}<polygon class="fill" points="{}"/>"#, indent, s.join(" "));
            }
            Prim::Rect { b, cls } => {
                let _ = writeln!(
                    out,
                    r#"{}<rect class="{}" x="{}" y="{}" width="{}" height="{}"/>"#,
                    indent,
                    cls,
                    f(b.min.x),
                    f(-b.max.y),
                    f(b.width()),
                    f(b.height())
                );
            }
            Prim::Text { p, h, anchor, rot, s, cls } => {
                let a = match anchor {
                    Anchor::Start => "start",
                    Anchor::Middle => "middle",
                    Anchor::End => "end",
                };
                let tr = if rot.abs() > 1e-9 { format!(r#" transform="rotate({} {} {})""#, f(-rot), f(p.x), f(-p.y)) } else { String::new() };
                let lines: Vec<&str> = s.split('\n').collect();
                if lines.len() == 1 {
                    let _ = writeln!(
                        out,
                        r#"{}<text class="{}" x="{}" y="{}" font-size="{}" text-anchor="{}"{}>{}</text>"#,
                        indent, cls, f(p.x), f(-p.y), f(*h), a, tr, esc(s)
                    );
                } else {
                    let _ = write!(
                        out,
                        r#"{}<text class="{}" x="{}" y="{}" font-size="{}" text-anchor="{}"{}>"#,
                        indent, cls, f(p.x), f(-p.y), f(*h), a, tr
                    );
                    for (i, l) in lines.iter().enumerate() {
                        let _ = write!(out, r#"<tspan x="{}" dy="{}">{}</tspan>"#, f(p.x), if i == 0 { "0".into() } else { f(1.4 * h) }, esc(l));
                    }
                    out.push_str("</text>\n");
                }
            }
        }
    }
}

pub fn style(cfg: &crate::config::Config) -> String {
    format!(
        r#"<style>
  line, path, circle, rect {{ fill: none; stroke: #000; stroke-linecap: round; stroke-linejoin: round; }}
  .visible {{ stroke-width: {vis}; }}
  .tangent {{ stroke-width: {thin}; stroke: #888; }}
  .hidden {{ stroke-width: {hid}; stroke-dasharray: 3 1.5; stroke-linecap: butt; }}
  .center {{ stroke-width: {thin}; stroke-dasharray: 8 1.5 1.5 1.5; stroke-linecap: butt; }}
  .phantom {{ stroke-width: {thin}; stroke-dasharray: 10 1.5 1.5 1.5 1.5 1.5; stroke-linecap: butt; }}
  .cutplane {{ stroke-width: {thin}; stroke-dasharray: 12 2 2 2; stroke-linecap: butt; }}
  .cutthick {{ stroke-width: {border}; }}
  .hatch {{ stroke-width: 0.18; }}
  .thin {{ stroke-width: {thin}; }}
  .border {{ stroke-width: {border}; }}
  .frame {{ stroke-width: 0.35; }}
  .box {{ stroke-width: {thin}; fill: #fff; }}
  .fill {{ fill: #000; stroke: none; }}
  text {{ font-family: {font}; fill: #000; stroke: none; }}
  .label {{ fill: #444; }}
  .title {{ font-weight: bold; }}
  .pick {{ display: none; }}
  .tbhit {{ fill: none; stroke: none; }}
</style>"#,
        vis = cfg.line_visible,
        hid = cfg.line_hidden,
        thin = cfg.line_thin,
        border = cfg.line_border,
        font = cfg.font_family
    )
}
