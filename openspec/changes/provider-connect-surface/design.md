# Connection UX design

## Operator feedback: connection-first actions

The fresh-provider pass changed provider rows to primary model selection and left
connection setup on a secondary key. This violates the /connect entry's purpose.
Provider rows must enter setup or an explicit supported-method menu. Free hosted
model selection remains an explicitly labeled exception; /model retains ordinary
route selection. Existing single-method direct setup remains available. Menus use
shared semantic actions and preserve the hidden credential editor and cancel state.

The same live feedback exposed route failures rendered with Rust Debug output.
The shared route summary owns plain-language problem descriptions so every caller
gets usable diagnostics without a TUI-specific error-string parser.

## Findings and implementation owners

`bootstrap_projection::render_bootstrap` loops over all provider statuses and renders
missing authentication with warning symbols. `setup.rs` uses that full panel for
fullscreen startup; inline startup already uses a compact route line. `NullBridge`
in `bridge.rs` prints a second list of suggested `/login` providers on a failed prompt.

`tui/auth_menu_projection.rs` already projects credential and selected/serving route
status into the shared searchable menu. `App::open_auth_menu` and the slash login
handler own its entry and authentication dispatch. Reuse those owners and the
existing borrowed fullscreen behavior for inline menus.

`runtime_commands.rs`, `command_registry.rs`, and `control_actions.rs` govern canonical
dispatch, discovery, and control authorization. Adding only a TUI alias would leave
those surfaces inconsistent. `auth.rs`, `route.rs`, `main.rs`, and `tui/footer.rs`
also generate credential remediation guidance.

## Startup policy

Give interactive startup an explicit compact projection. Provider-related output
is limited to one route summary and, if necessary, one actionable route diagnostic,
independent of catalog size. Do not convert unrelated missing credentials into
warnings. Preserve selected-versus-serving differences when a fallback is active.

Examples of the intended content, before width-dependent wrapping:

```text
omegon · <selected model> · /connect · /settings
om · No provider selected · /connect
omegon · <selected model> · Credentials expired: /connect <provider>
```

Use the actual launcher identity and admitted route. These examples do not specify
new provider/model identifiers or require live authentication probes. Consolidate
existing startup credential messages so the same problem is not repeated by
`main.rs` and the bootstrap projection. Keep non-provider operational diagnostics
under their existing policy.

Full detail does not opt an operator into startup catalog output. Explicit status
and diagnostic views retain their inventory. `render_bootstrap` also serves control
and ACP consumers; preserve their detailed response contract or introduce a separate
startup projection instead of globally stripping their data.

## Connection menu

The initial Connections view lists configured and expired credentials, with an
explicit Add provider action. Add provider opens the searchable available-provider
view. Empty Connections shows a single Add provider action. This prevents merely
moving the complete startup list into the first screen of a differently named menu.

Use existing provider/session status and credential provenance. Expired credentials
remain an existing connection. Configured means credentials are present according
to the existing resolver; it does not assert verified service health. Preserve
selected, serving, and fallback badges separately. Do not store a second connection
ledger or contact provider services when opening the menu. Local credential sources
are inspected through the existing resolver. Honor existing inventory and
authentication coverage; local endpoints must not acquire a fictitious login flow.

Adversarial review identified DwarfStar's endpoint URLs among its authentication
environment variables and Google Antigravity's lack of an interactive OAuth handler.
Both receive external-configuration guidance. Only supported OAuth handlers and
actual API-key names can start a connection flow. Credential parsing errors are
not projected into connection-row descriptions because they can contain secret values.

`CredentialState::Unreadable` now records whether a parsed store contained the
provider entry. A broken whole store does not establish a connection for every
known provider. The menu shows one store warning and keeps discovery available.
The credential probe reports parser error categories without offending values,
which also protects existing route-warning consumers.

`/connect <provider>` enters that provider's existing setup flow. Opening and searching
the menu are read-only. Credential submission or explicit OAuth selection may perform
their existing side effects. API key input stays hidden; opening a provider console
becomes a separate deliberate action rather than an automatic browser launch.
Cancellation preserves the draft and route. Keep current successful-login route
admission behavior; authentication success must not bypass model admission.

## Commands and migration

Register `/connect` with accurate per-surface availability and the same authentication
authorization for setup that `/login` currently requires. Remote/ACP callers must
receive supported guidance or an explicit interaction limitation when a secure input
flow is unavailable; do not advertise a nonexistent interactive capability.

Retain internal `AuthLogin` actions and protocol names where their meaning remains
correct. Replace operator-facing setup suggestions and menu labels with Connect.
Keep `/login` and `/auth login` working during this stage and identify `/login` as a
compatibility entry in help. Do not add it as a permanent synonym in new interfaces.
Keep `/logout` behavior and `/model` selection unchanged.

The current remote CLI coordinator and ACP transport reject `/connect` before
canonical authentication dispatch and return terminal setup guidance. This limit
holds with permission bypass enabled. Existing `/auth login` transport behavior is
outside this migration; the new spelling does not grant additional remote access.

Future `/login` will target existing connections that need renewal. No-argument use
should expose those targets; scoped use should renew or reauthenticate one target.
API keys need replacement rather than token refresh. Plugin participation requires
an advertised authentication/renewal capability. A later proposal must define those
capabilities and the compatibility cutover before changing `/login` semantics.
Do not implement bulk renewal or launch multiple browser flows in this change.

## Validation and remaining considerations

No architectural blocker was found for the first stage. The future plugin renewal
contract and `/login` cutover are deliberately outside it. During implementation,
verify connection grouping against external credentials and local route admission;
do not infer connection state from display strings.

First-run posture setup no longer makes its own partial provider-credential diagnosis.
Interactive startup emits its summary after credential refresh/adoption and route
admission, then projects a single optional problem into the TUI notification stream.

Existing API-key submission persists credentials but does not promise immediate
route replacement. `/model` remains the explicit route-selection operation. Tests
intercept the secrets-service request and use an isolated auth store for submission;
PTY captures exercise masked input and cancellation without accessing the OS keychain.

## PR integration scope

This branch inherits the earlier parity, retention, and shared TUI work, which was
not yet merged into main. Independent integration review found two prerequisite
corrections: temporary WezTerm panes were removed twice, and queued runtime decisions
could survive their authoritative turn/session. Separate corrective commits and
the `tui-review-regressions` scenarios track these fixes. The PR must describe the
accumulated work; connection-only review is not evidence for every inherited line.

Use regression tests before changing startup, dispatch, and secret-input behavior.
Capture the shared interaction through isolated headless PTYs for both layouts and
both detail levels. Stub provider/browser operations, use fixture credentials, and
record build identity with captures. Do not launch native terminal applications or
touch the operator's credentials. Land with the omegon crate gate and changed Clippy.
