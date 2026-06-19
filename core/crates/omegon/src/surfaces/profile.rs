use crate::settings::{ContextClass, Profile, ProfileSource, Settings, ThinkingLevel};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileDriftProjection {
    pub profile_label: String,
    pub source: ProfileSource,
    pub dirty: bool,
    pub changed_count: usize,
    pub rows: Vec<ProfileDriftRow>,
    pub actions: Vec<ProfileDriftAction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileDriftRow {
    pub key: &'static str,
    pub label: &'static str,
    pub profile_value: String,
    pub runtime_value: String,
    pub persistence: PersistenceSemantics,
    pub severity: DriftSeverity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistenceSemantics {
    SavedDefault,
    LiveOnly,
}

impl PersistenceSemantics {
    pub fn label(self) -> &'static str {
        match self {
            Self::SavedDefault => "saved default",
            Self::LiveOnly => "live only",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriftSeverity {
    Info,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileDriftAction {
    View,
    Save,
    Apply,
}

impl ProfileDriftProjection {
    pub fn from_profile_and_settings(
        profile: &Profile,
        source: ProfileSource,
        settings: &Settings,
    ) -> Self {
        let mut rows = Vec::new();

        if let Some(profile_thinking) = profile
            .thinking_level
            .as_deref()
            .and_then(ThinkingLevel::parse)
            && profile_thinking != settings.thinking
        {
            rows.push(ProfileDriftRow {
                key: "thinking",
                label: "Thinking",
                profile_value: profile_thinking.as_str().to_string(),
                runtime_value: settings.thinking.as_str().to_string(),
                persistence: PersistenceSemantics::LiveOnly,
                severity: DriftSeverity::Info,
            });
        }

        if let Some(profile_class) = profile
            .requested_context_class
            .as_deref()
            .and_then(ContextClass::parse)
            && settings.requested_context_class != Some(profile_class)
        {
            rows.push(ProfileDriftRow {
                key: "requestedContextClass",
                label: "Context class",
                profile_value: profile_class.short().to_lowercase(),
                runtime_value: settings
                    .requested_context_class
                    .map(|class| class.short().to_lowercase())
                    .unwrap_or_else(|| "track model".to_string()),
                persistence: PersistenceSemantics::LiveOnly,
                severity: DriftSeverity::Info,
            });
        }

        let dirty = !rows.is_empty();
        let actions = if dirty {
            vec![
                ProfileDriftAction::View,
                ProfileDriftAction::Save,
                ProfileDriftAction::Apply,
            ]
        } else {
            vec![ProfileDriftAction::View]
        };
        let changed_count = rows.len();

        Self {
            profile_label: source.label().to_string(),
            source,
            dirty,
            changed_count,
            rows,
            actions,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> ProfileSource {
        ProfileSource::BuiltInDefault
    }

    #[test]
    fn clean_profile_runtime_pair_has_no_drift() {
        let profile = Profile {
            thinking_level: Some("high".into()),
            requested_context_class: Some("massive".into()),
            ..Profile::default()
        };
        let mut settings = Settings {
            thinking: ThinkingLevel::High,
            ..Default::default()
        };
        settings.set_requested_context_class(ContextClass::Massive);

        let projection =
            ProfileDriftProjection::from_profile_and_settings(&profile, source(), &settings);

        assert!(!projection.dirty);
        assert_eq!(projection.changed_count, 0);
        assert!(projection.rows.is_empty());
        assert_eq!(projection.actions, vec![ProfileDriftAction::View]);
    }

    #[test]
    fn thinking_drift_yields_stable_row() {
        let profile = Profile {
            thinking_level: Some("medium".into()),
            ..Profile::default()
        };
        let settings = Settings {
            thinking: ThinkingLevel::High,
            ..Default::default()
        };

        let projection =
            ProfileDriftProjection::from_profile_and_settings(&profile, source(), &settings);

        assert!(projection.dirty);
        assert_eq!(projection.changed_count, 1);
        assert_eq!(projection.rows[0].key, "thinking");
        assert_eq!(projection.rows[0].profile_value, "medium");
        assert_eq!(projection.rows[0].runtime_value, "high");
        assert_eq!(
            projection.rows[0].persistence,
            PersistenceSemantics::LiveOnly
        );
    }

    #[test]
    fn requested_context_class_drift_yields_stable_row() {
        let profile = Profile {
            requested_context_class: Some("extended".into()),
            ..Profile::default()
        };
        let mut settings = Settings::default();
        settings.set_requested_context_class(ContextClass::Massive);

        let projection =
            ProfileDriftProjection::from_profile_and_settings(&profile, source(), &settings);

        assert!(projection.dirty);
        assert_eq!(projection.changed_count, 1);
        assert_eq!(projection.rows[0].key, "requestedContextClass");
        assert_eq!(projection.rows[0].label, "Context class");
        assert_eq!(projection.rows[0].profile_value, "extended");
        assert_eq!(projection.rows[0].runtime_value, "massive");
    }

    #[test]
    fn multiple_drift_rows_keep_stable_order() {
        let profile = Profile {
            thinking_level: Some("medium".into()),
            requested_context_class: Some("extended".into()),
            ..Profile::default()
        };
        let mut settings = Settings {
            thinking: ThinkingLevel::High,
            ..Default::default()
        };
        settings.set_requested_context_class(ContextClass::Massive);

        let projection =
            ProfileDriftProjection::from_profile_and_settings(&profile, source(), &settings);

        assert_eq!(projection.changed_count, 2);
        assert_eq!(projection.rows[0].key, "thinking");
        assert_eq!(projection.rows[1].key, "requestedContextClass");
        assert_eq!(
            projection.actions,
            vec![
                ProfileDriftAction::View,
                ProfileDriftAction::Save,
                ProfileDriftAction::Apply,
            ]
        );
    }
}
