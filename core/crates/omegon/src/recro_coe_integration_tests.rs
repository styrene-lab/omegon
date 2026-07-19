//! Opt-in host compatibility test for the separately versioned Recro COE extension.
//! Set `OMEGON_RECRO_COE_DIR` to its checkout to exercise the real binary/package.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::extensions::manifest::ExtensionManifest;
use crate::extensions::sdk_compat::SdkCompatibilityStatus;

fn recro_checkout() -> Option<PathBuf> {
    std::env::var_os("OMEGON_RECRO_COE_DIR").map(PathBuf::from)
}

struct EnvRestore {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvRestore {
    fn set(key: &'static str, value: &std::ffi::OsStr) -> Self {
        let previous = std::env::var_os(key);
        unsafe { std::env::set_var(key, value) };
        Self { key, previous }
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(value) => unsafe { std::env::set_var(self.key, value) },
            None => unsafe { std::env::remove_var(self.key) },
        }
    }
}

fn build_extension(checkout: &Path) -> Result<()> {
    let status = Command::new("cargo")
        .args(["build", "--release", "--locked"])
        .current_dir(checkout)
        .status()
        .context("failed to start Recro COE release build")?;
    if !status.success() {
        bail!("Recro COE release build failed with {status}");
    }
    Ok(())
}

fn stage_extension(checkout: &Path, extension_dir: &Path, data_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(extension_dir.join("target/release"))?;
    std::fs::create_dir_all(extension_dir.join("skills/recro-coe"))?;
    std::fs::copy(
        checkout.join("target/release/omegon-recro-coe"),
        extension_dir.join("target/release/omegon-recro-coe"),
    )?;
    std::fs::copy(
        checkout.join("skills/recro-coe/skill.md"),
        extension_dir.join("skills/recro-coe/skill.md"),
    )?;
    let manifest_path = checkout.join("manifest.toml");
    let manifest_source = std::fs::read_to_string(&manifest_path)?;
    let mut manifest: toml::Value = toml::from_str(&manifest_source)
        .with_context(|| format!("parse {}", manifest_path.display()))?;
    let runtime = manifest
        .get_mut("runtime")
        .and_then(toml::Value::as_table_mut)
        .context("Recro COE manifest runtime table")?;
    let mut runtime_config = toml::map::Map::new();
    runtime_config.insert(
        "data_dir".into(),
        toml::Value::String(data_dir.display().to_string()),
    );
    runtime.insert("config".into(), toml::Value::Table(runtime_config));
    std::fs::write(
        extension_dir.join("manifest.toml"),
        toml::to_string(&manifest)?,
    )?;
    Ok(())
}

#[tokio::test]
async fn real_recro_coe_manifest_handshake_config_tools_skill_and_execution() -> Result<()> {
    let _env_guard = crate::test_support::env::lock_async().await;
    let Some(checkout) = recro_checkout() else {
        eprintln!("skipping: OMEGON_RECRO_COE_DIR is not set");
        return Ok(());
    };
    let checkout = checkout
        .canonicalize()
        .context("canonicalize Recro COE checkout")?;
    build_extension(&checkout)?;

    let temp = tempfile::tempdir()?;
    let home = temp.path().join("home");
    let extension_dir = home.join("extensions/omegon-recro-coe");
    let data_dir = temp.path().join("data");
    stage_extension(&checkout, &extension_dir, &data_dir)?;
    let _home_guard = EnvRestore::set("OMEGON_HOME", home.as_os_str());

    let manifest = ExtensionManifest::from_extension_dir(&extension_dir)?;
    assert_eq!(manifest.extension.name, "omegon-recro-coe");
    assert_eq!(manifest.skills.len(), 1);
    assert_eq!(manifest.skills[0].path, "skills/recro-coe/skill.md");

    let spawned = crate::extensions::spawn_from_manifest(&extension_dir, &[]).await?;
    assert_eq!(
        spawned.sdk_compatibility.status,
        SdkCompatibilityStatus::Supported
    );
    let metadata = spawned.metadata.as_ref().context("initialize metadata")?;
    assert_eq!(metadata["extension_info"]["name"], "omegon-recro-coe");
    assert_eq!(metadata["extension_info"]["sdk_version"], "0.25");

    let tools = spawned.feature.tools();
    let tool_names: std::collections::HashSet<_> =
        tools.iter().map(|tool| tool.name.as_str()).collect();
    assert_eq!(
        tool_names.len(),
        tools.len(),
        "duplicate Recro COE tool names"
    );
    for required in [
        "recro_workspace_init",
        "recro_partnership_create",
        "recro_partnership_get",
        "recro_report",
    ] {
        assert!(
            tool_names.contains(required),
            "missing required tool {required}"
        );
    }

    let result = spawned
        .feature
        .execute(
            "recro_workspace_init",
            "recro-host-init",
            json!({"workspace_id": "field-litmus"}),
            CancellationToken::new(),
        )
        .await?;
    let text = result
        .content
        .first()
        .and_then(|block| block.as_text())
        .unwrap_or_default();
    assert!(
        text.contains("field-litmus"),
        "unexpected init result: {text}"
    );

    let result = spawned
        .feature
        .execute(
            "recro_partnership_create",
            "recro-host-create",
            json!({
                "id": "field-litmus",
                "name": "Field Litmus",
                "kind": "client"
            }),
            CancellationToken::new(),
        )
        .await?;
    let text = result
        .content
        .first()
        .and_then(|block| block.as_text())
        .unwrap_or_default();
    assert!(
        text.contains("field-litmus"),
        "unexpected create result: {text}"
    );

    let result = spawned
        .feature
        .execute(
            "recro_partnership_get",
            "recro-host-get",
            json!({"id": "field-litmus"}),
            CancellationToken::new(),
        )
        .await?;
    let text = result
        .content
        .first()
        .and_then(|block| block.as_text())
        .unwrap_or_default();
    assert!(
        text.contains("field-litmus"),
        "unexpected get result: {text}"
    );
    assert!(
        text.contains("Field Litmus"),
        "unexpected get result: {text}"
    );
    assert!(
        data_dir.join("partnerships/field-litmus.json").is_file(),
        "bootstrap_config was not applied"
    );

    let skills = crate::skills::list_structured()?;
    let skill = skills
        .iter()
        .find(|entry| entry.name == "recro-coe")
        .context("Recro COE extension skill was not discovered")?;
    assert_eq!(skill.source, "extension:omegon-recro-coe");
    assert!(!skill.editable);

    drop(spawned);
    Ok(())
}
