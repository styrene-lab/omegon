//! Spinner verbs — flavorful action messages during tool execution.
//!
//! Rotates through themed verb phrases on each tool call and turn start.
//! Displayed in the editor prompt area while the agent is working.
//!
//! ## User extras
//!
//! Users can add their own verbs by creating `~/.config/omegon/spinner-verbs.txt`
//! with one verb per line.  Blank lines and lines starting with `#` are ignored.
//! Extras are appended to the built-in list, not replacements.
//!
//! ## Contributing new built-in verbs
//!
//! See `SPINNER_VERBS.md` in the repository root for the contribution guide
//! and editorial criteria.

use std::path::Path;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Combined verb list: built-ins + user extras.  Initialised once by [`init`].
/// Falls back to built-ins only if [`init`] was never called.
static COMBINED: OnceLock<Vec<&'static str>> = OnceLock::new();

/// Initialise the spinner with a seed and optional user extras file.
///
/// Call once at startup.  The seed (e.g. process start time in ms) sets the
/// starting position so consecutive sessions don't begin on the same verb.
/// If `extras_path` points to a readable file, its non-empty, non-comment
/// lines are appended to the built-in verb list.
pub fn init(seed_value: usize, extras_path: Option<&Path>) {
    let mut combined: Vec<&'static str> = BUILTIN_VERBS.to_vec();

    if let Some(path) = extras_path
        && let Ok(content) = std::fs::read_to_string(path)
    {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            // Leak the string so it lives for the program's lifetime,
            // keeping the return type of next_verb() as &'static str.
            let leaked: &'static str = Box::leak(trimmed.to_string().into_boxed_str());
            combined.push(leaked);
        }
    }

    // Fisher-Yates shuffle so consecutive verbs don't cluster by category.
    // Uses a simple xorshift PRNG seeded from the same process-start value
    // — cryptographic quality is irrelevant here.
    let mut rng = seed_value.wrapping_add(1);
    for i in (1..combined.len()).rev() {
        // xorshift64-style step (truncated to usize)
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        let j = rng % (i + 1);
        combined.swap(i, j);
    }

    COUNTER.store(0, Ordering::Relaxed);
    let _ = COMBINED.set(combined);
}

/// Legacy seed-only init for tests.
pub fn seed(value: usize) {
    let verbs = active_verbs();
    COUNTER.store(value % verbs.len(), Ordering::Relaxed);
}

fn active_verbs() -> &'static [&'static str] {
    COMBINED
        .get()
        .map(|v| v.as_slice())
        .unwrap_or(BUILTIN_VERBS)
}

/// Get the next spinner verb.  Advances the counter each call.
pub fn next_verb() -> &'static str {
    let verbs = active_verbs();
    let idx = COUNTER.fetch_add(1, Ordering::Relaxed) % verbs.len();
    verbs[idx]
}

/// Number of verbs in the active list (built-ins + extras).
pub fn verb_count() -> usize {
    active_verbs().len()
}

// ── Glitch effect ─────────────────────────────────────────────────────────
//
// Once roughly every 60 seconds (at 60fps), corrupt 1-2 characters of the
// verb for a single frame.  The effect is so rare and brief that most users
// will never consciously notice it — but it gives the spinner a subliminal
// sense of the machine spirit being not entirely under control.

/// Unicode substitution table — visually similar but not identical glyphs.
/// Each entry is (ASCII char, glitched replacement).
const GLITCH_CHARS: &[(char, char)] = &[
    ('a', 'α'),
    ('e', 'ε'),
    ('i', 'ι'),
    ('o', 'σ'),
    ('u', 'μ'),
    ('s', '§'),
    ('t', '†'),
    ('n', 'η'),
    ('r', 'г'),
    ('l', 'ℓ'),
    ('c', 'ϲ'),
    ('d', 'δ'),
    ('g', 'ğ'),
    ('h', 'ħ'),
    ('m', 'ɱ'),
    ('C', 'Ↄ'),
    ('S', '§'),
    ('R', 'Я'),
    ('A', 'Λ'),
    ('T', 'Ŧ'),
    ('I', 'Ї'),
    ('N', 'Ͷ'),
    ('P', 'Ρ'),
    ('D', 'Ð'),
    ('G', 'Ǥ'),
];

/// Frame counter for glitch timing — not worth an AtomicU64, we don't care
/// about cross-thread precision here.
static GLITCH_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Probability: ~1 in 3600 frames ≈ once per minute at 60fps.
const GLITCH_ODDS: usize = 3600;

/// Maybe-glitch a verb string for this frame.  Returns `None` most of the
/// time (no glitch); returns `Some(glitched)` on the rare frame it fires.
/// Call once per draw frame, not per event.
pub fn maybe_glitch(verb: &str) -> Option<String> {
    let frame = GLITCH_COUNTER.fetch_add(1, Ordering::Relaxed);

    // Cheap modular check — avoids calling any PRNG on non-glitch frames.
    // Mix the frame counter a bit so it doesn't fire on exact multiples.
    let mixed = frame.wrapping_mul(2654435761); // Knuth multiplicative hash
    if !mixed.is_multiple_of(GLITCH_ODDS) {
        return None;
    }

    // Pick 1-2 character positions to corrupt.
    let chars: Vec<char> = verb.chars().collect();
    if chars.is_empty() {
        return None;
    }

    let mut out = chars.clone();
    let pos = mixed.wrapping_shr(12) % chars.len();

    if let Some(replacement) = GLITCH_CHARS
        .iter()
        .find(|(c, _)| *c == chars[pos])
        .map(|(_, g)| *g)
    {
        out[pos] = replacement;
    } else {
        // No mapping for this char — shift it by one codepoint.
        out[pos] = char::from_u32(chars[pos] as u32 + 1).unwrap_or(chars[pos]);
    }

    Some(out.into_iter().collect())
}

// ═══════════════════════════════════════════════════════════════════════════
//  Built-in verb list
//
//  See SPINNER_VERBS.md for the editorial criteria and contribution guide.
//  Categories are interleaved so consecutive verbs rarely share a theme.
// ═══════════════════════════════════════════════════════════════════════════

const BUILTIN_VERBS: &[&str] = &[
    // ─── Adeptus Mechanicus — Rites of the Omnissiah ───
    "Communing with the Machine Spirit",
    "Appeasing the Omnissiah",
    "Reciting the Litany of Ignition",
    "Applying sacred unguents",
    "Chanting binharic cant",
    "Performing the Rite of Clear Mind",
    "Querying the Noosphere",
    "Invoking the Motive Force",
    "Beseeching the Machine God",
    "Placating the logic engine",
    "Interfacing with the cogitator",
    "Calibrating the mechadendrites",
    "Whispering the Cant of Maintenance",
    "Conducting the Binary Psalm",
    "Purifying the corrupted sectors",
    "Performing the Thirteen Rituals",
    "Offering a binary prayer to the void",
    "Burning the sacred oils",
    // ─── Imperium of Man ───
    "Purging the heretical code",
    "Sanctifying the build pipeline",
    "Affixing the Purity Seal",
    "Fortifying this position",
    "Exorcising the daemon process",
    "Consulting the Codex Astartes",
    // ─── Classical Antiquity ───
    "Consulting the Oracle at Delphi",
    "Reading the auguries",
    "Descending into the labyrinth",
    "Weaving on Athena's loom",
    "Unraveling Ariadne's thread",
    "Stealing fire from Olympus",
    "Forging on Hephaestus's anvil",
    "Bargaining with the Sphinx",
    "Navigating between Scylla and Charybdis",
    "Gathering the golden fleece",
    "Crossing the River Styx",
    "Appeasing the Eumenides",
    // ─── Norse ───
    "Consulting the Norns",
    "Reading the runes",
    "Asking Mímir's head for guidance",
    "Hanging from Yggdrasil for wisdom",
    "Forging in the heart of Niðavellir",
    "Descending into Niflheim",
    "Awaiting Ragnarök",
    "Casting the runes of Urd",
    // ─── Arthurian & Medieval ───
    "Questing for the Grail",
    "Consulting Merlin's grimoire",
    "Entering the Chapel Perilous",
    "Crossing the Siege Perilous",
    "Reading from the Mabinogion",
    "Seeking the Fisher King",
    "Drawing the sword from the stone",
    "Wandering the Waste Land",
    // ─── Lovecraftian ───
    "Gazing into the non-Euclidean geometry",
    "Consulting the Necronomicon",
    "Invoking That Which Must Not Be Named",
    "Performing sanity-eroding rites",
    "Contemplating the Hounds of Tindalos",
    "Deciphering the Pnakotic Manuscripts",
    "Listening for the piping of Azathoth",
    // ─── Dune ───
    "Consulting the Mentat",
    "Folding space through the Holtzman drive",
    "Navigating the Golden Path",
    "Reciting the Litany Against Fear",
    "Surviving the Gom Jabbar",
    "Consulting the Orange Catholic Bible",
    "Prescience-tracing the decision tree",
    "Awaiting the Kwisatz Haderach",
    // ─── Tolkien — Middle-earth ───
    "Consulting the palantír",
    "Seeking the counsel of Elrond",
    "Delving too greedily and too deep",
    "Reading the Book of Mazarbul",
    "Passing through the Doors of Durin",
    "Listening for the Ainulindalë",
    "Walking the Straight Road",
    "Kindling the light of Eärendil",
    // ─── Gormenghast — Mervyn Peake ───
    "Ascending the Tower of Flints",
    "Observing the Bright Carvings ritual",
    "Navigating the corridors of Gormenghast",
    "Cataloguing in the Hall of Spiders",
    "Performing the ceremony as prescribed",
    "Consulting the Master of Ritual",
    // ─── Gene Wolfe — The Book of the New Sun ───
    "Consulting the brown book",
    "Descending into the oubliette",
    "Operating the Claw of the Conciliator",
    "Traversing the House Absolute",
    "Consulting the autarch's memories",
    "Ascending the Matachin Tower",
    "Reading from the Book of Gold",
    "Awaiting the coming of the New Sun",
    // ─── Ursula K. Le Guin — Earthsea & Hainish ───
    "Speaking the true name",
    "Consulting the Masters of Roke",
    "Sailing beyond the farthest shore",
    "Studying in the Immanent Grove",
    "Restoring the Equilibrium",
    "Crossing the wall of stones",
    "Listening on the ansible",
    "Walking the dry land",
    // ─── Jack Vance — The Dying Earth ───
    "Memorizing the Excellent Prismatic Spray",
    "Consulting Iucounu the Laughing Magician",
    "Perusing the Universal Compendium",
    "Invoking the Spell of Forlorn Encystment",
    "Studying in the manse of Pandelume",
    "Traversing the Land of the Falling Wall",
    // ─── Stanislaw Lem ───
    "Consulting the Golem XIV",
    "Orbiting the ocean of Solaris",
    "Navigating the Nth voyage",
    "Studying the exegesis of Vestrand",
    "Parsing the apocryphal transmissions",
    "Deciphering the message from the stars",
    // ─── Iain M. Banks — The Culture ───
    "Consulting the Ship Mind",
    "Navigating Outside Context Problems",
    "Subliming the architecture",
    "Querying the Interesting Times Gang",
    "Operating in infinite fun space",
    "Reviewing Conditions of Acceptance",
    // ─── Borges & the Fantastic ───
    "Searching the Library of Babel",
    "Entering the Garden of Forking Paths",
    "Consulting the Book of Sand",
    "Contemplating the Aleph",
    "Navigating the labyrinth of Ts'ui Pên",
    "Drawing from Tlön's encyclopedia",
    // ─── Alchemy & Hermetic ───
    "Transmuting the base code into gold",
    "Distilling the quintessence",
    "Performing the Great Work",
    "Drawing the sigil of binding",
    "Consulting the Emerald Tablet",
    "Dissolving in the alchemical bath",
    "Separating the subtle from the gross",
    "Sealing the athanor",
    // ─── Miscellaneous Erudite ───
    "Tracing the pattern in the carpet",
    "Consulting the I Ching",
    "Descending the staircase of Piranesi",
    "Navigating the Phantom Tollbooth",
    "Winding the golden bough",
    "Interpreting the Voynich manuscript",
    "Adjusting the Antikythera mechanism",
    "Crossing the desert of the real",
    "Studying the Codex Seraphinianus",
    "Turning the prayer wheel",
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static GLITCH_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn next_verb_cycles() {
        seed(0);
        let v1 = next_verb();
        let v2 = next_verb();
        assert_ne!(v1, v2, "consecutive verbs should differ");
    }

    #[test]
    fn next_verb_wraps() {
        seed(BUILTIN_VERBS.len() - 1);
        let _ = next_verb(); // last
        let v = next_verb(); // wraps to 0
        assert_eq!(v, BUILTIN_VERBS[0]);
    }

    #[test]
    fn all_verbs_non_empty() {
        for (i, v) in BUILTIN_VERBS.iter().enumerate() {
            assert!(!v.is_empty(), "verb at index {i} is empty");
        }
    }

    #[test]
    fn verb_count_minimum() {
        assert!(
            BUILTIN_VERBS.len() >= 100,
            "should have at least 100 built-in verbs, got {}",
            BUILTIN_VERBS.len()
        );
    }

    #[test]
    fn all_verbs_fit_narrow_terminal() {
        const MAX_LEN: usize = 40;
        for (i, v) in BUILTIN_VERBS.iter().enumerate() {
            assert!(
                v.len() <= MAX_LEN,
                "verb at index {i} is {} chars (max {MAX_LEN}): {v:?}",
                v.len()
            );
        }
    }

    #[test]
    fn no_duplicate_verbs() {
        let mut seen = std::collections::HashSet::new();
        for (i, v) in BUILTIN_VERBS.iter().enumerate() {
            assert!(seen.insert(*v), "duplicate verb at index {i}: {v:?}");
        }
    }

    #[test]
    fn extras_loaded_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let extras = dir.path().join("spinner-verbs.txt");
        std::fs::write(
            &extras,
            "# Custom verbs\nTraversing the Wandering Inn\n\nConsulting the Cthaeh\n",
        )
        .unwrap();

        // Reset for this test — OnceLock can only be set once per process,
        // so we test the parsing logic directly.
        let content = std::fs::read_to_string(&extras).unwrap();
        let parsed: Vec<&str> = content
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect();
        assert_eq!(
            parsed,
            &["Traversing the Wandering Inn", "Consulting the Cthaeh"]
        );
    }

    #[test]
    fn glitch_produces_visually_similar_output() {
        let _guard = GLITCH_TEST_LOCK.lock().expect("glitch test lock");
        // Force the counter to a value that will trigger a glitch.
        // mixed = frame * 2654435761; we need mixed % 3600 == 0.
        // Brute-force find a triggering frame value.
        let mut trigger_frame = None;
        for f in 0usize..10_000 {
            let mixed = f.wrapping_mul(2654435761);
            if mixed % GLITCH_ODDS == 0 {
                trigger_frame = Some(f);
                break;
            }
        }
        let f = trigger_frame.expect("should find a triggering frame in 0..10000");
        GLITCH_COUNTER.store(f, Ordering::Relaxed);

        let verb = "Consulting the palantír";
        let glitched = maybe_glitch(verb);
        assert!(
            glitched.is_some(),
            "glitch should fire on the triggering frame"
        );
        let g = glitched.unwrap();
        assert_ne!(g, verb, "glitched text should differ from original");
        // Same length in chars (substitution, not insertion/deletion)
        assert_eq!(
            g.chars().count(),
            verb.chars().count(),
            "glitch must preserve character count"
        );
    }

    #[test]
    fn glitch_is_rare() {
        let _guard = GLITCH_TEST_LOCK.lock().expect("glitch test lock");
        // Run 1000 frames, expect at most a handful of glitches.
        GLITCH_COUNTER.store(0, Ordering::Relaxed);
        let mut glitch_count = 0;
        for _ in 0..1000 {
            if maybe_glitch("Reading the runes").is_some() {
                glitch_count += 1;
            }
        }
        assert!(
            glitch_count <= 3,
            "glitch fired {glitch_count} times in 1000 frames — should be ≤3"
        );
    }
}
