//! Per-extension config storage — read/write `config.toml` in each extension dir.

use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;

/// Read config values from `<ext_dir>/config.toml`.
/// Returns empty map if the file doesn't exist.
pub fn read_config(ext_dir: &Path) -> Result<HashMap<String, String>> {
    let config_path = ext_dir.join("config.toml");
    if !config_path.exists() {
        return Ok(HashMap::new());
    }
    let raw = std::fs::read_to_string(&config_path)?;
    let table: toml::Table = toml::from_str(&raw)?;
    let mut config = HashMap::new();
    for (key, value) in table {
        let str_val = match value {
            toml::Value::String(s) => s,
            toml::Value::Boolean(b) => b.to_string(),
            toml::Value::Integer(i) => i.to_string(),
            toml::Value::Float(f) => f.to_string(),
            other => other.to_string(),
        };
        config.insert(key, str_val);
    }
    Ok(config)
}

/// Write a single config value to `<ext_dir>/config.toml`.
/// Preserves other existing values.
pub fn write_config_value(ext_dir: &Path, key: &str, value: &str) -> Result<()> {
    let config_path = ext_dir.join("config.toml");
    let mut table: toml::Table = if config_path.exists() {
        let raw = std::fs::read_to_string(&config_path)?;
        toml::from_str(&raw).unwrap_or_default()
    } else {
        toml::Table::new()
    };
    table.insert(key.to_string(), toml::Value::String(value.to_string()));
    let raw = toml::to_string_pretty(&table)?;
    // Atomic write
    let tmp_path = config_path.with_extension("toml.tmp");
    std::fs::write(&tmp_path, &raw)?;
    std::fs::rename(&tmp_path, &config_path)?;
    Ok(())
}

/// Validate a value against a config field schema.
pub fn validate_field(
    field: &omegon_extension::ConfigField,
    value: &str,
) -> Result<()> {
    use omegon_extension::ConfigFieldType;
    match field.field_type {
        ConfigFieldType::Boolean => {
            if !matches!(value, "true" | "false") {
                anyhow::bail!("expected 'true' or 'false', got '{value}'");
            }
        }
        ConfigFieldType::Number => {
            if value.parse::<f64>().is_err() {
                anyhow::bail!("expected a number, got '{value}'");
            }
        }
        ConfigFieldType::Enum => {
            if !field.values.contains(&value.to_string()) {
                anyhow::bail!(
                    "value '{value}' not in allowed values: {:?}",
                    field.values
                );
            }
        }
        ConfigFieldType::String | ConfigFieldType::Text => {
            if let Some(ref pattern) = field.pattern {
                if let Ok(re) = regex_lite::Regex::new(pattern) {
                    if !re.is_match(value) {
                        anyhow::bail!("value does not match pattern: {pattern}");
                    }
                }
            }
        }
    }
    Ok(())
}
