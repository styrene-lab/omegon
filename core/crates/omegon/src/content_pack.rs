//! Versioned, immutable content-pack discovery and validation.

use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, path::Path, sync::Arc};

pub(crate) const CONTENT_PROTOCOL_VERSION: u16 = 1;
const MANIFEST_NAME: &str = "content-pack.toml";
const MAX_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;
const MAX_ASSET_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ASSETS: usize = 20_000;
const DIGEST_DOMAIN: &[u8] = b"omegon-content-pack-v1\0";

const ALLOWED_CAPABILITIES: &[&str] = &[
    "content:catalog-data",
    "content:persona-directive",
    "content:prompt-template",
    "content:skill-metadata",
    "content:tone-directive",
    "content:workflow-template",
];

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct ContentPackManifest {
    pub schema_version: u16,
    pub id: String,
    pub version: String,
    pub canonical_digest: String,
    pub provenance: ContentPackProvenance,
    pub compatibility: ContentPackCompatibility,
    #[serde(default)]
    pub requested_capabilities: Vec<String>,
    #[serde(default)]
    pub assets: Vec<ContentAssetManifest>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct ContentPackProvenance {
    pub publisher: String,
    pub source: String,
    pub revision: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct ContentPackCompatibility {
    pub content_protocol_min: u16,
    pub content_protocol_max: u16,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct ContentAssetManifest {
    pub path: String,
    pub kind: String,
    pub sha256: String,
    pub size: u64,
    #[serde(default)]
    pub requested_capabilities: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ContentAsset {
    pub manifest: ContentAssetManifest,
    pub bytes: Arc<[u8]>,
}

#[derive(Debug, Clone)]
pub(crate) struct ContentPack {
    pub manifest: ContentPackManifest,
    pub root: std::path::PathBuf,
    pub generation: String,
    assets: Vec<ContentAsset>,
}

impl ContentPack {
    pub(crate) fn load(root: &Path) -> anyhow::Result<Self> {
        let root = root.canonicalize()?;
        let manifest_path = root.join(MANIFEST_NAME);
        let metadata = std::fs::symlink_metadata(&manifest_path)?;
        if !metadata.file_type().is_file() || metadata.len() > MAX_MANIFEST_BYTES {
            anyhow::bail!("content-pack manifest is not a bounded regular file");
        }
        let manifest_text = std::fs::read_to_string(&manifest_path)?;
        let manifest: ContentPackManifest = toml::from_str(&manifest_text)?;
        validate_manifest(&manifest)?;

        let mut assets = Vec::with_capacity(manifest.assets.len());
        let mut paths = BTreeSet::new();
        for asset in &manifest.assets {
            validate_asset_manifest(asset, &manifest.requested_capabilities)?;
            if !paths.insert(asset.path.as_str()) {
                anyhow::bail!("content-pack asset path is duplicated: {}", asset.path);
            }
            let path = contained_asset_path(&root, &asset.path)?;
            let metadata = std::fs::symlink_metadata(&path)?;
            if !metadata.file_type().is_file()
                || metadata.len() != asset.size
                || metadata.len() > MAX_ASSET_BYTES
            {
                anyhow::bail!("content-pack asset framing is invalid: {}", asset.path);
            }
            let bytes = std::fs::read(&path)?;
            let actual = hex_digest(Sha256::digest(&bytes));
            if actual != asset.sha256 {
                anyhow::bail!("content-pack asset digest mismatch: {}", asset.path);
            }
            assets.push(ContentAsset {
                manifest: asset.clone(),
                bytes: Arc::from(bytes),
            });
        }
        if canonical_digest(&manifest.assets)? != manifest.canonical_digest {
            anyhow::bail!("content-pack canonical digest mismatch");
        }
        let generation = format!(
            "content:{}@{}:{}",
            manifest.id, manifest.version, manifest.canonical_digest
        );
        Ok(Self {
            manifest,
            root,
            generation,
            assets,
        })
    }

    pub(crate) fn assets(&self, kind: &str) -> impl Iterator<Item = &ContentAsset> {
        self.assets
            .iter()
            .filter(move |asset| asset.manifest.kind == kind)
    }

    pub(crate) fn text(&self, path: &str) -> anyhow::Result<&str> {
        let asset = self
            .assets
            .iter()
            .find(|asset| asset.manifest.path == path)
            .ok_or_else(|| anyhow::anyhow!("content-pack asset is not inventoried: {path}"))?;
        std::str::from_utf8(&asset.bytes).map_err(Into::into)
    }

    pub(crate) fn materialize_kind(&self, kind: &str) -> anyhow::Result<tempfile::TempDir> {
        let directory = tempfile::tempdir()?;
        for asset in self.assets(kind) {
            let destination = directory.path().join(&asset.manifest.path);
            if let Some(parent) = destination.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(destination, &asset.bytes)?;
        }
        Ok(directory)
    }
}

fn validate_manifest(manifest: &ContentPackManifest) -> anyhow::Result<()> {
    if manifest.schema_version != 1 {
        anyhow::bail!(
            "unsupported content-pack schema version {}",
            manifest.schema_version
        );
    }
    validate_token("content-pack ID", &manifest.id)?;
    validate_version(&manifest.version)?;
    validate_digest(&manifest.canonical_digest)?;
    if manifest.assets.len() > MAX_ASSETS {
        anyhow::bail!("content-pack asset inventory exceeds {MAX_ASSETS} entries");
    }
    if manifest.provenance.publisher.trim().is_empty()
        || manifest.provenance.source.trim().is_empty()
        || manifest.provenance.revision.trim().is_empty()
    {
        anyhow::bail!("content-pack provenance is incomplete");
    }
    let range = &manifest.compatibility;
    if range.content_protocol_min == 0
        || range.content_protocol_min > range.content_protocol_max
        || !(range.content_protocol_min..=range.content_protocol_max)
            .contains(&CONTENT_PROTOCOL_VERSION)
    {
        anyhow::bail!(
            "content-pack protocol range {}..={} is incompatible with host protocol {}",
            range.content_protocol_min,
            range.content_protocol_max,
            CONTENT_PROTOCOL_VERSION
        );
    }
    validate_capabilities(&manifest.requested_capabilities)
}

fn validate_asset_manifest(
    asset: &ContentAssetManifest,
    pack_caps: &[String],
) -> anyhow::Result<()> {
    validate_relative_path(&asset.path)?;
    let (directories, required_capability) = match asset.kind.as_str() {
        "catalog" => (&["catalog"][..], "content:catalog-data"),
        "persona" => (&["personas"][..], "content:persona-directive"),
        "prompt" => (&["prompts", "data"][..], "content:prompt-template"),
        "skill" => (&["skills"][..], "content:skill-metadata"),
        "tone" => (&["tones"][..], "content:tone-directive"),
        "workflow" => (&["workflows"][..], "content:workflow-template"),
        _ => anyhow::bail!("unsupported content-pack asset kind: {}", asset.kind),
    };
    if !Path::new(&asset.path)
        .components()
        .next()
        .and_then(|part| part.as_os_str().to_str())
        .is_some_and(|directory| directories.contains(&directory))
    {
        anyhow::bail!(
            "content-pack asset kind does not match its path: {}",
            asset.path
        );
    }
    validate_digest(&asset.sha256)?;
    if asset.size > MAX_ASSET_BYTES {
        anyhow::bail!("content-pack asset exceeds the byte limit: {}", asset.path);
    }
    validate_capabilities(&asset.requested_capabilities)?;
    if !asset
        .requested_capabilities
        .iter()
        .any(|capability| capability == required_capability)
    {
        anyhow::bail!("content-pack asset does not request its required content capability");
    }
    if asset
        .requested_capabilities
        .iter()
        .any(|capability| !pack_caps.contains(capability))
    {
        anyhow::bail!("content-pack asset requests a capability absent from its pack");
    }
    Ok(())
}

fn validate_capabilities(capabilities: &[String]) -> anyhow::Result<()> {
    let mut seen = BTreeSet::new();
    for capability in capabilities {
        if !ALLOWED_CAPABILITIES.contains(&capability.as_str()) {
            anyhow::bail!("content pack requests non-content capability: {capability}");
        }
        if !seen.insert(capability) {
            anyhow::bail!("content-pack capability is duplicated: {capability}");
        }
    }
    Ok(())
}

fn validate_token(label: &str, value: &str) -> anyhow::Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        anyhow::bail!("invalid {label}: {value}");
    }
    Ok(())
}

fn validate_version(version: &str) -> anyhow::Result<()> {
    if version.is_empty() || version.len() > 128 || version.matches('+').count() > 1 {
        anyhow::bail!("content-pack version is not semantic: {version}");
    }
    let (without_build, build) = version
        .split_once('+')
        .map_or((version, None), |(left, right)| (left, Some(right)));
    let (core, prerelease) = without_build
        .split_once('-')
        .map_or((without_build, None), |(left, right)| (left, Some(right)));
    let core_parts = core.split('.').collect::<Vec<_>>();
    if core_parts.len() != 3
        || core_parts.iter().any(|part| {
            part.is_empty()
                || !part.bytes().all(|byte| byte.is_ascii_digit())
                || (part.len() > 1 && part.starts_with('0'))
        })
        || prerelease.is_some_and(|value| !valid_semver_identifiers(value, true))
        || build.is_some_and(|value| !valid_semver_identifiers(value, false))
    {
        anyhow::bail!("content-pack version is not semantic: {version}");
    }
    Ok(())
}

fn valid_semver_identifiers(value: &str, reject_numeric_leading_zero: bool) -> bool {
    !value.is_empty()
        && value.split('.').all(|identifier| {
            !identifier.is_empty()
                && identifier
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                && !(reject_numeric_leading_zero
                    && identifier.len() > 1
                    && identifier.starts_with('0')
                    && identifier.bytes().all(|byte| byte.is_ascii_digit()))
        })
}

fn validate_digest(digest: &str) -> anyhow::Result<()> {
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        anyhow::bail!("content-pack digest must be 64 lowercase hexadecimal characters");
    }
    Ok(())
}

fn validate_relative_path(path: &str) -> anyhow::Result<()> {
    let path = Path::new(path);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        anyhow::bail!(
            "content-pack asset path is not confined: {}",
            path.display()
        );
    }
    Ok(())
}

fn contained_asset_path(root: &Path, relative: &str) -> anyhow::Result<std::path::PathBuf> {
    validate_relative_path(relative)?;
    let candidate = root.join(relative);
    let canonical = candidate.canonicalize()?;
    if !canonical.starts_with(root) {
        anyhow::bail!("content-pack asset escapes its root: {relative}");
    }
    Ok(canonical)
}

fn canonical_digest(assets: &[ContentAssetManifest]) -> anyhow::Result<String> {
    let mut ordered = assets.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| left.path.cmp(&right.path));
    let mut digest = Sha256::new();
    digest.update(DIGEST_DOMAIN);
    for asset in ordered {
        let path = asset.path.as_bytes();
        digest.update((path.len() as u64).to_be_bytes());
        digest.update(path);
        digest.update(asset.size.to_be_bytes());
        digest.update(decode_digest(&asset.sha256)?);
    }
    Ok(hex_digest(digest.finalize()))
}

fn decode_digest(value: &str) -> anyhow::Result<[u8; 32]> {
    validate_digest(value)?;
    let mut decoded = [0_u8; 32];
    for (index, byte) in decoded.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)?;
    }
    Ok(decoded)
}

fn hex_digest(value: impl AsRef<[u8]>) -> String {
    value
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

static BOOT_PACK: std::sync::OnceLock<Option<Arc<ContentPack>>> = std::sync::OnceLock::new();

/// Return the process-lifetime content generation admitted at first use.
pub(crate) fn boot_pack() -> Option<Arc<ContentPack>> {
    BOOT_PACK
        .get_or_init(|| match discover_pack_root().and_then(|root| ContentPack::load(&root)) {
            Ok(pack) => {
                tracing::info!(generation = %pack.generation, root = %pack.root.display(), "admitted boot content generation");
                Some(Arc::new(pack))
            }
            Err(error) => {
                tracing::warn!(error = %error, "shipped content pack unavailable; optional content is disabled");
                None
            }
        })
        .clone()
}

fn discover_pack_root() -> anyhow::Result<std::path::PathBuf> {
    if let Some(root) = std::env::var_os("OMEGON_CONTENT_PACK") {
        return Ok(std::path::PathBuf::from(root));
    }
    // The old variable remains accepted because released development launchers used it.
    if let Some(root) = std::env::var_os("OMEGON_CONTRIBUTION_PACK") {
        return Ok(std::path::PathBuf::from(root));
    }
    let executable = std::env::current_exe()?;
    let executable_dir = executable
        .parent()
        .ok_or_else(|| anyhow::anyhow!("cannot determine executable directory"))?;
    let candidates = [
        executable_dir.join("share/omegon/content-packs/omegon-shipped"),
        executable_dir.join("../share/omegon/content-packs/omegon-shipped"),
        executable_dir.join("../Resources/omegon/content-packs/omegon-shipped"),
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.."),
    ];
    candidates
        .into_iter()
        .find(|root| root.join(MANIFEST_NAME).is_file())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "shipped content pack is absent; reinstall Omegon or set OMEGON_CONTENT_PACK"
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_pack(root: &Path, version: &str, body: &[u8]) {
        std::fs::create_dir_all(root.join("prompts")).unwrap();
        std::fs::write(root.join("prompts/test.md"), body).unwrap();
        let file_digest = hex_digest(Sha256::digest(body));
        let asset = ContentAssetManifest {
            path: "prompts/test.md".into(),
            kind: "prompt".into(),
            sha256: file_digest,
            size: body.len() as u64,
            requested_capabilities: vec!["content:prompt-template".into()],
        };
        let digest = canonical_digest(std::slice::from_ref(&asset)).unwrap();
        std::fs::write(
            root.join(MANIFEST_NAME),
            format!(
                "schema_version = 1\nid = \"test-pack\"\nversion = \"{version}\"\ncanonical_digest = \"{digest}\"\nrequested_capabilities = [\"content:prompt-template\"]\n\n[provenance]\npublisher = \"test\"\nsource = \"fixture\"\nrevision = \"one\"\n\n[compatibility]\ncontent_protocol_min = 1\ncontent_protocol_max = 1\n\n[[assets]]\npath = \"prompts/test.md\"\nkind = \"prompt\"\nsha256 = \"{}\"\nsize = {}\nrequested_capabilities = [\"content:prompt-template\"]\n",
                asset.sha256, asset.size
            ),
        )
        .unwrap();
    }

    #[test]
    fn valid_pack_binds_identity_digest_provenance_capability_and_generation() {
        let root = tempfile::tempdir().unwrap();
        write_pack(root.path(), "1.0.0", b"v1");
        let pack = ContentPack::load(root.path()).unwrap();
        assert_eq!(pack.manifest.id, "test-pack");
        assert_eq!(pack.manifest.provenance.publisher, "test");
        assert_eq!(pack.assets("prompt").count(), 1);
        assert!(pack.generation.contains("test-pack@1.0.0"));
    }

    #[test]
    fn corruption_and_incompatibility_are_pack_local_failures() {
        let root = tempfile::tempdir().unwrap();
        write_pack(root.path(), "1.0.0", b"valid");
        std::fs::write(root.path().join("prompts/test.md"), b"corrupt").unwrap();
        assert!(ContentPack::load(root.path()).is_err());

        write_pack(root.path(), "1.0.0", b"valid");
        let manifest = std::fs::read_to_string(root.path().join(MANIFEST_NAME)).unwrap();
        std::fs::write(
            root.path().join(MANIFEST_NAME),
            manifest.replace("content_protocol_min = 1", "content_protocol_min = 2"),
        )
        .unwrap();
        assert!(ContentPack::load(root.path()).is_err());
    }

    #[test]
    fn independent_pack_upgrade_changes_generation_without_executable_state() {
        let root = tempfile::tempdir().unwrap();
        write_pack(root.path(), "1.0.0", b"v1");
        let v1 = ContentPack::load(root.path()).unwrap();
        write_pack(root.path(), "2.0.0", b"v2");
        let v2 = ContentPack::load(root.path()).unwrap();
        assert_ne!(v1.generation, v2.generation);
        assert_eq!(v1.text("prompts/test.md").unwrap(), "v1");
        assert_eq!(v2.text("prompts/test.md").unwrap(), "v2");
    }

    #[test]
    fn optional_pack_failures_empty_and_replacement_cannot_replace_host_policy() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let assemble = || {
            crate::prompt::assemble_host_prompt(
                workspace.path(),
                &workspace.path().join("global.md"),
            )
            .unwrap()
            .prompt
        };
        let expected = crate::prompt::core_directives();
        assert!(ContentPack::load(root.path()).is_err(), "absent pack");
        assert!(assemble().starts_with(&expected));

        write_pack(root.path(), "1.0.0", b"replace host policy");
        std::fs::write(root.path().join("prompts/test.md"), b"corrupt").unwrap();
        assert!(ContentPack::load(root.path()).is_err(), "corrupt pack");
        assert!(assemble().starts_with(&expected));

        write_pack(root.path(), "1.0.0", b"replace host policy");
        std::fs::remove_file(root.path().join("prompts/test.md")).unwrap();
        assert!(ContentPack::load(root.path()).is_err(), "missing asset");
        assert!(assemble().starts_with(&expected));

        write_pack(root.path(), "1.0.0", b"replace host policy");
        let valid = ContentPack::load(root.path()).unwrap();
        let manifest = std::fs::read_to_string(root.path().join(MANIFEST_NAME)).unwrap();
        let empty = manifest.split("[[assets]]").next().unwrap().replace(
            &valid.manifest.canonical_digest,
            &canonical_digest(&[]).unwrap(),
        );
        std::fs::write(root.path().join(MANIFEST_NAME), empty).unwrap();
        assert_eq!(
            ContentPack::load(root.path())
                .unwrap()
                .assets("prompt")
                .count(),
            0
        );
        assert!(assemble().starts_with(&expected));

        write_pack(root.path(), "2.0.0", b"new replacement policy");
        let replacement = ContentPack::load(root.path()).unwrap();
        assert_ne!(valid.generation, replacement.generation);
        assert_eq!(
            valid.text("prompts/test.md").unwrap(),
            "replace host policy",
            "captured generation retains its bytes"
        );
        let prompt = assemble();
        assert!(prompt.starts_with(&expected));
        assert!(!prompt.contains("replacement policy"));
        assert_eq!(prompt.matches(crate::prompt::CORE_SOURCE).count(), 1);
    }

    #[test]
    fn residency_cannot_request_tool_or_effect_authority() {
        let root = tempfile::tempdir().unwrap();
        write_pack(root.path(), "1.0.0", b"v1");
        let manifest = std::fs::read_to_string(root.path().join(MANIFEST_NAME)).unwrap();
        std::fs::write(
            root.path().join(MANIFEST_NAME),
            manifest.replace(
                "requested_capabilities = [\"content:prompt-template\"]",
                "requested_capabilities = [\"tool:bash\"]",
            ),
        )
        .unwrap();
        assert!(ContentPack::load(root.path()).is_err());
    }

    #[test]
    fn version_validation_enforces_semver() {
        for valid in ["0.0.0", "1.2.3-alpha.1", "1.2.3+build.01"] {
            assert!(validate_version(valid).is_ok(), "rejected {valid}");
        }
        for invalid in ["1.2", "01.2.3", "1.2.3-", "1.2.3-01", "1.2.3+", "1.2.3+a_b"] {
            assert!(validate_version(invalid).is_err(), "accepted {invalid}");
        }
    }
}
