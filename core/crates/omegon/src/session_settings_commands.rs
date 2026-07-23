//! Canonical session-setting mutations shared by ACP selectors and command surfaces.

use std::path::Path;

use crate::settings::{
    ActiveProfileSelection, ContextClass, PosturePreset, Profile, ProfileRegistry, SharedSettings,
    ThinkingLevel,
};

fn with_settings<T>(
    shared: &SharedSettings,
    mutate: impl FnOnce(&mut crate::settings::Settings) -> Result<T, String>,
) -> Result<T, String> {
    let mut settings = shared
        .lock()
        .map_err(|_| "settings lock poisoned".to_string())?;
    mutate(&mut settings)
}

fn persist_runtime_profile(cwd: &Path, settings: &crate::settings::Settings) -> Result<(), String> {
    let mut profile = Profile::load(cwd);
    profile.capture_from(settings);
    profile
        .save(cwd)
        .map_err(|error| format!("could not persist runtime profile: {error}"))
}

pub(crate) fn set_model(shared: &SharedSettings, cwd: &Path, value: &str) -> Result<(), String> {
    with_settings(shared, |settings| {
        settings.set_model(value);
        persist_runtime_profile(cwd, settings)
    })
}

pub(crate) fn set_thinking(shared: &SharedSettings, cwd: &Path, value: &str) -> Result<(), String> {
    let level =
        ThinkingLevel::parse(value).ok_or_else(|| format!("unknown thinking level `{value}`"))?;
    with_settings(shared, |settings| {
        settings.thinking = level;
        persist_runtime_profile(cwd, settings)
    })
}

pub(crate) fn set_posture(shared: &SharedSettings, cwd: &Path, value: &str) -> Result<(), String> {
    let posture = match value {
        "fabricator" => PosturePreset::Fabricator,
        "architect" => PosturePreset::Architect,
        "explorator" => PosturePreset::Explorator,
        "devastator" => PosturePreset::Devastator,
        _ => return Err(format!("unknown posture `{value}`")),
    };
    with_settings(shared, |settings| {
        settings.set_posture(posture);
        persist_runtime_profile(cwd, settings)
    })
}

pub(crate) fn set_context_class(
    shared: &SharedSettings,
    cwd: &Path,
    value: &str,
) -> Result<(), String> {
    let context_class =
        ContextClass::parse(value).ok_or_else(|| format!("unknown context class `{value}`"))?;
    with_settings(shared, |settings| {
        settings.set_requested_context_class(context_class);
        persist_runtime_profile(cwd, settings)
    })
}

pub(crate) fn apply_profile(
    shared: &SharedSettings,
    cwd: &Path,
    selection: ActiveProfileSelection,
) -> Result<(), String> {
    let registry = ProfileRegistry::discover(cwd);
    let loaded = registry
        .resolve_explicit(&selection)
        .ok_or_else(|| format!("profile `{}` was not found", selection.id))?;
    crate::settings::save_project_active_profile_selection(cwd, &selection)
        .map_err(|error| format!("could not select profile: {error}"))?;
    with_settings(shared, |settings| {
        loaded.profile.apply_to_with_posture(settings, cwd);
        settings.provider_connected = crate::auth::provider_connected_for_model(&settings.model);
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_mutations_do_not_change_settings() {
        let cwd = tempfile::tempdir().unwrap();
        let shared = crate::settings::shared("test-model");
        let before = shared.lock().unwrap().clone();

        assert!(set_thinking(&shared, cwd.path(), "impossible").is_err());
        assert!(set_posture(&shared, cwd.path(), "impossible").is_err());
        assert!(set_context_class(&shared, cwd.path(), "impossible").is_err());

        let after = shared.lock().unwrap().clone();
        assert_eq!(after.thinking, before.thinking);
        assert_eq!(after.posture, before.posture);
        assert_eq!(
            after.requested_context_class,
            before.requested_context_class
        );
    }
}
