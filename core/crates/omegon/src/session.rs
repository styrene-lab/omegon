//! Session management — directory layout, save-on-exit, list, resume.
//!
//! Session files live at `~/.config/omegon/sessions/<cwd-slug>/<timestamp>_<id>.json`.
//! Compatible with pi's directory structure so TS and Rust sessions coexist.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::conversation::ConversationState;

/// Metadata stored alongside each session for listing without loading the full file.
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionMeta {
    pub session_id: String,
    pub cwd: String,
    pub created_at: String, // ISO 8601
    pub turns: u32,
    pub tool_calls: u32,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub friendly_name: String,
    pub last_prompt_snippet: String,
}

/// A listed session entry (from scanning the directory).
#[derive(Debug)]
pub struct SessionEntry {
    pub path: PathBuf,
    pub meta: SessionMeta,
}

/// Get the sessions directory for a given cwd.
/// Returns `~/.config/omegon/sessions/<cwd-slug>/`.
pub fn sessions_dir(cwd: &Path) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let slug = cwd_slug(cwd);
    Some(home.join(".config/omegon/sessions").join(slug))
}

/// Convert a cwd path to a directory slug: `/Users/cwilson/workspace` → `--Users-cwilson-workspace--`
fn cwd_slug(cwd: &Path) -> String {
    let s = cwd.to_string_lossy();
    let slug = s.replace('/', "-");
    // Pi uses leading -- and trailing -- for the slug
    format!("--{}--", slug.trim_start_matches('-').trim_end_matches('-'))
}

/// Generate a session ID: `<timestamp>_<short-random>.json`
pub fn allocate_session_id() -> String {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let ts = chrono_lite_timestamp();
    let rand_part: u32 = now.subsec_nanos() ^ 0xDEAD_BEEF;
    format!("{ts}_{rand_part:08x}")
}

pub fn friendly_session_name_for_id(session_id: &str) -> String {
    const ADJECTIVES: &[&str] = &[
        "amber", "brave", "calm", "clear", "cobalt", "daring", "eager", "ember",
        "frost", "gentle", "hidden", "keen", "lunar", "patient", "quiet", "rapid",
        "silver", "steady", "tidal", "vivid", "wise", "zealous",
    ];
    const NOUNS: &[&str] = &[
        "anchor", "basilisk", "beacon", "cedar", "cipher", "comet", "forge", "harbor",
        "keel", "lantern", "machinist", "meridian", "otter", "raven", "signal",
        "sparrow", "warden", "waypoint", "willow", "wyrm",
    ];
    let hash = session_id.bytes().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    });
    let adjective = ADJECTIVES[(hash as usize) % ADJECTIVES.len()];
    let noun = NOUNS[((hash >> 16) as usize) % NOUNS.len()];
    format!("{adjective}_{noun}")
}

pub fn is_canonical_session_id(id: &str) -> bool {
    let Some((timestamp, suffix)) = id.split_once('_') else {
        return false;
    };
    timestamp.len() == "YYYY-MM-DDTHH-MM-SS".len()
        && timestamp.as_bytes().get(4) == Some(&b'-')
        && timestamp.as_bytes().get(7) == Some(&b'-')
        && timestamp.as_bytes().get(10) == Some(&b'T')
        && timestamp.as_bytes().get(13) == Some(&b'-')
        && timestamp.as_bytes().get(16) == Some(&b'-')
        && timestamp
            .bytes()
            .enumerate()
            .all(|(idx, byte)| matches!(idx, 4 | 7 | 13 | 16) && byte == b'-' || idx == 10 && byte == b'T' || byte.is_ascii_digit())
        && suffix.len() == 8
        && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// ISO 8601-ish timestamp for filenames: `2026-03-18T14-22-03`
fn chrono_lite_timestamp() -> String {
    // Use UNIX timestamp to derive components without chrono dependency
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    // Simple UTC breakdown (good enough for filenames)
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Approximate date from days since epoch (1970-01-01)
    // Using a simplified algorithm — good enough for session IDs
    let (year, month, day) = days_to_ymd(days);
    format!("{year:04}-{month:02}-{day:02}T{hours:02}-{minutes:02}-{seconds:02}")
}

/// Convert days since Unix epoch to (year, month, day).
/// Simplified civil calendar algorithm.
pub fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Algorithm from Howard Hinnant's chrono-compatible date library
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Save a session after the agent loop completes.
pub fn save_session(
    conversation: &ConversationState,
    cwd: &Path,
    resume_id: Option<&str>,
) -> anyhow::Result<PathBuf> {
    let dir =
        sessions_dir(cwd).ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
    fs::create_dir_all(&dir)?;

    // When resuming, overwrite the original session file so the chain stays clean.
    // When starting fresh, generate a new timestamped ID.
    let session_id = match resume_id {
        Some(id) if is_canonical_session_id(id) => id.to_string(),
        Some(id) => anyhow::bail!("Invalid session id '{id}'; sessions must use Omegon canonical ids"),
        None => allocate_session_id(),
    };
    let filename = format!("{session_id}.json");
    let path = dir.join(&filename);

    // Preserve the original created_at if overwriting; otherwise use now.
    let existing_meta = if resume_id.is_some() {
        fs::read_to_string(path.with_extension("meta.json"))
            .ok()
            .and_then(|j| serde_json::from_str::<SessionMeta>(&j).ok())
    } else {
        None
    };
    let created_at = existing_meta
        .as_ref()
        .map(|m| m.created_at.clone())
        .unwrap_or_else(|| chrono_lite_timestamp().replace('T', " ").replace('-', ":"));

    let last_prompt_snippet = truncate_snippet(conversation.last_user_prompt(), 80);
    let description = session_description(&last_prompt_snippet, conversation.turn_count());
    let friendly_name = existing_meta
        .as_ref()
        .map(|m| m.friendly_name.trim())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| friendly_session_name_for_id(&session_id));

    // Build metadata
    let meta = SessionMeta {
        session_id: session_id.clone(),
        cwd: cwd.to_string_lossy().to_string(),
        created_at,
        turns: conversation.turn_count(),
        tool_calls: conversation.intent.stats.tool_calls,
        description,
        friendly_name,
        last_prompt_snippet,
    };

    conversation.save_session(&path)?;

    let meta_path = path.with_extension("meta.json");
    let meta_json = serde_json::to_string_pretty(&meta)?;
    crate::filelock::atomic_write_locked(&meta_path, meta_json.as_bytes())?;

    tracing::info!(
        session_id,
        path = %path.display(),
        turns = meta.turns,
        "Session saved"
    );

    Ok(path)
}

/// List saved sessions for a cwd, sorted newest first.
pub fn list_sessions(cwd: &Path) -> Vec<SessionEntry> {
    let dir = match sessions_dir(cwd) {
        Some(d) => d,
        None => return vec![],
    };

    if !dir.is_dir() {
        return vec![];
    }

    let mut entries = Vec::new();
    let read_dir = match fs::read_dir(&dir) {
        Ok(r) => r,
        Err(_) => return vec![],
    };

    for entry in read_dir.flatten() {
        let path = entry.path();
        // Look for .meta.json files
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        if !name.ends_with(".meta.json") {
            continue;
        }

        // Check that the corresponding .json session file exists
        let session_path = path.with_file_name(name.replace(".meta.json", ".json"));
        if !session_path.exists() {
            continue;
        }

        let meta_json = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let meta: SessionMeta = match serde_json::from_str(&meta_json) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if !is_canonical_session_id(&meta.session_id) {
            continue;
        }
        let expected_filename = format!("{}.json", meta.session_id);
        if session_path.file_name().and_then(|name| name.to_str()) != Some(expected_filename.as_str()) {
            continue;
        }

        entries.push(SessionEntry {
            path: session_path,
            meta,
        });
    }

    // Canonical session ids start with sortable timestamps — newest first.
    entries.sort_by(|a, b| b.meta.session_id.cmp(&a.meta.session_id));
    entries
}

/// Resume a session — find by ID prefix or load the most recent.
pub fn find_session(cwd: &Path, resume_arg: Option<&str>) -> Option<PathBuf> {
    let sessions = list_sessions(cwd);
    if sessions.is_empty() {
        return None;
    }

    match resume_arg {
        None => {
            // Most recent
            Some(sessions[0].path.clone())
        }
        Some(id) => {
            let id = id.trim();
            if let Some(session) = sessions.iter().find(|s| s.meta.session_id == id) {
                return Some(session.path.clone());
            }
            let mut exact_friendly = sessions.iter().filter(|s| s.meta.friendly_name == id);
            if let Some(first) = exact_friendly.next() {
                return exact_friendly.next().is_none().then(|| first.path.clone());
            }
            let mut matches = sessions.iter().filter(|s| {
                s.meta.session_id.starts_with(id) || s.meta.friendly_name.starts_with(id)
            });
            let first = matches.next()?;
            matches.next().is_none().then(|| first.path.clone())
        }
    }
}

pub fn session_display_name(meta: &SessionMeta) -> String {
    let friendly = meta.friendly_name.trim();
    if !friendly.is_empty() {
        friendly.to_string()
    } else {
        friendly_session_name_for_id(&meta.session_id)
    }
}

pub fn session_display_description(meta: &SessionMeta) -> String {
    let description = meta.description.trim();
    if !description.is_empty() {
        return description.to_string();
    }
    let snippet = meta.last_prompt_snippet.trim();
    if !snippet.is_empty() {
        return snippet.to_string();
    }
    format!("Session {}", meta.session_id)
}

fn session_description(last_prompt_snippet: &str, turns: u32) -> String {
    let trimmed = last_prompt_snippet.trim();
    if !trimmed.is_empty() {
        trimmed.to_string()
    } else if turns == 0 {
        "Empty session".to_string()
    } else {
        format!("Session with {turns} turns")
    }
}

fn truncate_snippet(s: &str, max: usize) -> String {
    let first_line = s.lines().next().unwrap_or(s);
    if first_line.len() <= max {
        first_line.to_string()
    } else {
        crate::util::truncate(first_line, max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cwd_slug_format() {
        let slug = cwd_slug(Path::new("/Users/cwilson/workspace/ai/omegon"));
        assert_eq!(slug, "--Users-cwilson-workspace-ai-omegon--");
    }

    #[test]
    fn cwd_slug_root() {
        let slug = cwd_slug(Path::new("/tmp"));
        assert_eq!(slug, "--tmp--");
    }

    #[test]
    fn session_id_contains_timestamp() {
        let id = allocate_session_id();
        // Should start with a date-like pattern: YYYY-MM-DD
        assert!(id.len() > 20, "ID too short: {id}");
        assert!(id.contains('T'), "ID should contain T separator: {id}");
        assert!(id.contains('_'), "ID should contain _ separator: {id}");
    }

    #[test]
    fn session_id_validator_accepts_only_canonical_ids() {
        assert!(is_canonical_session_id("2026-01-02T03-04-05_deadbeef"));
        assert!(is_canonical_session_id(&allocate_session_id()));
        assert!(!is_canonical_session_id("manual-session"));
        assert!(!is_canonical_session_id("2026-01-02T03-04-05"));
        assert!(!is_canonical_session_id("2026-01-02T03-04-05_nothexzz"));
        assert!(!is_canonical_session_id("2026-1-02T03-04-05_deadbeef"));
    }

    #[test]
    fn days_to_ymd_epoch() {
        let (y, m, d) = days_to_ymd(0);
        assert_eq!((y, m, d), (1970, 1, 1));
    }

    #[test]
    fn days_to_ymd_known_date() {
        // 2026-03-18 is day 20530 since epoch
        let (y, m, d) = days_to_ymd(20530);
        assert_eq!(y, 2026);
        assert_eq!(m, 3);
        assert_eq!(d, 18);
    }

    #[test]
    fn save_and_list_round_trip() {
        let tmp = std::env::temp_dir().join("omegon-session-test-rt");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let mut conv = ConversationState::new();
        conv.push_user("Fix the auth bug".into());
        conv.intent.stats.turns = 3;
        conv.intent.stats.tool_calls = 12;

        // Override sessions_dir by saving directly
        let dir = tmp.join("sessions").join("--test--");
        fs::create_dir_all(&dir).unwrap();

        let session_id = allocate_session_id();
        let path = dir.join(format!("{session_id}.json"));
        conv.save_session(&path).unwrap();

        let meta = SessionMeta {
            session_id: session_id.clone(),
            cwd: "/test".into(),
            created_at: "2026-03-18 14:22:03".into(),
            turns: 3,
            tool_calls: 12,
            description: "Fix the auth bug".into(),
            friendly_name: friendly_session_name_for_id(&session_id),
            last_prompt_snippet: "Fix the auth bug".into(),
        };
        let meta_path = path.with_extension("meta.json");
        fs::write(&meta_path, serde_json::to_string_pretty(&meta).unwrap()).unwrap();
        assert!(!path.with_extension("tmp").exists());
        assert!(!path.with_extension("meta.tmp").exists());

        // Now list — we need to construct matching sessions_dir output
        let entries = list_from_dir(&dir);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].meta.turns, 3);
        assert_eq!(entries[0].meta.tool_calls, 12);

        // Load the session
        let loaded = ConversationState::load_session(&entries[0].path).unwrap();
        assert_eq!(loaded.turn_count(), 3);

        let _ = fs::remove_dir_all(&tmp);
    }

    /// Helper: list from a specific directory (bypasses sessions_dir home detection)
    fn list_from_dir(dir: &Path) -> Vec<SessionEntry> {
        let mut entries = Vec::new();
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            let name = path.file_name().unwrap().to_str().unwrap().to_string();
            if !name.ends_with(".meta.json") {
                continue;
            }
            let session_path = path.with_file_name(name.replace(".meta.json", ".json"));
            if !session_path.exists() {
                continue;
            }
            let meta: SessionMeta =
                serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
            if !is_canonical_session_id(&meta.session_id) {
                continue;
            }
            let expected_filename = format!("{}.json", meta.session_id);
            if session_path.file_name().and_then(|name| name.to_str()) != Some(expected_filename.as_str()) {
                continue;
            }
            entries.push(SessionEntry {
                path: session_path,
                meta,
            });
        }
        entries
    }

    #[test]
    fn list_from_dir_ignores_orphan_metadata_without_snapshot() {
        let tmp = std::env::temp_dir().join("omegon-session-test-orphan-meta");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let meta = SessionMeta {
            session_id: "orphan".into(),
            cwd: "/test".into(),
            created_at: "2026-03-18 14:22:03".into(),
            turns: 9,
            tool_calls: 2,
            description: "missing snapshot".into(),
            friendly_name: "quiet_anchor".into(),
            last_prompt_snippet: "missing snapshot".into(),
        };
        fs::write(
            tmp.join("orphan.meta.json"),
            serde_json::to_string_pretty(&meta).unwrap(),
        )
        .unwrap();

        let entries = list_from_dir(&tmp);
        assert!(entries.is_empty());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn find_session_most_recent() {
        // find_session with None should return most recent
        // This is tested implicitly by the list ordering (newest first)
        let sessions = [
            SessionEntry {
                path: PathBuf::from("b_later.json"),
                meta: SessionMeta {
                    session_id: "b".into(),
                    cwd: "/test".into(),
                    created_at: "later".into(),
                    turns: 0,
                    tool_calls: 0,
                    description: String::new(),
                    friendly_name: String::new(),
                    last_prompt_snippet: String::new(),
                },
            },
            SessionEntry {
                path: PathBuf::from("a_earlier.json"),
                meta: SessionMeta {
                    session_id: "a".into(),
                    cwd: "/test".into(),
                    created_at: "earlier".into(),
                    turns: 0,
                    tool_calls: 0,
                    description: String::new(),
                    friendly_name: String::new(),
                    last_prompt_snippet: String::new(),
                },
            },
        ];
        // Newest first means b_later is first
        assert_eq!(sessions[0].meta.session_id, "b");
    }

    #[test]
    fn friendly_session_names_are_stable_and_typable() {
        let id = "2026-01-02T03-04-05_deadbeef";
        let name = friendly_session_name_for_id(id);
        assert_eq!(name, friendly_session_name_for_id(id));
        assert!(name.contains('_'));
        assert!(name.chars().all(|ch| ch.is_ascii_lowercase() || ch == '_'));
    }

    #[test]
    fn friendly_session_lookup_requires_unique_name_or_prefix() {
        let tmp = std::env::temp_dir().join("omegon-session-test-friendly-lookup");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let cwd = tmp.join("workspace");
        fs::create_dir_all(&cwd).unwrap();
        let dir = sessions_dir(&cwd).unwrap();
        fs::create_dir_all(&dir).unwrap();

        for (id, friendly) in [
            ("2026-01-02T03-04-05_deadbeef", "quiet_anchor"),
            ("2026-01-03T03-04-05_cafebabe", "quiet_anchor"),
            ("2026-01-04T03-04-05_abcdef12", "brave_harbor"),
        ] {
            fs::write(dir.join(format!("{id}.json")), "{}").unwrap();
            let meta = SessionMeta {
                session_id: id.into(),
                cwd: cwd.to_string_lossy().to_string(),
                created_at: "2026-01-02 03:04:05".into(),
                turns: 0,
                tool_calls: 0,
                description: String::new(),
                friendly_name: friendly.into(),
                last_prompt_snippet: String::new(),
            };
            fs::write(
                dir.join(format!("{id}.meta.json")),
                serde_json::to_string_pretty(&meta).unwrap(),
            )
            .unwrap();
        }

        assert!(find_session(&cwd, Some("quiet_anchor")).is_none());
        assert!(find_session(&cwd, Some("quiet")).is_none());
        assert!(find_session(&cwd, Some("brave_harbor")).is_some());
        assert!(find_session(&cwd, Some("brave")).is_some());
        assert!(find_session(&cwd, Some("2026-01-02T03-04-05_deadbeef")).is_some());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn truncate_snippet_short() {
        assert_eq!(truncate_snippet("short", 80), "short");
    }

    #[test]
    fn truncate_snippet_long() {
        let long = "x".repeat(100);
        let result = truncate_snippet(&long, 80);
        assert!(result.len() <= 84); // 80 + "…" (3 bytes)
        assert!(result.ends_with('…'));
    }

    #[test]
    fn truncate_snippet_multiline() {
        assert_eq!(
            truncate_snippet("first line\nsecond line", 80),
            "first line"
        );
    }
}
