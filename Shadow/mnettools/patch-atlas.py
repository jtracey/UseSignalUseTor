#!/usr/bin/env python3

import sys
import networkx as nx
import copy

if len(sys.argv) != 3:
    print("Usage: {} atlas.gml output.gml".format(sys.argv[0]))
    exit(1)

# read first arg as a graph
g = nx.read_gml(sys.argv[1])

# add our 10gig node at a valid IP we know won't be in an atlas probe
new_node = "node at 1.1.1.1"
g.add_node(new_node,
           label=new_node,
           ip_address="1.1.1.1",
           city_code="4744870",
           country_code="US",
           bandwidth_down="10000000 Kibit",
           bandwidth_up="10000000 Kibit"
           )

# then duplicate all edges with source or dest of the node we're imitating,
imitated_node = "node at 76.104.11.141"
edges_to_copy = g.edges(imitated_node, data=True)

# but remaped to our new node
def remap_edge(e):
    if e[0] == imitated_node:
        e[0] = new_node
    if e[1] == imitated_node:
        e[1] = new_node
    return e
# (list(e) also gives us a shallow copy of e, good enough for us)
copied_edges = map(remap_edge, [list(e) for e in edges_to_copy])
g.add_edges_from(copied_edges)

# create an edge between the new and old node
edge = copy.deepcopy(g.get_edge_data(imitated_node, imitated_node))
edge['label'] = "path from 1.1.1.1 to 76.104.11.141"
g.add_edge(new_node, imitated_node)
g[new_node][imitated_node].update(edge)

# finally, save to the second arg
nx.write_gml(g, sys.argv[2])
