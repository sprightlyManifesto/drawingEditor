//! `stepdraw serve`: a small local web server for the drawing editor.
//!
//! GET  /              the editor page
//! GET  /api/files     drawing JSON files under the folder
//! GET  /api/help      the drawing JSON reference (docs/drawing-json.md, Markdown)
//! POST /api/render    {"path": "<file under the folder>"} or
//!                     {"json": "<drawing JSON text>", "name": "<file name>"}
//!                     -> {"svg", "warnings", "name", "path", "spec"} or {"error"}
//!                     ("path" and "json" together: render the text, unsaved, as if it
//!                     were that file, for previews)
//! POST /api/selector  {"path" | "json", "kind": "faces|edges|vertices", "ids": [..]}
//!                     -> {"selector"}
//! POST /api/resolve   {"path" | "json", "selectors": [..]}
//!                     -> {"results": {"<selector>": {"kind", "ids"} | {"error"}}}
//! POST /api/save      {"path": "<drawing JSON under the folder>", "json": "<new text>"}
//!                     -> {"ok": true, "path"} or {"error"}
//!                     with "create": true (save as) the file may be new; then
//!                     -> {"exists": true} if it exists and "overwrite" isn't set,
//!                     -> {"step_file": "<path>"} if the drawing's STEP file wouldn't be
//!                        found from there (the path that would find it)
//!
//! The server only listens on 127.0.0.1, only reads files inside the folder,
//! and rejects requests whose Host is not local (DNS rebinding).

use crate::config::DrawingSpec;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

/// The last model loaded for selector requests (STEP path, modified time).
static MODEL: Mutex<Option<(PathBuf, Option<SystemTime>, crate::brep::Model)>> = Mutex::new(None);

const EDITOR: &str = include_str!("../tools/editor.html");
/// The drawing JSON reference, shown in the editor's Help panel.
const HELP: &str = include_str!("../docs/drawing-json.md");
const MAX_BODY: usize = 8 << 20;

pub fn serve(dir: &Path, port: u16, cfg_path: Option<&str>) -> Result<(), String> {
    let root = dir.canonicalize().map_err(|e| format!("{}: {}", dir.display(), e))?;
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("cannot listen on 127.0.0.1:{}: {}", port, e))?;
    eprintln!("stepdraw editor: http://127.0.0.1:{}/  (drawings in {})", port, root.display());
    eprintln!("press Ctrl+C to stop");
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        if let Err(e) = handle(stream, &root, port, cfg_path) {
            eprintln!("request failed: {}", e);
        }
    }
    Ok(())
}

struct Request {
    method: String,
    path: String,
    host: String,
    body: Vec<u8>,
}

fn read_request(stream: &TcpStream) -> Result<Request, String> {
    let mut r = BufReader::new(stream);
    let mut line = String::new();
    r.read_line(&mut line).map_err(|e| e.to_string())?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("/").to_string();
    let (mut len, mut host) = (0usize, String::new());
    loop {
        let mut h = String::new();
        if r.read_line(&mut h).map_err(|e| e.to_string())? == 0 || h.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            match k.trim().to_ascii_lowercase().as_str() {
                "content-length" => len = v.trim().parse().unwrap_or(0),
                "host" => host = v.trim().to_ascii_lowercase(),
                _ => {}
            }
        }
    }
    if len > MAX_BODY {
        return Err("request too large".into());
    }
    let mut body = vec![0; len];
    r.read_exact(&mut body).map_err(|e| e.to_string())?;
    Ok(Request { method, path, host, body })
}

fn respond(mut s: &TcpStream, status: &str, ctype: &str, body: &[u8]) -> Result<(), String> {
    let head = format!(
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        status,
        ctype,
        body.len()
    );
    s.write_all(head.as_bytes()).and_then(|_| s.write_all(body)).map_err(|e| e.to_string())
}

fn respond_json(s: &TcpStream, status: &str, v: &Value) -> Result<(), String> {
    respond(s, status, "application/json; charset=utf-8", v.to_string().as_bytes())
}

fn handle(stream: TcpStream, root: &Path, port: u16, cfg_path: Option<&str>) -> Result<(), String> {
    let req = read_request(&stream)?;
    let local = [format!("127.0.0.1:{}", port), format!("localhost:{}", port)];
    if !local.contains(&req.host) {
        return respond(&stream, "403 Forbidden", "text/plain", b"forbidden host");
    }
    let path = req.path.split('?').next().unwrap_or("/");
    match (req.method.as_str(), path) {
        ("GET", "/") | ("GET", "/index.html") => respond(&stream, "200 OK", "text/html; charset=utf-8", EDITOR.as_bytes()),
        ("GET", "/api/help") => respond(&stream, "200 OK", "text/markdown; charset=utf-8", HELP.as_bytes()),
        ("GET", "/api/files") => respond_json(&stream, "200 OK", &json!({ "folder": root.display().to_string(), "files": list_drawings(root) })),
        ("POST", "/api/resolve") => {
            let v = match resolve_request(root, &req.body) {
                Ok(v) => v,
                Err(e) => json!({ "error": e }),
            };
            respond_json(&stream, "200 OK", &v)
        }
        ("POST", "/api/selector") => {
            let v = match selector_request(root, &req.body) {
                Ok(s) => json!({ "selector": s }),
                Err(e) => json!({ "error": e }),
            };
            respond_json(&stream, "200 OK", &v)
        }
        ("POST", "/api/save") => {
            let v = match save_request(root, &req.body) {
                Ok(v) => v,
                Err(e) => json!({ "error": e }),
            };
            respond_json(&stream, "200 OK", &v)
        }
        ("POST", "/api/render") => {
            let v = match render_request(root, cfg_path, &req.body) {
                Ok(v) => v,
                Err(e) => json!({ "error": e }),
            };
            respond_json(&stream, "200 OK", &v)
        }
        _ => respond(&stream, "404 Not Found", "text/plain", b"not found"),
    }
}

/// JSON files under `root` that look like drawing specs (have "views").
fn list_drawings(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0)];
    while let Some((d, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }
            if p.is_dir() {
                if depth < 4 {
                    stack.push((p, depth + 1));
                }
            } else if name.ends_with(".json")
                && std::fs::read_to_string(&p).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).is_some_and(|v| v.get("views").is_some())
                && let Ok(rel) = p.strip_prefix(root)
            {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    out.sort();
    out
}

/// `rel` resolved inside `root`, or an error if it is outside or missing.
fn inside(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let p = root.join(rel).canonicalize().map_err(|_| format!("{} not found", rel))?;
    if !p.starts_with(root) {
        return Err(format!("{} is outside the served folder", rel));
    }
    Ok(p)
}

/// A file named `name` anywhere under `root` (for uploaded drawings).
fn find_by_name(root: &Path, name: &str) -> Option<PathBuf> {
    let mut stack = vec![(root.to_path_buf(), 0)];
    while let Some((d, depth)) = stack.pop() {
        for e in std::fs::read_dir(&d).ok()?.flatten() {
            let p = e.path();
            let n = e.file_name().to_string_lossy().to_string();
            if n.starts_with('.') || n == "target" {
                continue;
            }
            if p.is_dir() && depth < 4 {
                stack.push((p, depth + 1));
            } else if n == name {
                return Some(p);
            }
        }
    }
    None
}

/// The drawing a request refers to: its text, spec, STEP file, name and path.
struct Drawing {
    text: String,
    spec: DrawingSpec,
    step: PathBuf,
    name: String,
    rel: Option<String>,
}

fn drawing_of(root: &Path, req: &Value) -> Result<Drawing, String> {
    // the drawing JSON, the folder its relative paths start from, and its name
    let path = req.get("path").and_then(Value::as_str);
    let json = req.get("json").and_then(Value::as_str);
    let (text, base, name, rel) = if let Some(rel) = path {
        let p = inside(root, rel)?;
        let t = match json {
            Some(t) => t.to_string(), // unsaved text of that file (preview)
            None => std::fs::read_to_string(&p).map_err(|e| format!("{}: {}", rel, e))?,
        };
        (t, p.parent().unwrap_or(root).to_path_buf(), rel.rsplit('/').next().unwrap_or(rel).to_string(), Some(rel.to_string()))
    } else if let Some(t) = json {
        let name = req.get("name").and_then(Value::as_str).unwrap_or("drawing.json").to_string();
        (t.to_string(), root.to_path_buf(), name, None)
    } else {
        return Err("bad request: expected \"path\" or \"json\"".into());
    };
    let spec: DrawingSpec = serde_json::from_str(&text).map_err(|e| format!("{}: {}", name, e))?;
    let step_rel = spec.step_file.clone().ok_or("the drawing has no \"step_file\"")?;
    let step = match base.join(&step_rel).canonicalize() {
        Ok(p) if p.starts_with(root) => p,
        _ if rel.is_none() => {
            // an uploaded drawing: look for its STEP file by name in the folder
            let fname = Path::new(&step_rel).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
            find_by_name(root, &fname).ok_or_else(|| format!("STEP file '{}' not found in {}", step_rel, root.display()))?
        }
        _ => return Err(format!("STEP file '{}' not found inside the served folder", step_rel)),
    };
    Ok(Drawing { text, spec, step, name, rel })
}

fn render_request(root: &Path, cfg_path: Option<&str>, body: &[u8]) -> Result<Value, String> {
    let req: Value = serde_json::from_slice(body).map_err(|e| format!("bad request: {}", e))?;
    let d = drawing_of(root, &req)?;
    let mut cfg = crate::load_config(cfg_path)?;
    cfg.pick_layer = true;
    let (svg, warnings) = crate::render(&d.spec, &d.step, &cfg)?;
    eprintln!("rendered {} ({} warnings)", d.name, warnings.len());
    Ok(json!({ "svg": svg, "warnings": warnings, "name": d.name, "path": d.rel, "spec": d.text }))
}

/// A selector for picked model items.
fn selector_request(root: &Path, body: &[u8]) -> Result<String, String> {
    use crate::selector::Kind;
    let req: Value = serde_json::from_slice(body).map_err(|e| format!("bad request: {}", e))?;
    let d = drawing_of(root, &req)?;
    let kind = match req.get("kind").and_then(Value::as_str) {
        Some("faces") => Kind::Face,
        Some("edges") => Kind::Edge,
        Some("vertices") => Kind::Vertex,
        _ => return Err("bad request: \"kind\" must be faces, edges or vertices".into()),
    };
    let ids: Vec<usize> = req.get("ids").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).map(|i| i as usize).collect()).unwrap_or_default();
    with_model(&d.step, |m| {
        let n = match kind {
            Kind::Face => m.faces.len(),
            Kind::Edge => m.edges.len(),
            Kind::Vertex => m.verts.len(),
        };
        if ids.iter().any(|&i| i >= n) {
            return Err("the model has changed; re-render the drawing".into());
        }
        Ok(crate::selector::find_selector(m, kind, &ids))
    })
}

/// Run `f` on the model of `step`, loaded once and kept while the file is unchanged.
fn with_model<T>(step: &Path, f: impl FnOnce(&crate::brep::Model) -> Result<T, String>) -> Result<T, String> {
    let mtime = std::fs::metadata(step).and_then(|m| m.modified()).ok();
    let mut cache = MODEL.lock().map_err(|_| "model cache poisoned".to_string())?;
    if !cache.as_ref().is_some_and(|(p, t, _)| p == step && *t == mtime) {
        *cache = Some((step.to_path_buf(), mtime, crate::load_model(step)?));
    }
    f(&cache.as_ref().unwrap().2)
}

/// The model items each selector picks (for highlighting in the editor).
fn resolve_request(root: &Path, body: &[u8]) -> Result<Value, String> {
    use crate::selector::Kind;
    let req: Value = serde_json::from_slice(body).map_err(|e| format!("bad request: {}", e))?;
    let d = drawing_of(root, &req)?;
    let sels: Vec<String> = req.get("selectors").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect()).unwrap_or_default();
    with_model(&d.step, |m| {
        let mut out = serde_json::Map::new();
        for s in sels.iter().take(200) {
            let v = match crate::selector::select(m, s) {
                Ok(sel) => {
                    let kind = match sel.kind {
                        Kind::Face => "faces",
                        Kind::Edge => "edges",
                        Kind::Vertex => "vertices",
                    };
                    // a face also lights up its edges (in views where it is seen edge-on)
                    let mut edges: Vec<usize> = if kind == "faces" { sel.ids.iter().flat_map(|&f| m.faces[f].edges.iter().copied()).collect() } else { vec![] };
                    edges.sort();
                    edges.dedup();
                    json!({ "kind": kind, "ids": sel.ids, "edges": edges })
                }
                Err(e) => json!({ "error": e }),
            };
            out.insert(s.clone(), v);
        }
        Ok(json!({ "results": out }))
    })
}

/// Write an edited drawing JSON back to its file (only an existing drawing
/// JSON inside the folder, and only valid drawing JSON).
fn save_request(root: &Path, body: &[u8]) -> Result<Value, String> {
    let req: Value = serde_json::from_slice(body).map_err(|e| format!("bad request: {}", e))?;
    let rel = req.get("path").and_then(Value::as_str).ok_or("bad request: expected \"path\"")?;
    let text = req.get("json").and_then(Value::as_str).ok_or("bad request: expected \"json\"")?;
    let flag = |k: &str| req.get(k).and_then(Value::as_bool).unwrap_or(false);
    if !rel.ends_with(".json") {
        return Err(format!("{} is not a .json file", rel));
    }
    let spec: DrawingSpec = serde_json::from_str(text).map_err(|e| format!("not saved, the drawing JSON is invalid: {}", e))?;
    let p = if flag("create") {
        // a new file: its folder must exist inside the served folder
        let r = Path::new(rel);
        let fname = r.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
        if fname.is_empty() || fname.starts_with('.') || r.is_absolute() || r.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
            return Err(format!("'{}' is not a usable file name", rel));
        }
        let dir = inside(root, r.parent().map(|d| d.to_string_lossy().to_string()).as_deref().filter(|d| !d.is_empty()).unwrap_or("."))?;
        let p = dir.join(&fname);
        if p.exists() && !flag("overwrite") {
            return Ok(json!({ "exists": true }));
        }
        if p.is_dir() {
            return Err(format!("{} is a folder", rel));
        }
        if let Some(step) = &spec.step_file
            && !dir.join(step).canonicalize().is_ok_and(|s| s.starts_with(root))
        {
            let sname = Path::new(step).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
            let found = find_by_name(root, &sname).ok_or_else(|| format!("STEP file '{}' not found in {}", step, root.display()))?;
            return Ok(json!({ "step_file": relative(&dir, &found) }));
        }
        p
    } else {
        inside(root, rel)?
    };
    std::fs::write(&p, text).map_err(|e| format!("{}: {}", rel, e))?;
    let saved = p.canonicalize().ok().and_then(|c| c.strip_prefix(root).ok().map(|r| r.to_string_lossy().replace('\\', "/"))).unwrap_or_else(|| rel.to_string());
    eprintln!("saved {}", saved);
    Ok(json!({ "ok": true, "path": saved }))
}

/// `to` relative to the folder `from` (both absolute), with `/` separators.
fn relative(from: &Path, to: &Path) -> String {
    let (a, b): (Vec<_>, Vec<_>) = (from.components().collect(), to.components().collect());
    let n = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let mut parts: Vec<String> = vec!["..".into(); a.len() - n];
    parts.extend(b[n..].iter().map(|c| c.as_os_str().to_string_lossy().to_string()));
    parts.join("/")
}
