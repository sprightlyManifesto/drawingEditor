//! CadQuery-style topology selection.
//!
//! A chain such as `faces(">X").edges("|Z").vertices(">X")` is evaluated
//! against the model. Supported selector strings (as in CadQuery):
//!
//! * `>X`, `<Y`, `>(1,1,0)`      – max / min of object centre along a direction
//! * `>X[1]`, `<Z[-1]`           – n-th group along a direction
//! * `|Z`, `#Z`                  – parallel / perpendicular (planar faces, linear edges)
//! * `+Z`, `-Z`                  – face normal / edge direction equal to a direction
//! * `%PLANE`, `%CYLINDER`, `%LINE`, `%CIRCLE`, ... – geometry type
//! * `and`, `or`, `not`, `exc`, parentheses
//! * `@12`                       – the item with that index (as listed by the editor)

use crate::brep::Model;
use crate::geom::*;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Face,
    Edge,
    Vertex,
}

#[derive(Clone, Debug)]
pub struct Sel {
    pub kind: Kind,
    pub ids: Vec<usize>,
}

#[derive(Debug, Clone)]
enum Ex {
    Max(V3),
    Min(V3),
    Nth(V3, i64, bool),
    Par(V3),
    Perp(V3),
    Dir(V3),
    Type(String),
    And(Box<Ex>, Box<Ex>),
    Or(Box<Ex>, Box<Ex>),
    Exc(Box<Ex>, Box<Ex>),
    Not(Box<Ex>),
    Id(usize),
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Sym(char),
    Num(f64),
    Id(String),
}

fn lex(s: &str) -> Result<Vec<Tok>, String> {
    let c: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < c.len() {
        let ch = c[i];
        if ch.is_whitespace() {
            i += 1;
        } else if "<>|#+-%[](),@".contains(ch) {
            // A '-' directly followed by a digit inside brackets/vectors is a number.
            let prev_open = matches!(out.last(), Some(Tok::Sym('[')) | Some(Tok::Sym('(')) | Some(Tok::Sym(',')));
            if (ch == '-' || ch == '+') && prev_open && i + 1 < c.len() && (c[i + 1].is_ascii_digit() || c[i + 1] == '.') {
                let st = i;
                i += 1;
                while i < c.len() && (c[i].is_ascii_digit() || c[i] == '.' || c[i] == 'e' || c[i] == 'E') {
                    i += 1;
                }
                let t: String = c[st..i].iter().collect();
                out.push(Tok::Num(t.parse().map_err(|_| format!("bad number '{}'", t))?));
            } else {
                out.push(Tok::Sym(ch));
                i += 1;
            }
        } else if ch.is_ascii_digit() || ch == '.' {
            let st = i;
            while i < c.len() && (c[i].is_ascii_digit() || c[i] == '.' || c[i] == 'e' || c[i] == 'E') {
                i += 1;
            }
            let t: String = c[st..i].iter().collect();
            out.push(Tok::Num(t.parse().map_err(|_| format!("bad number '{}'", t))?));
        } else if ch.is_alphabetic() || ch == '_' {
            let st = i;
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_') {
                i += 1;
            }
            out.push(Tok::Id(c[st..i].iter().collect()));
        } else {
            return Err(format!("unexpected character '{}' in selector '{}'", ch, s));
        }
    }
    Ok(out)
}

struct Parser {
    t: Vec<Tok>,
    i: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.i)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.t.get(self.i).cloned();
        self.i += 1;
        t
    }
    fn is_word(&self, w: &str) -> bool {
        matches!(self.peek(), Some(Tok::Id(s)) if s.eq_ignore_ascii_case(w))
    }
    fn expr(&mut self) -> Result<Ex, String> {
        let mut a = self.and()?;
        while self.is_word("or") {
            self.i += 1;
            a = Ex::Or(Box::new(a), Box::new(self.and()?));
        }
        Ok(a)
    }
    fn and(&mut self) -> Result<Ex, String> {
        let mut a = self.not()?;
        loop {
            if self.is_word("and") {
                self.i += 1;
                a = Ex::And(Box::new(a), Box::new(self.not()?));
            } else if self.is_word("exc") {
                self.i += 1;
                a = Ex::Exc(Box::new(a), Box::new(self.not()?));
            } else {
                return Ok(a);
            }
        }
    }
    fn not(&mut self) -> Result<Ex, String> {
        if self.is_word("not") {
            self.i += 1;
            return Ok(Ex::Not(Box::new(self.not()?)));
        }
        self.atom()
    }
    fn dir(&mut self) -> Result<V3, String> {
        match self.next() {
            Some(Tok::Id(s)) => match s.to_ascii_uppercase().as_str() {
                "X" => Ok(v3(1.0, 0.0, 0.0)),
                "Y" => Ok(v3(0.0, 1.0, 0.0)),
                "Z" => Ok(v3(0.0, 0.0, 1.0)),
                _ => Err(format!("unknown direction '{}'", s)),
            },
            Some(Tok::Sym('(')) => {
                let mut c = [0.0; 3];
                for (k, ck) in c.iter_mut().enumerate() {
                    match self.next() {
                        Some(Tok::Num(n)) => *ck = n,
                        t => return Err(format!("expected number in vector, got {:?}", t)),
                    }
                    if k < 2 && self.next() != Some(Tok::Sym(',')) {
                        return Err("expected ',' in vector".into());
                    }
                }
                if self.next() != Some(Tok::Sym(')')) {
                    return Err("expected ')' after vector".into());
                }
                let v = v3(c[0], c[1], c[2]);
                if v.len() < 1e-12 {
                    return Err("zero direction vector".into());
                }
                Ok(v.norm())
            }
            t => Err(format!("expected direction, got {:?}", t)),
        }
    }
    fn atom(&mut self) -> Result<Ex, String> {
        match self.next() {
            Some(Tok::Sym('(')) => {
                let e = self.expr()?;
                if self.next() != Some(Tok::Sym(')')) {
                    return Err("expected ')'".into());
                }
                Ok(e)
            }
            Some(Tok::Sym(c @ ('>' | '<'))) => {
                let d = self.dir()?;
                if self.peek() == Some(&Tok::Sym('[')) {
                    self.i += 1;
                    let n = match self.next() {
                        Some(Tok::Num(n)) => n as i64,
                        t => return Err(format!("expected index, got {:?}", t)),
                    };
                    if self.next() != Some(Tok::Sym(']')) {
                        return Err("expected ']'".into());
                    }
                    return Ok(Ex::Nth(d, n, c == '>'));
                }
                Ok(if c == '>' { Ex::Max(d) } else { Ex::Min(d) })
            }
            Some(Tok::Sym('@')) => match self.next() {
                Some(Tok::Num(n)) if n >= 0.0 && n.fract() == 0.0 => Ok(Ex::Id(n as usize)),
                t => Err(format!("expected an index after @, got {:?}", t)),
            },
            Some(Tok::Sym('|')) => Ok(Ex::Par(self.dir()?)),
            Some(Tok::Sym('#')) => Ok(Ex::Perp(self.dir()?)),
            Some(Tok::Sym('+')) => Ok(Ex::Dir(self.dir()?)),
            Some(Tok::Sym('-')) => Ok(Ex::Dir(-self.dir()?)),
            Some(Tok::Sym('%')) => match self.next() {
                Some(Tok::Id(s)) => Ok(Ex::Type(s.to_ascii_uppercase())),
                t => Err(format!("expected type name after %, got {:?}", t)),
            },
            t => Err(format!("unexpected token {:?}", t)),
        }
    }
}

fn parse_expr(s: &str) -> Result<Ex, String> {
    let mut p = Parser { t: lex(s)?, i: 0 };
    let e = p.expr()?;
    if p.i < p.t.len() {
        return Err(format!("trailing tokens in selector '{}'", s));
    }
    Ok(e)
}

fn center(m: &Model, k: Kind, i: usize) -> V3 {
    match k {
        Kind::Face => m.faces[i].center,
        Kind::Edge => m.edges[i].center,
        Kind::Vertex => m.verts[i],
    }
}

/// Characteristic direction: planar face normal or linear edge direction.
fn direction(m: &Model, k: Kind, i: usize) -> Option<V3> {
    match k {
        Kind::Face => m.faces[i].plane_normal(),
        Kind::Edge => {
            let e = &m.edges[i];
            match e.curve {
                crate::brep::Curve::Line { .. } => Some((m.verts[e.v1] - m.verts[e.v0]).norm()),
                _ => None,
            }
        }
        Kind::Vertex => None,
    }
}

fn type_of(m: &Model, k: Kind, i: usize) -> &str {
    match k {
        Kind::Face => m.faces[i].surf.type_name(),
        Kind::Edge => m.edges[i].curve.type_name(),
        Kind::Vertex => "VERTEX",
    }
}

fn eval(m: &Model, k: Kind, e: &Ex, cands: &[usize]) -> Vec<usize> {
    let tol = (m.diag() * 1e-6).max(1e-4);
    let ang_tol = 1e-4;
    match e {
        Ex::Max(d) | Ex::Min(d) => {
            let sign = if matches!(e, Ex::Max(_)) { 1.0 } else { -1.0 };
            let vals: Vec<f64> = cands.iter().map(|&i| center(m, k, i).dot(*d) * sign).collect();
            let best = vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            cands.iter().zip(vals).filter(|(_, v)| *v >= best - tol).map(|(i, _)| *i).collect()
        }
        Ex::Nth(d, n, max) => {
            let mut v: Vec<(f64, usize)> = cands.iter().map(|&i| (center(m, k, i).dot(*d), i)).collect();
            v.sort_by(|a, b| a.0.total_cmp(&b.0));
            if *max {
                v.reverse();
            }
            let mut groups: Vec<Vec<usize>> = Vec::new();
            let mut last = f64::NAN;
            for (val, i) in v {
                if groups.is_empty() || (val - last).abs() > tol * 10.0 {
                    groups.push(vec![]);
                    last = val;
                }
                groups.last_mut().unwrap().push(i);
            }
            let len = groups.len() as i64;
            let idx = if *n < 0 { len + n } else { *n };
            if idx < 0 || idx >= len { vec![] } else { groups[idx as usize].clone() }
        }
        Ex::Par(d) => cands
            .iter()
            .copied()
            .filter(|&i| direction(m, k, i).is_some_and(|v| v.dot(*d).abs() > 1.0 - ang_tol))
            .collect(),
        Ex::Perp(d) => cands
            .iter()
            .copied()
            .filter(|&i| direction(m, k, i).is_some_and(|v| v.dot(*d).abs() < ang_tol))
            .collect(),
        Ex::Dir(d) => cands
            .iter()
            .copied()
            .filter(|&i| direction(m, k, i).is_some_and(|v| v.dot(*d) > 1.0 - ang_tol))
            .collect(),
        Ex::Type(t) => cands.iter().copied().filter(|&i| type_of(m, k, i) == t).collect(),
        Ex::Id(n) => cands.iter().copied().filter(|&i| i == *n).collect(),
        Ex::And(a, b) => {
            let sb: BTreeSet<usize> = eval(m, k, b, cands).into_iter().collect();
            eval(m, k, a, cands).into_iter().filter(|i| sb.contains(i)).collect()
        }
        Ex::Or(a, b) => {
            let mut s: BTreeSet<usize> = eval(m, k, a, cands).into_iter().collect();
            s.extend(eval(m, k, b, cands));
            cands.iter().copied().filter(|i| s.contains(i)).collect()
        }
        Ex::Exc(a, b) => {
            let sb: BTreeSet<usize> = eval(m, k, b, cands).into_iter().collect();
            eval(m, k, a, cands).into_iter().filter(|i| !sb.contains(i)).collect()
        }
        Ex::Not(a) => {
            let sa: BTreeSet<usize> = eval(m, k, a, cands).into_iter().collect();
            cands.iter().copied().filter(|i| !sa.contains(i)).collect()
        }
    }
}

/// Split `faces(">X").edges("|Z")` into [("faces", ">X"), ("edges", "|Z")].
fn parse_chain(s: &str) -> Result<Vec<(String, String)>, String> {
    let c: Vec<char> = s.trim().chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < c.len() {
        while i < c.len() && (c[i].is_whitespace() || c[i] == '.') {
            i += 1;
        }
        if i >= c.len() {
            break;
        }
        let st = i;
        while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_') {
            i += 1;
        }
        let name: String = c[st..i].iter().collect();
        while i < c.len() && c[i].is_whitespace() {
            i += 1;
        }
        if i >= c.len() || c[i] != '(' {
            return Err(format!("expected '(' after '{}' in '{}'", name, s));
        }
        i += 1;
        while i < c.len() && c[i].is_whitespace() {
            i += 1;
        }
        let mut arg = String::new();
        if i < c.len() && (c[i] == '"' || c[i] == '\'') {
            let q = c[i];
            i += 1;
            while i < c.len() && c[i] != q {
                arg.push(c[i]);
                i += 1;
            }
            i += 1;
        }
        while i < c.len() && c[i].is_whitespace() {
            i += 1;
        }
        if i >= c.len() || c[i] != ')' {
            return Err(format!("expected ')' in '{}'", s));
        }
        i += 1;
        out.push((name, arg));
    }
    if out.is_empty() {
        return Err(format!("empty selector '{}'", s));
    }
    Ok(out)
}

pub fn select(m: &Model, chain: &str) -> Result<Sel, String> {
    let calls = parse_chain(chain)?;
    let mut cur: Option<Sel> = None;
    for (name, arg) in calls {
        let kind = match name.as_str() {
            "faces" => Kind::Face,
            "edges" => Kind::Edge,
            "vertices" | "verts" => Kind::Vertex,
            "Workplane" | "cq" | "workplane" => continue,
            n => return Err(format!("unknown selector method '{}' in '{}'", n, chain)),
        };
        let mut set: BTreeSet<usize> = BTreeSet::new();
        match &cur {
            None => match kind {
                Kind::Face => set.extend(0..m.faces.len()),
                Kind::Edge => set.extend(0..m.edges.len()),
                Kind::Vertex => set.extend(0..m.verts.len()),
            },
            Some(s) => {
                for &i in &s.ids {
                    match (s.kind, kind) {
                        (a, b) if a == b => {
                            set.insert(i);
                        }
                        (Kind::Face, Kind::Edge) => set.extend(m.faces[i].edges.iter().copied()),
                        (Kind::Face, Kind::Vertex) => {
                            for &e in &m.faces[i].edges {
                                set.insert(m.edges[e].v0);
                                set.insert(m.edges[e].v1);
                            }
                        }
                        (Kind::Edge, Kind::Vertex) => {
                            set.insert(m.edges[i].v0);
                            set.insert(m.edges[i].v1);
                        }
                        (Kind::Edge, Kind::Face) => set.extend(m.edges[i].faces.iter().copied()),
                        (Kind::Vertex, Kind::Edge) => set.extend(m.vert_edges[i].iter().copied()),
                        (Kind::Vertex, Kind::Face) => {
                            for &e in &m.vert_edges[i] {
                                set.extend(m.edges[e].faces.iter().copied());
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        let cands: Vec<usize> = set.into_iter().collect();
        let ids = if arg.trim().is_empty() {
            cands
        } else {
            let ex = parse_expr(&arg).map_err(|e| format!("{} (in '{}')", e, chain))?;
            eval(m, kind, &ex, &cands)
        };
        cur = Some(Sel { kind, ids });
    }
    let s = cur.ok_or_else(|| format!("selector '{}' selects nothing", chain))?;
    if std::env::var_os("STEPDRAW_DEBUG").is_some() {
        eprintln!("select {}:", chain);
        for &i in &s.ids {
            let (t, c) = match s.kind {
                Kind::Face => (format!("{:?}", m.faces[i].surf.kind), m.faces[i].center),
                Kind::Edge => (m.edges[i].curve.type_name().to_string(), m.edges[i].center),
                Kind::Vertex => ("VERTEX".into(), m.verts[i]),
            };
            eprintln!("  #{} {} centre ({:.2}, {:.2}, {:.2})", i, t, c.x, c.y, c.z);
        }
    }
    if s.ids.is_empty() {
        return Err(format!("selector '{}' matched no geometry", chain));
    }
    Ok(s)
}

fn dir_name(d: V3) -> String {
    for (v, n) in [(v3(1.0, 0.0, 0.0), "X"), (v3(0.0, 1.0, 0.0), "Y"), (v3(0.0, 0.0, 1.0), "Z")] {
        if d.dot(v) > 1.0 - 1e-9 {
            return n.into();
        }
    }
    let f = |x: f64| crate::annot::fmt_num(x, 5, true);
    format!("({},{},{})", f(d.x), f(d.y), f(d.z))
}

/// The shortest selector chain (up to three steps) that selects exactly
/// `ids`, e.g. `faces("%CYLINDER").faces("<X[2]")`; otherwise the items by
/// index, `faces("@3 or @7")`.
pub fn find_selector(m: &Model, kind: Kind, ids: &[usize]) -> String {
    let method = match kind {
        Kind::Face => "faces",
        Kind::Edge => "edges",
        Kind::Vertex => "vertices",
    };
    let target: BTreeSet<usize> = ids.iter().copied().collect();
    let by_id = || {
        let s: Vec<String> = target.iter().map(|i| format!("@{}", i)).collect();
        format!("{}('{}')", method, s.join(" or "))
    };
    if target.is_empty() {
        return String::new();
    }
    let all: Vec<usize> = (0..match kind {
        Kind::Face => m.faces.len(),
        Kind::Edge => m.edges.len(),
        Kind::Vertex => m.verts.len(),
    })
        .collect();
    // vocabulary of single steps
    let mut dirs = vec![v3(1.0, 0.0, 0.0), v3(0.0, 1.0, 0.0), v3(0.0, 0.0, 1.0)];
    for &i in &target {
        if let Some(d) = direction(m, kind, i)
            && !dirs.iter().any(|q| q.dot(d).abs() > 1.0 - 1e-6)
        {
            dirs.push(d);
        }
    }
    let mut steps: Vec<(String, Ex)> = Vec::new();
    let types: BTreeSet<String> = target.iter().map(|&i| type_of(m, kind, i).to_string()).collect();
    if types.len() == 1 {
        let t = types.into_iter().next().unwrap();
        steps.push((format!("%{}", t), Ex::Type(t)));
    }
    for &d in &dirs {
        let n = dir_name(d);
        steps.push((format!(">{}", n), Ex::Max(d)));
        steps.push((format!("<{}", n), Ex::Min(d)));
        for k in 1..12 {
            steps.push((format!(">{}[{}]", n, k), Ex::Nth(d, k, true)));
            steps.push((format!("<{}[{}]", n, k), Ex::Nth(d, k, false)));
        }
        if kind != Kind::Vertex {
            steps.push((format!("|{}", n), Ex::Par(d)));
            steps.push((format!("+{}", n), Ex::Dir(d)));
            steps.push((format!("-{}", n), Ex::Dir(-d)));
        }
    }
    // breadth-first over chains of steps that keep the targets and shrink the set
    let mut frontier: Vec<(Vec<usize>, Vec<String>)> = vec![(all, vec![])];
    let mut best: Option<String> = None;
    for _depth in 0..3 {
        let mut next: Vec<(Vec<usize>, Vec<String>)> = Vec::new();
        for (set, chain) in &frontier {
            // several targets: an "or" of steps that each pick out one of them
            if target.len() > 1 && target.len() <= 6 {
                let mut parts: Vec<&str> = Vec::new();
                for &t in &target {
                    let one = steps
                        .iter()
                        .filter(|(_, ex)| eval(m, kind, ex, set) == [t])
                        .map(|(l, _)| l.as_str())
                        .min_by_key(|l| l.len());
                    match one {
                        Some(l) => parts.push(l),
                        None => break,
                    }
                }
                if parts.len() == target.len() {
                    let mut c: Vec<String> = chain.clone();
                    c.push(parts.join(" or "));
                    let s: Vec<String> = c.iter().map(|x| format!("{}('{}')", method, x)).collect();
                    let s = s.join(".");
                    if best.as_ref().is_none_or(|b| s.len() < b.len()) {
                        best = Some(s);
                    }
                }
            }
            for (label, ex) in &steps {
                let r = eval(m, kind, ex, set);
                if r.len() >= set.len() || !target.iter().all(|t| r.contains(t)) {
                    continue;
                }
                let mut c = chain.clone();
                c.push(label.clone());
                if r.len() == target.len() {
                    let s: Vec<String> = c.iter().map(|x| format!("{}('{}')", method, x)).collect();
                    let s = s.join(".");
                    if best.as_ref().is_none_or(|b| s.len() < b.len()) {
                        best = Some(s);
                    }
                } else if !next.iter().any(|(s2, _)| *s2 == r) {
                    next.push((r, c));
                }
            }
        }
        // keep the most selective candidates (all depths are searched; the shortest result wins)
        next.sort_by_key(|(s, c)| (s.len(), c.iter().map(|x| x.len()).sum::<usize>()));
        next.truncate(60);
        frontier = next;
    }
    best.unwrap_or_else(by_id)
}
