# Instruction loading — Delta Spec

Accepted replacement of the two instruction requirements in the existing domain.
Global loading becomes complete and cross-route while retaining its separate owner.
All other requirements in this baseline domain remain unchanged.

## MODIFIED Requirements

### Requirement: Project instruction construction includes all applicable ancestors

Prompt construction must include each applicable ancestor AGENTS.md once, from
active worktree root through cwd, with source labels and complete UTF-8 content.
Global guidance retains its separate loading owner and is included completely
with source attribution on every common agent route, independently of grade or
posture. Host core authority is unchanged by these sources. Preserve existing
operator-before-project ordering and root-to-cwd project ordering; nearest-scope
project guidance adds to root policy. This change does not create a semantic
global-versus-project override engine or grant authority from source labels alone.

#### Scenario: Intermediate ancestor and long root policy
Given distinct root, intermediate, and cwd AGENTS.md files
And root guidance exceeds 4000 bytes and includes multibyte characters
And the complete guidance fits the request budget
When the harness constructs the project instruction section
Then all three files appear completely in root-to-cwd order with source labels
And no source appears twice

#### Scenario: Linked worktree boundary
Given cwd is nested in a linked worktree with its own root AGENTS.md
And the main checkout and a directory above the worktree contain different guidance
When the harness constructs project instructions
Then it loads the active worktree ancestors
And it excludes main-checkout and above-worktree project guidance
And global guidance retains its separate owner and complete cross-route loading

#### Scenario: Canonical duplicate
Given two discovered project paths resolve to the same permitted instruction file
And a permitted explicit symlink points outside the worktree
When the harness constructs project instructions
Then each canonical file contributes content only once
And the explicit linked file remains permitted without expanding ancestor discovery

#### Scenario: Missing ancestor file and non-Git directory
Given cwd is outside a Git worktree and contains an AGENTS.md
And optional global or intermediate instruction files are absent
When the harness constructs project instructions
Then it loads the cwd file without scanning unrelated ancestors
And absent optional instruction files do not cause an error

#### Scenario: Complete global policy on every common route
Given global instructions exceed 3000 bytes with multibyte text and a distinct project chain
And the combined mandatory input fits the selected route
When normal, headless, ACP, daemon/control, and child fixtures construct instructions across model grades and postures
Then each fixture contains complete global content under its own source label before the project chain
And neither truncation nor a model-grade or posture exception removes either source
And equal text in distinct legitimate sources is not deleted as core deduplication

### Requirement: Required project guidance is never silently omitted

Prompt preparation must distinguish absent optional files from read errors and
preserve complete required global and project guidance. Existing unreadable,
invalid-UTF-8, or dangling instruction sources must fail actionably before
dispatch. Complete mandatory input must pass the existing resolved request-budget
policy, including provider prefix, actual schemas, and applicable output reserves.
This applies at existing construction/preparation boundaries and does not require
live refresh or durable instruction generations. Unlike the prior global loader,
errors must not silently become an empty instruction section.

#### Scenario: Unreadable applicable file
Given an applicable project AGENTS.md exists but cannot be read
When the harness prepares a model request
Then preparation reports the source and a recoverable read error
And no model request is dispatched with silently omitted guidance

#### Scenario: Required guidance cannot fit
Given complete core, global/project instructions, mandatory context, actual provider prefix, and schemas exceed the selected route budget after existing reserves
When the harness prepares a model request
Then it reports an actionable budget error before network dispatch
And it does not truncate policy or switch to a reduced prompt family to make the request fit
And schemas, cached input, and reasoning are not double-counted
And an allowed optional-context or tool-surface repair requires a newly valid preparation

#### Scenario: Existing invalid global source fails closed
Given separate fixtures where the global AGENTS.md is unreadable, invalid UTF-8, or a dangling symlink
When each fixture constructs common agent instructions
Then construction fails with the source path, failure reason, and corrective action
And no provider request proceeds with an empty substitute for that global policy
