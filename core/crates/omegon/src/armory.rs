//! Unified Armory discovery for extensions, plugins, skills, and catalog agents.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

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
                format!("https://github.com/styrene-lab/omegon-armory/tree/main/{root}/{id}")
            }
            ArmoryItemKind::Plugin => {
                format!("https://github.com/styrene-lab/omegon-armory/tree/main/{root}/{id}")
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
                install_hint:
                    "https://github.com/styrene-lab/omegon-armory/tree/main/skills/security".into(),
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
        assert_eq!(
            item.install_hint,
            "https://github.com/styrene-lab/omegon-armory/tree/main/skills/security"
        );
    }
}
