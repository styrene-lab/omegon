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
    let value = value.trim();
    if value.is_empty() {
        return Err("model must not be empty".into());
    }
    with_settings(shared, |settings| {
        settings.set_model(value);
        persist_runtime_profile(cwd, settings)
    })
}

pub(crate) fn set_thinking(shared: &SharedSettings, cwd: &Path, value: &str) -> Result<(), String> {
    let level = ThinkingLevel::parse(value);
    let structured = matches!(value, "provider-default" | "adaptive" | "enabled")
        || value
            .strip_prefix("budget:")
            .and_then(|value| value.parse::<usize>().ok())
            .is_some_and(|tokens| tokens > 0);
    if level.is_none() && !structured {
        return Err(format!("unknown thinking level `{value}`"));
    }
    with_settings(shared, |settings| {
        if let Some(level) = level {
            settings.set_thinking(level);
        } else {
            settings.reasoning_intent = (value != "provider-default").then(|| value.to_string());
        }
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

pub(crate) fn set_context_capacity(shared: &SharedSettings, value: &str) -> Result<String, String> {
    use crate::inference_policy::{NumericTarget, TargetSource};
    with_settings(shared, |settings| {
        let capture = settings.selected_policy_capture.clone().ok_or_else(||
            "Serving route is not captured yet; connect/select the route before setting capacity".to_string())?;
        if value == "reset" {
            settings.route_targets.retain(|(source, target)| {
                *source != TargetSource::Session || target.route != capture.route
            });
            return Ok(
                "Session route target cleared; saved profile targets remain available.".into(),
            );
        }
        let target = if value == "maximum" {
            capture.maximum_target().map_err(|e| e.to_string())?
        } else {
            NumericTarget {
                route: capture.route.clone(),
                tokens: value
                    .parse::<usize>()
                    .ok()
                    .filter(|n| *n > 0)
                    .ok_or_else(|| {
                        "Use /context capacity <positive-token-count|maximum|reset>".to_string()
                    })?,
                maximum_revision: None,
                invalid_tokens: None,
            }
        };
        let mut validation = capture.clone();
        validation.intent = settings.inference_intent();
        validation
            .intent
            .targets
            .retain(|(source, t)| *source != TargetSource::Session || t.route != capture.route);
        validation
            .intent
            .targets
            .push((TargetSource::Session, target.clone()));
        validation
            .resolve(None, Default::default())
            .map_err(|e| e.to_string())?;
        settings.route_targets = validation.intent.targets;
        Ok(format!(
            "Session route target: {} tokens ({}) before narrower host caps and reserves. Inspect /context status.",
            target.tokens,
            target.route.storage_key()
        ))
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
        loaded.apply_to_runtime(settings, cwd, true);
        settings.provider_connected = crate::auth::provider_connected_for_model(&settings.model);
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_policy_context_control_and_saved_off_preserve_profile_intent() {
        let shared = crate::settings::shared("openai-codex:gpt-6-astra");
        let capture = crate::inference_policy::capture_route(
            "openai-codex:gpt-6-astra",
            None,
            None,
            None,
            None,
        );
        shared.lock().unwrap().selected_policy_capture = Some(capture.clone());
        assert!(set_context_capacity(&shared, "600000").is_ok());
        assert!(set_context_capacity(&shared, "900000").is_err());
        assert_eq!(shared.lock().unwrap().route_targets[0].1.tokens, 600_000);
        assert!(set_context_capacity(&shared, "maximum").is_ok());
        assert_eq!(shared.lock().unwrap().route_targets[0].1.tokens, 872_000);
        assert!(set_context_capacity(&shared, "reset").is_ok());
        assert!(shared.lock().unwrap().route_targets.is_empty());
        let profile: Profile = serde_json::from_value(
            serde_json::json!({"thinkingLevel":"off", "contextWindow":900000}),
        )
        .unwrap();
        let before = serde_json::to_value(&profile).unwrap();
        profile.apply_to(&mut shared.lock().unwrap());
        assert!(
            shared
                .lock()
                .unwrap()
                .inference_projection()
                .needs_resolution
                .unwrap()
                .contains("off")
        );
        assert_eq!(serde_json::to_value(&profile).unwrap(), before);
        assert_eq!(profile.context_window, Some(serde_json::json!(900000)));
        assert!(set_context_capacity(&shared, "0").is_err());
        assert!(set_context_capacity(&shared, "garbage").is_err());
        let malformed = serde_json::json!({"routeContextTargets": [{"route":capture.route,"tokens":"not-a-number"}]});
        let profile: Profile = serde_json::from_value(malformed.clone()).unwrap();
        let mut settings = shared.lock().unwrap();
        settings.profile_source =
            crate::settings::ProfileSource::Project("fixture-profile.json".into());
        profile.apply_to(&mut settings);
        let mut lower = capture.maximum_target().unwrap();
        lower.tokens = 500_000;
        settings
            .route_targets
            .push((crate::inference_policy::TargetSource::User, lower));
        let mut resolution = capture.clone();
        resolution.intent = settings.inference_intent();
        assert!(
            resolution
                .resolve(None, Default::default())
                .unwrap_err()
                .to_string()
                .contains("positive")
        );
        assert_eq!(
            serde_json::to_value(&profile).unwrap()["routeContextTargets"][0]["tokens"],
            "not-a-number"
        );
    }

    #[test]
    fn invalid_mutations_do_not_change_settings() {
        let cwd = tempfile::tempdir().unwrap();
        let shared = crate::settings::shared("test-model");
        let before = shared.lock().unwrap().clone();

        assert!(set_model(&shared, cwd.path(), " ").is_err());
        assert!(set_thinking(&shared, cwd.path(), "impossible").is_err());
        assert!(set_posture(&shared, cwd.path(), "impossible").is_err());
        assert!(set_context_class(&shared, cwd.path(), "impossible").is_err());

        let after = shared.lock().unwrap().clone();
        assert_eq!(after.model, before.model);
        assert_eq!(after.thinking, before.thinking);
        assert_eq!(after.posture, before.posture);
        assert_eq!(
            after.requested_context_class,
            before.requested_context_class
        );
    }
}
