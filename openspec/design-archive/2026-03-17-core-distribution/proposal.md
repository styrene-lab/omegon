+++
id = "5bd9a450-558a-417b-b50e-697c1cf580f9"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Core binary distribution — shipping Rust alongside and eventually instead of TypeScript

## Intent

Omegon today ships as a single npm package (`npm install -g omegon`, ~191MB unpacked) containing the TypeScript runtime + vendored pi-mono. The Rust core lives in a separate repo (styrene-lab/omegon-core) submoduled at `core/`. This node defines how the Rust binary gets built, distributed, versioned, and integrated — from Phase 0 (binary bundled alongside npm package for cleave children) through Phase 3 (Rust binary IS the product, Node.js optional).

See [design doc](../../../docs/core-distribution.md).
