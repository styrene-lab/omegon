+++
id = "00bd7cab-e4dc-4c0b-a67b-421fac52f8ae"
name = "security"
description = "Defensive coding practices for implementation and review"
tags = []
aliases = ["sec"]
activation = "always"
profile = ["coding"]
+++

# Security Skill

Defensive coding practices. Apply these checks during implementation and review.

## Input Escaping

### Never interpolate user input into templates, scripts, or SQL

```rust
// ❌ XSS: user-controlled value injected into JS string
let js = format!("var id = '{}';", user_input);

// ✅ Escape special characters before interpolation
fn escape_js_string(s: &str) -> String {
    s.replace('\\', "\\\\")
     .replace('\'', "\\'")
     .replace('"', "\\\"")
     .replace('<', "\\x3c")
     .replace('>', "\\x3e")
     .replace('\n', "\\n")
     .replace('\r', "\\r")
}
let js = format!("var id = '{}';", escape_js_string(user_input));
```

```typescript
// ❌ Template injection
const html = `<div>${userInput}</div>`;

// ✅ Escape HTML entities
function escapeHtml(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;")
          .replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}
const html = `<div>${escapeHtml(userInput)}</div>`;
```

### Context matters — escape for the target language

| Context | Escape | Characters |
|---------|--------|------------|
| HTML body | HTML entities | `& < > "` |
| HTML attribute | HTML entities + quote | `& < > " '` |
| JavaScript string | JS escapes | `\ ' " < > \n \r` |
| URL parameter | `encodeURIComponent` | All non-unreserved |
| SQL | Parameterized queries | Never interpolate |
| Shell command | Avoid if possible | Use `spawn(cmd, [args])` not `exec(string)` |

## Path Traversal

### Validate that resolved paths stay within the expected root

```typescript
// ❌ User can escape with ../../etc/passwd
const filePath = join(rootDir, userInput);

// ✅ Resolve and verify containment
const resolved = resolve(rootDir, userInput);
if (!resolved.startsWith(resolve(rootDir) + sep) && resolved !== resolve(rootDir)) {
  throw new Error("Path traversal attempt");
}
```

```rust
// ❌ No validation
let path = root.join(&user_path);

// ✅ Canonicalize and check prefix
let canonical = path.canonicalize()?;
if !canonical.starts_with(&root.canonicalize()?) {
    return Err("Path traversal".into());
}
```

### Reject suspicious path components

- `..` segments
- Null bytes (`%00`, `\0`)
- Absolute paths when relative expected
- Symlinks that escape the root (use `canonicalize`/`realpath`)

## Process Spawning

### Use `spawn` with argument arrays, never shell interpolation

```typescript
// ❌ Shell injection via userInput
execSync(`grep ${userInput} file.txt`);

// ✅ Arguments are not shell-interpreted
spawn("grep", [userInput, "file.txt"], { stdio: "pipe" });
```

### Never use `stdio: "inherit"` in TUI applications

Inherited stdio corrupts the terminal UI. Always use `"pipe"` or `"ignore"`.

```typescript
// ❌ Corrupts TUI
spawn("some-tool", [], { stdio: "inherit" });

// ✅ Capture or discard
spawn("some-tool", [], { stdio: "pipe" });
spawn("some-tool", [], { stdio: "ignore" }); // fire-and-forget
```

### Limit `pkill`/`killall` scope

```typescript
// ❌ Matches any process with "serve" in its command line
execSync("pkill -f 'ollama serve'");

// ✅ Track the PID you spawned and kill that specifically
if (child) { child.kill("SIGTERM"); child = null; }
```

### Set timeouts on child processes

```typescript
const child = spawn("cmd", args);
const timer = setTimeout(() => {
  child.kill("SIGTERM");
}, 300_000); // 5 min timeout

child.on("exit", () => clearTimeout(timer));
```

## Dependency Integrity

### Pin CDN resources with Subresource Integrity (SRI)

```html
<!-- ❌ No integrity check — CDN compromise = XSS -->
<script src="https://cdn.example.com/lib.js"></script>

<!-- ✅ SRI hash — browser rejects tampered content -->
<script src="https://cdn.example.com/lib.js"
        integrity="sha384-abc123..."
        crossorigin="anonymous"></script>
```

### Prefer bundling over CDN for offline-first tools

```rust
// ✅ Bundle at compile time — no network dependency
const JS: &str = include_str!("../static/lib.min.js");
```

```typescript
// ✅ Import from node_modules, bundler handles it
import { force } from "force-graph";
```

## Secrets Management

- **Never hardcode secrets** — use environment variables or secret managers.
- **Never log secrets** — mask or omit sensitive values from log output.
- **Never commit secrets** — use `.gitignore` and pre-commit hooks.
- **Rotate on exposure** — if a secret appears in a commit, rotate immediately.

```typescript
// ❌ Hardcoded
const apiKey = "sk-abc123";

// ✅ Environment variable
const apiKey = process.env.API_KEY;
if (!apiKey) throw new Error("API_KEY not set");
```

## TOCTOU (Time-of-Check to Time-of-Use)

When you check a condition and then act on it, the condition may change between check and action.

```typescript
// ❌ TOCTOU: file may be deleted between check and read
if (existsSync(path)) {
  const data = readFileSync(path); // may throw ENOENT
}

// ✅ Just try the operation and handle the error
try {
  const data = readFileSync(path);
} catch (err) {
  if ((err as NodeJS.ErrnoException).code === "ENOENT") {
    // file doesn't exist — handle gracefully
  } else throw err;
}
```

## Checklist

Before submitting code that handles external input:

- [ ] All user/external input is escaped for its target context
- [ ] File paths are validated against a root directory
- [ ] Child processes use argument arrays, not shell strings
- [ ] No `stdio: "inherit"` in TUI-hosted code
- [ ] CDN resources have SRI hashes or are bundled locally
- [ ] No secrets in source code or logs
- [ ] Error messages don't leak internal paths or stack traces to end users
- [ ] Timeouts set on all network requests and child processes
