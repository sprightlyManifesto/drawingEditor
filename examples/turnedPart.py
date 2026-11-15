from cadquery import Workplane as WP
import cadquery as cq
from cadquery.selectors import RadiusNthSelector as NthR

pts = [(0,0),(10,0),(10,10),(20,10),(20,15),(40,15),(40,20),(45,20),(45,10),(60,10),(60,0)]

a = WP().polyline(pts).close().revolve(360,(0,0),(1,0))

a = a.faces(">X[1]").edges(NthR(0)).fillet(1)
a = a.faces(">X[2]").edges(NthR(0)).fillet(1)
a = a.faces(">X[3]").edges(NthR(0)).fillet(1)

cq.exporters.export(a,"turnedPart.step")