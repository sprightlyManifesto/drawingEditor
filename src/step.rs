//! Minimal ISO-10303-21 (STEP physical file) reader.
//!
//! Parses the DATA section into a map of entity id -> entity. Complex
//! (multi-part) entities such as `( BOUNDED_CURVE() B_SPLINE_CURVE(..) .. )`
//! are kept as a list of named parts.

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum P {
    Ref(usize),
    Num(f64),
    Str(String),
    Enum(String),
    List(Vec<P>),
    Typed(String, Vec<P>),
    Null,
}

impl P {
    pub fn as_ref(&self) -> Option<usize> {
        match self {
            P::Ref(r) => Some(*r),
            _ => None,
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            P::Num(n) => Some(*n),
            P::Typed(_, v) if v.len() == 1 => v[0].as_f64(),
            _ => None,
        }
    }
    pub fn as_list(&self) -> Option<&[P]> {
        match self {
            P::List(l) => Some(l),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            P::Enum(e) if e == "T" => Some(true),
            P::Enum(e) if e == "F" => Some(false),
            _ => None,
        }
    }
    pub fn as_enum(&self) -> Option<&str> {
        match self {
            P::Enum(e) => Some(e),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            P::Str(s) => Some(s),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Ent {
    pub parts: Vec<(String, Vec<P>)>,
}

impl Ent {
    /// Name of a simple entity (first part of a complex one).
    pub fn name(&self) -> &str {
        self.parts.first().map(|p| p.0.as_str()).unwrap_or("")
    }
    pub fn is_simple(&self) -> bool {
        self.parts.len() == 1
    }
    pub fn part(&self, name: &str) -> Option<&[P]> {
        self.parts.iter().find(|p| p.0 == name).map(|p| p.1.as_slice())
    }
    pub fn has(&self, name: &str) -> bool {
        self.parts.iter().any(|p| p.0 == name)
    }
    pub fn params(&self) -> &[P] {
        self.parts.first().map(|p| p.1.as_slice()).unwrap_or(&[])
    }
}

pub struct StepFile {
    pub ents: HashMap<usize, Ent>,
}

fn strip_comments(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    let mut in_str = false;
    while i < b.len() {
        let c = b[i];
        if in_str {
            out.push(c);
            if c == b'\'' {
                if i + 1 < b.len() && b[i + 1] == b'\'' {
                    out.push(b'\'');
                    i += 2;
                    continue;
                }
                in_str = false;
            }
            i += 1;
        } else if c == b'\'' {
            in_str = true;
            out.push(c);
            i += 1;
        } else if c == b'/' && i + 1 < b.len() && b[i + 1] == b'*' {
            i += 2;
            while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
        } else {
            out.push(c);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

struct Lx<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Lx<'a> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }
    fn peek(&self) -> u8 {
        if self.i < self.s.len() { self.s[self.i] } else { 0 }
    }
    fn err<T>(&self, msg: &str) -> Result<T, String> {
        let lo = self.i.saturating_sub(30);
        let hi = (self.i + 30).min(self.s.len());
        Err(format!("STEP parse error: {} near '{}'", msg, String::from_utf8_lossy(&self.s[lo..hi])))
    }
    fn expect(&mut self, c: u8) -> Result<(), String> {
        self.ws();
        if self.peek() == c {
            self.i += 1;
            Ok(())
        } else {
            self.err(&format!("expected '{}'", c as char))
        }
    }
    fn ident(&mut self) -> String {
        let st = self.i;
        while self.i < self.s.len() && (self.s[self.i].is_ascii_alphanumeric() || self.s[self.i] == b'_') {
            self.i += 1;
        }
        String::from_utf8_lossy(&self.s[st..self.i]).to_ascii_uppercase()
    }
    fn int(&mut self) -> Result<usize, String> {
        let st = self.i;
        while self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
            self.i += 1;
        }
        std::str::from_utf8(&self.s[st..self.i])
            .ok()
            .and_then(|t| t.parse().ok())
            .map_or_else(|| self.err("bad integer"), Ok)
    }
    fn number(&mut self) -> Result<f64, String> {
        let st = self.i;
        while self.i < self.s.len() && matches!(self.s[self.i], b'0'..=b'9' | b'+' | b'-' | b'.' | b'e' | b'E') {
            self.i += 1;
        }
        let t = std::str::from_utf8(&self.s[st..self.i]).unwrap_or("");
        let fixed;
        let t = if t.ends_with('.') || t.contains(".E") || t.contains(".e") {
            fixed = t.replacen(".E", ".0E", 1).replacen(".e", ".0e", 1);
            if fixed.ends_with('.') { format!("{}0", fixed) } else { fixed }
        } else {
            t.to_string()
        };
        t.parse::<f64>().map_or_else(|_| self.err("bad number"), Ok)
    }
    fn string(&mut self) -> String {
        // assumes current char is '
        self.i += 1;
        let mut out = Vec::new();
        while self.i < self.s.len() {
            let c = self.s[self.i];
            if c == b'\'' {
                if self.i + 1 < self.s.len() && self.s[self.i + 1] == b'\'' {
                    out.push(b'\'');
                    self.i += 2;
                    continue;
                }
                self.i += 1;
                break;
            }
            out.push(c);
            self.i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    }
    /// Parse params after the opening '(' up to and including ')'.
    fn params(&mut self) -> Result<Vec<P>, String> {
        let mut v = Vec::new();
        self.ws();
        if self.peek() == b')' {
            self.i += 1;
            return Ok(v);
        }
        loop {
            v.push(self.param()?);
            self.ws();
            match self.peek() {
                b',' => self.i += 1,
                b')' => {
                    self.i += 1;
                    return Ok(v);
                }
                _ => return self.err("expected ',' or ')'"),
            }
        }
    }
    fn param(&mut self) -> Result<P, String> {
        self.ws();
        let c = self.peek();
        match c {
            b'$' | b'*' => {
                self.i += 1;
                Ok(P::Null)
            }
            b'\'' => Ok(P::Str(self.string())),
            b'"' => {
                self.i += 1;
                let st = self.i;
                while self.i < self.s.len() && self.s[self.i] != b'"' {
                    self.i += 1;
                }
                let s = String::from_utf8_lossy(&self.s[st..self.i]).into_owned();
                self.i += 1;
                Ok(P::Str(s))
            }
            b'#' => {
                self.i += 1;
                Ok(P::Ref(self.int()?))
            }
            b'(' => {
                self.i += 1;
                Ok(P::List(self.params()?))
            }
            b'.' => {
                if self.i + 1 < self.s.len() && self.s[self.i + 1].is_ascii_alphabetic() {
                    self.i += 1;
                    let e = self.ident();
                    self.expect(b'.')?;
                    Ok(P::Enum(e))
                } else {
                    Ok(P::Num(self.number()?))
                }
            }
            b'0'..=b'9' | b'-' | b'+' => Ok(P::Num(self.number()?)),
            c if c.is_ascii_alphabetic() => {
                let name = self.ident();
                self.ws();
                if self.peek() == b'(' {
                    self.i += 1;
                    Ok(P::Typed(name, self.params()?))
                } else {
                    Ok(P::Enum(name))
                }
            }
            _ => self.err("unexpected character"),
        }
    }
    fn starts_with(&self, t: &str) -> bool {
        self.s[self.i..].starts_with(t.as_bytes())
    }
    fn skip_past(&mut self, c: u8) {
        let mut in_str = false;
        while self.i < self.s.len() {
            let ch = self.s[self.i];
            self.i += 1;
            if ch == b'\'' {
                in_str = !in_str;
            } else if ch == c && !in_str {
                return;
            }
        }
    }
}

pub fn parse(text: &str) -> Result<StepFile, String> {
    let clean = strip_comments(text);
    let start = clean.find("DATA;").ok_or("STEP file has no DATA section")? + 5;
    let mut lx = Lx { s: clean.as_bytes(), i: start };
    let mut ents = HashMap::new();
    loop {
        lx.ws();
        if lx.i >= lx.s.len() || lx.starts_with("ENDSEC") {
            break;
        }
        if lx.peek() != b'#' {
            lx.skip_past(b';');
            continue;
        }
        lx.i += 1;
        let id = lx.int()?;
        lx.expect(b'=')?;
        lx.ws();
        let parts = if lx.peek() == b'(' {
            lx.i += 1;
            let mut v = Vec::new();
            loop {
                lx.ws();
                if lx.peek() == b')' {
                    lx.i += 1;
                    break;
                }
                let name = lx.ident();
                if name.is_empty() {
                    return lx.err("expected entity name in complex entity");
                }
                lx.expect(b'(')?;
                v.push((name, lx.params()?));
            }
            v
        } else {
            let name = lx.ident();
            lx.expect(b'(')?;
            vec![(name, lx.params()?)]
        };
        lx.ws();
        if lx.peek() == b';' {
            lx.i += 1;
        }
        ents.insert(id, Ent { parts });
    }
    Ok(StepFile { ents })
}
