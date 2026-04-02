---
name: rust
description: Rust development guidance. Covers project setup, testing, clippy, rustfmt, and CI/CD patterns. Use when creating, modifying, or debugging Rust code.
---

# Rust Development Skill

Conventions for Rust development, with a dedicated section for Zellij WASM plugin development.

## Core Conventions

- **Rust stable** toolchain (`rustup default stable`)
- **Cargo** for build, test, lint, format — no external build tools needed
- **clippy** for linting, **rustfmt** for formatting
- **Edition 2021** minimum
- Workspace layout for multi-crate projects, single `Cargo.toml` otherwise

## Project Scaffold

```
<project>/
├── Cargo.toml              # Package metadata, deps, lint config
├── rustfmt.toml            # max_width = 100
├── src/
│   ├── lib.rs              # Library root (or main.rs for binary)
│   └── ...
├── tests/
│   └── integration_test.rs
└── .github/workflows/ci.yml
```

## Tooling Quick Reference

### Clippy (Linting)

```bash
cargo clippy                        # Lint
cargo clippy -- -D warnings         # Warnings as errors (CI)
cargo clippy --all-targets           # Include tests/benches
cargo clippy --fix                   # Auto-fix
```

Project config in `Cargo.toml`:
```toml
[lints.clippy]
pedantic = { level = "warn", priority = -1 }
unwrap_used = "warn"
```

### Rustfmt (Formatting)

```bash
cargo fmt                           # Format
cargo fmt -- --check                # Check only (CI)
```

### Build & Test

```bash
cargo build                         # Debug build
cargo build --release               # Release build
cargo test                          # All tests
cargo test -- --nocapture            # Show println output
cargo test test_name                 # Specific test
cargo test --lib                     # Unit tests only
cargo test --test integration_test   # Specific integration test
```

### Other Useful Commands

```bash
cargo doc --open                    # Generate and browse docs
cargo audit                         # Security vulnerability check
cargo tree                          # Dependency tree
cargo expand                        # Macro expansion
```

## Testing Patterns

### Unit Tests (in-module)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid() {
        let result = parse("input");
        assert_eq!(result, expected);
    }
}
```

### Async Tests (tokio)

```rust
#[tokio::test]
async fn test_async_op() {
    let result = fetch_data().await;
    assert!(result.is_ok());
}
```

## Error Handling

| Context | Pattern |
|---------|---------|
| Libraries | `thiserror::Error` derive for custom error types |
| Applications | `anyhow::Result` for ergonomic error propagation |
| Unwrap | Never in library code; `expect("reason")` in main/tests only |

## Common Dependencies

| Crate | Purpose |
|-------|---------|
| `serde` + `serde_json` | Serialization |
| `tokio` | Async runtime |
| `anyhow` / `thiserror` | Error handling |
| `clap` | CLI parsing |
| `tracing` | Structured logging |

## CI/CD

```yaml
name: CI
on: [push, pull_request]
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo fmt -- --check
      - run: cargo clippy -- -D warnings
      - run: cargo test
```
