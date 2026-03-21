//! Output redaction — scrub secret values from tool results.
//!
//! Uses Aho-Corasick for single-pass multi-pattern replacement.
//! Longer secrets are prioritized to avoid partial-match issues.

use aho_corasick::AhoCorasick;
use secrecy::{ExposeSecret, SecretString};
use std::collections::HashMap;

/// Minimum secret length for redaction (avoids false positives on short values).
const MIN_REDACT_LEN: usize = 8;

/// Compiled redactor — build once from the redaction set, reuse per-turn.
pub struct Redactor {
    automaton: Option<AhoCorasick>,
    replacements: Vec<String>,
}

impl Redactor {
    /// Build a redactor from the current redaction set.
    /// Returns None if no secrets are long enough to redact.
    pub fn build(secrets: &HashMap<String, SecretString>) -> Self {
        let mut patterns: Vec<(String, String)> = secrets
            .iter()
            .filter(|(_, v)| v.expose_secret().len() >= MIN_REDACT_LEN)
            .map(|(name, val)| (val.expose_secret().to_string(), format!("[REDACTED:{name}]")))
            .collect();

        // Sort by pattern length descending — longest match wins
        patterns.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

        if patterns.is_empty() {
            return Self {
                automaton: None,
                replacements: Vec::new(),
            };
        }

        let (pats, repls): (Vec<_>, Vec<_>) = patterns.into_iter().unzip();

        let automaton = AhoCorasick::builder()
            .match_kind(aho_corasick::MatchKind::LeftmostFirst)
            .build(&pats)
            .ok();

        Self {
            automaton,
            replacements: repls,
        }
    }

    /// Redact all known secret values from a string in a single pass.
    pub fn redact(&self, input: &str) -> String {
        match &self.automaton {
            Some(ac) => ac.replace_all(input, &self.replacements),
            None => input.to_string(),
        }
    }

    /// Redact secrets from content blocks (text blocks only).
    pub fn redact_content_blocks(&self, content: &mut Vec<omegon_traits::ContentBlock>) {
        if self.automaton.is_none() {
            return;
        }
        for block in content.iter_mut() {
            if let omegon_traits::ContentBlock::Text { text } = block {
                *text = self.redact(text);
            }
        }
    }
}

/// Build+redact in one call. Used internally and in tests.
#[cfg(test)]
pub(crate) fn redact_string(input: &str, secrets: &HashMap<String, SecretString>) -> String {
    let redactor = Redactor::build(secrets);
    redactor.redact(input)
}

/// Redact content blocks. Used internally and in tests.
#[cfg(test)]
pub(crate) fn redact_content_blocks(
    content: &mut Vec<omegon_traits::ContentBlock>,
    secrets: &HashMap<String, SecretString>,
) {
    let redactor = Redactor::build(secrets);
    redactor.redact_content_blocks(content);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secret(s: &str) -> SecretString {
        SecretString::from(s.to_string())
    }

    #[test]
    fn redact_single_secret() {
        let mut secrets = HashMap::new();
        secrets.insert("API_KEY".into(), secret("sk-ant-api03-very-secret-key"));
        let input = "Authorization: Bearer sk-ant-api03-very-secret-key";
        let result = redact_string(input, &secrets);
        assert_eq!(result, "Authorization: Bearer [REDACTED:API_KEY]");
    }

    #[test]
    fn redact_multiple_secrets() {
        let mut secrets = HashMap::new();
        secrets.insert("TOKEN_A".into(), secret("aaaa-bbbb-cccc-dddd"));
        secrets.insert("TOKEN_B".into(), secret("xxxx-yyyy-zzzz-1234"));
        let input = "a=aaaa-bbbb-cccc-dddd b=xxxx-yyyy-zzzz-1234";
        let result = redact_string(input, &secrets);
        assert_eq!(result, "a=[REDACTED:TOKEN_A] b=[REDACTED:TOKEN_B]");
    }

    #[test]
    fn skip_short_values() {
        let mut secrets = HashMap::new();
        secrets.insert("SHORT".into(), secret("abc")); // < 8 chars
        let input = "the abc value should not be redacted";
        let result = redact_string(input, &secrets);
        assert_eq!(result, input); // unchanged
    }

    #[test]
    fn empty_input_returns_empty() {
        let secrets = HashMap::new();
        assert_eq!(redact_string("", &secrets), "");
    }

    #[test]
    fn no_secrets_returns_input() {
        let secrets = HashMap::new();
        let input = "nothing to redact";
        assert_eq!(redact_string(input, &secrets), input);
    }

    #[test]
    fn longest_match_first() {
        let mut secrets = HashMap::new();
        secrets.insert("FULL".into(), secret("sk-ant-api03-full-key-here"));
        secrets.insert("PREFIX".into(), secret("sk-ant-api03"));
        let input = "key is sk-ant-api03-full-key-here done";
        let result = redact_string(input, &secrets);
        assert_eq!(result, "key is [REDACTED:FULL] done");
    }

    #[test]
    fn redactor_reuse() {
        let mut secrets = HashMap::new();
        secrets.insert("KEY".into(), secret("super-secret-value-here"));
        let redactor = Redactor::build(&secrets);
        assert_eq!(
            redactor.redact("got super-secret-value-here"),
            "got [REDACTED:KEY]"
        );
        assert_eq!(
            redactor.redact("another super-secret-value-here mention"),
            "another [REDACTED:KEY] mention"
        );
    }
}
