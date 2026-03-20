//! Recipe storage — persisted instructions for resolving secrets.
//!
//! Recipes are stored in `~/.omegon/secrets.json` as a simple name→recipe map.
//! Recipe values are resolution instructions (e.g., "env:API_KEY", "keychain:myapp"),
//! never the actual secret values.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A recipe describes how to resolve a secret (not the secret itself).
pub type Recipe = String;

/// Persistent recipe store backed by a JSON file.
#[derive(Debug)]
pub struct RecipeStore {
    recipes: HashMap<String, Recipe>,
    path: PathBuf,
}

#[derive(Serialize, Deserialize, Default)]
struct RecipeFile {
    #[serde(flatten)]
    recipes: HashMap<String, Recipe>,
}

impl RecipeStore {
    /// Load recipes from the config directory.
    pub fn load(config_dir: &Path) -> anyhow::Result<Self> {
        let path = config_dir.join("secrets.json");
        let recipes = if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            let file: RecipeFile = serde_json::from_str(&content).unwrap_or_default();
            file.recipes
        } else {
            HashMap::new()
        };

        tracing::debug!(count = recipes.len(), path = %path.display(), "loaded secret recipes");

        Ok(Self { recipes, path })
    }

    /// Create an empty recipe store (for testing).
    pub fn empty() -> Self {
        Self {
            recipes: HashMap::new(),
            path: PathBuf::new(),
        }
    }

    /// Get a recipe by secret name.
    pub fn get(&self, name: &str) -> Option<&Recipe> {
        self.recipes.get(name)
    }

    /// Set a recipe for a secret.
    pub fn set(&mut self, name: String, recipe: Recipe) -> anyhow::Result<()> {
        self.recipes.insert(name, recipe);
        self.save()
    }

    /// Remove a recipe.
    pub fn remove(&mut self, name: &str) -> anyhow::Result<bool> {
        let existed = self.recipes.remove(name).is_some();
        if existed {
            self.save()?;
        }
        Ok(existed)
    }

    /// Iterate over all recipes.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Recipe)> {
        self.recipes.iter()
    }

    /// Number of stored recipes.
    pub fn len(&self) -> usize {
        self.recipes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.recipes.is_empty()
    }

    fn save(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = RecipeFile {
            recipes: self.recipes.clone(),
        };
        let json = serde_json::to_string_pretty(&file)?;
        std::fs::write(&self.path, json)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = RecipeStore::load(dir.path()).unwrap();
        assert!(store.is_empty());

        store.set("MY_KEY".into(), "env:MY_KEY".into()).unwrap();
        store
            .set("KEYCHAIN_KEY".into(), "keychain:myapp".into())
            .unwrap();
        assert_eq!(store.len(), 2);

        // Reload from disk
        let store2 = RecipeStore::load(dir.path()).unwrap();
        assert_eq!(store2.get("MY_KEY"), Some(&"env:MY_KEY".to_string()));
        assert_eq!(
            store2.get("KEYCHAIN_KEY"),
            Some(&"keychain:myapp".to_string())
        );
    }

    #[test]
    fn remove_recipe() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = RecipeStore::load(dir.path()).unwrap();
        store.set("X".into(), "env:X".into()).unwrap();
        assert!(store.remove("X").unwrap());
        assert!(!store.remove("X").unwrap()); // already gone
        assert!(store.is_empty());
    }
}
