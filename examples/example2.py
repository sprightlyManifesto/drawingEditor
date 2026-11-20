import cadquery as cq
from cadquery import Workplane as WP

a = WP().circle(40/2).extrude(10).faces("<Z").edges().fillet(1)

a = a.cut(WP().polygon(7,20,forConstruction=True).vertices().circle(1).extrude(10))
a = a.cut(WP().polygon(7,21).extrude(10))
a = a.cut(WP().polygon(30,40.5,forConstruction=True).vertices().circle(1).extrude(10))
a = a.edges("|Z").fillet(0.4)

cq.exporters.export(a,"examples2.step")