+++
id = "3cc272b1-0e52-4955-bec4-5ae6a92e75c0"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Knowledge quadrant lifecycle — guide design progression through the Rumsfeld Matrix

## Intent

The design tree's status machine (seed → exploring → decided → implementing → implemented) implicitly tracks knowledge state, but doesn't make it explicit. The Rumsfeld Matrix (Known Knowns / Known Unknowns / Unknown Knowns / Unknown Unknowns) provides a framework that maps directly onto this lifecycle and could guide progression organically.\n\nThe hypothesis: a design node is ready to advance when everything relevant has moved OUT of the 'Unknown' column and INTO the 'Known' column. The design tree already tracks the pieces (open questions = known unknowns, decisions = known knowns, research = the process of discovery) — we just need to surface the quadrant state explicitly.
