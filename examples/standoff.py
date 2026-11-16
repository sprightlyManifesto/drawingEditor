from cadquery import Workplane as WP
import cadquery as cq

a = WP().circle(10/2).circle(5/2).extrude(10)

cq.exporters.export(a,"standoff.step")