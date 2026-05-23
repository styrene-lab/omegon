# Cockpit reader test assets

Scratch assets for testing `.tmp/cockpit-probe` with reader-like files.

Files:

- `pride-and-prejudice.epub` — Project Gutenberg EPUB no-images edition, Jane Austen, public domain. Source: https://www.gutenberg.org/ebooks/1342
- `dummy.pdf` — W3C WAI test dummy PDF. Source: https://www.w3.org/WAI/ER/tests/xhtml/testfiles/resources/pdf/dummy.pdf

Example runs from repo root:

```bash
cd .tmp/cockpit-probe
cargo run -- /usr/bin/less ../cockpit-test-assets/README.md
cargo run -- bookokrat ../cockpit-test-assets/pride-and-prejudice.epub
cargo run -- bookokrat ../cockpit-test-assets/dummy.pdf
```

Controls in the probe:

- `Ctrl+N` toggles focus into the embedded reader PTY.
- `Ctrl+Q` exits the probe.
