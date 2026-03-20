//! Spinner verbs — flavorful action messages during tool execution.
//!
//! Rotates through themed verb phrases on each tool call and turn start.
//! Displayed in the editor prompt area while the agent is working.

use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Get a random spinner verb. Advances the counter each call.
pub fn next_verb() -> &'static str {
    let idx = COUNTER.fetch_add(1, Ordering::Relaxed) % VERBS.len();
    VERBS[idx]
}

/// Shuffle the starting position based on a seed (e.g. process start time).
pub fn seed(value: usize) {
    COUNTER.store(value % VERBS.len(), Ordering::Relaxed);
}

const VERBS: &[&str] = &[
    // ═══ Adeptus Mechanicus — Rites of the Omnissiah ═══
    "Communing with the Machine Spirit",
    "Appeasing the Omnissiah",
    "Reciting the Litany of Ignition",
    "Applying sacred unguents",
    "Chanting binharic cant",
    "Performing the Rite of Clear Mind",
    "Querying the Noosphere",
    "Invoking the Motive Force",
    "Beseeching the Machine God",
    "Parsing sacred data-stacks",
    "Placating the logic engine",
    "Interfacing with the cogitator",
    "Calibrating the mechadendrites",
    "Burning sacred incense over the server",
    "Whispering the Cant of Maintenance",
    "Cataloguing the STC fragments",
    "Venerating the sacred repository",
    "Decrypting the Archaeotech",
    "Conducting the Binary Psalm",
    "Purifying the corrupted sectors",
    "Performing the Thirteen Rituals of Compilation",
    "Soothing the belligerent plasma coil",
    "Offering a binary prayer to the void",
    "Reciting the Canticle of the Blessed Diff",
    "Applying the Unguent of Optimal Throughput",

    // ═══ Imperium of Man — Warfare & Devotion ═══
    "Purging the heretical code",
    "Deploying the Exterminatus on tech debt",
    "Consulting the Codex Astartes",
    "Exorcising the daemon process",
    "Sanctifying the build pipeline",
    "Fortifying the firewall bastions",
    "Affixing the Purity Seal to the commit",
    "Routing the xenos from the dependency tree",
    "Scourging the technical debt",
    "Crusading through the backlog",
    "Debugging with extreme prejudice",
    "Declaring Exterminatus on the node_modules",
    "Fortifying this position",

    // ═══ Classical Antiquity ═══
    "Consulting the Oracle at Delphi",
    "Reading the auguries",
    "Descending into the labyrinth",
    "Weaving on Athena's loom",
    "Unraveling Ariadne's thread",
    "Stealing fire from Olympus",
    "Forging on Hephaestus's anvil",
    "Bargaining with the Sphinx",
    "Cleaning the Augean stables of legacy code",
    "Charting a course between Scylla and Charybdis",

    // ═══ Norse — Sagas & Runes ═══
    "Consulting the Norns",
    "Reading the runes",
    "Asking Mímir's head for guidance",
    "Hanging from Yggdrasil for wisdom",
    "Forging in the heart of Niðavellir",
    "Feeding Huginn and Muninn the latest telemetry",
    "Braving the Fimbulwinter of dependency hell",

    // ═══ Arthurian & Medieval ═══
    "Questing for the Holy Grail of zero bugs",
    "Pulling the sword from the CI/CD stone",
    "Convening the Round Table for design review",
    "Consulting Merlin's grimoire",

    // ═══ Lovecraftian — Cosmic Horror ═══
    "Gazing into the non-Euclidean geometry of the type system",
    "Consulting the Necronomicon of legacy documentation",
    "Invoking That Which Should Not Be Refactored",
    "Descending into the R'lyeh of nested callbacks",
    "Performing rites that would drive lesser compilers mad",

    // ═══ Dune — Arrakis & the Imperium ═══
    "Consulting the Mentat about computational complexity",
    "Folding space through the Holtzman drive",
    "Navigating the Golden Path of the refactor",
    "Consuming the spice of stack traces",
    "Reciting the Litany Against Fear",
    "Surviving the Gom Jabbar of code review",

    // ═══ Tolkien — Middle-earth ═══
    "Consulting the palantír",
    "Speaking 'friend' and entering the API",
    "Seeking the counsel of Elrond",
    "Delving too greedily and too deep",
    "Riding the Eagles to production",

    // ═══ Eastern — Sun Tzu, Zen ═══
    "Contemplating the sound of one hand coding",
    "Achieving mushin no shin — mind without mind",
    "Sitting with the kōan of the failing assertion",

    // ═══ Alchemy & Occult ═══
    "Transmuting the base code into gold",
    "Distilling the quintessence from the logs",
    "Performing the Great Work upon the monolith",
    "Drawing the sigil of binding upon the interface",

    // ═══ The Expanse ═══
    "Performing a hard burn toward the solution",
    "Navigating the Ring Gate to the next module",
    "Running diagnostics on the Epstein drive",
    "Drifting in the slow zone, waiting on I/O",
    "Deploying PDCs against incoming regressions",
    "Reading the Roci's threat board",

    // ═══ Three Body Problem ═══
    "Unfolding the proton into two dimensions",
    "Monitoring the sophon for interference",
    "Computing the three-body orbital solution",
    "Awaiting the next Stable Era",
    "Wallface-ing the architecture decision",

    // ═══ Annihilation — Area X ═══
    "Crossing the border into Area X",
    "Observing the refraction through the Shimmer",
    "Watching the code bloom into something unrecognizable",
    "Submitting to the annihilation of the old architecture",

    // ═══ Starfleet Engineering ═══
    "Rerouting auxiliary power to the build server",
    "Realigning the dilithium matrix",
    "Compensating for subspace interference",
    "Recalibrating the EPS conduits",
    "Reinitializing the pattern buffer",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_verb_cycles() {
        seed(0);
        let v1 = next_verb();
        let v2 = next_verb();
        assert_ne!(v1, v2, "consecutive verbs should differ");
    }

    #[test]
    fn next_verb_wraps() {
        seed(VERBS.len() - 1);
        let _ = next_verb(); // last
        let v = next_verb(); // wraps to 0
        assert_eq!(v, VERBS[0]);
    }

    #[test]
    fn all_verbs_non_empty() {
        for (i, v) in VERBS.iter().enumerate() {
            assert!(!v.is_empty(), "verb at index {i} is empty");
        }
    }

    #[test]
    fn verb_count() {
        assert!(VERBS.len() >= 100, "should have at least 100 verbs, got {}", VERBS.len());
    }
}
