# stepdraw

Headless generator of A3 engineering drawings (SVG) from a STEP part and a JSON drawing description.

```
cargo build --release
./target/release/stepdraw examples/bracket.json -o examples/bracket.svg
./target/release/stepdraw drawing.json part.step -o out.svg -c config.json
./target/release/stepdraw serve examples        # drawing editor at http://127.0.0.1:8080/
./target/release/stepdraw auto examples/bracket.step   # a first drawing JSON for a STEP file (--base: base view only)
```

The STEP file can be passed as the second argument or as `"step_file"` in the JSON (relative to the JSON file).
`config.json` is read from `-c`, otherwise from the current directory, then next to the executable, otherwise built-in defaults.
Warnings (unmatched selectors, views that don't fit, etc.) go to stderr; the SVG is still written.

## Output

- ISO A0–A4 sheet, landscape or portrait (default A3 landscape), border with reference zones of about 50 mm: letters across the top and bottom, numbers down the sides (A–G and 1–5 on A3 landscape). A4 has narrower margins so the 180 mm title block fits.
- Title block (bottom right): drawing number, issue, revision, size, description, material, finish, general tolerances (default `UNLESS STATED: X.X ±0.4  X.XX ±0.15  ANGLES ±0.5°`, set with `general_tolerances`; two or more spaces separate its columns), notes, drawn by, date, scale, projection symbol.
- One SVG group per view (`id` = view name), each containing `<name>-geometry`, `<name>-GDandT`, `<name>-dimensions`
  (also labelled `geometry`, `GDandT`, `dimensions` for Inkscape). Each view group is positioned by a single `translate`.

## Editing the layout

```
./target/release/stepdraw serve examples          # folder with the drawing JSON files; --port 8080 by default
```

Then open http://127.0.0.1:8080/ in a browser.
- **Opening a drawing:** pick a drawing JSON from the list (every `.json` with `views` in the folder), or open/drop one from elsewhere. An opened JSON's STEP file is looked up by name in the served folder. stepdraw renders it and the SVG is shown, with stepdraw's warnings behind the warnings button.
- **New from STEP…:** makes a new drawing for a STEP file in the folder and opens it. The dialog asks what to start with: **Base view only** (the default; `stepdraw auto --base`: the base view with nothing on it, to build up yourself) or an **Auto drawing** (as `stepdraw auto`, described below). The drawing JSON is written next to the STEP file as `<part>.json`, or `<part>-auto.json` (`-auto-2`, …) if that exists; nothing is overwritten. What the auto drawing puts in:
  - **Views:** a base view looking at the side with the most holes (the front if there are none), a projected view for each other side that has holes (left, right, top, bottom or back), and a side view for the depth if none of those shows it. The scale is the largest standard one that fits A3.
  - **Overall dimensions:** width and height in the base view and depth in a side view, between the part's outermost faces (or edges, or points). A round outline gets its diameter instead.
  - **Holes:** round holes along X, Y or Z, each shown in the view of a side it opens onto (a through hole in only one of them). One diameter per set of alike holes (`2X Ø<> THRU`, `Ø<> x 5 DEEP`) and ordinate positions across and up the view, from its left and bottom edges, or from the axis of a round part.
  - **Shafts and bosses:** the diameter of each, in a view that shows it from the side where there is one.

  It's a starting point: drag things into place, add tolerances and GD&T, and Save. Holes at an angle aren't dimensioned (the status bar says so).
- **Re-render** (<kbd>R</kbd>): after editing the JSON, renders it again. View moves are kept by view name. Annotation moves are kept if the drawing still has the same annotations in the same order.
- **Selection mode:** the **Select** list sets what a click picks, and <kbd>Tab</kbd> / <kbd>Shift</kbd>+<kbd>Tab</kbd> cycles through the options:
  - **Annotations:** dimensions, frames, datums and notes.
  - **Views:** a whole view, by clicking anywhere in its area.
  - **Geometry:** individual faces, edges and points (hidden edges too, where hidden lines are drawn), never whole views or annotations.
    - Click one to select it; <kbd>Ctrl</kbd>+click adds or removes; click empty space or press <kbd>Esc</kbd> to clear.
    - The selection is highlighted in every view and listed in a panel, with a selector that matches exactly the selected items (copy it, or insert it in the JSON editor).
    - Dragging pans the sheet; views don't move in this mode.
    - **Add dimension** (in the selection panel) adds the dimension below straight away to the view clicked last, and re-renders (<kbd>Ctrl</kbd>+<kbd>Z</kbd> removes it). Only if it can't be drawn (a selector that matches nothing, a dimension stepdraw can't place) does the JSON editor open on it, with the reason, so you can fix it and Apply.
    - **Add GD&T / Add datum / Add note / Add thread** open the JSON editor on a new entry for the selection, in the view clicked last, with the selectors filled in. Apply adds it to that view's list.
      - **Dimension:** a circle, cylinder or cone gives a diameter, an arc a radius, a straight edge its length (between its end points), two straight lines (edges or construction lines) an angle unless they are parallel in the view (then the distance across them), a construction circle its diameter as a pitch circle (`Ø<> PCD`), any other two items the distance between them, three or more equal holes `nX Ø<>`, and three or more other items ordinates from the first one picked.
      - **GD&T:** holes and bosses get position Ø0.1 Ⓜ to the drawing's datums; planes flatness (or parallelism to datum A once there are datums); straight edges straightness; cones circular runout; anything else surface profile.
      - **Datum:** the next free letter (I, O and Q are skipped). **Note:** `Ø5 THRU` / `2X Ø5 THRU` for holes, otherwise `NOTE`.
      - **Thread:** the metric size is worked out from the tap drill (Ø5 → M6, Ø3.3 → M4). It works on any hole or shaft: its cylinder (between its outline lines), a straight edge of it, or a circle or arc (seen end-on, or the rim of a chamfer, which picks the coaxial cylinder). A picked circle or arc is written as the cylinder it bounds, so the thread is drawn on that cylinder in the view (end-on or side-on). A blind hole is threaded to 70% of its depth unless `depth` says otherwise. With a flat face picked it is greyed out.
- **Moving views:** a view moves with its annotations. A projected or section view slides only along its projection axis and follows its parent view across it, so it stays aligned. Moving a base view takes its aligned views along. Detail views and other base views move freely.
  - **Recorded in the JSON:** when you let go of a view (or stop nudging it), its place is written to the drawing JSON and the drawing re-renders with it there. A base view gets `"location"`, and a detail view gets `"location"` of its centre. A projected or section view gets `"distance"`, its distance from its parent along the projection axis, so it stays aligned with its parent. Views placed this way are not moved by stepdraw's own layout (they can overlap other views or the title block; you get a warning). <kbd>Ctrl</kbd>+<kbd>Z</kbd> undoes a move, and double-clicking a view placed this way removes its `location` / `distance` so stepdraw places it again.
- **Moving annotations:** dimensions, GD&T frames, datum symbols, datum targets and notes can be dragged.
  - **Linear and aligned dimensions:** the dimension line moves square to the feature, and the extension lines stretch to follow. The text slides along the line; if it goes beyond the extension lines, the dimension line is extended to it. <kbd>Shift</kbd>-drag moves only the line.
  - **Leaders:** the symbol moves, and the leader stays on its feature. A selected leader that doesn't end in an arrow (datum triangle, a dot on a face, a datum-target cross) shows a handle on its end at the part. On an edge seen edge-on the end slides along the edge's line, and past the edge it gets a thin extension line (a datum on an extension line); on a face seen face-on it moves anywhere. The symbol stays put. On an edge seen edge-on, it slides along the edge, square to it. On a circle it stays radial (diameter and radius callouts too). On a face seen face-on, it keeps its point.
  - **Angle dimensions:** the arc changes radius.
  - **Ordinate labels:** each label moves with its jogged leader. A selected label shows a handle on the other end of its leader, the end nearest the view: drag it to slide that end along the leader (from the feature up to just short of its bend), e.g. to clear other lines. The leader length is set per feature: letting go writes that feature's `gap` into the JSON (`{ "of": selector, "gap": mm }` in `origin` or `targets`) and re-renders; <kbd>Ctrl</kbd>+<kbd>Z</kbd> undoes it.
- **Editing an annotation's JSON:** in Annotations mode, right-click an annotation to open its entry from the drawing JSON in a code editor (Monaco, the VS Code editor), with:
  - **Completion** for property names, value lists (types, sides, characteristics, modifiers, tolerance types) and snippets (tolerances, common frames).
  - **Selector suggestions:** the selectors already used in the drawing, plus common ones.
  - **Hover help** for every property.
  - **Checking:** misspelt properties, bad values and missing required fields are marked.
  - **Comma fixing:** when you pause typing, commas missing at line ends are added and extra ones (before `}` / `]`, doubled) are removed, if that makes the JSON valid. A comma next to the cursor is left alone while you type. **Apply** always fixes them. <kbd>Ctrl</kbd>+<kbd>Z</kbd> undoes a fix.

  - **Preview:** beside the editor, the drawing is re-rendered with your edit as you type (not saved), zoomed to the annotation's view, with stepdraw's warnings. If the annotation can't be drawn, the preview says so.
  - **Highlighting:** the preview shows the geometry the edited item refers to (every selector in it: `from`, `to`, `of`, `attach`, `targets`, also in nested frames) in purple, and the geometry of the selector on the cursor's line in green, with a count above the preview ("at cursor: 2 edges", or the selector's error). A face seen edge-on lights up its edges.
  - **Selected geometry:** its selector comes first in the selector completions, and the **Insert** button puts it at the cursor.

  **Apply** (<kbd>Ctrl</kbd>+<kbd>Enter</kbd>) replaces just that entry in the drawing JSON, keeping the rest of its formatting, then re-renders, keeping your moves. The edit stays in the page (the file name shows "• unsaved") until you save it.
  - **Requirements:** the editor loads from cdn.jsdelivr.net on first use; offline, you get a plain text box without completion.
  - **Frames on a dimension:** a frame attached to a dimension is edited as part of that dimension. A frame that refers to a dimension by `id` is not shown there.
  - **→ Radius / → Diameter:** in the JSON window of a diameter or radius dimension, switches it to the other kind and re-renders (an `R<>` / `Ø<>` in its `text` is swapped too; <kbd>Ctrl</kbd>+<kbd>Z</kbd> undoes it). Any circle, arc, cylinder or fillet can be dimensioned either way.
  - **Merge with…:** for a diameter / radius dimension, cosmetic thread, GD&T frame or note on a feature, this button puts other items on the same feature (same hole or shaft axis, same face or edge) in the same view onto its leader. Those items are highlighted in purple, with the one being edited in orange; click one to merge it, or <kbd>Ctrl</kbd>+click several and release <kbd>Ctrl</kbd> to merge them all (a banner above the drawing has **Merge** and **Cancel** too; <kbd>Esc</kbd> cancels). An edit made in the editor is applied first. A merged item has no leader of its own, so the editor of the item it is merged onto lists it under **Merged:** (click it to edit it) with an **Unmerge** button. <kbd>Ctrl</kbd>+<kbd>Z</kbd> undoes a merge or unmerge. See [Merged callouts](docs/drawing-json.md#merged-callouts).
- **Inserting a base view:** in Views mode, right-click empty sheet (away from the views) and pick which side the new view looks at (Front by default), then **Insert**: a base view is added there, at the scale of the first base view.
- **Converting a projected view to a base view:** a projected view's right-click popup has **Convert to base view**. The view becomes a base view looking the same way, at the same place and scale, taken out of its parent's projection group so it moves on its own; its annotations and the views made from it go with it. <kbd>Ctrl</kbd>+<kbd>Z</kbd> undoes it.
- **Adding projected views:** right-click a base, projected or section view (off its annotations) for a small pad of four arrows at the pointer; an arrow adds a projected view on that side of it on the sheet and re-renders (`"position"` follows the drawing's first / third angle `projection`; a side the view already has is greyed out). Detail views can't have projected views. In Geometry mode right-click does nothing.
- **Section on / off:** the same pad has a **Section view** checkbox. Ticking it on a projected view turns it into a section of its parent view: it moves from the parent's `projected` list to its `sections`, gets the next free letter, and the parent view shows the section arrows (the cutting plane goes through the centre of the part until you move it). Unticking a section turns it back into a projected view (its `label`, `through`, `point` and hatch settings are dropped). Its annotations and the views projected from it go along. Base and detail views can't be sections, nor can a view projected from a section. It's greyed out when it can't be done (the tooltip says why). <kbd>Ctrl</kbd>+<kbd>Z</kbd> undoes it.
- **Hidden lines:** every view's popup has a **Show hidden lines** checkbox (the detail popup has a **Hidden lines** box with its other settings). It sets that view's `"hidden_lines"` to `true` or `false`. <kbd>Ctrl</kbd>+<kbd>Z</kbd> undoes it.
- **Scale of a base view:** a base view's popup has a scale box. For the first base view it is labelled **Sheet scale**: that view's scale is the scale shown in the title block. Change the number and press <kbd>Enter</kbd> (or leave the box) to set the view's `"scale"` and re-render; its projected, section and detail views follow. <kbd>Ctrl</kbd>+<kbd>Z</kbd> undoes it.
- **Deleting one ordinate label:** with an ordinate label selected, <kbd>Del</kbd> removes just that feature from the dimension's `targets` (the other labels keep their moves). The origin's label, or the last one left, deletes the whole dimension.
- **Orientation of a base view:** a base view's popup also has a list of standard orientations: Front, Back, Right, Left, Top, Bottom and Isometric (model Z up; Top and Bottom have Y up and down). Next to it, ⟲ and ⟳ turn the view 90° on the sheet. These set the view's `"direction"` and `"up"`, and its projected, section and detail views follow. Its annotations keep their selectors, so some may no longer fit the new view; the status bar names any new warning. <kbd>Ctrl</kbd>+<kbd>Z</kbd> undoes it.
- **Detail views:** the view popup's **Add detail view** button adds a circular detail of that view centred where you right-clicked, with the next free letter, a radius of about a sixth of the view and twice the view's scale. In Views mode a detail's circle and letter on the parent view can be selected and dragged; when you let go the detail's `"point"` is set to the new centre (replacing `center`) and the drawing re-renders. Right-clicking a detail view, or its circle on the parent, opens a popup to change its **shape** (circle or ellipse), **radius**, the ellipse's **radius 2** and **angle**, and its **scale**. The drawing re-renders every time a value changes; **Accept** (or <kbd>Enter</kbd>) keeps the change, while **Cancel**, <kbd>Esc</kbd> or a click elsewhere puts the detail back as it was. <kbd>Ctrl</kbd>+<kbd>Z</kbd> undoes each of these.
- **View labels:** a view's label ("SECTION A-A", "DETAIL D (2:1)", or "SCALE: 2:1" on a view not at the sheet scale; a section not at the sheet scale has it as a second line) can be selected and dragged in Views or Annotations mode, anywhere relative to its view. When you let go, the move is written to the view's `"title_offset"` ([x, y] sheet mm from its usual place under the view) and the view's own place is recorded too (`location` / `distance`), so it doesn't shift because its label moved. The label goes with its view when the view moves. Double-click a label to put it back under its view; <kbd>Ctrl</kbd>+<kbd>Z</kbd> undoes a move.
- **Moving a cutting plane:** in Views mode the section arrows on a parent view (line, arrows and letters) can be selected and dragged. They move square to the cutting plane, and when you let go the section's `"point"` is set to the new place (replacing `through`) and the drawing re-renders, so the section view shows the new cut. <kbd>Ctrl</kbd>+<kbd>Z</kbd> undoes it.
- **Title block and sheet:** click the title block (or right-click it) to edit its JSON. The same window has the scale of the first base view (the one the title block shows) as a number, and the sheet size (A0–A4) and orientation, and **Hide border and title block** (`"sheet": { "border": false }`), the drawing's **line widths** (visible, hidden, thin and border, in mm; empty boxes use config.json's, shown greyed; written as `"line_weights"`), which leaves both out so the views can use their space; a faint dashed box (editor only, not in the downloaded SVG) shows where the title block was, and clicking it opens this window again. All four show in the preview and are applied with **Apply**. Views with a fixed `location` keep it when the sheet size changes, so you may need to move them.
- **Centre lines:** in Annotations mode a centre line or centre mark can be selected like an annotation; <kbd>Del</kbd> removes it from that view (its feature goes in the view's `hidden_axes`; <kbd>Ctrl</kbd>+<kbd>Z</kbd> brings it back).
- **Construction geometry:** in Geometry mode pick two straight lines of a view (<kbd>Ctrl</kbd>+click the second) and press **Add construction point** (where they meet, extended if need be: a virtual sharp), or two points (vertices, circles' centres, construction points) and press **Add construction line** or **Add construction circle** (centred on the first point, through the second). They go in the view's `constructedGeometry`, drawn thin (lines and circles dashed). Pick them in Geometry mode (the cross, or the dashes) to dimension them (`cpoint('P1')`, `cline('L1')`, `ccircle('C1')`; **Add dimension** gives a circle's diameter and a line's length); in Annotations mode, clicking (or right-clicking) one opens a menu of its dimensions instead of the JSON editor: a circle's **Diameter** / **Radius** and distances from its centre, a line's **Length** (aligned, horizontal or vertical) and **Angle to…** / **Distance to…** another line or feature, a point's horizontal, vertical or aligned **distance to…**; for the "to…" ones a banner asks you to pick the other feature (in Geometry mode) and press **Use selection**. The menu also has **Edit JSON…** and **Delete**.
- **Change reference…:** in an annotation's JSON window, this button changes the feature it is attached to: the editor switches to Geometry mode with a banner; pick the new feature (choose which reference first, e.g. `from` or `to`, when it has several) and press **Use selection**. <kbd>Esc</kbd> or Cancel leaves it as it was; <kbd>Ctrl</kbd>+<kbd>Z</kbd> undoes it.
- **Deleting:** <kbd>Del</kbd> deletes the selected annotation or view (with everything in it) from the drawing JSON at once, without asking, and re-renders; the other items keep their moves. <kbd>Ctrl</kbd>+<kbd>Z</kbd> (or the Undo button) puts deleted items back, the last one first, in their old place in the JSON; moves made since the deletion are undone first. <kbd>Ctrl</kbd>+<kbd>Y</kbd> also puts deletions back (when there are none it redoes a move; <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>Z</kbd> always redoes). Deletions are unsaved edits like any other until you Save. The drawing's only view can't be deleted.
- **Editor controls:** undo/redo, double-click to reset an item, arrow keys to nudge, pan and zoom.
- **Saving the JSON:** **Save** (<kbd>Ctrl</kbd>+<kbd>S</kbd>) writes the edited drawing JSON back to its file. **Save as…** (<kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>S</kbd>) asks for a path inside the served folder (asking before replacing a file); if the drawing's `step_file` wouldn't be found from the new place, it is changed to a path that is. A JSON opened from elsewhere is saved with Save as. The browser warns before you leave the page or open another drawing with unsaved edits. View moves are written to the JSON (see Moving views); annotation moves are not part of the JSON, and are kept in the downloaded SVG.
- **Saving the SVG:** **Download SVG** saves the edited drawing as `<drawing>.svg`. The offsets are stored in the file.
- **Help** (<kbd>F1</kbd>): a panel with the drawing JSON reference ([docs/drawing-json.md](docs/drawing-json.md)), a contents list and a find box. Every example has a **Copy** button. In the JSON editor, **Help** (or <kbd>F1</kbd>) opens it at the section for the entry being edited, and the examples get an **Insert** button that puts them at the cursor.
- **Linking:** `?drawing=<path>` in the URL opens that drawing straight away.
- **Security:** the server listens on 127.0.0.1 only, reads files only inside the served folder, and refuses requests that don't come from a local host name.
- **Speed:** the server keeps each view's hidden-line result (by STEP file contents and the view's direction, scale and settings), so a re-render only computes views that changed; the curves of a view are classified on all CPU cores. `STEPDRAW_PROFILE=1` prints where a render spends its time.
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
| `hidden_lines` | false | default for every view (`"hidden_lines": true` on a view turns them on) |
| `centre_lines` | true | default for every view; section views show only the axes of holes and other round features the cut passes through, with no centre marks |
| `tangent_edges` | `"thin"` | smooth (tangent) edges, such as where a fillet blends into a face: `"thin"` draws them as thin grey witness lines (class `tangent`), `"none"` leaves them out, `"visible"` draws them like any other edge |
| `dim_offset`, `dim_spacing` | 10, 8 | first dimension line distance from the view, spacing of stacked dimensions |
| `view_gap` | 20 | minimum gap between views |
| `decimals`, `trim_zeros`, `angle_decimals` | 2, true, 1 | number formatting |
| `hatch_angle`, `hatch_spacing` | 45, 3 | section hatching |
| `leader_shoulder` | 4 | horizontal shoulder between a GD&T symbol and its leader |
| `font_family`, line widths, arrow size, `frame_height` | | styling |

## Drawing JSON

The full reference, with an example for every field, is in [docs/drawing-json.md](docs/drawing-json.md). The editor shows the same text under **Help** (<kbd>F1</kbd>).

```jsonc
{
  "step_file": "part.step",
  "projection": "third",                 // optional, overrides config
  "sheet": { "size": "A3", "orientation": "landscape" },
  "title": { "drawing_number": "", "description": "", "material": "" },
  "views": [
    {
      "name": "front",
      "direction": [0, -1, 0],            // from the part towards the viewer
      "up": [0, 0, 1],
      "scale": 1.5,
      "projected": [ { "position": "top" } ],
      "sections": [ { "label": "A", "position": "left", "through": "faces('%CYLINDER')" } ],
      "details": [ { "label": "D", "center": "edges('%CIRCLE').edges('>Z')", "radius": 10 } ],
      "dimensions": [ { "type": "linear", "from": "faces('<X')", "to": "faces('>X')" } ],
      "gdt": [], "datums": [], "datum_targets": [], "notes": [], "threads": []
    }
  ]
}
```

## Layout

Once each view is built (geometry plus annotations), base views are placed at their `location` and projected views alongside their parent.
Overlapping projected views are pushed outward along their projection axis only, so they stay aligned.
Whole view clusters are then moved apart and away from the title block, and kept inside the frame.
Detail views are placed last, in the free space nearest their detail circle (or at their `location`).
If a view still doesn't fit, you get a warning (use a smaller scale).

## Limitations

- Own STEP reader: AP203/AP214/AP242 B-rep (`ADVANCED_FACE`). Planes, cylinders, cones and spheres take part in hidden-line removal.
  Tori (e.g. fillets) and B-spline surfaces (rational or not) are drawn with their edges and outline and hide what is behind them. Other surface types (surfaces of revolution, offset surfaces, ...) are drawn by their edges only.
- Single-body parts: assembly placement transforms are ignored.
- Units: mm, m and inch length units are converted to mm.

## Test data

`tools/make_test_step.py` generates `examples/bracket.step` (extruded profile with chamfer, fillet and two holes).
