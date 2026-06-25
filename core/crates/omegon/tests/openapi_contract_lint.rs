//! OpenAPI contract linter.
//!
//! Structurally validates every `docs/**/*.openapi.{yaml,yml}` spec in the
//! repository. Rust-native on purpose: the project ships no Node/Python
//! toolchain, so the contract is gated by `cargo test` (and therefore by the
//! `rust-integration` CI job and `just test-rust`) rather than an external
//! linter like redocly.
//!
//! Checks performed per spec:
//!   - `openapi` is a 3.x version string
//!   - `info.title` and `info.version` are present and non-empty
//!   - `paths` is a non-empty object
//!   - every operation declares at least one response
//!   - `operationId`s are unique across the document
//!   - every path-template `{param}` has a matching `required: true` path parameter
//!   - every `$ref` is local (`#/...`) and resolves to an existing node

use std::path::{Path, PathBuf};

use serde_json::Value;

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR = <repo>/core/crates/omegon → repo root is three up.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("repo root is three ancestors above the crate manifest")
        .to_path_buf()
}

fn collect_specs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_specs(&path, out);
        } else if is_openapi_spec(&path) {
            out.push(path);
        }
    }
}

fn is_openapi_spec(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    name.ends_with(".openapi.yaml") || name.ends_with(".openapi.yml")
}

/// Walk the JSON tree, invoking `f` on every node with its JSON-pointer path.
fn walk<'a>(node: &'a Value, ptr: &mut String, f: &mut impl FnMut(&str, &'a Value)) {
    f(ptr, node);
    match node {
        Value::Object(map) => {
            for (k, v) in map {
                let len = ptr.len();
                ptr.push('/');
                ptr.push_str(&escape_token(k));
                walk(v, ptr, f);
                ptr.truncate(len);
            }
        }
        Value::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                let len = ptr.len();
                ptr.push('/');
                ptr.push_str(&i.to_string());
                walk(v, ptr, f);
                ptr.truncate(len);
            }
        }
        _ => {}
    }
}

fn escape_token(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

/// Resolve a local `$ref` (e.g. `#/components/schemas/Foo`) against the root.
fn ref_resolves(root: &Value, reference: &str) -> bool {
    let Some(pointer) = reference.strip_prefix('#') else {
        return false; // non-local refs are not allowed
    };
    if pointer.is_empty() {
        return true;
    }
    root.pointer(pointer).is_some()
}

fn path_template_params(path: &str) -> Vec<String> {
    let mut params = Vec::new();
    let mut rest = path;
    while let Some(open) = rest.find('{') {
        if let Some(close) = rest[open..].find('}') {
            let name = &rest[open + 1..open + close];
            if !name.is_empty() {
                params.push(name.to_string());
            }
            rest = &rest[open + close + 1..];
        } else {
            break;
        }
    }
    params
}

const HTTP_METHODS: &[&str] = &[
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];

fn lint_spec(path: &Path, errors: &mut Vec<String>) {
    let rel = path
        .strip_prefix(repo_root())
        .unwrap_or(path)
        .display()
        .to_string();
    let mut err = |msg: String| errors.push(format!("{rel}: {msg}"));

    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            err(format!("could not read spec: {e}"));
            return;
        }
    };
    let spec: Value = match serde_yaml::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            err(format!("YAML did not parse: {e}"));
            return;
        }
    };

    // openapi version
    match spec.get("openapi").and_then(Value::as_str) {
        Some(v) if v.starts_with("3.") => {}
        Some(v) => err(format!("unsupported openapi version {v:?} (expected 3.x)")),
        None => err("missing `openapi` version string".to_string()),
    }

    // info
    for field in ["title", "version"] {
        let present = spec
            .pointer(&format!("/info/{field}"))
            .and_then(Value::as_str)
            .is_some_and(|s| !s.trim().is_empty());
        if !present {
            err(format!("missing or empty `info.{field}`"));
        }
    }

    // paths
    let Some(paths) = spec.get("paths").and_then(Value::as_object) else {
        err("missing `paths` object".to_string());
        return;
    };
    if paths.is_empty() {
        err("`paths` is empty".to_string());
    }

    // Per-path / per-operation checks + operationId uniqueness.
    let mut seen_op_ids: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for (route, item) in paths {
        let Some(item_obj) = item.as_object() else {
            err(format!("path {route:?} is not an object"));
            continue;
        };
        let template_params = path_template_params(route);
        let path_level_params = collect_param_names(item.get("parameters"));

        for method in HTTP_METHODS {
            let Some(op) = item_obj.get(*method) else {
                continue;
            };

            // responses present and non-empty
            match op.get("responses").and_then(Value::as_object) {
                Some(r) if !r.is_empty() => {}
                _ => err(format!(
                    "{} {route}: operation has no responses",
                    method.to_uppercase()
                )),
            }

            // operationId unique
            match op.get("operationId").and_then(Value::as_str) {
                Some(id) => {
                    if let Some(prev) = seen_op_ids
                        .insert(id.to_string(), format!("{} {route}", method.to_uppercase()))
                    {
                        err(format!(
                            "duplicate operationId {id:?} ({} {route} and {prev})",
                            method.to_uppercase()
                        ));
                    }
                }
                None => err(format!(
                    "{} {route}: missing operationId",
                    method.to_uppercase()
                )),
            }

            // path-template params must be declared as required path params
            let op_params = collect_required_path_params(op.get("parameters"));
            for tp in &template_params {
                let declared = op_params.contains(tp) || path_level_params.contains(tp);
                if !declared {
                    err(format!(
                        "{} {route}: path template param {{{tp}}} has no `required: true` path parameter",
                        method.to_uppercase()
                    ));
                }
            }
        }
    }

    // Every $ref is local and resolves.
    let mut ptr = String::new();
    let mut ref_errors: Vec<String> = Vec::new();
    walk(&spec, &mut ptr, &mut |where_at, node| {
        if let Some(Value::String(reference)) = node.get("$ref") {
            if !reference.starts_with('#') {
                ref_errors.push(format!("non-local $ref {reference:?} at {where_at}"));
            } else if !ref_resolves(&spec, reference) {
                ref_errors.push(format!("dangling $ref {reference:?} at {where_at}"));
            }
        }
    });
    for re in ref_errors {
        err(re);
    }
}

/// Names of parameters in a `parameters` array that are `in: path` and required.
fn collect_required_path_params(params: Option<&Value>) -> Vec<String> {
    let Some(arr) = params.and_then(Value::as_array) else {
        return Vec::new();
    };
    arr.iter()
        .filter(|p| p.get("in").and_then(Value::as_str) == Some("path"))
        .filter(|p| p.get("required").and_then(Value::as_bool) == Some(true))
        .filter_map(|p| p.get("name").and_then(Value::as_str).map(str::to_string))
        .collect()
}

/// Names of all parameters declared at the path-item level (any `in`).
fn collect_param_names(params: Option<&Value>) -> Vec<String> {
    collect_required_path_params(params)
}

#[test]
fn openapi_specs_are_structurally_valid() {
    let docs = repo_root().join("docs");
    let mut specs = Vec::new();
    collect_specs(&docs, &mut specs);
    specs.sort();

    assert!(
        !specs.is_empty(),
        "expected at least one docs/**/*.openapi.yaml spec to lint; found none under {}",
        docs.display()
    );

    let mut errors = Vec::new();
    for spec in &specs {
        lint_spec(spec, &mut errors);
    }

    assert!(
        errors.is_empty(),
        "OpenAPI contract lint failed ({} spec(s) checked):\n  - {}",
        specs.len(),
        errors.join("\n  - ")
    );
}

#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn extracts_path_template_params() {
        assert_eq!(path_template_params("/a/{id}/b/{kind}"), vec!["id", "kind"]);
        assert!(path_template_params("/static/path").is_empty());
    }

    #[test]
    fn local_ref_resolution() {
        let root = serde_json::json!({
            "components": { "schemas": { "Foo": { "type": "object" } } }
        });
        assert!(ref_resolves(&root, "#/components/schemas/Foo"));
        assert!(!ref_resolves(&root, "#/components/schemas/Missing"));
        assert!(!ref_resolves(&root, "https://example.com/x.yaml#/Foo"));
    }

    #[test]
    fn walk_visits_nested_ref() {
        let doc = serde_json::json!({
            "a": { "b": [ { "$ref": "#/x" } ] }
        });
        let mut found = Vec::new();
        let mut ptr = String::new();
        walk(&doc, &mut ptr, &mut |where_at, node| {
            if node.get("$ref").is_some() {
                found.push(where_at.to_string());
            }
        });
        assert_eq!(found, vec!["/a/b/0"]);
    }
}
