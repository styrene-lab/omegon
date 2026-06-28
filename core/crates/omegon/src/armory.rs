//! Unified Armory discovery for extensions, plugins, skills, and catalog agents.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

const ARMORY_RAW_BASE: &str = "https://raw.githubusercontent.com/styrene-lab/omegon-armory/main";
const ARMORY_API_BASE: &str = "https://api.github.com/repos/styrene-lab/omegon-armory/contents";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum ArmoryKind {
    All,
    Extensions,
    Plugins,
    Skills,
    Agents,
}

impl ArmoryKind {
    fn matches(self, item: ArmoryItemKind) -> bool {
        matches!(self, Self::All)
            || matches!(
                (self, item),
                (Self::Extensions, ArmoryItemKind::Extension)
                    | (Self::Plugins, ArmoryItemKind::Plugin)
                    | (Self::Skills, ArmoryItemKind::Skill)
                    | (Self::Agents, ArmoryItemKind::Agent)
            )
    }
}

impl std::fmt::Display for ArmoryKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::All => write!(f, "all"),
            Self::Extensions => write!(f, "extensions"),
            Self::Plugins => write!(f, "plugins"),
            Self::Skills => write!(f, "skills"),
            Self::Agents => write!(f, "agents"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArmoryItemKind {
    Extension,
    Plugin,
    Skill,
    Agent,
}

impl std::fmt::Display for ArmoryItemKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Extension => write!(f, "extension"),
            Self::Plugin => write!(f, "plugin"),
            Self::Skill => write!(f, "skill"),
            Self::Agent => write!(f, "agent"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ArmoryItem {
    pub kind: ArmoryItemKind,
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub version: Option<String>,
    pub source: String,
    pub manifest_id: Option<String>,
    pub installed: bool,
    pub install_hint: String,
}

#[derive(Debug, Clone, Copy)]
pub struct BrowseOptions<'a> {
    pub kind: ArmoryKind,
    pub query: Option<&'a str>,
    pub cwd: &'a Path,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArmoryInstallKind {
    Auto,
    Extension,
    Plugin,
    Skill,
}

impl From<ArmoryKind> for ArmoryInstallKind {
    fn from(kind: ArmoryKind) -> Self {
        match kind {
            ArmoryKind::Extensions => Self::Extension,
            ArmoryKind::Plugins => Self::Plugin,
            ArmoryKind::Skills => Self::Skill,
            ArmoryKind::All | ArmoryKind::Agents => Self::Auto,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ArmoryInstallResult {
    pub kind: ArmoryItemKind,
    pub id: String,
    pub path: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InstallTarget {
    root: Option<String>,
    slug: String,
}

impl<'a> BrowseOptions<'a> {
    pub fn new(kind: ArmoryKind, query: Option<&'a str>, cwd: &'a Path) -> Self {
        Self { kind, query, cwd }
    }
}

#[derive(Debug, Deserialize)]
struct CatalogRegistryEntry {
    name: String,
    version: String,
    domain: String,
    description: String,
}

#[derive(Debug, Deserialize)]
struct GitHubContentEntry {
    name: String,
    #[serde(rename = "type")]
    entry_type: String,
}

pub async fn browse(options: BrowseOptions<'_>) -> anyhow::Result<Vec<ArmoryItem>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent("omegon")
        .build()?;

    let installed = InstalledState::collect(options.cwd);
    let mut items = Vec::new();

    if matches!(options.kind, ArmoryKind::All | ArmoryKind::Extensions) {
        items.extend(fetch_extensions(&client, &installed).await?);
    }
    if matches!(options.kind, ArmoryKind::All | ArmoryKind::Agents) {
        items.extend(fetch_agents(&client, &installed).await?);
    }
    if matches!(
        options.kind,
        ArmoryKind::All | ArmoryKind::Plugins | ArmoryKind::Skills
    ) {
        items.extend(fetch_plugin_manifests(&client, options.kind, &installed).await?);
    }

    items.retain(|item| options.kind.matches(item.kind));
    if let Some(query) = options.query.map(str::trim).filter(|q| !q.is_empty()) {
        let query = query.to_lowercase();
        items.retain(|item| {
            item.id.to_lowercase().contains(&query)
                || item.name.to_lowercase().contains(&query)
                || item.description.to_lowercase().contains(&query)
                || item.category.to_lowercase().contains(&query)
                || item
                    .manifest_id
                    .as_ref()
                    .is_some_and(|id| id.to_lowercase().contains(&query))
        });
    }

    items.sort_by(|a, b| {
        a.kind
            .to_string()
            .cmp(&b.kind.to_string())
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(items)
}

pub async fn cmd_browse(
    kind: ArmoryKind,
    query: Option<&str>,
    json: bool,
    cwd: &Path,
) -> anyhow::Result<()> {
    let items = browse(BrowseOptions::new(kind, query, cwd)).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&items)?);
    } else {
        println!("{}", render_items(&items));
    }
    Ok(())
}

pub async fn cmd_install(spec: &str, kind: ArmoryInstallKind, cwd: &Path) -> anyhow::Result<()> {
    let result = install(spec, kind, cwd).await?;
    println!("{}", result.message);
    Ok(())
}

pub async fn install(
    spec: &str,
    kind: ArmoryInstallKind,
    cwd: &Path,
) -> anyhow::Result<ArmoryInstallResult> {
    let spec = spec.trim();
    if spec.is_empty() {
        anyhow::bail!("armory install target cannot be empty");
    }

    match kind {
        ArmoryInstallKind::Extension => install_extension(spec, None).await,
        ArmoryInstallKind::Skill => install_skill(spec).await,
        ArmoryInstallKind::Plugin => install_plugin(spec).await,
        ArmoryInstallKind::Auto => install_auto(spec, cwd).await,
    }
}

async fn install_auto(spec: &str, cwd: &Path) -> anyhow::Result<ArmoryInstallResult> {
    if let Some(target) = parse_armory_plugin_target(spec)
        && let Some(root) = target.root.as_deref()
    {
        return match root {
            "skills" => install_skill(&target.slug).await,
            "personas" | "tones" | "examples" => install_plugin(spec).await,
            _ => anyhow::bail!("unsupported armory root '{root}'"),
        };
    }

    if crate::extension_registry::is_bare_name(spec) {
        if matches!(
            find_exact_item(ArmoryKind::Extensions, spec, cwd).await,
            Ok(Some(_))
        ) {
            return install_extension(spec, None).await;
        }
        if matches!(
            find_exact_item(ArmoryKind::Skills, spec, cwd).await,
            Ok(Some(_))
        ) {
            return install_skill(spec).await;
        }
        if matches!(
            find_exact_item(ArmoryKind::Plugins, spec, cwd).await,
            Ok(Some(_))
        ) {
            return install_plugin(spec).await;
        }
        return install_extension(spec, None).await;
    }

    install_extension(spec, None).await
}

async fn find_exact_item(
    kind: ArmoryKind,
    id: &str,
    cwd: &Path,
) -> anyhow::Result<Option<ArmoryItem>> {
    Ok(browse(BrowseOptions::new(kind, Some(id), cwd))
        .await?
        .into_iter()
        .find(|item| item.id == id))
}

pub async fn install_extension(
    spec: &str,
    version: Option<&str>,
) -> anyhow::Result<ArmoryInstallResult> {
    if crate::extension_registry::is_bare_name(spec) {
        crate::extension_registry::install_by_name(spec, version).await?;
        let path = crate::extension_cli::extensions_dir()?.join(spec);
        Ok(ArmoryInstallResult {
            kind: ArmoryItemKind::Extension,
            id: spec.to_string(),
            path: Some(path.display().to_string()),
            message: format!("Installed extension {spec} from armory"),
        })
    } else {
        if version.is_some() {
            anyhow::bail!("--version is only supported for armory extension names");
        }
        crate::extension_cli::install(spec)?;
        Ok(ArmoryInstallResult {
            kind: ArmoryItemKind::Extension,
            id: spec.to_string(),
            path: None,
            message: format!("Installed extension from {spec}"),
        })
    }
}

async fn install_skill(spec: &str) -> anyhow::Result<ArmoryInstallResult> {
    let target = parse_armory_plugin_target(spec).unwrap_or_else(|| InstallTarget {
        root: None,
        slug: spec.to_string(),
    });
    if target.root.as_deref().is_some_and(|root| root != "skills") {
        anyhow::bail!("'{spec}' is not an armory skill target");
    }
    validate_slug(&target.slug)?;

    let client = armory_client(std::time::Duration::from_secs(20))?;
    let manifest_path = format!("skills/{}/plugin.toml", target.slug);
    let manifest_text = fetch_raw_text(&client, &manifest_path).await?;
    let manifest = crate::plugins::armory::ArmoryManifest::parse(&manifest_text)?;
    if manifest.plugin.plugin_type != crate::plugins::armory::PluginType::Skill {
        anyhow::bail!("armory item '{}' is not a skill", target.slug);
    }
    let skill = manifest
        .skill
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("skill plugin '{}' has no [skill] section", target.slug))?;
    let guidance_path = normalize_relative_path(&skill.guidance)?;
    let guidance = fetch_raw_text(
        &client,
        &format!("skills/{}/{}", target.slug, guidance_path),
    )
    .await?;
    let content = ensure_skill_frontmatter(&target.slug, &manifest, &guidance);

    let skill_dir = crate::paths::omegon_home()?
        .join("skills")
        .join(&target.slug);
    fs::create_dir_all(&skill_dir)?;
    fs::write(skill_dir.join("SKILL.md"), content)?;

    Ok(ArmoryInstallResult {
        kind: ArmoryItemKind::Skill,
        id: target.slug.clone(),
        path: Some(skill_dir.display().to_string()),
        message: format!("Installed skill {} to {}", target.slug, skill_dir.display()),
    })
}

async fn install_plugin(spec: &str) -> anyhow::Result<ArmoryInstallResult> {
    let target = parse_armory_plugin_target(spec).unwrap_or_else(|| InstallTarget {
        root: None,
        slug: spec.to_string(),
    });
    let root = target.root.as_deref().unwrap_or("plugins");
    let root = match root {
        "personas" | "tones" | "examples" => root,
        "skills" => return install_skill(spec).await,
        "plugins" => infer_plugin_root(&target.slug).await?,
        other => anyhow::bail!("unsupported armory plugin root '{other}'"),
    };
    validate_slug(&target.slug)?;

    let (plugin_dir, temp_root) = checkout_armory_subdir(root, &target.slug)?;
    let manifest_text = fs::read_to_string(plugin_dir.join("plugin.toml"))?;
    let manifest = crate::plugins::armory::ArmoryManifest::parse(&manifest_text)?;
    if manifest.plugin.plugin_type == crate::plugins::armory::PluginType::Skill {
        return install_skill(&format!("skills/{}", target.slug)).await;
    }

    let plugins_dir = crate::plugin_cli::plugins_dir()?;
    fs::create_dir_all(&plugins_dir)?;
    let install_id = target.slug.clone();
    let final_dir = plugins_dir.join(&install_id);
    if final_dir.exists() || final_dir.is_symlink() {
        fs::remove_file(&final_dir).or_else(|_| fs::remove_dir_all(&final_dir))?;
    }
    if fs::rename(&plugin_dir, &final_dir).is_err() {
        copy_dir_recursive(&plugin_dir, &final_dir)?;
        fs::remove_dir_all(&plugin_dir)?;
    }
    fs::remove_dir_all(&temp_root).ok();

    Ok(ArmoryInstallResult {
        kind: ArmoryItemKind::Plugin,
        id: install_id,
        path: Some(final_dir.display().to_string()),
        message: format!(
            "Installed armory plugin {} from {root}/{} to {}",
            manifest.plugin.name,
            target.slug,
            final_dir.display()
        ),
    })
}

pub fn render_items(items: &[ArmoryItem]) -> String {
    if items.is_empty() {
        return "No armory items found.".into();
    }

    let mut out = String::new();
    let mut current = None;
    for item in items {
        if current != Some(item.kind) {
            if !out.is_empty() {
                out.push('\n');
            }
            current = Some(item.kind);
            out.push_str(&format!("{}s\n\n", title_case(item.kind.to_string())));
        }
        let marker = if item.installed { "+" } else { "o" };
        out.push_str(&format!(
            "  {marker} {:<28} {:<12} {}\n",
            item.id, item.category, item.description
        ));
        out.push_str(&format!("    install: {}\n", item.install_hint));
    }
    out.push_str("\n  + = installed    o = available");
    out
}

async fn fetch_extensions(
    client: &reqwest::Client,
    installed: &InstalledState,
) -> anyhow::Result<Vec<ArmoryItem>> {
    let registry = crate::extension_registry::fetch_registry(client).await?;
    Ok(registry
        .into_iter()
        .map(|(id, entry)| ArmoryItem {
            kind: ArmoryItemKind::Extension,
            name: id.clone(),
            installed: installed.extension(&id),
            install_hint: format!("omegon extension install {id}"),
            id,
            description: entry.description,
            category: entry.category,
            version: None,
            source: entry.repo,
            manifest_id: None,
        })
        .collect())
}

async fn fetch_agents(
    client: &reqwest::Client,
    installed: &InstalledState,
) -> anyhow::Result<Vec<ArmoryItem>> {
    let url = format!("{ARMORY_RAW_BASE}/catalog-registry.toml");
    let text = client
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let registry: HashMap<String, CatalogRegistryEntry> = toml::from_str(&text)?;
    Ok(registry
        .into_iter()
        .map(|(id, entry)| ArmoryItem {
            kind: ArmoryItemKind::Agent,
            name: entry.name,
            description: entry.description,
            category: entry.domain,
            version: Some(entry.version),
            source: format!("catalog/{id}/agent.toml"),
            manifest_id: None,
            installed: installed.agent(&id),
            install_hint: "omegon catalog install".into(),
            id,
        })
        .collect())
}

async fn fetch_plugin_manifests(
    client: &reqwest::Client,
    requested: ArmoryKind,
    installed: &InstalledState,
) -> anyhow::Result<Vec<ArmoryItem>> {
    let mut items = Vec::new();
    for root in ["personas", "tones", "skills", "examples"] {
        if matches!(requested, ArmoryKind::Skills) && root != "skills" {
            continue;
        }
        if matches!(requested, ArmoryKind::Plugins) && root == "skills" {
            continue;
        }

        let url = format!("{ARMORY_API_BASE}/{root}");
        let entries: Vec<GitHubContentEntry> = client
            .get(&url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        for entry in entries
            .into_iter()
            .filter(|entry| entry.entry_type == "dir")
        {
            let slug = entry.name;
            let manifest_path = format!("{root}/{slug}/plugin.toml");
            let raw_url = format!("{ARMORY_RAW_BASE}/{manifest_path}");
            let manifest_text = client
                .get(&raw_url)
                .send()
                .await?
                .error_for_status()?
                .text()
                .await?;
            let manifest = crate::plugins::armory::ArmoryManifest::parse(&manifest_text)?;
            items.push(item_from_manifest(
                root,
                &slug,
                &manifest_path,
                manifest,
                installed,
            ));
        }
    }
    Ok(items)
}

fn item_from_manifest(
    root: &str,
    slug: &str,
    manifest_path: &str,
    manifest: crate::plugins::armory::ArmoryManifest,
    installed: &InstalledState,
) -> ArmoryItem {
    let plugin_type = manifest.plugin.plugin_type.to_string();
    let kind = if matches!(
        manifest.plugin.plugin_type,
        crate::plugins::armory::PluginType::Skill
    ) {
        ArmoryItemKind::Skill
    } else {
        ArmoryItemKind::Plugin
    };
    let id = slug.to_string();
    ArmoryItem {
        kind,
        id: id.clone(),
        name: manifest.plugin.name,
        description: manifest.plugin.description,
        category: plugin_type,
        version: Some(manifest.plugin.version),
        source: manifest_path.to_string(),
        manifest_id: Some(manifest.plugin.id),
        installed: match kind {
            ArmoryItemKind::Skill => installed.skill(&id),
            ArmoryItemKind::Plugin => installed.plugin(root, &id),
            ArmoryItemKind::Extension | ArmoryItemKind::Agent => false,
        },
        install_hint: match kind {
            ArmoryItemKind::Skill => {
                format!("omegon armory install {root}/{id}")
            }
            ArmoryItemKind::Plugin => {
                format!("omegon armory install {root}/{id}")
            }
            ArmoryItemKind::Extension | ArmoryItemKind::Agent => String::new(),
        },
    }
}

#[derive(Debug, Default)]
struct InstalledState {
    home: Option<PathBuf>,
    cwd: PathBuf,
}

impl InstalledState {
    fn collect(cwd: &Path) -> Self {
        Self {
            home: crate::paths::omegon_home().ok(),
            cwd: cwd.to_path_buf(),
        }
    }

    fn extension(&self, id: &str) -> bool {
        self.home
            .as_ref()
            .is_some_and(|home| home.join("extensions").join(id).exists())
    }

    fn agent(&self, id: &str) -> bool {
        self.home
            .as_ref()
            .is_some_and(|home| home.join("catalog").join(id).join("agent.toml").exists())
    }

    fn skill(&self, slug: &str) -> bool {
        self.home
            .as_ref()
            .is_some_and(|home| home.join("skills").join(slug).join("SKILL.md").exists())
            || self
                .cwd
                .join(".omegon")
                .join("skills")
                .join(slug)
                .join("SKILL.md")
                .exists()
            || self.plugin("skills", slug)
    }

    fn plugin(&self, root: &str, slug: &str) -> bool {
        let project_plugin = self
            .cwd
            .join(".omegon")
            .join("plugins")
            .join(slug)
            .join("plugin.toml")
            .exists();
        let home_plugin = self.home.as_ref().is_some_and(|home| {
            home.join("plugins").join(slug).join("plugin.toml").exists()
                || home
                    .join("armory")
                    .join(root)
                    .join(slug)
                    .join("plugin.toml")
                    .exists()
        });
        project_plugin || home_plugin
    }
}

fn title_case(input: String) -> String {
    let mut chars = input.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => input,
    }
}

fn armory_client(timeout: std::time::Duration) -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(timeout)
        .user_agent("omegon")
        .build()?)
}

async fn fetch_raw_text(client: &reqwest::Client, path: &str) -> anyhow::Result<String> {
    let url = format!("{ARMORY_RAW_BASE}/{path}");
    Ok(client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?)
}

fn parse_armory_plugin_target(spec: &str) -> Option<InstallTarget> {
    let spec = spec.trim();
    let path = spec
        .strip_prefix("https://github.com/styrene-lab/omegon-armory/tree/main/")
        .or_else(|| spec.strip_prefix("https://github.com/styrene-lab/omegon-armory/"))
        .unwrap_or(spec)
        .trim_matches('/');
    let mut parts = path.split('/').filter(|part| !part.is_empty());
    let first = parts.next()?;
    let second = parts.next();
    match (first, second, parts.next()) {
        ("personas" | "tones" | "skills" | "examples", Some(slug), None) => Some(InstallTarget {
            root: Some(first.to_string()),
            slug: slug.to_string(),
        }),
        _ => None,
    }
}

fn validate_slug(slug: &str) -> anyhow::Result<()> {
    let valid = !slug.is_empty()
        && !slug.contains("..")
        && slug
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
    if !valid {
        anyhow::bail!("invalid armory item name '{slug}'");
    }
    Ok(())
}

fn normalize_relative_path(path: &str) -> anyhow::Result<String> {
    let trimmed = path.trim().trim_start_matches("./");
    if trimmed.is_empty()
        || trimmed.starts_with('/')
        || trimmed.contains('\\')
        || trimmed
            .split('/')
            .any(|part| part == ".." || part.is_empty())
    {
        anyhow::bail!("invalid relative path '{path}' in armory manifest");
    }
    Ok(trimmed.to_string())
}

fn ensure_skill_frontmatter(
    slug: &str,
    manifest: &crate::plugins::armory::ArmoryManifest,
    guidance: &str,
) -> String {
    let trimmed = guidance.trim_start();
    if trimmed.starts_with("+++\n") || trimmed.starts_with("---\n") {
        return guidance.to_string();
    }

    let skill_manifest = omegon_skills::SkillManifest {
        name: slug.to_string(),
        description: manifest.plugin.description.clone(),
        id: Some(manifest.plugin.id.clone()),
        version: Some(manifest.plugin.version.clone()),
        tags: Vec::new(),
        aliases: Vec::new(),
        triggers: Vec::new(),
        activation: None,
        profile: Vec::new(),
        project_signals: Vec::new(),
        trusted_paths: Vec::new(),
        output_path: None,
        output_format: None,
        max_turns: None,
        posture: None,
        provenance: None,
    };
    format!("{}\n\n{}", skill_manifest.to_frontmatter(), guidance)
}

async fn infer_plugin_root(slug: &str) -> anyhow::Result<&'static str> {
    validate_slug(slug)?;
    let client = armory_client(std::time::Duration::from_secs(15))?;
    for root in ["personas", "tones", "examples"] {
        let url = format!("{ARMORY_RAW_BASE}/{root}/{slug}/plugin.toml");
        let status = client.get(url).send().await?.status();
        if status.is_success() {
            return Ok(root);
        }
    }
    anyhow::bail!(
        "armory plugin '{slug}' not found. Use personas/{slug}, tones/{slug}, or examples/{slug}"
    )
}

fn checkout_armory_subdir(root: &str, slug: &str) -> anyhow::Result<(PathBuf, PathBuf)> {
    let tmp = std::env::temp_dir().join(format!("omegon-armory-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp)?;
    let repo_dir = tmp.join("repo");
    let source = format!("{root}/{slug}");

    run_git([
        "clone",
        "--depth",
        "1",
        "--filter=blob:none",
        "--sparse",
        "https://github.com/styrene-lab/omegon-armory.git",
        repo_dir
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("temporary path is not valid UTF-8"))?,
    ])?;
    run_git([
        "-C",
        repo_dir
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("temporary path is not valid UTF-8"))?,
        "sparse-checkout",
        "set",
        &source,
    ])?;

    let plugin_dir = repo_dir.join(root).join(slug);
    if !plugin_dir.join("plugin.toml").exists() {
        fs::remove_dir_all(&tmp).ok();
        anyhow::bail!("armory item {source} does not contain plugin.toml");
    }
    Ok((plugin_dir, tmp))
}

fn run_git<const N: usize>(args: [&str; N]) -> anyhow::Result<()> {
    let status = Command::new("git").args(args).status()?;
    if !status.success() {
        anyhow::bail!("git command failed while installing armory plugin");
    }
    Ok(())
}

fn copy_dir_recursive(from: &Path, to: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let source = entry.path();
        let target = to.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_dir_recursive(&source, &target)?;
        } else if file_type.is_file() {
            fs::copy(&source, &target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_groups_items_by_kind() {
        let items = vec![
            ArmoryItem {
                kind: ArmoryItemKind::Extension,
                id: "flynt".into(),
                name: "flynt".into(),
                description: "Graph workflow builder".into(),
                category: "automation".into(),
                version: None,
                source: "https://github.com/styrene-lab/flynt".into(),
                manifest_id: None,
                installed: false,
                install_hint: "omegon extension install flynt".into(),
            },
            ArmoryItem {
                kind: ArmoryItemKind::Skill,
                id: "security".into(),
                name: "Security Review".into(),
                description: "Security checklist".into(),
                category: "skill".into(),
                version: Some("1.0.0".into()),
                source: "skills/security/plugin.toml".into(),
                manifest_id: Some("dev.styrene.omegon.skill.security".into()),
                installed: true,
                install_hint: "omegon armory install skills/security".into(),
            },
        ];

        let rendered = render_items(&items);
        assert!(rendered.contains("Extensions"));
        assert!(rendered.contains("Skills"));
        assert!(rendered.contains("o flynt"));
        assert!(rendered.contains("+ security"));
    }

    #[test]
    fn item_from_skill_manifest_uses_slug_and_manifest_id() {
        let manifest = crate::plugins::armory::ArmoryManifest::parse(
            r#"
            [plugin]
            type = "skill"
            id = "dev.styrene.omegon.skill.security"
            name = "Security Review"
            version = "1.0.0"
            description = "Security checklist"

            [skill]
            guidance = "SKILL.md"
            "#,
        )
        .unwrap();
        let installed = InstalledState::default();

        let item = item_from_manifest(
            "skills",
            "security",
            "skills/security/plugin.toml",
            manifest,
            &installed,
        );

        assert_eq!(item.kind, ArmoryItemKind::Skill);
        assert_eq!(item.id, "security");
        assert_eq!(
            item.manifest_id.as_deref(),
            Some("dev.styrene.omegon.skill.security")
        );
        assert_eq!(item.install_hint, "omegon armory install skills/security");
    }

    #[test]
    fn parse_armory_plugin_target_accepts_scoped_paths_and_tree_urls() {
        let target = parse_armory_plugin_target("skills/security").unwrap();
        assert_eq!(target.root.as_deref(), Some("skills"));
        assert_eq!(target.slug, "security");

        let target = parse_armory_plugin_target(
            "https://github.com/styrene-lab/omegon-armory/tree/main/personas/tutor",
        )
        .unwrap();
        assert_eq!(target.root.as_deref(), Some("personas"));
        assert_eq!(target.slug, "tutor");
    }

    #[test]
    fn ensure_skill_frontmatter_adds_canonical_metadata() {
        let manifest = crate::plugins::armory::ArmoryManifest::parse(
            r#"
            [plugin]
            type = "skill"
            id = "dev.styrene.omegon.skill.security"
            name = "Security Review"
            version = "1.0.0"
            description = "Security checklist"

            [skill]
            guidance = "SKILL.md"
            "#,
        )
        .unwrap();
        let content = ensure_skill_frontmatter("security", &manifest, "# Security\n");
        let (manifest, body) = omegon_skills::parse_skill_file(&content);
        assert_eq!(manifest.name, "security");
        assert_eq!(manifest.description, "Security checklist");
        assert_eq!(
            manifest.id.as_deref(),
            Some("dev.styrene.omegon.skill.security")
        );
        assert_eq!(manifest.version.as_deref(), Some("1.0.0"));
        assert!(body.contains("# Security"));
    }
}
