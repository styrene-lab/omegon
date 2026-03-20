//! Decay math — compute confidence for facts based on time and reinforcement.
//!
//! Direct port of extensions/project-memory/core.ts::computeConfidence.
//! Must produce identical results for the same float inputs.

use crate::types::DecayProfileName;

/// Maximum effective half-life regardless of reinforcement count.
/// Prevents immortal facts — even highly reinforced facts decay eventually.
const MAX_HALF_LIFE_DAYS: f64 = 90.0;

/// Decay profile parameters.
///
/// Note: `base_rate` and `minimum_confidence` are carried for TS compatibility
/// but are NOT used by `compute_confidence`. The TS implementation has the same
/// dead fields. Only `half_life_days` and `reinforcement_factor` affect computation.
#[derive(Debug, Clone, Copy)]
pub struct DecayProfile {
    /// Base decay rate — UNUSED by compute_confidence. Carried for TS compat.
    pub base_rate: f64,
    /// Multiplier per reinforcement for extending half-life.
    pub reinforcement_factor: f64,
    /// Confidence floor — UNUSED by compute_confidence. Carried for TS compat.
    pub minimum_confidence: f64,
    /// Base half-life in days before reinforcement scaling.
    pub half_life_days: f64,
}

/// Project-level decay. Base half-life 14d; each reinforcement extends by 1.8×.
pub const STANDARD: DecayProfile = DecayProfile {
    base_rate: 0.049_504_950_495, // ≈ ln(2)/14
    reinforcement_factor: 1.8,
    minimum_confidence: 0.1,
    half_life_days: 14.0,
};

/// Global-level decay. Shorter base (30d); cross-project reinforcement dramatically extends.
pub const GLOBAL: DecayProfile = DecayProfile {
    base_rate: 0.023_104_906_018, // ln(2)/30
    reinforcement_factor: 2.5,
    minimum_confidence: 0.1,
    half_life_days: 30.0,
};

/// Recent Work decay — ephemeral session breadcrumbs.
/// halfLifeDays=2: written Monday, gone by Wednesday at ~50%.
/// reinforcementFactor=1.0: reinforcement does NOT extend half-life.
pub const RECENT_WORK: DecayProfile = DecayProfile {
    base_rate: 0.346_573_590_279, // ln(2)/2
    reinforcement_factor: 1.0,
    minimum_confidence: 0.01,
    half_life_days: 2.0,
};

/// Resolve a stored profile name to its parameters.
pub fn resolve_profile(name: &DecayProfileName) -> DecayProfile {
    match name {
        DecayProfileName::Standard => STANDARD,
        DecayProfileName::Global => GLOBAL,
        DecayProfileName::RecentWork => RECENT_WORK,
    }
}

/// Compute current confidence for a fact.
///
/// ```text
/// halfLife = profile.half_life_days × (profile.reinforcement_factor ^ (reinforcement_count - 1))
/// halfLife = clamp(halfLife, 0, MAX_HALF_LIFE_DAYS)
/// confidence = e^(−ln(2) × days_since_reinforced / halfLife)
/// ```
///
/// Matches `extensions/project-memory/core.ts::computeConfidence` exactly.
pub fn compute_confidence(
    days_since_reinforced: f64,
    reinforcement_count: u32,
    profile: &DecayProfile,
) -> f64 {
    let raw_half_life = profile.half_life_days
        * profile
            .reinforcement_factor
            .powi(reinforcement_count as i32 - 1);
    let half_life = raw_half_life.min(MAX_HALF_LIFE_DAYS);
    let confidence = (-std::f64::consts::LN_2 * days_since_reinforced / half_life).exp();
    confidence.max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_fact_has_full_confidence() {
        let c = compute_confidence(0.0, 1, &STANDARD);
        assert!((c - 1.0).abs() < 1e-10);
    }

    #[test]
    fn confidence_at_half_life() {
        // At exactly 14 days with 1 reinforcement, confidence ≈ 0.5
        let c = compute_confidence(14.0, 1, &STANDARD);
        assert!((c - 0.5).abs() < 0.01, "expected ~0.5, got {c}");
    }

    #[test]
    fn reinforcement_extends_half_life() {
        // With 3 reinforcements: halfLife = 14 * 1.8^2 = 45.36
        // At 14 days: confidence should be much higher than 0.5
        let c = compute_confidence(14.0, 3, &STANDARD);
        assert!(c > 0.7, "expected >0.7 with 3 reinforcements, got {c}");
    }

    #[test]
    fn max_half_life_caps_immortality() {
        // 20 reinforcements: raw halfLife = 14 * 1.8^19 = absurdly large
        // Should be capped at 90 days
        let c_20 = compute_confidence(90.0, 20, &STANDARD);
        let _c_5 = compute_confidence(90.0, 5, &STANDARD);
        // At 90 days with 90-day half-life (capped): e^(-ln2) ≈ 0.5
        assert!((c_20 - 0.5).abs() < 0.01, "c_20={c_20}");
    }

    #[test]
    fn recent_work_decays_fast() {
        // halfLifeDays=2, reinforcementFactor=1.0
        let c = compute_confidence(2.0, 1, &RECENT_WORK);
        assert!((c - 0.5).abs() < 0.01, "expected ~0.5 at 2 days, got {c}");

        let c4 = compute_confidence(4.0, 1, &RECENT_WORK);
        assert!(c4 < 0.3, "expected <0.3 at 4 days, got {c4}");
    }

    #[test]
    fn recent_work_reinforcement_does_not_extend() {
        // reinforcementFactor=1.0 means reinforcement doesn't change half-life
        let c1 = compute_confidence(2.0, 1, &RECENT_WORK);
        let c5 = compute_confidence(2.0, 5, &RECENT_WORK);
        assert!((c1 - c5).abs() < 1e-10, "reinforcement shouldn't matter: {c1} vs {c5}");
    }

    #[test]
    fn global_profile_is_slower() {
        // Global halfLife=30 vs standard=14
        let standard = compute_confidence(14.0, 1, &STANDARD);
        let global = compute_confidence(14.0, 1, &GLOBAL);
        assert!(global > standard, "global should decay slower: {global} vs {standard}");
    }

    #[test]
    fn resolve_profile_exhaustive() {
        let s = resolve_profile(&DecayProfileName::Standard);
        assert_eq!(s.half_life_days, 14.0);
        let g = resolve_profile(&DecayProfileName::Global);
        assert_eq!(g.half_life_days, 30.0);
        let r = resolve_profile(&DecayProfileName::RecentWork);
        assert_eq!(r.half_life_days, 2.0);
    }
}
