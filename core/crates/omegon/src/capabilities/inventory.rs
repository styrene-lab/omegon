use std::path::Path;

use serde::{Deserialize, Serialize};

use super::agents::{list_agent_bundle_summaries_from_dir, AgentBundleSummary};
use super::armory::{list_armory_profiles_from_root, ArmoryProfileSummary};
use super::extensions::{list_installed_extension_capabilities_from_dir, ExtensionCapabilitySummary};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityInventorySnapshot {
    pub installed_extensions: Vec<ExtensionCapabilitySummary>,
    pub armory_profiles: Vec<ArmoryProfileSummary>,
    pub agent_bundles: Vec<AgentBundleSummary>,
}

#[derive(Debug, Clone, Copy)]
pub struct CapabilityInventoryRoots<'a> {
    pub extensions_dir: &'a Path,
    pub armory_root: &'a Path,
    pub catalog_dir: &'a Path,
}

pub fn build_capability_inventory_snapshot(
    roots: CapabilityInventoryRoots<'_>,
) -> anyhow::Result<CapabilityInventorySnapshot> {
    Ok(CapabilityInventorySnapshot {
        installed_extensions: list_installed_extension_capabilities_from_dir(roots.extensions_dir)?,
        armory_profiles: list_armory_profiles_from_root(roots.armory_root)?,
        agent_bundles: list_agent_bundle_summaries_from_dir(roots.catalog_dir)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_roots_build_empty_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let snapshot = build_capability_inventory_snapshot(CapabilityInventoryRoots {
            extensions_dir: &temp.path().join("extensions"),
            armory_root: &temp.path().join("armory"),
            catalog_dir: &temp.path().join("catalog"),
        })
        .unwrap();

        assert!(snapshot.installed_extensions.is_empty());
        assert!(snapshot.armory_profiles.is_empty());
        assert!(snapshot.agent_bundles.is_empty());
    }
}
