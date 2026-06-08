use std::path::{Component, Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageSourceKind {
    Armory,
    Git,
    LocalPath,
    Url,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PackageKindHint {
    #[default]
    Auto,
    Skill,
    Extension,
    Agent,
    Persona,
    Tone,
    Package,
    LegacyPlugin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContributionKind {
    Unknown,
    Skill,
    SdkExtension,
    Agent,
    Persona,
    Tone,
    Script,
    McpServer,
    HostActionPolicy,
    LegacyPlugin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContributionRisk {
    Unknown,
    PromptInfluence,
    LocalCodeExecution,
    PersistentToolExecution,
    ExtensionRuntime,
    HostCapabilityRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContributionStatus {
    Planned,
    Installed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageRef {
    pub id: String,
    pub source: String,
    pub source_kind: PackageSourceKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub package_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installed_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributionReport {
    pub kind: ContributionKind,
    pub id: Option<String>,
    pub path: Option<String>,
    pub risk: ContributionRisk,
    pub status: ContributionStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageReport {
    pub ok: bool,
    pub status: PackageInstallStatus,
    pub package: PackageRef,
    pub contributions: Vec<ContributionReport>,
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_check: Option<VersionCheck>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageInstallStatus {
    Planned,
    Installed,
    AlreadyInstalled,
    UpdateAvailable,
    Updated,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionCheck {
    pub relation: String,
    pub installed: Option<String>,
    pub candidate: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PackageInstallRequest {
    pub source: String,
    #[serde(default)]
    pub kind_hint: PackageKindHint,
}

pub fn parse_kind_hint(value: Option<&str>) -> anyhow::Result<PackageKindHint> {
    match value.unwrap_or("auto").trim().to_ascii_lowercase().as_str() {
        "" | "auto" => Ok(PackageKindHint::Auto),
        "skill" | "skills" => Ok(PackageKindHint::Skill),
        "extension" | "extensions" | "sdk_extension" | "sdk_extensions" => {
            Ok(PackageKindHint::Extension)
        }
        "agent" | "agents" | "catalog" => Ok(PackageKindHint::Agent),
        "persona" | "personas" => Ok(PackageKindHint::Persona),
        "tone" | "tones" => Ok(PackageKindHint::Tone),
        "package" | "packages" => Ok(PackageKindHint::Package),
        "plugin" | "plugins" | "legacy_plugin" | "legacy_plugins" => {
            Ok(PackageKindHint::LegacyPlugin)
        }
        other => anyhow::bail!("invalid package kind_hint '{other}'"),
    }
}

pub fn request_from_params(params: &serde_json::Value) -> anyhow::Result<PackageInstallRequest> {
    let source = params
        .get("source")
        .and_then(|value| value.as_str())
        .or_else(|| params.get("target").and_then(|value| value.as_str()))
        .or_else(|| params.get("uri").and_then(|value| value.as_str()))
        .map(str::trim)
        .filter(|source| !source.is_empty())
        .ok_or_else(|| anyhow::anyhow!("missing 'source' field"))?;
    let kind_hint = parse_kind_hint(
        params
            .get("kind_hint")
            .and_then(|value| value.as_str())
            .or_else(|| params.get("kind").and_then(|value| value.as_str())),
    )?;
    Ok(PackageInstallRequest {
        source: source.to_string(),
        kind_hint,
    })
}

pub fn plan(request: &PackageInstallRequest) -> PackageReport {
    let package = package_ref(&request.source);
    let (contributions, warnings) = match detect_local_contributions(&request.source) {
        Ok(Some(contributions)) => (contributions, Vec::new()),
        Ok(None) => {
            let contribution = planned_contribution(request.kind_hint);
            let warnings = if matches!(contribution.kind, ContributionKind::Unknown) {
                vec![
                    "contribution type will be detected from package contents during install"
                        .to_string(),
                ]
            } else {
                Vec::new()
            };
            (vec![contribution], warnings)
        }
        Err(err) => (
            vec![planned_contribution(request.kind_hint)],
            vec![err.to_string()],
        ),
    };
    PackageReport {
        ok: true,
        status: PackageInstallStatus::Planned,
        package,
        contributions,
        warnings,
        version_check: None,
    }
}

pub async fn install(request: PackageInstallRequest, cwd: &Path) -> anyhow::Result<PackageReport> {
    if let Some(target) = request.source.strip_prefix("armory:") {
        let kind = armory_install_kind(request.kind_hint);
        let result = crate::armory::install(target, kind, cwd).await?;
        return Ok(report_from_armory_result(&request.source, result));
    }

    match request.kind_hint {
        PackageKindHint::Extension => {
            crate::extension_cli::install(&request.source)?;
            Ok(report_from_install(
                &request.source,
                ContributionKind::SdkExtension,
                ContributionRisk::ExtensionRuntime,
            ))
        }
        PackageKindHint::Agent => {
            let result = crate::armory::install(
                &request.source,
                crate::armory::ArmoryInstallKind::Auto,
                cwd,
            )
            .await
            .with_context(|| format!("failed to install agent/package '{}'", request.source))?;
            Ok(report_from_armory_result(&request.source, result))
        }
        PackageKindHint::Skill
        | PackageKindHint::Package
        | PackageKindHint::LegacyPlugin
        | PackageKindHint::Auto
        | PackageKindHint::Persona
        | PackageKindHint::Tone => match crate::plugin_cli::install(&request.source) {
            Ok(result) => Ok(report_from_plugin_install(&request.source, result)),
            Err(error) if is_already_installed_error(&error) => {
                already_installed_plugin_report(&request.source)
            }
            Err(error) => Err(error),
        },
    }
}

pub fn list() -> anyhow::Result<serde_json::Value> {
    let plugins_dir = crate::plugin_cli::plugins_dir()?;
    let mut items = Vec::new();
    if plugins_dir.exists() {
        for entry in std::fs::read_dir(&plugins_dir)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() && !path.is_symlink() {
                continue;
            }
            let resolved = if path.is_symlink() {
                std::fs::read_link(&path).unwrap_or(path.clone())
            } else {
                path.clone()
            };
            let source = resolved.display().to_string();
            let request = PackageInstallRequest {
                source: source.clone(),
                kind_hint: PackageKindHint::Auto,
            };
            let mut report = plan(&request);
            report.package.id = entry.file_name().to_string_lossy().to_string();
            for contribution in &mut report.contributions {
                contribution.status = ContributionStatus::Installed;
                contribution
                    .id
                    .get_or_insert_with(|| report.package.id.clone());
            }
            items.push(serde_json::to_value(report)?);
        }
    }
    Ok(serde_json::json!({ "items": items }))
}

pub fn remove(params: &serde_json::Value) -> anyhow::Result<serde_json::Value> {
    let id = package_id_param(params)?;
    validate_package_id(&id)?;
    match parse_kind_hint(params.get("kind").and_then(|value| value.as_str()))? {
        PackageKindHint::Extension => crate::extension_cli::remove(&id)?,
        PackageKindHint::Agent => {
            remove_from_dir(crate::paths::omegon_home()?.join("catalog"), &id)?
        }
        PackageKindHint::Skill => {
            remove_from_dir(crate::paths::omegon_home()?.join("skills"), &id)?
        }
        PackageKindHint::Auto
        | PackageKindHint::Package
        | PackageKindHint::LegacyPlugin
        | PackageKindHint::Persona
        | PackageKindHint::Tone => crate::plugin_cli::remove(&id)?,
    }
    Ok(serde_json::json!({ "ok": true, "id": id }))
}

fn remove_from_dir(dir: PathBuf, id: &str) -> anyhow::Result<()> {
    validate_package_id(id)?;
    let path = contained_child_path(&dir, id)?;
    if !path.exists() && !path.is_symlink() {
        anyhow::bail!("Package '{}' not found in {}", id, dir.display());
    }
    if path.is_symlink() {
        std::fs::remove_file(path)?;
    } else {
        std::fs::remove_dir_all(path)?;
    }
    Ok(())
}

pub fn update(params: &serde_json::Value) -> anyhow::Result<serde_json::Value> {
    let id = params
        .get("id")
        .and_then(|value| value.as_str())
        .or_else(|| params.get("name").and_then(|value| value.as_str()))
        .map(str::trim)
        .filter(|id| !id.is_empty());
    if let Some(id) = id {
        validate_package_id(id)?;
    }
    match parse_kind_hint(params.get("kind").and_then(|value| value.as_str()))? {
        PackageKindHint::Extension => crate::extension_cli::update(id)?,
        PackageKindHint::Agent | PackageKindHint::Skill => {
            anyhow::bail!(
                "packages/update does not support bundled {} packages yet",
                params
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .unwrap_or("kind")
            );
        }
        PackageKindHint::Auto
        | PackageKindHint::Package
        | PackageKindHint::LegacyPlugin
        | PackageKindHint::Persona
        | PackageKindHint::Tone => crate::plugin_cli::update(id)?,
    }
    Ok(serde_json::json!({ "ok": true, "id": id }))
}

fn package_id_param(params: &serde_json::Value) -> anyhow::Result<String> {
    params
        .get("id")
        .and_then(|value| value.as_str())
        .or_else(|| params.get("name").and_then(|value| value.as_str()))
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow::anyhow!("missing 'id' field"))
}

fn validate_package_id(id: &str) -> anyhow::Result<()> {
    if id.is_empty()
        || id.contains('/')
        || id.contains('\\')
        || id.contains('\0')
        || id == "."
        || id == ".."
        || Path::new(id).components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        anyhow::bail!("invalid package id '{id}'");
    }
    Ok(())
}

fn contained_child_path(root: &Path, id: &str) -> anyhow::Result<PathBuf> {
    validate_package_id(id)?;
    let path = root.join(id);
    if path.parent() != Some(root) {
        anyhow::bail!("invalid package id '{id}'");
    }
    Ok(path)
}

fn installed_entry_target(root: &Path, entry_path: &Path) -> anyhow::Result<PathBuf> {
    if !entry_path.is_symlink() {
        return Ok(entry_path.to_path_buf());
    }
    let target = std::fs::read_link(entry_path)
        .with_context(|| format!("failed to read symlink {}", entry_path.display()))?;
    let resolved = if target.is_absolute() {
        target
    } else {
        root.join(target)
    };
    Ok(resolved)
}

pub async fn search(params: &serde_json::Value, cwd: &Path) -> anyhow::Result<serde_json::Value> {
    let query = params.get("query").and_then(|value| value.as_str());
    let kind = params
        .get("contributes")
        .and_then(|value| value.as_array())
        .and_then(|values| values.first())
        .and_then(|value| value.as_str())
        .or_else(|| params.get("kind").and_then(|value| value.as_str()))
        .map(armory_kind_for_contribution)
        .transpose()?
        .unwrap_or(crate::armory::ArmoryKind::All);
    let items = crate::armory::browse(crate::armory::BrowseOptions::new(kind, query, cwd)).await?;
    let items: Vec<serde_json::Value> = items.into_iter().map(armory_item_package_json).collect();
    Ok(serde_json::json!({ "items": items }))
}

fn armory_item_package_json(item: crate::armory::ArmoryItem) -> serde_json::Value {
    let contribution = contribution_kind_for_armory(item.kind);
    serde_json::json!({
        "id": item.id,
        "kind": "package",
        "name": item.name,
        "version": item.version,
        "description": item.description,
        "source": item.source,
        "contributions": [contribution_name(contribution)],
        "installed": item.installed,
        "install_hint": item.install_hint,
        "legacy_kind": armory_item_kind_name(item.kind),
    })
}

fn package_ref(source: &str) -> PackageRef {
    PackageRef {
        id: infer_package_id(source),
        source: source.to_string(),
        source_kind: infer_source_kind(source),
        name: None,
        package_type: None,
        installed_version: None,
        candidate_version: None,
        path: None,
    }
}

fn infer_source_kind(source: &str) -> PackageSourceKind {
    if source.starts_with("armory:") {
        PackageSourceKind::Armory
    } else if source.starts_with("git@") || source.ends_with(".git") {
        PackageSourceKind::Git
    } else if source.starts_with("http://") || source.starts_with("https://") {
        PackageSourceKind::Url
    } else if source.starts_with('/') || source.starts_with("./") || source.starts_with("../") {
        PackageSourceKind::LocalPath
    } else {
        PackageSourceKind::Unknown
    }
}

fn infer_package_id(source: &str) -> String {
    source
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .rsplit(['/', ':'])
        .next()
        .filter(|id| !id.is_empty())
        .unwrap_or(source)
        .to_string()
}

fn planned_contribution(kind_hint: PackageKindHint) -> ContributionReport {
    ContributionReport {
        kind: contribution_kind_for_hint(kind_hint),
        id: None,
        path: None,
        risk: risk_for_hint(kind_hint),
        status: ContributionStatus::Planned,
    }
}

fn detect_local_contributions(source: &str) -> anyhow::Result<Option<Vec<ContributionReport>>> {
    let path = Path::new(source);
    if !path.exists() || !path.is_dir() {
        return Ok(None);
    }

    let mut contributions = Vec::new();
    if path.join("manifest.toml").exists() {
        contributions.push(detected_contribution(
            ContributionKind::SdkExtension,
            ContributionRisk::ExtensionRuntime,
            path.join("manifest.toml"),
        ));
    }
    if path.join("SKILL.md").exists() {
        contributions.push(detected_contribution(
            ContributionKind::Skill,
            ContributionRisk::PromptInfluence,
            path.join("SKILL.md"),
        ));
    }
    let plugin_manifest = path.join("plugin.toml");
    if plugin_manifest.exists() {
        let text = std::fs::read_to_string(&plugin_manifest)
            .with_context(|| format!("failed to read {}", plugin_manifest.display()))?;
        let kind = if text.contains("type = \"skill\"") || text.contains("type='skill'") {
            ContributionKind::Skill
        } else if text.contains("type = \"persona\"") || text.contains("type='persona'") {
            ContributionKind::Persona
        } else if text.contains("type = \"tone\"") || text.contains("type='tone'") {
            ContributionKind::Tone
        } else {
            ContributionKind::LegacyPlugin
        };
        contributions.push(detected_contribution(
            kind,
            risk_for_contribution(kind),
            plugin_manifest,
        ));
    }

    if contributions.is_empty() {
        Ok(None)
    } else {
        Ok(Some(contributions))
    }
}

fn detected_contribution(
    kind: ContributionKind,
    risk: ContributionRisk,
    path: std::path::PathBuf,
) -> ContributionReport {
    ContributionReport {
        kind,
        id: path
            .parent()
            .and_then(|parent| parent.file_name())
            .map(|name| name.to_string_lossy().to_string()),
        path: Some(path.display().to_string()),
        risk,
        status: ContributionStatus::Planned,
    }
}

fn report_from_install(
    source: &str,
    kind: ContributionKind,
    risk: ContributionRisk,
) -> PackageReport {
    PackageReport {
        ok: true,
        status: PackageInstallStatus::Installed,
        package: package_ref(source),
        contributions: vec![ContributionReport {
            kind,
            id: None,
            path: None,
            risk,
            status: ContributionStatus::Installed,
        }],
        warnings: Vec::new(),
        version_check: None,
    }
}

fn report_from_plugin_install(
    source: &str,
    result: crate::plugin_cli::PluginInstallResult,
) -> PackageReport {
    let contributions = plugin_manifest_summary(&result.path.join("plugin.toml"))
        .ok()
        .map(|summary| {
            let kind = contribution_kind_for_plugin_type(&summary.plugin_type);
            vec![ContributionReport {
                kind,
                id: None,
                path: Some(result.path.join("plugin.toml").display().to_string()),
                risk: risk_for_contribution(kind),
                status: ContributionStatus::Planned,
            }]
        })
        .or_else(|| {
            detect_local_contributions(&result.path.display().to_string())
                .ok()
                .flatten()
        })
        .unwrap_or_else(|| vec![planned_contribution(PackageKindHint::LegacyPlugin)]);
    let contributions = contributions
        .into_iter()
        .map(|mut contribution| {
            contribution.id.get_or_insert_with(|| result.name.clone());
            contribution.status = ContributionStatus::Installed;
            contribution
        })
        .collect();

    PackageReport {
        ok: true,
        status: PackageInstallStatus::Installed,
        package: PackageRef {
            id: result.name,
            source: source.to_string(),
            source_kind: infer_source_kind(source),
            name: None,
            package_type: None,
            installed_version: None,
            candidate_version: None,
            path: Some(result.path.display().to_string()),
        },
        contributions,
        warnings: Vec::new(),
        version_check: None,
    }
}

fn report_from_armory_result(
    source: &str,
    result: crate::armory::ArmoryInstallResult,
) -> PackageReport {
    let kind = contribution_kind_for_armory(result.kind);
    PackageReport {
        ok: true,
        status: PackageInstallStatus::Installed,
        package: package_ref(source),
        contributions: vec![ContributionReport {
            kind,
            id: Some(result.id),
            path: result.path,
            risk: risk_for_contribution(kind),
            status: ContributionStatus::Installed,
        }],
        warnings: vec![result.message],
        version_check: None,
    }
}

fn is_already_installed_error(error: &anyhow::Error) -> bool {
    let text = error.to_string().to_ascii_lowercase();
    text.contains("already exists") || text.contains("already installed")
}

fn already_installed_plugin_report(source: &str) -> anyhow::Result<PackageReport> {
    already_installed_plugin_report_in(source, &crate::plugin_cli::plugins_dir()?)
}

fn already_installed_plugin_report_in(
    source: &str,
    plugins_dir: &Path,
) -> anyhow::Result<PackageReport> {
    let candidate_manifest = plugin_manifest_path_for_source(source);
    let candidate_summary = candidate_manifest
        .as_deref()
        .and_then(|path| plugin_manifest_summary(path).ok());
    let name = candidate_summary
        .as_ref()
        .map(|summary| summary.name.clone())
        .or_else(|| crate::plugin_cli::infer_plugin_name(source).ok())
        .ok_or_else(|| {
            anyhow::anyhow!("could not infer installed package name from source: {source}")
        })?;
    let path = plugins_dir.join(&name);
    if !path.exists() && !path.is_symlink() {
        anyhow::bail!("package '{name}' is not installed at {}", path.display());
    }
    let installed_summary = plugin_manifest_summary(&path.join("plugin.toml")).ok();
    let installed_version = installed_summary
        .as_ref()
        .map(|summary| summary.version.clone());
    let candidate_version = candidate_summary
        .as_ref()
        .map(|summary| summary.version.clone())
        .or_else(|| installed_version.clone());
    let relation = if installed_version.is_some() && candidate_version.is_some() {
        if installed_version == candidate_version {
            "same"
        } else {
            "unknown"
        }
    } else {
        "unknown"
    };
    let kind = installed_summary
        .as_ref()
        .map(|summary| contribution_kind_for_plugin_type(&summary.plugin_type))
        .unwrap_or_else(|| contribution_kind_for_hint(PackageKindHint::LegacyPlugin));
    Ok(PackageReport {
        ok: true,
        status: PackageInstallStatus::AlreadyInstalled,
        package: PackageRef {
            id: name.clone(),
            source: source.to_string(),
            source_kind: infer_source_kind(source),
            name: installed_summary
                .as_ref()
                .map(|summary| summary.name.clone()),
            package_type: installed_summary
                .as_ref()
                .map(|summary| summary.plugin_type.clone()),
            installed_version: installed_version.clone(),
            candidate_version: candidate_version.clone(),
            path: Some(path.display().to_string()),
        },
        contributions: vec![ContributionReport {
            kind,
            id: Some(name),
            path: Some(path.display().to_string()),
            risk: risk_for_contribution(kind),
            status: ContributionStatus::Installed,
        }],
        warnings: Vec::new(),
        version_check: Some(VersionCheck {
            relation: relation.to_string(),
            installed: installed_version,
            candidate: candidate_version,
            previous: None,
        }),
    })
}

struct PluginManifestSummary {
    name: String,
    plugin_type: String,
    version: String,
}

fn plugin_manifest_path_for_source(source: &str) -> Option<PathBuf> {
    let path = Path::new(source);
    if path.exists() {
        let manifest = path.join("plugin.toml");
        if manifest.exists() {
            return Some(manifest);
        }
    }
    None
}

fn plugin_manifest_summary(path: &Path) -> anyhow::Result<PluginManifestSummary> {
    let content = std::fs::read_to_string(path)?;
    let parsed = crate::plugins::armory::ArmoryManifest::parse(&content)?;
    Ok(PluginManifestSummary {
        name: parsed.plugin.name,
        plugin_type: plugin_type_from_manifest_text(&content)
            .unwrap_or_else(|| parsed.plugin.plugin_type.to_string()),
        version: parsed.plugin.version,
    })
}

fn plugin_type_from_manifest_text(text: &str) -> Option<String> {
    if text.contains("type = \"skill\"") || text.contains("type='skill'") {
        Some("skill".to_string())
    } else if text.contains("type = \"persona\"") || text.contains("type='persona'") {
        Some("persona".to_string())
    } else if text.contains("type = \"tone\"") || text.contains("type='tone'") {
        Some("tone".to_string())
    } else {
        None
    }
}

fn contribution_kind_for_plugin_type(plugin_type: &str) -> ContributionKind {
    match plugin_type.trim().to_ascii_lowercase().as_str() {
        "skill" => ContributionKind::Skill,
        "persona" => ContributionKind::Persona,
        "tone" => ContributionKind::Tone,
        _ => ContributionKind::LegacyPlugin,
    }
}

fn armory_install_kind(kind_hint: PackageKindHint) -> crate::armory::ArmoryInstallKind {
    match kind_hint {
        PackageKindHint::Skill => crate::armory::ArmoryInstallKind::Skill,
        PackageKindHint::Extension => crate::armory::ArmoryInstallKind::Extension,
        PackageKindHint::LegacyPlugin
        | PackageKindHint::Persona
        | PackageKindHint::Tone
        | PackageKindHint::Package => crate::armory::ArmoryInstallKind::Plugin,
        PackageKindHint::Auto | PackageKindHint::Agent => crate::armory::ArmoryInstallKind::Auto,
    }
}

fn armory_kind_for_contribution(kind: &str) -> anyhow::Result<crate::armory::ArmoryKind> {
    match kind.trim().to_ascii_lowercase().as_str() {
        "" | "all" | "package" | "packages" => Ok(crate::armory::ArmoryKind::All),
        "skill" | "skills" => Ok(crate::armory::ArmoryKind::Skills),
        "extension" | "extensions" | "sdk_extension" | "sdk_extensions" => {
            Ok(crate::armory::ArmoryKind::Extensions)
        }
        "agent" | "agents" => Ok(crate::armory::ArmoryKind::Agents),
        "persona" | "personas" | "tone" | "tones" | "plugin" | "plugins" => {
            Ok(crate::armory::ArmoryKind::Plugins)
        }
        other => anyhow::bail!("invalid contribution filter '{other}'"),
    }
}

fn contribution_kind_for_hint(kind_hint: PackageKindHint) -> ContributionKind {
    match kind_hint {
        PackageKindHint::Skill => ContributionKind::Skill,
        PackageKindHint::Extension => ContributionKind::SdkExtension,
        PackageKindHint::Agent => ContributionKind::Agent,
        PackageKindHint::Persona => ContributionKind::Persona,
        PackageKindHint::Tone => ContributionKind::Tone,
        PackageKindHint::LegacyPlugin => ContributionKind::LegacyPlugin,
        PackageKindHint::Auto | PackageKindHint::Package => ContributionKind::Unknown,
    }
}

fn contribution_kind_for_armory(kind: crate::armory::ArmoryItemKind) -> ContributionKind {
    match kind {
        crate::armory::ArmoryItemKind::Extension => ContributionKind::SdkExtension,
        crate::armory::ArmoryItemKind::Plugin => ContributionKind::LegacyPlugin,
        crate::armory::ArmoryItemKind::Skill => ContributionKind::Skill,
        crate::armory::ArmoryItemKind::Agent => ContributionKind::Agent,
    }
}

fn risk_for_hint(kind_hint: PackageKindHint) -> ContributionRisk {
    risk_for_contribution(contribution_kind_for_hint(kind_hint))
}

fn risk_for_contribution(kind: ContributionKind) -> ContributionRisk {
    match kind {
        ContributionKind::Skill | ContributionKind::Persona | ContributionKind::Tone => {
            ContributionRisk::PromptInfluence
        }
        ContributionKind::Script => ContributionRisk::LocalCodeExecution,
        ContributionKind::McpServer => ContributionRisk::PersistentToolExecution,
        ContributionKind::SdkExtension => ContributionRisk::ExtensionRuntime,
        ContributionKind::HostActionPolicy => ContributionRisk::HostCapabilityRequest,
        ContributionKind::LegacyPlugin | ContributionKind::Agent | ContributionKind::Unknown => {
            ContributionRisk::Unknown
        }
    }
}

fn contribution_name(kind: ContributionKind) -> &'static str {
    match kind {
        ContributionKind::Unknown => "unknown",
        ContributionKind::Skill => "skill",
        ContributionKind::SdkExtension => "sdk_extension",
        ContributionKind::Agent => "agent",
        ContributionKind::Persona => "persona",
        ContributionKind::Tone => "tone",
        ContributionKind::Script => "script",
        ContributionKind::McpServer => "mcp_server",
        ContributionKind::HostActionPolicy => "host_action_policy",
        ContributionKind::LegacyPlugin => "legacy_plugin",
    }
}

fn armory_item_kind_name(kind: crate::armory::ArmoryItemKind) -> &'static str {
    match kind {
        crate::armory::ArmoryItemKind::Extension => "extension",
        crate::armory::ArmoryItemKind::Plugin => "plugin",
        crate::armory::ArmoryItemKind::Skill => "skill",
        crate::armory::ArmoryItemKind::Agent => "agent",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn request_from_params_accepts_source_and_kind_hint() {
        let request = request_from_params(&json!({
            "source": "https://github.com/recro/recro-omegon",
            "kind_hint": "skill"
        }))
        .unwrap();

        assert_eq!(request.source, "https://github.com/recro/recro-omegon");
        assert_eq!(request.kind_hint, PackageKindHint::Skill);
    }

    #[test]
    fn request_from_params_accepts_legacy_uri_alias() {
        let request = request_from_params(&json!({
            "uri": "https://github.com/recro/recro-omegon"
        }))
        .unwrap();

        assert_eq!(request.source, "https://github.com/recro/recro-omegon");
        assert_eq!(request.kind_hint, PackageKindHint::Auto);
    }

    #[test]
    fn request_from_params_requires_source() {
        let error = request_from_params(&json!({})).unwrap_err();
        assert!(error.to_string().contains("missing 'source' field"));
    }

    #[test]
    fn plan_reports_package_and_contribution_hint() {
        let request = PackageInstallRequest {
            source: "https://github.com/recro/recro-omegon".to_string(),
            kind_hint: PackageKindHint::Skill,
        };

        let report = plan(&request);

        assert!(report.ok);
        assert_eq!(report.package.id, "recro-omegon");
        assert_eq!(report.package.source_kind, PackageSourceKind::Url);
        assert_eq!(report.contributions[0].kind, ContributionKind::Skill);
        assert_eq!(
            report.contributions[0].risk,
            ContributionRisk::PromptInfluence
        );
        assert_eq!(report.contributions[0].status, ContributionStatus::Planned);
    }

    #[test]
    fn plan_detects_local_skill_plugin_package() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("plugin.toml"),
            "[plugin]\nname = \"recro\"\ntype = \"skill\"\n",
        )
        .unwrap();
        let request = PackageInstallRequest {
            source: dir.path().display().to_string(),
            kind_hint: PackageKindHint::Auto,
        };

        let report = plan(&request);

        assert!(report.ok);
        assert_eq!(report.package.source_kind, PackageSourceKind::LocalPath);
        assert_eq!(report.contributions[0].kind, ContributionKind::Skill);
        assert_eq!(
            report.contributions[0].risk,
            ContributionRisk::PromptInfluence
        );
        assert!(
            report.contributions[0]
                .path
                .as_deref()
                .unwrap()
                .ends_with("plugin.toml")
        );
    }

    #[test]
    fn armory_item_package_json_reports_contribution_summary() {
        let item = crate::armory::ArmoryItem {
            kind: crate::armory::ArmoryItemKind::Skill,
            id: "skills/recro".to_string(),
            name: "recro".to_string(),
            description: "Recro workflows".to_string(),
            category: "workflow".to_string(),
            version: Some("0.1.0".to_string()),
            source: "skills/recro/plugin.toml".to_string(),
            manifest_id: Some("recro".to_string()),
            installed: false,
            install_hint: "armory install skills/recro".to_string(),
        };

        let json = armory_item_package_json(item);

        assert_eq!(json["kind"], "package");
        assert_eq!(json["contributions"][0], "skill");
        assert_eq!(json["legacy_kind"], "skill");
        assert_eq!(json["source"], "skills/recro/plugin.toml");
    }

    #[tokio::test]
    async fn install_reports_already_installed_plugin_as_structured_status() {
        let home = tempfile::tempdir().unwrap();
        let installed = home.path().join("plugins/recro-omegon");
        std::fs::create_dir_all(&installed).unwrap();
        std::fs::write(
            installed.join("plugin.toml"),
            "[plugin]\nid = \"recro-omegon\"\nname = \"recro-omegon\"\ntype = \"skill\"\nversion = \"0.1.0\"\ndescription = \"Recro workflows\"\n",
        )
        .unwrap();
        let previous_home = std::env::var_os("OMEGON_HOME");
        unsafe {
            std::env::set_var("OMEGON_HOME", home.path());
        }

        let request = PackageInstallRequest {
            source: "https://github.com/recro/recro-omegon".to_string(),
            kind_hint: PackageKindHint::Auto,
        };
        let second = install(request, Path::new(".")).await.unwrap();

        match previous_home {
            Some(value) => unsafe { std::env::set_var("OMEGON_HOME", value) },
            None => unsafe { std::env::remove_var("OMEGON_HOME") },
        }

        assert_eq!(second.status, PackageInstallStatus::AlreadyInstalled);
        assert_eq!(second.package.id, "recro-omegon");
        assert_eq!(second.package.name.as_deref(), Some("recro-omegon"));
        assert_eq!(second.package.package_type.as_deref(), Some("skill"));
        assert_eq!(second.package.installed_version.as_deref(), Some("0.1.0"));
        assert_eq!(second.package.candidate_version.as_deref(), Some("0.1.0"));
        assert_eq!(second.version_check.as_ref().unwrap().relation, "same");
        assert_eq!(second.contributions[0].kind, ContributionKind::Skill);
        assert!(
            second
                .package
                .path
                .as_deref()
                .unwrap()
                .ends_with("plugins/recro-omegon")
        );
    }

    #[test]
    fn remove_requires_id() {
        let error = remove(&json!({})).unwrap_err();
        assert!(error.to_string().contains("missing 'id' field"));
    }

    #[test]
    fn package_id_rejects_path_traversal() {
        for id in ["../evil", "nested/name", "/tmp/evil", "..", "."] {
            assert!(validate_package_id(id).is_err(), "accepted {id}");
        }
    }

    #[test]
    fn contained_child_path_stays_under_root() {
        let root = Path::new("/tmp/packages");
        assert_eq!(
            contained_child_path(root, "recro").unwrap(),
            PathBuf::from("/tmp/packages/recro")
        );
        assert!(contained_child_path(root, "../recro").is_err());
    }
}
