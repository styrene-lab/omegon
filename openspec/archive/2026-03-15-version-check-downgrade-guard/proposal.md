+++
id = "8f69b73c-0041-46c0-8cbb-387f4e7dee8a"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Version check downgrade guard — suppress false update prompts from older registry versions

## Intent

Fix false update notifications caused by treating any registry version mismatch as an available update, even when the registry reports an older version than the running build.
