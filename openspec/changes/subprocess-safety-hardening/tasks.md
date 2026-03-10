# Subprocess safety hardening — Tasks

## 1. Harden browser launch helpers
<!-- specs: security/processes -->

- [ ] 1.1 Replace `extensions/web-ui/index.ts` browser opening with explicit executable + argv spawning
- [ ] 1.2 Preserve cross-platform browser-open behavior for macOS, Linux, and Windows launchers
- [ ] 1.3 Update/add tests for safer web-ui browser launching

## 2. Remove broad Ollama shutdown patterns
<!-- specs: security/processes -->

- [ ] 2.1 Replace broad `pkill -f` shutdown in `extensions/local-inference/index.ts` with tracked-child termination or a narrowly scoped fallback
- [ ] 2.2 Ensure shutdown when no managed Ollama child exists does not terminate unrelated processes
- [ ] 2.3 Update/add tests for managed and no-managed-child shutdown behavior

## 3. Tighten bootstrap helper execution boundaries
<!-- specs: security/processes -->

- [ ] 3.1 Audit the bootstrap helper execution path in `extensions/bootstrap/index.ts`
- [ ] 3.2 Replace shell-fragment concatenation with explicit executable + argv dispatch where feasible, or isolate any unavoidable shell usage behind a constrained helper wrapper
- [ ] 3.3 Update/add regression tests for hardened bootstrap helper execution

## 4. Validate the subprocess hardening slice
<!-- specs: security/processes -->

- [ ] 4.1 Run targeted tests for web-ui, local-inference, and bootstrap subprocess behavior
- [ ] 4.2 Run `npm run typecheck`
