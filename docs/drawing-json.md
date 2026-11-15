# Drawing JSON reference

A drawing is one JSON file: the STEP part to draw, the sheet and title block, and a list of views. Each view holds its annotations (dimensions, GD&T frames, datums, notes, threads) and the views made from it (projected, section and detail views).

- **Units:** model sizes and points are in model millimetres. Sheet positions are in sheet millimetres from the bottom-left corner of the sheet.
- **Selectors:** geometry is picked with CadQuery-style selector strings such as `faces('%CYLINDER').faces('<X')`. See [Selectors](#selectors).
- **Optional fields:** everything not marked *required* can be left out.

## Smallest drawing

One view of the part, looking at it from the front, with nothing on it:

```json
{
  "step_file": "part.step",
  "views": [
    { "name": "front", "direction": [0, -1, 0] }
  ]
}
```

A front view with the top and right views projected from it, and an overall length:

```json
{
  "step_file": "part.step",
  "views": [
    {
      "name": "front",
      "direction": [0, -1, 0],
      "up": [0, 0, 1],
      "scale": 2,
      "projected": [ { "position": "top" }, { "position": "right" } ],
      "dimensions": [
        { "type": "linear", "from": "faces('<X')", "to": "faces('>X')" }
      ]
    }
  ]
}
```

## Top level

| field | type | default | meaning |
|---|---|---|---|
| `step_file` | string | | The STEP part, relative to the JSON file. It can be given on the command line instead. |
| `projection` | `"third"` or `"first"` | config.json (`"third"`) | Projection system. `"3rd"`, `"third_angle"`, `"1st"` and `"first_angle"` also work. |
| `sheet` | object | A3 landscape | `{ "size": "A0".."A4", "orientation": "landscape" or "portrait" }` |
| `title` | object | | Title block text (also accepted as `title_block`). See [Title block](#title-block). |
| `views` | list | *required* | Base views. See [Base views](#base-views). |

```json
{
  "step_file": "bracket.step",
  "projection": "first",
  "sheet": { "size": "A2", "orientation": "portrait" },
  "title": { "drawing_number": "BR-0001", "description": "BRACKET" },
  "views": [ { "name": "front", "direction": [0, -1, 0] } ]
}
```

## Title block

All fields are text and all are optional.

| field | shown as |
|---|---|
| `drawing_number` | drawing number (large) |
| `description` | description |
| `material`, `finish` | material, finish |
| `notes` | the notes box; long text wraps |
| `revision`, `issue` | revision and issue boxes |
| `drawn_by`, `date` | drawn by, date |
| `general_tolerances` | the general tolerances row. Two or more spaces separate its columns. Default `UNLESS STATED:  X.X ±0.4    X.XX ±0.15    ANGLES ±0.5°` |

The scale shown is the first base view's `scale`. The sheet size and projection symbol are filled in for you.

```json
"title": {
  "drawing_number": "TP-0001",
  "description": "TURNED PART",
  "material": "EN1A",
  "finish": "ZINC PLATE",
  "notes": "REMOVE ALL BURRS AND SHARP EDGES. FILLETS R1.",
  "revision": "A",
  "drawn_by": "R",
  "date": "2026-11-15",
  "general_tolerances": "UNLESS STATED:  X.X ±0.2    X.XX ±0.05    ANGLES ±0.5°"
}
```

## Base views

A base view looks at the part from a direction you give. The views made from it are aligned with it.

| field | type | default | meaning |
|---|---|---|---|
| `name` | string | *required* | View name; also the SVG group id. |
| `direction` | [x, y, z] | *required* | Vector from the part towards the viewer. |
| `up` | [x, y, z] | Z, or Y for views along Z | Model direction that points up on the sheet. |
| `scale` | number | 1 | Drawing scale (2 = twice full size). |
| `location` | [x, y] | placed for you | Sheet position of the view centre. A view with a `location` is not moved by the layout. |
| `hidden_lines` | true / false | config.json (false) | Draw hidden edges dashed. |
| `projected` | list | | [Projected views](#projected-views) made from this view. |
| `sections` | list | | [Section views](#section-views) cut from this view. |
| `details` | list | | [Detail views](#detail-views) of parts of this view. |
| `dimensions`, `gdt`, `datums`, `datum_targets`, `notes`, `threads` | lists | | [Annotations](#annotations) shown in this view. |

Common orientations (Z up in the model):

| view | `direction` | `up` |
|---|---|---|
| front | `[0, -1, 0]` | `[0, 0, 1]` |
| back | `[0, 1, 0]` | `[0, 0, 1]` |
| right | `[1, 0, 0]` | `[0, 0, 1]` |
| left | `[-1, 0, 0]` | `[0, 0, 1]` |
| top | `[0, 0, 1]` | `[0, 1, 0]` |
| bottom | `[0, 0, -1]` | `[0, -1, 0]` |
| isometric | `[1, -1, 1]` | `[0, 0, 1]` |

A drawing can have more than one base view, for example a front view and an isometric:

```json
"views": [
  { "name": "front", "direction": [0, -1, 0], "up": [0, 0, 1], "scale": 1.5, "location": [130, 170] },
  { "name": "iso", "direction": [1, -1, 1], "up": [0, 0, 1], "scale": 1, "location": [320, 200] }
]
```

## Projected views

A projected view looks at its parent view from one side and stays aligned with it. It takes the parent's scale. In third angle the view from the right goes on the right and the view from above on top; first angle swaps them.

| field | type | default | meaning |
|---|---|---|---|
| `position` | string | *required* | `top`, `bottom`, `left`, `right` or `rear` (`above`/`plan`, `below`/`underside` and `back` also work). |
| `name` | string | `<parent>_<position>` | View name. |
| `distance` | number | placed for you | Sheet mm from the parent's centre along the projection axis. A view with a `distance` is not moved by the layout. |
| `hidden_lines` | true / false | as the parent | Draw hidden edges. |
| `projected`, `sections`, `details` | lists | | Views made from this one. |
| annotation lists | lists | | As for base views. |

```json
"projected": [
  { "position": "top", "distance": 90,
    "dimensions": [ { "type": "linear", "from": "faces('<Y')", "to": "faces('>Y')" } ] },
  { "position": "right", "hidden_lines": true }
]
```

Projected views can chain. This is the view from the right of the top view:

```json
"projected": [ { "position": "top", "projected": [ { "position": "right" } ] } ]
```

## Section views

A section view cuts the part with a plane and looks at the cut. The plane is square to the side the section is looked at from. The parent view shows the cutting-plane line with arrows and the letter at each end, and the section is titled "SECTION A-A".

| field | type | default | meaning |
|---|---|---|---|
| `label` | string | *required* | Section letter, e.g. `"A"`. |
| `position` | string | *required* | Side of the parent it is looked at from, as for projected views. |
| `through` | selector | | The plane passes through this feature's reference point (e.g. a hole's axis). |
| `point` | [x, y, z] | centre of the part | The plane passes through this model point (instead of `through`). |
| `name` | string | `section_<label>` | View name. |
| `distance` | number | placed for you | As for projected views. |
| `hidden_lines` | true / false | false | Hidden edges behind the cut. |
| `hatch_angle`, `hatch_spacing` | numbers | config.json (45°, 3 mm) | Hatching of the cut faces. |
| `title_offset` | [x, y] | [0, 0] | Moves the "SECTION A-A" label from its place under the view (sheet mm). |
| `projected` | list | | Views projected from the section. They show the whole part, not cut, and can't have sections. |
| `details` | list | | Details of the section; they keep its hatching. |
| annotation lists | lists | | As for base views. |

```json
"sections": [
  { "label": "A", "position": "left", "through": "faces('%CYLINDER').faces('<X[1]')",
    "dimensions": [ { "type": "diameter", "of": "faces('%CYLINDER').faces('<X[1]')" } ] }
]
```

- **The section view:** material in front of the plane is removed, the cut faces are outlined and hatched, and anything behind them is hidden. Centre lines are drawn only for holes and other round features the plane cuts through.
- **Placement:** like a projected view looked at from the same side, so in third angle a section looked at from the left goes on the left.
- **Surfaces:** the cut outline is computed for planes, cylinders, cones and spheres. A cut through a torus or B-spline face leaves a gap in the outline.

A section through a point, with closer hatching:

```json
"sections": [ { "label": "B", "position": "top", "point": [0, 0, 5], "hatch_spacing": 2 } ]
```

## Detail views

A detail view enlarges a circular or elliptical region of its parent view. The parent shows the circle with the letter, and the detail is titled "DETAIL D (2:1)".

| field | type | default | meaning |
|---|---|---|---|
| `label` | string | *required* | Detail letter. |
| `center` | selector | | The detail is centred on this feature's reference point. |
| `point` | [x, y, z] | centre of the part | Centre as a model point (instead of `center`). |
| `radius` | number | about 1/8 of the part | Radius of the circle in model mm; for an ellipse the semi-axis along `angle`. |
| `shape` | `"circle"` or `"ellipse"` | `"circle"` | Shape of the boundary. |
| `radius2` | number | 0.6 × `radius` | Ellipse: the other semi-axis. |
| `angle` | number | 0 | Ellipse: angle of `radius` on the sheet, degrees anticlockwise from horizontal. |
| `scale` | number | twice the parent's | Scale of the detail. |
| `location` | [x, y] | nearest free space | Sheet position of the detail's centre. |
| `border` | true / false | true | Draw the boundary around the detail view. |
| `hidden_lines` | true / false | as the parent | Draw hidden edges. |
| `name` | string | `detail_<label>` | View name. |
| `title_offset` | [x, y] | [0, 0] | Moves the label from its place under the view. |
| annotation lists | lists | | As for base views. |

```json
"details": [
  { "label": "D", "center": "faces('>Z').faces('>(0.86603,0.5,0)')", "radius": 24, "scale": 1.5,
    "dimensions": [ { "type": "radius", "of": "faces('%CYLINDER').faces('>Y')", "text": "R<> TYP" } ] }
]
```

An angled elliptical detail:

```json
"details": [ { "label": "E", "point": [40, 0, 10], "shape": "ellipse", "radius": 12, "radius2": 6, "angle": 30 } ]
```

## Annotations

Every view (base, projected, section or detail) can hold these lists. Annotations are placed in free space around their view; in the editor they can be dragged.

| list | holds |
|---|---|
| `dimensions` | [Dimensions](#dimensions) |
| `gdt` | [GD&T frames](#gdt-frames) |
| `datums` | [Datum feature symbols](#datums) |
| `datum_targets` | [Datum targets](#datum-targets) |
| `notes` | [Notes](#notes) |
| `threads` | [Cosmetic threads](#threads) |

## Dimensions

| `type` | fields | measures |
|---|---|---|
| `linear` | `from`, `to`, `direction`, `side` | Distance between two features. |
| `diameter` | `of` | A circle or cylinder. Seen end-on: a leader to the circle. Seen side-on: a dimension across it. |
| `radius` | `of` | An arc or cylinder, with a leader. |
| `angle` | `from`, `to` | Angle between two straight edges, or two flat faces seen edge-on. |
| `ordinate` | `origin`, `targets`, `direction`, `side` | Distances of several features from one origin. |

Fields for all dimension types:

| field | type | default | meaning |
|---|---|---|---|
| `type` | string | *required* | One of the types above. |
| `from`, `to` | selector | | `linear` and `angle`: the two features. |
| `of` | selector | | `diameter` and `radius`: the feature. |
| `origin`, `targets` | selector, list of selectors | | `ordinate`: the zero and the features measured from it. |
| `direction` | string | the larger offset | `linear`: `horizontal`, `vertical` or `aligned`. `ordinate`: `x` or `y`. |
| `side` | string | the side of the view's centre the feature is on | `above`, `below` (horizontal), `left` or `right` (vertical) of the view. |
| `text` | string | the value | Text; `<>` stands for the measured value, e.g. `"2X Ø<> THRU"`. Without `<>` the text replaces the value. |
| `prefix`, `suffix` | string | `Ø` / `R` for diameters and radii | Text before and after the value (ignored when `text` is given). |
| `decimals` | number | config.json (2, angles 1) | Decimal places. |
| `tolerance` | see [Tolerances](#tolerances) | | Tolerance after the value. |
| `id` | string | | Name other items use to attach to this dimension. |
| `gdt` | list | | GD&T frames on this dimension (no `attach` needed). |
| `datum` | string | | A datum feature symbol on this dimension, e.g. `"A"`. |

Linear dimensions are stacked for you: shorter ones nearest the view, and a dimension moves out a level when it would overlap another. Ordinate labels are spread apart with jogged leaders. Diameter and radius leaders go in the nearest free space.

What a feature contributes to a measurement: a vertex gives its point, a straight edge its midpoint, a circle or arc its centre, a flat face its centre, and a cylinder a point on its axis.

```json
"dimensions": [
  { "type": "linear", "from": "faces('<X')", "to": "faces('>X')", "direction": "horizontal", "side": "below" },
  { "type": "linear", "from": "edges('<Z')", "to": "edges('>Z')", "direction": "vertical", "tolerance": 0.1 },
  { "type": "diameter", "of": "faces('%CYLINDER').faces('<X')", "tolerance": "h7" },
  { "type": "diameter", "of": "edges('%CIRCLE').edges('>Z')", "text": "4X Ø<> THRU" },
  { "type": "radius", "of": "faces('%CYLINDER').faces('>Y')", "text": "R<> TYP" },
  { "type": "angle", "from": "edges('%LINE').edges('<X')", "to": "edges('%LINE').edges('>Y')" },
  { "type": "ordinate", "origin": "faces('<X')", "targets": ["faces('%CYLINDER').faces('<X')", "faces('%CYLINDER').faces('>X')"],
    "direction": "x", "side": "below", "tolerance": "basic" }
]
```

### Tolerances

| `tolerance` | drawn as |
|---|---|
| `0.1` or `{ "type": "symmetric", "value": 0.1 }` | `25 ±0.1` |
| `{ "type": "asymmetric", "plus": 0.2, "minus": -0.1 }` | `25` with `+0.2` over `-0.1` |
| `{ "plus": 0.2, "minus": 0 }` | asymmetric, as above (a zero deviation is drawn as `0`) |
| `{ "type": "limits", "plus": 0.1, "minus": 0 }` | `25.1` over `25.0` |
| `{ "type": "limits", "upper": 25.1, "lower": 25 }` | `25.1` over `25.0` |
| `"basic"` | the value in a box (theoretically exact) |
| `"reference"` | the value in brackets, `(25)` |
| a fit class, e.g. `"H7"`, `"g6"` | `Ø10 H7` with its deviations for that size, `+0.015` over `0` |
| any other text | added after the value |

- **Signs:** `plus` and `minus` are signed deviations from the nominal, so `"minus": -0.05` means 0.05 under.
- **Fit classes:** capitals are holes (`H7`, `JS9`), lower case shafts (`g6`, `js6`). The deviations come from `named_fits.json` (ISO 286-2, sizes up to 500 mm). A class or size not in the table is drawn as plain text, with a warning.
- **Fit classes in the built-in table:** holes D8–D11, E7–E9, F6–F8, G6–G7, H5–H11, JS6–JS9, K6–K7, M6–M7, N6–N7, P6–P7; shafts d8–d11, e7–e9, f6–f8, g5–g7, h5–h11, js5–js9, k5–k7, m5–m7, n5–n7, p5–p7, r5–r7, s5–s7. To change them, put your own `named_fits.json` beside `config.json` (copy the one in the repository).
- **Decimals:** both deviations, or both limits, get the same number of decimal places.
- **Placement:** deviations go straight after the value, before any text that follows `<>` (in `Ø<> x 5 DEEP`, before ` x 5 DEEP`).
- **Angles:** angle dimensions take the same tolerances, in degrees.
- **Ordinates:** the tolerance applies to every value; typically `"basic"`.

## GD&T frames

A feature control frame, on its own leader to a feature, or on a dimension.

| field | type | default | meaning |
|---|---|---|---|
| `characteristic` | string | *required* | `straightness`, `flatness`, `circularity`, `cylindricity`, `profile_line`, `profile_surface`, `angularity`, `perpendicularity`, `parallelism`, `position`, `concentricity`, `symmetry`, `circular_runout`, `total_runout`. |
| `tolerance` | number or text | *required* | Tolerance value, e.g. `0.05`. |
| `attach` | selector | | The feature the leader points to. |
| `dimension` | string | | Instead of `attach`: the `id` of a dimension to put the frame on. |
| `diameter` | true / false | false | Ø before the tolerance (cylindrical zone). |
| `modifier` | text or list | | After the tolerance: `M`, `L`, `F`, `T`, `S`, `E` (circled), `P12`, `U0.2` (circled letter and value), `ST` (hexagon), or plain text such as `CZ`. |
| `datums` | list | | Datum references, with modifiers in brackets: `"A"`, `"B(M)"`, `"A-B"`. |
| `all_around` | true / false | false | The all-around circle on the leader's bend (frames on their own leader only). |
| `datum` | string | | A datum feature symbol on the frame: the controlled feature is that datum. |
| `id` | string | | Name datums and merged items use to refer to this frame. |
| `merge` | string | | Put this frame on the leader of the item with this `id`. See [Merged callouts](#merged-callouts). |

```json
"gdt": [
  { "attach": "faces('>Z')", "characteristic": "flatness", "tolerance": 0.05 },
  { "attach": "faces('%CYLINDER')", "characteristic": "position", "tolerance": 0.1,
    "diameter": true, "modifier": "M", "datums": ["A", "B(M)"] },
  { "attach": "faces('%PLANE').faces('#Z')", "characteristic": "profile_surface", "tolerance": 0.1,
    "datums": ["A"], "all_around": true }
]
```

A frame on a dimension, which is how a feature of size (hole, shaft, slot width) is toleranced:

```json
{ "type": "diameter", "of": "faces('%CYLINDER').faces('>X')",
  "gdt": [ { "characteristic": "total_runout", "tolerance": 0.1, "datums": ["A-B"] } ] }
```

The same, with the frame kept in the view's `gdt` list and pointing at the dimension by `id`:

```json
"dimensions": [ { "id": "boss", "type": "diameter", "of": "faces('%CYLINDER').faces('>Z')" } ],
"gdt": [ { "dimension": "boss", "characteristic": "perpendicularity", "tolerance": 0.02, "diameter": true, "datums": ["A"] } ]
```

## Datums

A datum feature symbol: a letter in a box with a filled triangle on the feature.

| field | type | meaning |
|---|---|---|
| `label` | string | *Required.* The datum letter. |
| `attach` | selector | The datum feature. |
| `dimension` | string | Instead of `attach`: the `id` of a dimension (a datum feature of size, e.g. a shaft's axis). |
| `gdt` | string | Instead of `attach`: the `id` of a GD&T frame; the symbol hangs on the frame. |

```json
"datums": [
  { "label": "A", "attach": "faces('<Z')" },
  { "label": "B", "dimension": "bore" },
  { "label": "C", "gdt": "base" }
]
```

The shortest way to make a diameter a datum is `"datum"` on the dimension itself:

```json
{ "type": "diameter", "of": "faces('%CYLINDER').faces('<X')", "datum": "A" }
```

## Datum targets

A datum target symbol (target size above, label below) with a leader to a point, line or area on the part.

| field | type | default | meaning |
|---|---|---|---|
| `label` | string | *required* | e.g. `"A1"`. |
| `attach` | selector | *required* | The surface, edge or vertex the target is on. |
| `kind` | string | `point` | `point` (an X), `line` (an X and a phantom line), or `area` (a hatched circle). |
| `size` | text | | Shown in the upper half, e.g. `"Ø6"`; also the drawn size of a line or area. |
| `point` | [x, y, z] | nearest suitable spot | Exact model position of the target. |

```json
"datum_targets": [
  { "label": "A1", "kind": "point", "attach": "faces('<Z')", "point": [15, 30, 0] },
  { "label": "C1", "kind": "area", "attach": "faces('>X')", "point": [100, 20, 10], "size": "Ø6" }
]
```

## Notes

| field | type | default | meaning |
|---|---|---|---|
| `text` | string | *required* | Note text. |
| `attach` | selector | | A leader to this feature. Without it, the note goes in free space near the view. |
| `boxed` | true / false | true | Draw a box round the text. |
| `id` | string | | Name merged items use to refer to this note. |
| `merge` | string | | Put this note on the leader of the item with this `id`. |

```json
"notes": [
  { "text": "2 HOLES THRU", "attach": "faces('%CYLINDER')" },
  { "text": "BREAK ALL EDGES 0.5 MAX", "boxed": false }
]
```

## Threads

A cosmetic thread (ISO 6410) on a hole or shaft, with an optional callout. Seen end-on it is drawn as a thin ¾ circle; seen side-on, as thin lines with a thick line where the thread ends.

| field | type | default | meaning |
|---|---|---|---|
| `of` | selector | *required* | The threaded cylinder (or a circular edge of it). |
| `size` | text | | `"M6"` (coarse pitch) or `"M10x1.25"`. |
| `major`, `pitch` | numbers | | Instead of `size`: major diameter and pitch in mm. |
| `depth` | number | the whole hole or shaft | Thread length from the open end. |
| `all_alike` | true / false | false | Also every hole or shaft with the same diameter, length and direction. |
| `callout` | true / false | true | A leader note such as `2X M6 x 8 DEEP`. |
| `text` | string | | Callout text instead of the automatic one. |
| `kind` | `"internal"` or `"external"` | from the face | Hole or shaft. |
| `id`, `merge` | string | | As for notes. |

- **Hole or shaft:** a hole should be modelled at its tap-drill size (you get a warning if it isn't smaller than the thread); its thread is drawn at the major diameter. A shaft's thread is drawn at the minor diameter.
- **Seen side-on:** a hole's thread lines are dashed unless a section cuts through it, and left out if the view has no hidden lines.
- **Seen end-on:** drawn only when the threaded end faces the viewer. In other directions (e.g. isometric views) threads are not drawn.

```json
"threads": [
  { "of": "faces('%CYLINDER').faces('<Y[1]')", "size": "M6", "depth": 8, "all_alike": true },
  { "of": "faces('%CYLINDER').faces('>X')", "size": "M20x1.5", "depth": 12, "kind": "external" }
]
```

## Merged callouts

A diameter or radius dimension, threads, GD&T frames and notes on the same feature can share one leader. The item that heads the leader gets an `id`, and the others name it in `merge`. In the editor, **Merge with…** writes these fields for you.

```json
"dimensions": [ { "id": "hole1", "type": "diameter", "of": "faces('%CYLINDER').faces('<Y[1]')", "text": "Ø<> x 5 DEEP" } ],
"threads": [ { "of": "faces('%CYLINDER').faces('<Y[1]')", "size": "M4", "depth": 4, "merge": "hole1" } ],
"notes": [ { "text": "SPOTFACE BOTH SIDES", "attach": "faces('%CYLINDER').faces('<Y[1]')", "boxed": false, "merge": "hole1" } ]
```

- **Order:** the head's text, then thread callouts, then notes, one per line, then the frames.
- **Head:** when a dimension is in the group, it heads it (only one dimension per group). Otherwise a frame, thread or note can head it.
- **Chains:** `merge` may name an item that is itself merged; they all end on the same leader.

## Leaders

GD&T frames, datum symbols, datum targets and notes attached to geometry share one leader style: a short horizontal shoulder (`leader_shoulder` in config.json, 4 mm) out of the symbol, then a straight leader that meets the feature square.

- **Face or edge seen edge-on:** the leader follows the face's outward normal and can slide along the edge to find free space.
- **Circle seen face-on:** the leader is radial.
- **Cylinder seen side-on:** the leader is square to its outline lines.
- **Surface seen face-on:** the leader ends in a dot.
- **Ends:** an arrow for frames and notes, a filled triangle for datum symbols, an X for datum targets.

Symbols never overlap text, frames, dimension lines or the view, and leaders never cross text or frames.

## Selectors

Selectors use CadQuery's syntax. `faces(...)`, `edges(...)` and `vertices(...)` can be chained, each step narrowing the previous one. Quote the selector inside the call with single quotes, so the JSON string needs no escaping.

| selector | picks |
|---|---|
| `>X`, `<Y`, `>Z` | the item(s) furthest along +X, −Y, +Z |
| `>X[1]`, `<Z[-1]` | the n-th group from that end (`[0]` is the furthest) |
| `>(1,1,0)` | furthest along any direction |
| `\|Z` | straight edges along Z; flat faces facing ±Z |
| `#Z` | straight edges square to Z; flat faces whose normal is square to Z (side faces) |
| `+Z`, `-Z` | flat faces facing +Z / −Z; straight edges running +Z / −Z |
| `%PLANE`, `%CYLINDER`, `%CONE`, `%SPHERE`, `%TORUS` | faces of that surface type |
| `%LINE`, `%CIRCLE`, `%ELLIPSE`, `%BSPLINE` | edges of that curve type |
| `@12` | item 12 of the STEP model, as the editor shows it (only stable while the STEP file doesn't change) |
| `and`, `or`, `not`, `exc`, `( )` | combine selectors |

```json
"faces('>Z')"
"faces('%CYLINDER').faces('<X[1]')"
"edges('%CIRCLE').edges('>Z')"
"faces('not(>Z or <Z)')"
"faces('%PLANE').faces('#Z')"
"faces('<Y[1] or <X[3]')"
```

- **Checking a selector:** set `STEPDRAW_DEBUG=1` and stepdraw prints what each selector matched (type, radius, centre).
- **From the editor:** in Geometry mode, pick faces, edges or points and the selection panel shows a selector that matches exactly those items.

## Complete example

A turned shaft: a front view with a right-hand end view, diameters with a fit, runout to a common datum axis, lengths, and a thread.

```json
{
  "step_file": "turnedPart.step",
  "title": {
    "drawing_number": "TP-0001",
    "description": "TURNED PART",
    "material": "EN1A",
    "notes": "REMOVE ALL BURRS AND SHARP EDGES. FILLETS R1.",
    "revision": "A",
    "drawn_by": "R",
    "date": "2026-11-15"
  },
  "views": [
    {
      "name": "front",
      "direction": [0, -1, 0],
      "up": [0, 0, 1],
      "scale": 2,
      "projected": [ { "position": "right" } ],
      "dimensions": [
        { "type": "diameter", "of": "faces('%CYLINDER').faces('<X')", "tolerance": "h7", "datum": "A" },
        { "type": "diameter", "of": "faces('%CYLINDER').faces('<X[1]')", "tolerance": "H8",
          "gdt": [ { "characteristic": "total_runout", "tolerance": 0.1, "datums": ["A-B"] } ] },
        { "type": "diameter", "of": "faces('%CYLINDER').faces('<X[2]')" },
        { "type": "diameter", "of": "faces('%CYLINDER').faces('>X')", "tolerance": "h7", "datum": "B" },
        { "type": "linear", "from": "faces('%PLANE').faces('<X')", "to": "faces('%PLANE').faces('>X')", "side": "below" },
        { "type": "linear", "from": "faces('%PLANE').faces('<X')", "to": "faces('%PLANE').faces('<X[1]')", "side": "below" }
      ],
      "threads": [
        { "of": "faces('%CYLINDER').faces('>X')", "size": "M20x1.5", "depth": 10 }
      ],
      "notes": [
        { "text": "CENTRE DRILL BOTH ENDS", "boxed": false }
      ]
    }
  ]
}
```
