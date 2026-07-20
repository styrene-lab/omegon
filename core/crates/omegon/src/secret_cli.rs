//! Operator-facing secret management CLI.
//!
//! ## Set a secret (stored in keyring)
//!
//! ```sh
//! omegon secret set GITHUB_TOKEN ghp_abc123
//! echo "ghp_abc123" | omegon secret set GITHUB_TOKEN --stdin
//! omegon secret set VOX_DISCORD_BOT_TOKEN --recipe "env:DISCORD_TOKEN"
//! omegon secret set VAULT_TOKEN --recipe "keyring:VAULT_ROOT_TOKEN"
//! ```
//!
//! ## List configured secrets
//!
//! ```sh
//! omegon secret list
//! ```
//!
//! ## Delete a secret
//!
//! ```sh
//! omegon secret delete GITHUB_TOKEN
//! ```

/// Set a secret — either a raw value (stored in keyring) or a recipe.
///
/// When `from_stdin` is true, reads the value from stdin (one line, trimmed).
/// This avoids exposing the secret in shell history or `ps` output.
pub fn set(
    name: &str,
    value: Option<&str>,
    recipe: Option<&str>,
    from_stdin: bool,
) -> anyhow::Result<()> {
    let secrets = create_manager()?;

    if from_stdin {
        if recipe.is_some() {
            anyhow::bail!("--stdin and --recipe are mutually exclusive");
        }
        if value.is_some() {
            anyhow::bail!("--stdin and a positional value are mutually exclusive");
        }
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        let val = line.trim_end_matches('\n').trim_end_matches('\r');
        if val.is_empty() {
            anyhow::bail!("no value read from stdin");
        }
        secrets.set_keyring_secret(name, val)?;
        println!("Stored '{name}' in keyring.");
        return Ok(());
    }

    match (value, recipe) {
        (Some(_), Some(_)) => {
            anyhow::bail!("provide either a value or --recipe, not both");
        }
        (None, None) => {
            anyhow::bail!(
                "provide a secret value, --recipe, or --stdin\n\
                 Hint: use --stdin to avoid exposing the value in shell history"
            );
        }
        (Some(val), None) => {
            secrets.set_keyring_secret(name, val)?;
            println!("Stored '{name}' in Omegon's encrypted store.");
        }
        (None, Some(recipe)) => {
            secrets.set_recipe(name, recipe)?;
            println!("Stored recipe for '{name}': {recipe}");
        }
    }

    Ok(())
}

/// List all configured secret names and their recipes (values are never shown).
pub fn list() -> anyhow::Result<()> {
    let secrets = create_manager()?;
    let entries = secrets.list_recipes();

    if entries.is_empty() {
        println!("No secrets configured.");
        println!("  Set one with: omegon secret set <NAME> <VALUE>");
        return Ok(());
    }

    println!("{:<30} RECIPE", "NAME");
    println!("{}", "─".repeat(60));
    for (name, recipe) in &entries {
        println!("{:<30} {recipe}", name);
    }

    Ok(())
}

/// Migrate legacy per-secret keyring values into the single encrypted store.
pub fn migrate() -> anyhow::Result<()> {
    let secrets = create_manager()?;
    let migrated = secrets.migrate_legacy_keyring_secrets()?;
    println!("Migrated {migrated} legacy secret(s) into Omegon's encrypted store.");
    Ok(())
}

/// Delete a secret recipe and its associated managed value.
pub fn delete(name: &str) -> anyhow::Result<()> {
    let secrets = create_manager()?;
    secrets.delete_recipe(name)?;
    println!("Deleted secret '{name}'.");
    Ok(())
}

fn create_manager() -> anyhow::Result<omegon_secrets::SecretsManager> {
    let dir = crate::paths::omegon_home()?;
    std::fs::create_dir_all(&dir)?;
    omegon_secrets::SecretsManager::new(&dir)
}
