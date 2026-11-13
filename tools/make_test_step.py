#!/usr/bin/env python3
"""Generate a test STEP (AP214) B-rep: an extruded profile with fillet,
chamfer and through holes.  Usage: make_test_step.py out.step"""
import math
import sys


class W:
    def __init__(self):
        self.lines = []
        self.n = 0

    def add(self, s):
        self.n += 1
        self.lines.append(f"#{self.n}={s};")
        return self.n

    def pt(self, p):
        return self.add("CARTESIAN_POINT('',(%s))" % ",".join(f"{c:.6f}" for c in p))

    def dir(self, d):
        l = math.sqrt(sum(c * c for c in d))
        return self.add("DIRECTION('',(%s))" % ",".join(f"{c / l:.9f}" for c in d))

    def ax(self, o, z, x):
        return self.add(f"AXIS2_PLACEMENT_3D('',#{self.pt(o)},#{self.dir(z)},#{self.dir(x)})")


def main(path):
    H = 20.0
    # CCW profile in XY: list of (kind, start, end, centre)
    R = 10.0
    prof = [
        ("line", (0, 0), (100, 0)),
        ("line", (100, 0), (100, 40)),
        ("line", (100, 40), (80, 60)),          # 45 deg chamfer
        ("line", (80, 60), (R, 60)),
        ("arc", (R, 60), (0, 60 - R), (R, 60 - R)),  # fillet R10
        ("line", (0, 60 - R), (0, 0)),
    ]
    holes = [((25.0, 25.0), 6.0), ((75.0, 20.0), 4.0)]

    w = W()
    vb, vt = {}, {}
    for seg in prof:
        p = seg[1]
        vb[p] = w.add(f"VERTEX_POINT('',#{w.pt((p[0], p[1], 0))})")
        vt[p] = w.add(f"VERTEX_POINT('',#{w.pt((p[0], p[1], H))})")

    def line_edge(a, b, va, vb_):
        d = [b[i] - a[i] for i in range(3)]
        l = math.sqrt(sum(c * c for c in d))
        vec = w.add(f"VECTOR('',#{w.dir(d)},{l:.6f})")
        ln = w.add(f"LINE('',#{w.pt(a)},#{vec})")
        return w.add(f"EDGE_CURVE('',#{va},#{vb_},#{ln},.T.)")

    def circle(c, r, z=(0, 0, 1), x=(1, 0, 0)):
        return w.add(f"CIRCLE('',#{w.ax(c, z, x)},{r:.6f})")

    bot_e, top_e, side_faces_geo = [], [], []
    for seg in prof:
        kind, a, b = seg[0], seg[1], seg[2]
        if kind == "line":
            eb = line_edge((a[0], a[1], 0), (b[0], b[1], 0), vb[a], vb[b])
            et = line_edge((a[0], a[1], H), (b[0], b[1], H), vt[a], vt[b])
        else:
            c = seg[3]
            r = math.dist(a, c)
            eb = w.add(f"EDGE_CURVE('',#{vb[a]},#{vb[b]},#{circle((c[0], c[1], 0), r)},.T.)")
            et = w.add(f"EDGE_CURVE('',#{vt[a]},#{vt[b]},#{circle((c[0], c[1], H), r)},.T.)")
        bot_e.append(eb)
        top_e.append(et)
    vert_e = {}
    for seg in prof:
        p = seg[1]
        vert_e[p] = line_edge((p[0], p[1], 0), (p[0], p[1], H), vb[p], vt[p])

    def oe(e, f):
        return w.add(f"ORIENTED_EDGE('',*,*,#{e},{'.T.' if f else '.F.'})")

    def loop(oes, outer=True):
        el = w.add("EDGE_LOOP('',(%s))" % ",".join(f"#{o}" for o in oes))
        kind = "FACE_OUTER_BOUND" if outer else "FACE_BOUND"
        return w.add(f"{kind}('',#{el},.T.)")

    faces = []
    # side faces
    for i, seg in enumerate(prof):
        kind, a, b = seg[0], seg[1], seg[2]
        lp = loop([oe(bot_e[i], True), oe(vert_e[b], True), oe(top_e[i], False), oe(vert_e[a], False)])
        if kind == "line":
            dx, dy = b[0] - a[0], b[1] - a[1]
            n = (dy, -dx, 0)
            surf = w.add(f"PLANE('',#{w.ax((a[0], a[1], 0), n, (dx, dy, 0))})")
            faces.append(w.add(f"ADVANCED_FACE('',(#{lp}),#{surf},.T.)"))
        else:
            c = seg[3]
            r = math.dist(a, c)
            surf = w.add(f"CYLINDRICAL_SURFACE('',#{w.ax((c[0], c[1], 0), (0, 0, 1), (1, 0, 0))},{r:.6f})")
            faces.append(w.add(f"ADVANCED_FACE('',(#{lp}),#{surf},.T.)"))

    # holes
    hole_bounds_bot, hole_bounds_top = [], []
    for (c, r) in holes:
        pb = (c[0] + r, c[1], 0)
        pt_ = (c[0] + r, c[1], H)
        hvb = w.add(f"VERTEX_POINT('',#{w.pt(pb)})")
        hvt = w.add(f"VERTEX_POINT('',#{w.pt(pt_)})")
        cb = w.add(f"EDGE_CURVE('',#{hvb},#{hvb},#{circle((c[0], c[1], 0), r)},.T.)")
        ct = w.add(f"EDGE_CURVE('',#{hvt},#{hvt},#{circle((c[0], c[1], H), r)},.T.)")
        seam = line_edge(pb, pt_, hvb, hvt)
        lp = loop([oe(cb, False), oe(seam, True), oe(ct, True), oe(seam, False)])
        surf = w.add(f"CYLINDRICAL_SURFACE('',#{w.ax((c[0], c[1], 0), (0, 0, 1), (1, 0, 0))},{r:.6f})")
        faces.append(w.add(f"ADVANCED_FACE('',(#{lp}),#{surf},.F.)"))
        hole_bounds_bot.append(loop([oe(cb, True)], False))
        hole_bounds_top.append(loop([oe(ct, False)], False))

    # bottom (normal -Z): profile reversed
    lp = loop([oe(e, False) for e in reversed(bot_e)])
    surf = w.add(f"PLANE('',#{w.ax((0, 0, 0), (0, 0, -1), (1, 0, 0))})")
    faces.append(w.add("ADVANCED_FACE('',(%s),#%d,.T.)" % (",".join(f"#{b}" for b in [lp] + hole_bounds_bot), surf)))
    lp = loop([oe(e, True) for e in top_e])
    surf = w.add(f"PLANE('',#{w.ax((0, 0, H), (0, 0, 1), (1, 0, 0))})")
    faces.append(w.add("ADVANCED_FACE('',(%s),#%d,.T.)" % (",".join(f"#{b}" for b in [lp] + hole_bounds_top), surf)))

    shell = w.add("CLOSED_SHELL('',(%s))" % ",".join(f"#{f}" for f in faces))
    brep = w.add(f"MANIFOLD_SOLID_BREP('bracket',#{shell})")
    lu = w.add("( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.) )")
    au = w.add("( NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.) )")
    su = w.add("( NAMED_UNIT(*) SI_UNIT($,.STERADIAN.) SOLID_ANGLE_UNIT() )")
    unc = w.add(f"UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-07),#{lu},'distance_accuracy_value','')")
    ctx = w.add(
        f"( GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{unc})) "
        f"GLOBAL_UNIT_ASSIGNED_CONTEXT((#{lu},#{au},#{su})) REPRESENTATION_CONTEXT('Context','3D') )"
    )
    origin = w.ax((0, 0, 0), (0, 0, 1), (1, 0, 0))
    w.add(f"ADVANCED_BREP_SHAPE_REPRESENTATION('',(#{origin},#{brep}),#{ctx})")

    hdr = (
        "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('test bracket'),'2;1');\n"
        "FILE_NAME('bracket.step','2026-01-01T00:00:00',(''),(''),'make_test_step.py','','');\n"
        "FILE_SCHEMA(('AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }'));\nENDSEC;\nDATA;\n"
    )
    with open(path, "w") as f:
        f.write(hdr + "\n".join(w.lines) + "\nENDSEC;\nEND-ISO-10303-21;\n")


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "bracket.step")
