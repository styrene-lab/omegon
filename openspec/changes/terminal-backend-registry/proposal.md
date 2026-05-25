# Terminal Backend Registry

## Intent

Route `terminal.create@1` through a host-side backend registry so visual terminal hosts can satisfy placement requests without changing extensions such as `omegon-reader`.

## Scope

- Add a backend selection seam behind `terminal.create@1`.
- Preserve current portable PTY background behavior as the fallback backend.
- Return structured degradation warnings when requested placement cannot be satisfied.
- Test fake visual backend preference for `side_pane`.
- Keep manifest/runtime policy before backend execution.

## Non-goals

- Implement Flynt renderer integration inside Omegon.
- Replace the `terminal.create@1` wire contract.
- Remove background PTY fallback.
- Implement ACP delegated terminal backend in this slice.
