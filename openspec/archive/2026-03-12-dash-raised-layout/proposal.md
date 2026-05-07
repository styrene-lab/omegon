+++
id = "3b283663-d4ff-41b5-acac-e2d2c721ce00"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Raised Dashboard: Horizontal Split Layout

## Intent

The raised /dash view currently stacks all sections (Design Tree → OpenSpec → Recovery → Cleave → Meta → Footer) vertically, consuming 10 lines while only filling ~half the terminal width. Terminals are typically much wider than tall. A proper left/right column split would use available horizontal space and show more useful content at once.

Key observation: `renderRaisedColumns()` already exists in footer.ts but is never called from `renderRaised()` — it's dead code. That method is a start but needs refinement.
