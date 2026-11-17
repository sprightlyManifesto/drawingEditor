//! Application configuration (config.json) and drawing specification types.

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Projection {
    #[serde(alias = "3rd", alias = "third_angle")]
    Third,
    #[serde(alias = "1st", alias = "first_angle")]
    First,
}

/// How smooth (tangent) edges are drawn, e.g. where a fillet meets the face
/// it blends into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TangentEdges {
    /// Not drawn.
    None,
    /// Thin grey witness lines (default).
    Thin,
    /// Like any other visible edge.
    Visible,
}

impl<'de> Deserialize<'de> for TangentEdges {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // "none" | "thin" | "visible", or true (visible) / false (none)
        match serde_json::Value::deserialize(d)? {
            serde_json::Value::Bool(b) => Ok(if b { TangentEdges::Visible } else { TangentEdges::None }),
            serde_json::Value::String(s) => match s.to_ascii_lowercase().as_str() {
                "none" | "off" | "hide" | "hidden" => Ok(TangentEdges::None),
                "thin" | "witness" | "grey" | "gray" => Ok(TangentEdges::Thin),
                "visible" | "on" | "show" => Ok(TangentEdges::Visible),
                o => Err(serde::de::Error::custom(format!("tangent_edges: expected \"none\", \"thin\" or \"visible\", not \"{}\"", o))),
            },
            o => Err(serde::de::Error::custom(format!("tangent_edges: expected \"none\", \"thin\" or \"visible\", not {}", o))),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Add an invisible layer of pickable faces / edges / vertices to each
    /// view (for the drawing editor; not read from config.json).
    #[serde(skip)]
    pub pick_layer: bool,
    /// Limit deviations of named fits (H7, g6, ...): named_fits.json.
    #[serde(skip)]
    pub fits: std::sync::Arc<NamedFits>,
    pub projection: Projection,
    pub font_family: String,
    /// Dimension / note text height (mm).
    pub text_height: f64,
    /// Title block label text height (mm).
    pub label_height: f64,
    pub line_visible: f64,
    pub line_hidden: f64,
    pub line_thin: f64,
    pub line_border: f64,
    pub hidden_lines: bool,
    pub centre_lines: bool,
    /// Smooth (tangent) edges, e.g. where a fillet meets a face: "thin"
    /// (default, thin grey witness lines), "none" or "visible".
    pub tangent_edges: TangentEdges,
    pub arrow_length: f64,
    pub arrow_width: f64,
    /// Distance from the geometry to the first dimension line.
    pub dim_offset: f64,
    /// Distance between stacked dimension lines.
    pub dim_spacing: f64,
    pub ext_gap: f64,
    pub ext_overshoot: f64,
    pub decimals: usize,
    pub trim_zeros: bool,
    pub angle_decimals: usize,
    /// Minimum clear space between views (mm).
    pub view_gap: f64,
    pub frame_height: f64,
    /// Length of the horizontal shoulder between a GD&T symbol and its leader.
    pub leader_shoulder: f64,
    /// Section hatching: angle (deg) and line spacing (mm).
    pub hatch_angle: f64,
    pub hatch_spacing: f64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            pick_layer: false,
            fits: Default::default(),
            projection: Projection::Third,
            font_family: "osifont, 'ISOCPEUR', 'DejaVu Sans', Arial, sans-serif".into(),
            text_height: 3.5,
            label_height: 2.0,
            line_visible: 0.5,
            line_hidden: 0.25,
            line_thin: 0.25,
            line_border: 0.7,
            hidden_lines: false,
            centre_lines: true,
            tangent_edges: TangentEdges::Thin,
            arrow_length: 3.0,
            arrow_width: 1.0,
            dim_offset: 10.0,
            dim_spacing: 8.0,
            ext_gap: 1.0,
            ext_overshoot: 2.0,
            decimals: 2,
            trim_zeros: true,
            angle_decimals: 1,
            view_gap: 20.0,
            frame_height: 7.0,
            leader_shoulder: 4.0,
            hatch_angle: 45.0,
            hatch_spacing: 3.0,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct TitleBlock {
    pub drawing_number: String,
    pub description: String,
    pub material: String,
    pub finish: String,
    pub notes: String,
    pub revision: String,
    pub issue: String,
    /// General tolerances box (default "UNLESS STATED: X.X ±0.4  X.XX ±0.15  ANGLES ±0.5°").
    pub general_tolerances: Option<String>,
    pub drawn_by: String,
    pub date: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Tolerance {
    /// ±value
    Sym(f64),
    /// {"type": "symmetric" | "asymmetric" | "limits" | "basic" | "reference", ...}
    Spec(TolSpec),
    /// free text; "basic" / "reference" are recognised
    Text(String),
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct TolSpec {
    /// symmetric | asymmetric (default when plus/minus are given) | limits | basic | reference
    #[serde(rename = "type")]
    pub kind: Option<String>,
    /// symmetric: the ± value
    pub value: Option<f64>,
    /// signed deviations from the nominal (asymmetric, limits)
    pub plus: Option<f64>,
    pub minus: Option<f64>,
    /// limits given directly
    pub upper: Option<f64>,
    pub lower: Option<f64>,
}

/// A string or a list of strings.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

impl OneOrMany {
    pub fn list(&self) -> Vec<String> {
        match self {
            OneOrMany::One(s) => s.split_whitespace().map(String::from).collect(),
            OneOrMany::Many(v) => v.clone(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct DimSpec {
    /// linear | diameter | radius | angle | ordinate
    #[serde(rename = "type")]
    pub kind: String,
    pub from: Option<String>,
    pub to: Option<String>,
    /// Linear: where the dimension meets a circle / cylinder at `from` / `to`
    /// ("fromPoint" / "toPoint"): "center" (default), "max" (its far side: the
    /// largest distance) or "min" (its near side: the smallest).
    #[serde(rename = "fromPoint")]
    pub from_point: Option<String>,
    #[serde(rename = "toPoint")]
    pub to_point: Option<String>,
    /// Feature for diameter / radius.
    pub of: Option<String>,
    /// Linear (and side-on diameter / radius): arrows the other way round:
    /// outside the extension lines pointing in where they would be inside,
    /// inside pointing out where they would be outside.
    #[serde(rename = "flipArrows")]
    pub flip_arrows: bool,
    /// Ordinate: the zero point, and the features dimensioned from it.
    pub origin: Option<OrdRef>,
    pub targets: Vec<OrdRef>,
    /// horizontal | vertical | aligned (linear); x | y (ordinate)
    pub direction: Option<String>,
    /// above | below | left | right
    pub side: Option<String>,
    pub text: Option<String>,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub tolerance: Option<Tolerance>,
    pub decimals: Option<usize>,
    /// Name other annotations can refer to with "dimension": "<id>".
    pub id: Option<String>,
    /// Feature control frames attached to this dimension (no "attach" needed).
    pub gdt: Vec<GdtSpec>,
    /// Datum feature symbol attached to this dimension (feature of size).
    pub datum: Option<String>,
    /// Lines merged under the dimension text (notes, thread callouts merged into it).
    #[serde(skip)]
    pub extra: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct GdtSpec {
    /// Geometry selector the frame's leader points to ...
    pub attach: String,
    /// ... or the id of a dimension the frame is attached to.
    pub dimension: Option<String>,
    /// straightness, flatness, circularity, cylindricity, profile_line,
    /// profile_surface, angularity, perpendicularity, parallelism, position,
    /// concentricity, symmetry, circular_runout, total_runout
    pub characteristic: String,
    pub tolerance: serde_json::Value,
    pub diameter: bool,
    /// Modifiers after the tolerance value: M (MMC), L (LMC), F (free state),
    /// P (projected zone, e.g. "P12"), T (tangent plane), U (unequal profile, e.g. "U0.2"),
    /// S (RFS), E (envelope), ST (statistical), or plain text such as CZ. A string or a list.
    pub modifier: Option<OneOrMany>,
    /// Datum references; modifiers in brackets, e.g. "A", "B(M)", "A-B", "C(L)(P)"
    pub datums: Vec<String>,
    /// All around: a circle at the leader's bend (the tolerance applies all
    /// around the profile in the view). Only for a frame on a leader.
    pub all_around: bool,
    /// Datum feature symbol attached to the frame (the frame's feature is that datum).
    pub datum: Option<String>,
    /// Name datum symbols (and merged items) can refer to with "gdt": "<id>".
    pub id: Option<String>,
    /// Merged onto the leader of the dimension, frame, note or thread with this id.
    pub merge: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct DatumSpec {
    pub label: String,
    /// Geometry selector, or
    pub attach: String,
    /// the id of a dimension (datum feature of size), or
    pub dimension: Option<String>,
    /// the id of a GD&T frame the symbol is attached to.
    pub gdt: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct DatumTargetSpec {
    /// e.g. "A1"
    pub label: String,
    /// Surface / edge / vertex the target lies on.
    pub attach: String,
    /// point | line | area
    pub kind: String,
    /// Target area size shown in the upper half, e.g. "Ø6" (also sets the drawn size).
    pub size: Option<String>,
    /// Exact target location in model coordinates (optional).
    pub point: Option<[f64; 3]>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct NoteSpec {
    pub text: String,
    pub attach: Option<String>,
    pub boxed: bool,
    /// Name other items can be merged onto with "merge": "<id>".
    pub id: Option<String>,
    /// Merged onto the leader of the dimension, frame, note or thread with this id.
    pub merge: Option<String>,
}

impl Default for NoteSpec {
    fn default() -> Self {
        NoteSpec { text: String::new(), attach: None, boxed: true, id: None, merge: None }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Annotations {
    pub dimensions: Vec<DimSpec>,
    pub gdt: Vec<GdtSpec>,
    pub datums: Vec<DatumSpec>,
    pub datum_targets: Vec<DatumTargetSpec>,
    pub notes: Vec<NoteSpec>,
    pub threads: Vec<ThreadSpec>,
}

/// A cosmetic thread on a hole (internal) or a shaft (external).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct ThreadSpec {
    /// The threaded cylindrical face (or a circular edge of it).
    pub of: String,
    /// Designation: "M6", "M10x1.25" (sets the major diameter and pitch).
    pub size: Option<String>,
    /// Major diameter (mm) when there is no metric size.
    pub major: Option<f64>,
    pub pitch: Option<f64>,
    /// Thread length from the open end (default: the whole hole / shaft).
    pub depth: Option<f64>,
    /// Also every alike hole / shaft: same diameter, length and direction.
    pub all_alike: bool,
    /// Leader note with the designation (default true).
    pub callout: Option<bool>,
    /// Callout text instead of the automatic one.
    pub text: Option<String>,
    /// "internal" or "external" (default: hole or shaft, from the face).
    pub kind: Option<String>,
    /// Name other items can be merged onto with "merge": "<id>".
    pub id: Option<String>,
    /// Callout merged onto the leader of the dimension, frame, note or thread with this id.
    pub merge: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DetailSpec {
    /// Detail letter, e.g. "D" -> "DETAIL D (2:1)".
    pub label: String,
    pub name: Option<String>,
    /// Centre of the detail: a selector (its reference point) ...
    pub center: Option<String>,
    /// ... or a model point (default: the centre of the part).
    pub point: Option<[f64; 3]>,
    /// Radius of the detail circle in model units (mm); for an ellipse the
    /// semi-axis along `angle`. Default: about an eighth of the part's size.
    #[serde(default)]
    pub radius: f64,
    /// "circle" (default) or "ellipse".
    pub shape: Option<String>,
    /// Ellipse: the other semi-axis (mm), square to `angle` (default 0.6 x radius).
    pub radius2: Option<f64>,
    /// Ellipse: angle of the `radius` axis on the sheet, degrees anticlockwise from horizontal.
    pub angle: Option<f64>,
    /// Scale of the detail view (default: twice the parent's).
    pub scale: Option<f64>,
    /// Sheet position of the detail centre (mm from bottom-left); default:
    /// nearest free space.
    pub location: Option<Vec<f64>>,
    pub hidden_lines: Option<bool>,
    /// Draw the detail circle as the boundary of the detail view (default true).
    pub border: Option<bool>,
    /// Move of the "DETAIL D (2:1)" label from its default place under the view [x, y] (sheet mm).
    pub title_offset: Option<[f64; 2]>,
    #[serde(flatten)]
    pub ann: Annotations,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SectionSpec {
    /// Section letter, e.g. "A" -> "SECTION A-A".
    pub label: String,
    pub name: Option<String>,
    /// Side of the parent view the section is looked at from (as for
    /// projected views): right | left | top | bottom | rear. The cutting plane
    /// is normal to that direction.
    pub position: String,
    /// The cutting plane passes through this feature (selector) ...
    pub through: Option<String>,
    /// ... or through this model point (default: the centre of the part).
    pub point: Option<[f64; 3]>,
    /// Hidden lines behind the cut (default false).
    pub hidden_lines: Option<bool>,
    /// Distance (sheet mm) from the parent view's origin along the projection
    /// axis; default: next to the parent, clear of other views.
    pub distance: Option<f64>,
    pub hatch_angle: Option<f64>,
    pub hatch_spacing: Option<f64>,
    /// Move of the "SECTION A-A" label from its default place under the view [x, y] (sheet mm).
    pub title_offset: Option<[f64; 2]>,
    /// Views projected from the section view, aligned with it (not sectioned).
    #[serde(default)]
    pub projected: Vec<ProjectedSpec>,
    #[serde(default)]
    pub details: Vec<DetailSpec>,
    #[serde(flatten)]
    pub ann: Annotations,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProjectedSpec {
    pub name: Option<String>,
    /// Move of the view's "SCALE: 2:1" label (when not at the sheet scale) from under the view [x, y] (sheet mm).
    pub title_offset: Option<[f64; 2]>,
    /// top | bottom | left | right | rear : the side the view is looked at from.
    pub position: String,
    pub hidden_lines: Option<bool>,
    /// Distance (sheet mm) from the parent view's origin along the projection
    /// axis; default: next to the parent, clear of other views.
    pub distance: Option<f64>,
    /// Views projected from this one, aligned with it.
    #[serde(default)]
    pub projected: Vec<ProjectedSpec>,
    #[serde(default)]
    pub sections: Vec<SectionSpec>,
    #[serde(default)]
    pub details: Vec<DetailSpec>,
    #[serde(flatten)]
    pub ann: Annotations,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ViewSpec {
    pub name: String,
    /// Move of the view's "SCALE: 2:1" label (when not at the sheet scale) from under the view [x, y] (sheet mm).
    pub title_offset: Option<[f64; 2]>,
    /// View direction: vector from the part towards the viewer.
    pub direction: [f64; 3],
    /// Vector that should point up on the sheet.
    pub up: Option<[f64; 3]>,
    /// Sheet position (mm from bottom-left corner) of the view centre. The z
    /// component, if given, is ignored.
    pub location: Option<Vec<f64>>,
    #[serde(default = "one")]
    pub scale: f64,
    pub hidden_lines: Option<bool>,
    #[serde(default)]
    pub projected: Vec<ProjectedSpec>,
    #[serde(default)]
    pub sections: Vec<SectionSpec>,
    #[serde(default)]
    pub details: Vec<DetailSpec>,
    #[serde(flatten)]
    pub ann: Annotations,
}

fn one() -> f64 {
    1.0
}

/// Limit deviations of named fits (ISO 286 tolerance classes), by size range.
#[derive(Debug, Clone, Deserialize)]
pub struct NamedFits {
    pub fits: std::collections::HashMap<String, Vec<FitRange>>,
}

/// Deviations (mm) of a fit for nominal sizes over `over` up to and including `to`.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct FitRange {
    pub over: f64,
    pub to: f64,
    pub upper: f64,
    pub lower: f64,
}

/// The copy of named_fits.json built into stepdraw.
pub const BUILTIN_FITS: &str = include_str!("../named_fits.json");

impl Default for NamedFits {
    fn default() -> Self {
        serde_json::from_str(BUILTIN_FITS).expect("built-in named_fits.json")
    }
}

impl NamedFits {
    /// (upper, lower) deviations of fit `name` (case matters: H7 hole, h7 shaft) for nominal size `size`.
    pub fn lookup(&self, name: &str, size: f64) -> Option<(f64, f64)> {
        let s = size.abs();
        self.fits.get(name)?.iter().find(|r| (r.over < s || (r.over == 0.0 && s == 0.0)) && s <= r.to + 1e-9).map(|r| (r.upper, r.lower))
    }
}

/// Sheet size: "A0" .. "A4", landscape (default) or portrait.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SheetSpec {
    pub size: String,
    pub orientation: String,
    /// false: no drawing border and no title block
    pub border: bool,
}

impl Default for SheetSpec {
    fn default() -> Self {
        SheetSpec { size: "A3".into(), orientation: "landscape".into(), border: true }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct DrawingSpec {
    pub step_file: Option<String>,
    pub projection: Option<Projection>,
    /// Sheet size and orientation (default A3 landscape).
    pub sheet: Option<SheetSpec>,
    #[serde(default, alias = "title_block")]
    pub title: TitleBlock,
    pub views: Vec<ViewSpec>,
}

/// A feature of an ordinate dimension: a selector, or `{ "of": selector,
/// "gap": mm }` with the gap between the feature and the start of its leader
/// (sheet mm; default config `ext_gap`).
#[derive(Debug, Clone, Default)]
pub struct OrdRef {
    pub of: String,
    pub gap: Option<f64>,
}

impl<'de> Deserialize<'de> for OrdRef {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Sel(String),
            Obj { of: String, #[serde(default)] gap: Option<f64> },
        }
        Ok(match Raw::deserialize(d).map_err(|_| serde::de::Error::custom("an ordinate feature is a selector or { \"of\": selector, \"gap\": mm }"))? {
            Raw::Sel(of) => OrdRef { of, gap: None },
            Raw::Obj { of, gap } => OrdRef { of, gap },
        })
    }
}
