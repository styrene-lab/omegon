+++
id = "a9b71e0d-6aae-4592-8822-f64c574a910a"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Spinner Verbs — Contribution Guide

The spinner verbs are the rotating action phrases displayed while omegon is
working (e.g. `⟳ Consulting the palantir`).  They live in
`core/crates/omegon/src/tui/spinner.rs`.

Adding a new verb is a great **good first issue** for new contributors.

## Visual rendering

The spinner displays in the editor block's title bar:

- **Active**: `⟳ {verb}` — bright accent icon (`accent_bright`),
  muted accent verb text (`accent_muted`), with an HSL glow animation
  (30° hue shift, 2s ping-pong SineInOut cycle).
- **Idle**: `{model} ▸` — standard accent color.

The verb list is **shuffled at startup** using a Fisher-Yates shuffle seeded
from the process start time, so consecutive verbs never cluster by category.
The verb advances on each `TurnStart` and `ToolStart` event.

## How to contribute a verb

1. Fork the repo and create a branch: `git checkout -b spinner/your-verb`
2. Add your verb to `BUILTIN_VERBS` in `core/crates/omegon/src/tui/spinner.rs`,
   under the most appropriate category heading.
3. Run `cargo test -p omegon spinner` to confirm no duplicates and the minimum
   count is met.
4. Open a PR with the title `spinner: add "<Your Verb Here>"` and include a
   brief justification (see below).

## Editorial criteria

Every verb must meet **all** of the following:

- **Literate source material.** The reference must come from established
  literature, mythology, or erudite speculative fiction.  Think: Borges,
  Wolfe, Le Guin, Peake, Vance, Lem, Banks, Herbert, Lovecraft, Tolkien,
  classical mythology, alchemical tradition.

- **No programming puns.** The verb should stand on its own as an evocative
  phrase, not map the reference back to a coding concept.  "Consulting the
  palantir" is good.  "Speaking 'friend' and entering the API" is not.

- **No mass-market genre TV/film.** References should not date to a specific
  show or franchise that will feel stale in five years.  Star Trek, Marvel,
  recent Netflix adaptations, and anime are out.

- **Tone: reverent, not winking.** The verb should read as though the tool
  genuinely is performing an arcane rite, not making a joke about it.
  Deadpan sincerity, not a t-shirt slogan.

- **Length: max 40 characters, 3-8 words.**  The display format is
  `⟳ {verb}` in a block title.  On an 80-column terminal, anything over
  40 chars will clip.  The test suite enforces this limit.

## Justification format

In your PR description, include:

```
Source: [Work title] by [Author/tradition]
Why: [1-2 sentences on why this reference fits the editorial criteria]
```

Example:

```
Source: "The Book of the New Sun" by Gene Wolfe
Why: The Matachin Tower is Severian's home — an ancient, half-understood
structure full of tools whose original purpose has been lost.  Perfect
analogy for navigating a legacy codebase.
```

## Current categories

| Category | Source material |
|----------|---------------|
| Adeptus Mechanicus | Warhammer 40,000 lore (the Mechanicus rites, not the bolter porn) |
| Imperium of Man | Warhammer 40,000 (restrained — atmosphere, not memes) |
| Classical Antiquity | Greek/Roman mythology |
| Norse | Norse mythology and Eddas |
| Arthurian & Medieval | Matter of Britain, the Mabinogion, Grail literature |
| Lovecraftian | H.P. Lovecraft and the Cthulhu Mythos |
| Dune | Frank Herbert's Dune (the novels, not the films) |
| Tolkien | J.R.R. Tolkien's legendarium |
| Gormenghast | Mervyn Peake |
| Gene Wolfe | The Book of the New Sun, The Book of the Long Sun |
| Ursula K. Le Guin | Earthsea, Hainish Cycle |
| Jack Vance | The Dying Earth |
| Stanislaw Lem | Solaris, The Cyberiad, Golem XIV |
| Iain M. Banks | The Culture novels |
| Borges | Jorge Luis Borges — Ficciones, Labyrinths |
| Alchemy & Hermetic | Western alchemical and hermetic tradition |
| Miscellaneous Erudite | One-offs from other qualifying sources |

New categories are welcome if you can justify 4+ verbs from the same source
and the source meets the editorial criteria.

## User overrides

Users who want their own verbs without contributing upstream can create
`~/.config/omegon/spinner-verbs.txt` with one verb per line:

```
# My custom spinner verbs
Consulting the Witch of Endor
Navigating the Wood Between the Worlds
Activating the Subtle Knife
```

Blank lines and `#` comments are ignored.  User verbs are appended to the
built-in list, not replacements.
