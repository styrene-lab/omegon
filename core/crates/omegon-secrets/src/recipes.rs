//! Recipe storage — persisted instructions for resolving secrets.
//!
//! Recipes are stored in `~/.omegon/secrets.json` as a simple name→recipe map.
//! Recipe values are resolution instructions (e.g., "env:API_KEY", "keychain:myapp"),
//! never the actual secret values.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A recipe describes how to resolve a secret (not the secret itself).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Recipe {
    /// String-based recipe (env:VAR, keyring:name, cmd:command, file:/path)
    String(String),
    /// Vault KV v2 recipe with path validation
    Vault { path: String },
}

impl Recipe {
    /// Get the recipe as a string for backward compatibility with string-based recipes
    pub fn as_string(&self) -> String {
        match self {
            Recipe::String(s) => s.clone(),
            Recipe::Vault { path } => format!("vault:{}", path),
        }
    }

    /// Check if this recipe is a vault recipe
    pub fn is_vault(&self) -> bool {
        matches!(self, Recipe::Vault { .. })
    }

    /// Create a vault recipe from a path
    pub fn vault(path: String) -> Self {
        Recipe::Vault { path }
    }

    /// Create a string recipe
    pub fn string(recipe: String) -> Self {
        Recipe::String(recipe)
    }
}

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
        self.mutate_locked(move |recipes| {
            recipes.insert(name, recipe);
        })
    }

    /// Set a vault recipe for a secret.
    pub fn set_vault(&mut self, name: String, path: String) -> anyhow::Result<()> {
        self.set(name, Recipe::vault(path))
    }

    /// Set a string recipe for a secret.
    pub fn set_string(&mut self, name: String, recipe: String) -> anyhow::Result<()> {
        self.set(name, Recipe::string(recipe))
    }

    /// Remove a recipe.
    pub fn remove(&mut self, name: &str) -> anyhow::Result<bool> {
        let name = name.to_string();
        let mut existed = false;
        self.mutate_locked(|recipes| {
            existed = recipes.remove(&name).is_some();
        })?;
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

    fn mutate_locked(
        &mut self,
        mutation: impl FnOnce(&mut HashMap<String, Recipe>),
    ) -> anyhow::Result<()> {
        if self.path.as_os_str().is_empty() {
            mutation(&mut self.recipes);
            return Ok(());
        }
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let lock_path = self.path.with_extension("json.lock");
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)?;
        lock_exclusive(&lock)?;

        // Reload while holding the inter-process lock. Each mutation is applied
        // to current disk state rather than this process's potentially stale
        // startup snapshot, preventing one Omegon process from erasing another's
        // newly-created recipe.
        let mut merged = load_recipe_map(&self.path)?;
        mutation(&mut merged);
        save_recipe_map(&self.path, &merged)?;
        self.recipes = merged;
        unlock(&lock)?;
        Ok(())
    }
}

fn load_recipe_map(path: &Path) -> anyhow::Result<HashMap<String, Recipe>> {
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let content = std::fs::read_to_string(path)?;
    let file: RecipeFile = serde_json::from_str(&content)?;
    Ok(file.recipes)
}

fn save_recipe_map(path: &Path, recipes: &HashMap<String, Recipe>) -> anyhow::Result<()> {
    let file = RecipeFile {
        recipes: recipes.clone(),
    };
    let json = serde_json::to_string_pretty(&file)?;
    let tmp_path = path.with_extension(format!("json.tmp.{}", std::process::id()));
    std::fs::write(&tmp_path, &json)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

#[cfg(unix)]
fn lock_exclusive(file: &std::fs::File) -> anyhow::Result<()> {
    use std::os::fd::AsRawFd;
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().into())
    }
}

#[cfg(unix)]
fn unlock(file: &std::fs::File) -> anyhow::Result<()> {
    use std::os::fd::AsRawFd;
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().into())
    }
}

#[cfg(not(unix))]
fn lock_exclusive(_file: &std::fs::File) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(not(unix))]
fn unlock(_file: &std::fs::File) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_instances_merge_mutations_without_lost_updates() {
        let dir = tempfile::tempdir().unwrap();
        let mut first = RecipeStore::load(dir.path()).unwrap();
        let mut stale = RecipeStore::load(dir.path()).unwrap();

        first
            .set_string("MOONSHOT_API_KEY".into(), "keyring:MOONSHOT_API_KEY".into())
            .unwrap();
        stale
            .set_string("OTHER_API_KEY".into(), "env:OTHER_API_KEY".into())
            .unwrap();

        let merged = RecipeStore::load(dir.path()).unwrap();
        assert!(merged.get("MOONSHOT_API_KEY").is_some());
        assert!(merged.get("OTHER_API_KEY").is_some());
    }

    #[test]
    fn round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = RecipeStore::load(dir.path()).unwrap();
        assert!(store.is_empty());

        store
            .set("MY_KEY".into(), Recipe::string("env:MY_KEY".into()))
            .unwrap();
        store
            .set(
                "KEYCHAIN_KEY".into(),
                Recipe::string("keychain:myapp".into()),
            )
            .unwrap();
        assert_eq!(store.len(), 2);

        // Reload from disk
        let store2 = RecipeStore::load(dir.path()).unwrap();
        assert_eq!(store2.get("MY_KEY").unwrap().as_string(), "env:MY_KEY");
        assert_eq!(
            store2.get("KEYCHAIN_KEY").unwrap().as_string(),
            "keychain:myapp"
        );
    }

    #[test]
    fn remove_recipe() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = RecipeStore::load(dir.path()).unwrap();
        store
            .set("X".into(), Recipe::string("env:X".into()))
            .unwrap();
        assert!(store.remove("X").unwrap());
        assert!(!store.remove("X").unwrap()); // already gone
        assert!(store.is_empty());
    }

    #[test]
    fn vault_recipe() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = RecipeStore::load(dir.path()).unwrap();

        // Test vault recipe
        store
            .set_vault(
                "VAULT_KEY".into(),
                "secret/data/omegon/api#anthropic".into(),
            )
            .unwrap();
        let recipe = store.get("VAULT_KEY").unwrap();
        assert!(recipe.is_vault());
        assert_eq!(recipe.as_string(), "vault:secret/data/omegon/api#anthropic");

        // Test roundtrip with mixed recipe types
        store
            .set_string("ENV_KEY".into(), "env:MY_VAR".into())
            .unwrap();

        let store2 = RecipeStore::load(dir.path()).unwrap();
        let vault_recipe = store2.get("VAULT_KEY").unwrap();
        let env_recipe = store2.get("ENV_KEY").unwrap();

        assert!(vault_recipe.is_vault());
        assert!(!env_recipe.is_vault());
        assert_eq!(
            vault_recipe.as_string(),
            "vault:secret/data/omegon/api#anthropic"
        );
        assert_eq!(env_recipe.as_string(), "env:MY_VAR");
    }
}
