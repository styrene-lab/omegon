use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::collections::HashSet;

const MAX_DEPTH: usize = 64;

pub fn resolve_refs(doc: &mut Value) -> Result<()> {
    let root = doc.clone();
    let mut resolving = HashSet::new();
    resolve_value(doc, &root, &mut resolving, 0)
}

fn resolve_value(
    node: &mut Value,
    root: &Value,
    resolving: &mut HashSet<String>,
    depth: usize,
) -> Result<()> {
    if depth > MAX_DEPTH {
        *node = circular_placeholder();
        return Ok(());
    }

    match node {
        Value::Object(map) => {
            if let Some(Value::String(ref_str)) = map.get("$ref") {
                let pointer = ref_str.clone();
                if !pointer.starts_with("#/") {
                    bail!("unsupported $ref format: {pointer}");
                }
                let json_pointer = pointer.strip_prefix('#').unwrap();

                if resolving.contains(&pointer) {
                    *node = circular_placeholder();
                    return Ok(());
                }

                let resolved = root
                    .pointer(json_pointer)
                    .with_context(|| format!("unresolvable $ref: {pointer}"))?
                    .clone();

                resolving.insert(pointer.clone());
                *node = resolved;
                resolve_value(node, root, resolving, depth + 1)?;
                resolving.remove(&pointer);
            } else {
                let keys: Vec<String> = map.keys().cloned().collect();
                for key in keys {
                    if let Some(child) = map.get_mut(&key) {
                        resolve_value(child, root, resolving, depth + 1)?;
                    }
                }
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                resolve_value(item, root, resolving, depth + 1)?;
            }
        }
        _ => {}
    }

    Ok(())
}

fn circular_placeholder() -> Value {
    json!({
        "type": "object",
        "description": "(circular reference)"
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resolves_simple_ref() {
        let mut doc = json!({
            "components": {
                "schemas": {
                    "Pet": { "type": "object", "properties": { "name": { "type": "string" } } }
                }
            },
            "paths": {
                "/pets": { "schema": { "$ref": "#/components/schemas/Pet" } }
            }
        });
        resolve_refs(&mut doc).unwrap();
        assert_eq!(
            doc["paths"]["/pets"]["schema"]["properties"]["name"]["type"],
            "string"
        );
    }

    #[test]
    fn resolves_nested_refs() {
        let mut doc = json!({
            "components": {
                "schemas": {
                    "Address": { "type": "object" },
                    "Person": {
                        "type": "object",
                        "properties": {
                            "address": { "$ref": "#/components/schemas/Address" }
                        }
                    }
                }
            },
            "root": { "$ref": "#/components/schemas/Person" }
        });
        resolve_refs(&mut doc).unwrap();
        assert_eq!(doc["root"]["properties"]["address"]["type"], "object");
    }

    #[test]
    fn circular_ref_replaced_with_placeholder() {
        let mut doc = json!({
            "components": {
                "schemas": {
                    "Node": {
                        "type": "object",
                        "properties": {
                            "child": { "$ref": "#/components/schemas/Node" }
                        }
                    }
                }
            },
            "root": { "$ref": "#/components/schemas/Node" }
        });
        resolve_refs(&mut doc).unwrap();
        assert_eq!(
            doc["root"]["properties"]["child"]["description"],
            "(circular reference)"
        );
    }

    #[test]
    fn unresolvable_ref_returns_error() {
        let mut doc = json!({
            "field": { "$ref": "#/components/schemas/Missing" }
        });
        let err = resolve_refs(&mut doc).unwrap_err();
        assert!(err.to_string().contains("unresolvable"));
    }

    #[test]
    fn non_local_ref_returns_error() {
        let mut doc = json!({
            "field": { "$ref": "https://example.com/schema.json" }
        });
        let err = resolve_refs(&mut doc).unwrap_err();
        assert!(err.to_string().contains("unsupported"));
    }

    #[test]
    fn resolves_refs_inside_arrays() {
        let mut doc = json!({
            "components": {
                "schemas": {
                    "Tag": { "type": "string" }
                }
            },
            "items": [
                { "$ref": "#/components/schemas/Tag" },
                { "$ref": "#/components/schemas/Tag" }
            ]
        });
        resolve_refs(&mut doc).unwrap();
        assert_eq!(doc["items"][0]["type"], "string");
        assert_eq!(doc["items"][1]["type"], "string");
    }
}
