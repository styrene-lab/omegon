# Design

Remove first_run::run_interactive and its call before AgentSetup. Shared settings
bootstrap already resolves profile, CLI, and entrypoint preferences; another
posture choice after that resolution overwrites policy and writes unwanted state.
The side-effect-free first-launch predicate remains solely for startup splash policy.

Extend the existing isolated tmux runner with a fresh-install startup-only path:
remove its permissions profile, omit OMEGON_CHILD, exercise the real entrypoint
launcher without layout/detail flags, capture the editor, type/clear a draft, and
quit. Assert no profile is created before quitting and no inference is requested.
Existing exit-time session/profile persistence is unchanged. Existing full-flow
acceptance continues testing connection browsing, multiple turns, and permission denial.

Back up and remove only the profile introduced by the mistaken wizard acceptance
and the preview's exit-time project snapshot, after the old preview has stopped.
Retain credentials, named custom profiles, and session history.
