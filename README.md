# stepdraw

Headless generator of A3 engineering drawings (SVG) from a STEP part and a JSON drawing description.

```
cargo build --release
./target/release/stepdraw examples/bracket.json -o examples/bracket.svg
./target/release/stepdraw drawing.json part.step -o out.svg -c config.json
./target/release/stepdraw serve examples        # drawing editor at http://127.0.0.1:8080/
```

The STEP file can be passed as the second argument or as `"step_file"` in the JSON (relative to the JSON file).
`config.json` is read from `-c`, otherwise from the current directory, then next to the executable, otherwise built-in defaults.
Warnings (unmatched selectors, views that don't fit, etc.) go to stderr; the SVG is still written.

## Output

- ISO A0–A4 sheet, landscape or portrait (default A3 landscape), border with reference zones of about 50 mm: letters across the top and bottom, numbers down the sides (A–G and 1–5 on A3 landscape). A4 has narrower margins so the 180 mm title block fits.
- Title block (bottom right): drawing number, description, material, finish, notes, revision, drawn by, date, scale, projection symbol.
- One SVG group per view (`id` = view name), each containing `<name>-geometry`, `<name>-GDandT`, `<name>-dimensions`
  (also labelled `geometry`, `GDandT`, `dimensions` for Inkscape). Each view group is positioned by a single `translate`.

## Editing the layout

```
./target/release/stepdraw serve examples          # folder with the drawing JSON files; --port 8080 by default
```

Then open http://127.0.0.1:8080/ in a browser.
- **Opening a drawing:** pick a drawing JSON from the list (every `.json` with `views` in the folder), or open/drop one from elsewhere. An opened JSON's STEP file is looked up by name in the served folder. stepdraw renders it and the SVG is shown, with stepdraw's warnings behind the warnings button.
- **Re-render** (<kbd>R</kbd>): after editing the JSON, renders it again. View moves are kept by view name. Annotation moves are kept if the drawing still has the same annotations in the same order.
- **Selection mode:** the **Select** list sets what a click picks, and <kbd>Tab</kbd> / <kbd>Shift</kbd>+<kbd>Tab</kbd> cycles through the options:
  - **Annotations:** dimensions, frames, datums and notes.
  - **Views:** a whole view, by clicking anywhere in its area.
  - **Geometry:** individual faces, edges and points (hidden edges too, where hidden lines are drawn), never whole views or annotations.
    - Click one to select it; <kbd>Ctrl</kbd>+click adds or removes; click empty space or press <kbd>Esc</kbd> to clear.
    - The selection is highlighted in every view and listed in a panel, with a selector that matches exactly the selected items (copy it, or insert it in the JSON editor).
    - Dragging pans the sheet; views don't move in this mode.
    - **Add dimension / Add GD&T / Add datum / Add note** (in the selection panel) open the JSON editor on a new entry for the selection, in the view clicked last, with the selectors filled in. Apply adds it to that view's list.
      - **Dimension:** a circle, cylinder or cone gives a diameter, an arc a radius, a straight edge its length (between its end points), two straight edges an angle (or the distance between them if they look parallel), any other two items the distance between them, three or more equal holes `nX Ø<>`, and three or more other items ordinates from the first one picked.
      - **GD&T:** holes and bosses get position Ø0.1 Ⓜ to the drawing's datums; planes flatness (or parallelism to datum A once there are datums); straight edges straightness; cones circular runout; anything else surface profile.
      - **Datum:** the next free letter (I, O and Q are skipped). **Note:** `Ø5 THRU` / `2X Ø5 THRU` for holes, otherwise `NOTE`.
- **Moving views:** a view moves with its annotations. A projected or section view slides only along its projection axis and follows its parent view across it, so it stays aligned. Moving a base view takes its aligned views along. Detail views and other base views move freely.
- **Moving annotations:** dimensions, GD&T frames, datum symbols, datum targets and notes can be dragged.
  - **Linear and aligned dimensions:** the dimension line moves square to the feature, and the extension lines stretch to follow. The text slides along the line; if it goes beyond the extension lines, the dimension line is extended to it. <kbd>Shift</kbd>-drag moves only the line.
  - **Leaders:** the symbol moves, and the leader stays on its feature. On an edge seen edge-on, it slides along the edge, square to it. On a circle it stays radial (diameter and radius callouts too). On a face seen face-on, it keeps its point.
  - **Angle dimensions:** the arc changes radius.
  - **Ordinate labels:** each label moves with its jogged leader.
- **Editing an annotation's or a view's JSON:** right-click an annotation, or a view anywhere off its annotations, to open its entry from the drawing JSON in a code editor (Monaco, the VS Code editor), with:
  - **Completion** for property names, value lists (types, sides, characteristics, modifiers, tolerance types) and snippets (tolerances, common frames).
  - **Selector suggestions:** the selectors already used in the drawing, plus common ones.
  - **Hover help** for every property.
  - **Checking:** misspelt properties, bad values and missing required fields are marked.
  - **Comma fixing:** when you pause typing, commas missing at line ends are added and extra ones (before `}` / `]`, doubled) are removed, if that makes the JSON valid. A comma next to the cursor is left alone while you type. **Apply** always fixes them. <kbd>Ctrl</kbd>+<kbd>Z</kbd> undoes a fix.

  - **Preview:** beside the editor, the drawing is re-rendered with your edit as you type (not saved), zoomed to the annotation's view, with stepdraw's warnings. If the annotation can't be drawn, the preview says so.
  - **Adding projected views:** when editing a base or projected view, the four arrow buttons add a projected view on that side of it on the sheet (`"position"` follows the drawing's first / third angle `projection`; a side the view already has is greyed out). The preview shows the whole sheet with the new view.
  - **Selected geometry:** its selector comes first in the selector completions, and the **Insert** button puts it at the cursor.

  **Apply** (<kbd>Ctrl</kbd>+<kbd>Enter</kbd>) replaces just that entry in the drawing JSON, keeping the rest of its formatting, then re-renders, keeping your moves. The edit stays in the page (the file name shows "• unsaved") until you save it.
  - **Requirements:** the editor loads from cdn.jsdelivr.net on first use; offline, you get a plain text box without completion.
  - **Frames on a dimension:** a frame attached to a dimension is edited as part of that dimension. A frame that refers to a dimension by `id` is not shown there.
- **Title block and sheet:** click the title block (or right-click it) to edit its JSON. The same window has the scale of the first base view (the one the title block shows) as a number, and the sheet size (A0–A4) and orientation. All three show in the preview and are applied with **Apply**. Views with a fixed `location` keep it when the sheet size changes, so you may need to move them.
- **Deleting:** <kbd>Del</kbd> deletes the selected annotation or view (with everything in it) from the drawing JSON at once, without asking, and re-renders; the other items keep their moves. <kbd>Ctrl</kbd>+<kbd>Z</kbd> (or the Undo button) puts deleted items back, the last one first, in their old place in the JSON; moves made since the deletion are undone first. <kbd>Ctrl</kbd>+<kbd>Y</kbd> also puts deletions back (when there are none it redoes a move; <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>Z</kbd> always redoes). Deletions are unsaved edits like any other until you Save. The drawing's only view can't be deleted.
- **Editor controls:** undo/redo, double-click to reset an item, arrow keys to nudge, pan and zoom.
- **Saving the JSON:** **Save** (<kbd>Ctrl</kbd>+<kbd>S</kbd>) writes the edited drawing JSON back to its file. **Save as…** (<kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>S</kbd>) asks for a path inside the served folder (asking before replacing a file); if the drawing's `step_file` wouldn't be found from the new place, it is changed to a path that is. A JSON opened from elsewhere is saved with Save as. The browser warns before you leave the page or open another drawing with unsaved edits. View and annotation moves are not part of the JSON; they are kept in the downloaded SVG.
- **Saving the SVG:** **Download SVG** saves the edited drawing as `<drawing>.svg`. The offsets are stored in the file.
- **Linking:** `?drawing=<path>` in the URL opens that drawing straight away.
- **Security:** the server listens on 127.0.0.1 only, reads files only inside the served folder, and refuses requests that don't come from a local host name.
- **Changing the page:** the editor page (`tools/editor.html`) is built into the binary, so rebuild after changing it.

Selectors for picked geometry:
- **Shortest chain:** the server searches for the shortest chain of up to three steps (`%TYPE`, `>X`, `<Y[n]`, `|Z`, `+Z`, directions of the picked items, and `a or b` within a step) that selects exactly the picked items. Two holes, for example, give `edges(">Z").edges("<Y[1] or >Y[5]")`.
- **Fallback:** if no chain matches, it uses item numbers, e.g. `faces("@3 or @7")`. `@n` is a selector atom (the n-th face / edge / vertex of the STEP model); it is only stable while the STEP file doesn't change.
- **Pick layer:** renders done by the server carry an invisible pick layer (`<g class="pick">`) for this. The command line output and **Download SVG** don't include it.

Each annotation is written as `<g class="ann" data-kind="linear|leader|angle|ordinate|free" data-src="/views/0/dimensions/3">`; `data-src` is the JSON pointer of its entry in the drawing JSON. Its `data-` attributes record what it is attached to, in the view's coordinates.

## config.json

| key | default | meaning |
|---|---|---|
| `projection` | `"third"` | `"third"` or `"first"`; the drawing JSON may override |
| `text_height` | 3.5 | dimension / note text height (mm) |
| `hidden_lines`, `centre_lines` | true | default for every view |
| `dim_offset`, `dim_spacing` | 10, 8 | first dimension line distance from the view, spacing of stacked dimensions |
| `view_gap` | 20 | minimum gap between views |
| `decimals`, `trim_zeros`, `angle_decimals` | 2, true, 1 | number formatting |
| `hatch_angle`, `hatch_spacing` | 45, 3 | section hatching |
| `leader_shoulder` | 4 | horizontal shoulder between a GD&T symbol and its leader |
| `font_family`, line widths, arrow size, `frame_height` | | styling |

## Drawing JSON

```jsonc
{
  "step_file": "part.step",
  "projection": "third",                 // optional, overrides config
  "sheet": { "size": "A3", "orientation": "landscape" },   // optional: A0..A4, landscape | portrait
  "title": { "drawing_number": "", "description": "", "material": "", "finish": "",
             "notes": "", "revision": "", "drawn_by": "", "date": "" },
  "views": [
    {
      "name": "front",
      "direction": [0, -1, 0],            // from the part towards the viewer
      "up": [0, 0, 1],                    // optional; default Z (or Y for views along Z)
      "location": [120, 175],             // sheet mm from bottom-left of the view centre (z ignored); optional
      "scale": 1.5,
      "hidden_lines": true,
      "projected": [                      // views projected from this one, kept aligned
        { "name": "top", "position": "top", "dimensions": [], "gdt": [], "datums": [], "notes": [] }
      ],                                  // position: top | bottom | left | right | rear
      "dimensions": [], "gdt": [], "datums": [], "notes": []
    }
  ]
}
```

Projected views take the parent's scale and are placed according to the projection system:
3rd angle puts the view from the right on the right, the view from above on top; 1st angle swaps them.
A projected view can have its own `"projected"` list: those views are projected from it, aligned with it, and so on. For example, the view from the right of the top view (an auxiliary-style chain):

```json
"projected": [ { "position": "top", "projected": [ { "position": "right" } ] } ]
```

### Sections

Any view (base or projected) can have `"sections"`:

```json
"sections": [
  { "label": "A", "position": "left", "through": "faces(\"%CYLINDER\").faces(\"<X[1]\")",
    "dimensions": [ { "type": "diameter", "of": "faces(\"%CYLINDER\").faces(\"<X[1]\")" } ] }
]
```

- **`position`**: the side of the parent view the section is looked at from (`right`, `left`, `top`, `bottom`, `rear`), as for projected views. The cutting plane is square to that direction.
  The section is placed and aligned by the projection rules: in 3rd angle a section looked at from the left goes on the left; in 1st angle it goes on the right.
- **`through`**: a selector the plane passes through; its reference point is used, e.g. a hole's axis. Alternatively, `"point": [x, y, z]`.
- **Parent view**: shows the cutting-plane line (chain line with thick ends), arrows in the direction of sight, and the letter at each end.
- **Section view**:
  - Material in front of the plane is removed.
  - The cut face is outlined and hatched (`hatch_angle` / `hatch_spacing` in config.json, or per section).
  - Anything behind the cut face is hidden by it.
  - Hidden lines are off unless `"hidden_lines": true`.
  - The view is titled "SECTION A-A".
- **Annotations**: sections take `dimensions`, `gdt`, `datums`, `datum_targets` and `notes` like any other view, and their SVG group has the same three sub-groups.
- **Surface types**: the cut outline is computed for planes, cylinders, cones and spheres. A cut through a torus or B-spline face leaves a gap in the outline.

### Detail views

Any view (base, projected or section) can have `"details"`, which enlarge a circular region of that view:

```json
"details": [
  { "label": "D", "center": "faces(\">Z\").faces(\">(0.86603,0.5,0)\")", "radius": 24, "scale": 1.5,
    "dimensions": [ { "type": "radius", "of": "faces(\"%CYLINDER\").faces(\">Y\")", "text": "R<> TYP" } ] }
]
```

- **`center`**: a selector whose reference point is the centre of the detail, or `"point": [x, y, z]`.
- **`radius`**: the radius of the detail circle in model mm.
- **`scale`**: the detail's scale; the default is twice the parent's.
- **`location`**: optional sheet position (mm) of the detail centre. Without it, the detail goes in the free space nearest its circle.
- **Parent view**: shows a thin circle with the letter next to it, placed clear of the geometry.
- **Detail view**:
  - Shows the parent's geometry inside the circle, with hidden lines as in the parent.
  - A detail of a section keeps the section's cut and hatching.
  - It has a thin circular border (`"border": false` removes it) and is titled "DETAIL D (1.5:1)".
- **Annotations**: details take the same annotations as any other view, and their SVG group has the same three sub-groups (class `view detail`).

### Selectors (CadQuery syntax)

`faces(">X").edges("|Z").vertices(">X")`, with `faces`, `edges`, `vertices` chained.
Selector strings: `>X` `<Y` `>(1,1,0)` (max/min centre along a direction), `>Z[1]` / `<X[-1]` (n-th group),
`|Z` (parallel), `#Z` (perpendicular), `+Z` / `-Z` (normal/direction), `%PLANE` `%CYLINDER` `%CONE` `%SPHERE` `%TORUS`
`%LINE` `%CIRCLE` `%ELLIPSE` `%BSPLINE`, `@12` (item number, as shown by the editor), combined with `and`, `or`, `not`, `exc` and parentheses.

Set `STEPDRAW_DEBUG=1` to print what each selector matched (type, radius, centre) to stderr.

What a selected item contributes to a dimension: a vertex gives its point; a line edge gives its midpoint;
a circle/arc gives its centre; a planar face gives its centroid; a cylindrical face gives a point on its axis.

### Dimensions

| `type` | fields |
|---|---|
| `linear` | `from`, `to`, `direction`: `horizontal` / `vertical` / `aligned` (default: the dominant axis), `side`: `above`/`below`/`left`/`right` |
| `diameter`, `radius` | `of` (circle edge or cylindrical face). Seen end-on: leader to the circle. Seen side-on: linear dimension across the silhouette |
| `angle` | `from`, `to`: straight edges, or planar faces seen edge-on |
| `ordinate` | `origin`, `targets` [...], `direction`: `x`/`y`, `side` |

Common options: `text` (`<>` is replaced by the measured value), `prefix`, `suffix`, `decimals`, `tolerance`.

#### Tolerances

| `tolerance` | drawn as |
|---|---|
| `0.1` or `{"type": "symmetric", "value": 0.1}` | `25 ±0.1` |
| `{"type": "asymmetric", "plus": 0.2, "minus": -0.1}` | `25` with `+0.2` over `-0.1` (a zero deviation is drawn as `0`) |
| `{"type": "limits", "plus": 0.1, "minus": 0}` or `{"type": "limits", "upper": 25.1, "lower": 25}` | `25.1` over `25.0` (upper limit on top) |
| `"basic"` or `{"type": "basic"}` | the value in a box (theoretically exact) |
| `"reference"` | the value in brackets, `(25)` |
| a named fit, e.g. `"H7"`, `"g6"` | `Ø10 H7` with its deviations for that size stacked after it: `+0.015` over `0` |
| any other string | appended to the value |

- **Named fits:** a tolerance class (one or two letters and a grade: `H7`, `JS9`, `g6`, `js6`; capitals are holes, lower case shafts) is looked up in `named_fits.json` with the dimension's value as the nominal size. Each class there lists size ranges (`over` < size ≤ `to`) with their `upper` and `lower` deviations in mm, as in ISO 286-2. The built-in table has the common classes up to 500 mm: holes D8–D11, E7–E9, F6–F8, G6–G7, H5–H11, JS6–JS9, K6–K7, M6–M7, N6–N7, P6–P7; shafts d8–d11, e7–e9, f6–f8, g5–g7, h5–h11, js5–js9, k5–k7, m5–m7, n5–n7, p5–p7, r5–r7, s5–s7. To change or add classes, put a `named_fits.json` beside `config.json` (copy the one in the repository); it replaces the built-in table. A class or size that isn't in the table is drawn as plain text, with a warning.
- **Signs:** `plus` and `minus` are signed deviations from the nominal, so `"minus": -0.05` means 0.05 under. `{"plus": .., "minus": ..}` without a `type` is asymmetric.
- **Decimals:** both deviations or both limits are given the same number of decimal places.
- **Placement:** deviations and limits go straight after the value, before any text that follows `<>` (e.g. `Ø<> x 5 DEEP`). Angles take the same tolerances, in degrees.
- **Ordinates:** an ordinate dimension's tolerance applies to each of its values, typically `"basic"`.

Linear dimensions are stacked automatically: shorter ones go nearest the view, and a dimension moves out a level when it would overlap another or its extension lines would cross another's text.
Ordinate labels are spread apart with jogged leaders. Diameter/radius leaders, GD&T frames, datums and notes are placed in the nearest free space outside the view.

### GD&T, datums, notes

```json
"gdt":    [{ "attach": "faces(\">Z\")", "characteristic": "position", "tolerance": 0.1,
             "diameter": true, "modifier": "M", "datums": ["A", "B(M)"] }],
"datums": [{ "label": "A", "attach": "faces(\"<Z\")" }],
"notes":  [{ "text": "2 HOLES THRU", "attach": "faces(\"%CYLINDER\")", "boxed": true }]
```

**Datum on a frame.** A datum feature symbol can be attached to a feature control frame: the feature the frame controls is then that datum. Give the frame a `"datum"`, or give it an `"id"` and refer to it from the view's `datums` list with `"gdt"` in place of `"attach"`:

```json
"gdt":    [ { "id": "base", "attach": "faces(\"<Z\")", "characteristic": "flatness", "tolerance": 0.05 } ],
"datums": [ { "gdt": "base", "label": "A" } ]
```

On a frame with its own leader the symbol hangs below the frame. On a frame attached to a dimension it goes on the far side of the frame, away from the dimension. Both work for frames anywhere in the view, including frames nested in a dimension.

**All around.** `"all_around": true` on a frame draws the all-around circle on its leader's bend, meaning the tolerance applies all around the profile shown in that view (typically profile of a line or surface). It needs the frame on its own leader (`attach`); on a frame attached to a dimension it is ignored with a warning.

```json
{ "attach": "faces(\"%PLANE\").faces(\"|Z\")", "characteristic": "profile_surface", "tolerance": 0.1, "datums": ["A"], "all_around": true }
```

**Modifiers.** `modifier` (a string or a list) is drawn after the tolerance value. A datum reference takes its modifiers in brackets: `"B(M)"`, `"A-B(L)"`.

| modifier | symbol |
|---|---|
| `M`, `L` | maximum / least material condition, in a circle |
| `F`, `T`, `S`, `E` | free state, tangent plane, regardless of feature size, envelope, in a circle |
| `P12`, `U0.2` | projected tolerance zone of 12, unequal profile of 0.2: the circled letter followed by the value |
| `ST` | statistical tolerance, in a hexagon |
| anything else (`CZ`, `ACS`, ...) | plain text |

```json
{ "attach": "faces(\"%CYLINDER\")", "characteristic": "perpendicularity", "tolerance": 0.02,
  "diameter": true, "modifier": ["L", "P5"], "datums": ["A", "B(M)"] }
```

**Attaching to a dimension.** A frame or datum symbol can go on a dimension instead of on geometry, which is how a feature of size (hole, boss, slot width) is toleranced. Either nest it in the dimension:

```json
{ "type": "diameter", "of": "faces(\"%CYLINDER\")", "text": "2X Ø<> THRU", "tolerance": {"type": "limits", "plus": 0.1, "minus": 0},
  "gdt": [ { "characteristic": "position", "tolerance": 0.2, "diameter": true, "modifier": "M", "datums": ["A", "B", "C"] } ],
  "datum": "D" }
```

or give the dimension an `"id"` and refer to it from the view's `gdt` / `datums` lists with `"dimension"` in place of `"attach"`:

```json
"dimensions": [ { "id": "boss", "type": "diameter", "of": "faces(\"%CYLINDER\").faces(\">Z\")" } ],
"gdt":        [ { "dimension": "boss", "characteristic": "perpendicularity", "tolerance": 0.02, "diameter": true, "datums": ["A"] } ],
"datums":     [ { "dimension": "boss", "label": "E" } ]
```

- **Frames** are stacked on the far side of the dimension text from the dimension line (or leader shoulder) and turn with the text. They are centred on the text, or aligned with it on a leader.
- **Datum symbol:** its box sits beside the text, and its triangle sits on the dimension line or shoulder.
- **Spacing:** stacked dimensions and leaders leave room for what is attached.
- **Ordinates:** ordinate dimensions can't take attached frames or datums.

Datum targets (ISO 5459 / ASME Y14.5 symbol: target size in the upper half, label in the lower half):

```json
"datum_targets": [
  { "label": "A1", "kind": "point", "attach": "faces(\"<Z\")", "point": [15, 30, 0] },
  { "label": "C1", "kind": "area",  "attach": "faces(\">X\")", "point": [100, 20, 10], "size": "Ø6" }
]
```

`kind`: `point` (X on the surface), `line` (X plus a phantom line of length `size`, default 10 mm), `area`.
An `area` on a surface seen face-on is drawn as a hatched circle of diameter `size`; on a surface seen edge-on it is drawn like a `line` target.
`point` (model coordinates) fixes where the target sits. Without it, the target goes on the nearest suitable spot of the selected feature.

**Leaders.** Every GD&T frame, datum symbol, datum target and note attached to geometry has the same leader:
a horizontal shoulder (`leader_shoulder` in config.json, default 4 mm) out of the symbol, then a straight leader that meets the feature at 90°.
- **Face or edge seen edge-on:** the leader follows the face's outward normal. The landing point can slide along the edge to find free space.
- **Circle seen face-on:** the leader is radial.
- **Cylinder seen side-on:** the leader is perpendicular to the cylinder's outline lines.
- **Surface seen face-on:** the leader ends in a dot, in any direction.

Terminators: arrow for GD&T frames and notes, filled triangle for datum features, X for datum targets.
Symbols never overlap text, frames, dimension lines or the view. Leaders never cross text or frames. They may cross dimension and extension lines, but placements that avoid crossings are preferred.

Characteristics: straightness, flatness, circularity, cylindricity, profile_line, profile_surface, angularity,
perpendicularity, parallelism, position, concentricity, symmetry, circular_runout, total_runout.

## Layout

Once each view is built (geometry plus annotations), base views are placed at their `location` and projected views alongside their parent.
Overlapping projected views are pushed outward along their projection axis only, so they stay aligned.
Whole view clusters are then moved apart and away from the title block, and kept inside the frame.
Detail views are placed last, in the free space nearest their detail circle (or at their `location`).
If a view still doesn't fit, you get a warning (use a smaller scale).

## Limitations

- Own STEP reader: AP203/AP214/AP242 B-rep (`ADVANCED_FACE`). Planes, cylinders, cones and spheres take part in hidden-line removal.
  Tori and B-spline surfaces are drawn (their edges), but they do not hide anything behind them, and they have no silhouette lines.
- Single-body parts: assembly placement transforms are ignored.
- Units: mm, m and inch length units are converted to mm.

## Test data

`tools/make_test_step.py` generates `examples/bracket.step` (extruded profile with chamfer, fillet and two holes).
