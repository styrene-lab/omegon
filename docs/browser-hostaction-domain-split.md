---
id: browser-hostaction-domain-split
title: "Browser HostAction Domain Split"
status: deferred
tags: [host-actions, browser, resource-open, security, sdk]
open_questions:
  - "Should browser navigation be modeled as browser.open@1, browser.navigate@1, or a broader browser.action@1 operation enum?"
  - "Should Tampermonkey/userscript integration be host-owned through a browser bridge extension, or extension-origin HostActions that target a separately installed browser bridge?"
  - "What is the minimum safe profile selector contract: named profile, ephemeral profile, persistent isolated profile, or host default profile with manual approval?"
dependencies: []
related: []
---

# Browser HostAction Domain Split

## Overview

Track the future split of browser interactions into their own HostAction domain rather than treating web URLs and browser automation as resource.open@1 fallbacks. This preserves resource.open@1 as local/static resource presentation while reserving browser.open/browser.script style actions for profile-aware navigation and DOM/userscript automation.

## Research

### Domain split rationale from #83 planning

The operator explicitly expects browser profile selection, DOM/navigation control, and potentially direct integration with browser extensions such as Tampermonkey. These are not merely resource presentation concerns. resource.open@1 should avoid web URL/default browser escape hatches in v1 and remain focused on local/static resources. Browser features should be first-class HostAction domains with separate approval and policy controls.

## Decisions

### Browser interactions are a separate HostAction domain

**Status:** decided

**Rationale:** Browser interactions can involve profile selection, authenticated cookies, tab lifecycle, navigation, DOM state, browser extensions, and userscript execution. These policy axes are materially different from local resource presentation, so browser work should not be hidden behind resource.open@1.

### resource.open@1 v1 excludes web URL/browser session routing

**Status:** decided

**Rationale:** resource.open@1 should initially handle local/static resources and host-owned viewers/editors. If given a web URL before browser.open@1 exists, the host should return unsupported or a suggested future browser action rather than silently invoking a browser/default opener.

### Future browser.open@1 owns URL/profile/tab navigation

**Status:** proposed

**Rationale:** A browser.open/browser.navigate family should own URL scheme/origin allowlists, profile selection, tab/window targeting, isolated vs persistent sessions, and external browser fallback. This keeps browser session control explicit and auditable.

### Future browser.script@1 owns DOM/userscript automation

**Status:** proposed

**Rationale:** DOM and userscript automation should be stricter than URL opening. It needs origin allowlists, trusted script identity, bridge/extension identity, argument schemas, manual approval defaults, and explicit handling of page secrets and authenticated sessions.

## Open Questions

- Should browser navigation be modeled as browser.open@1, browser.navigate@1, or a broader browser.action@1 operation enum?
- Should Tampermonkey/userscript integration be host-owned through a browser bridge extension, or extension-origin HostActions that target a separately installed browser bridge?
- What is the minimum safe profile selector contract: named profile, ephemeral profile, persistent isolated profile, or host default profile with manual approval?

## Hygiene note

Browser HostAction domain design is post-0.27 substrate/security work. It remains reference material for a future browser HostAction workstream; 0.27.0 should not grow new browser automation scope.
