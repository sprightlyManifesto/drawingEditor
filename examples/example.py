from cadquery import Workplane as WP
import cadquery as cq

a = WP().rect(100,100).extrude(10).translate((0,0,0)).edges("Z")
a = a.cut(WP().rect(30,30).rect(12,15).extrude(5).translate((30,0,5)).edges("Z").fillet(3).rotate((0,0,0),(0,0,1),30))
a = a.cut(WP().rect(20,20).extrude(10).translate((0,0,5)).edges("Z").fillet(3).rotate((0,0,0),(0,1,0),-10))

a = a.edges(">Z and <X").fillet(3)
a = a.edges(">Z and >X").chamfer(3)

a = a.cut(WP().circle(3.3/2).extrude(5,both=True).translate((-25,-25,10)))
a = a.cut(WP().pushPoints([(-25,22),(-25,-33)]).circle(5/2).extrude(10,both=True))

cq.exporters.export(a,"example.step")

