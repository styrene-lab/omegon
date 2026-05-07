+++
id = "9145b1b0-64b6-4f31-aa9f-4dbbfc2aa3ab"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Assess bridge returns completed structured results — Tasks

## 1. Complete bridged /assess execution in-band
<!-- specs: harness/slash-commands -->

- [x] 1.1 Refactor `/assess spec` execution so bridged/tool callers receive a completed structured assessment result instead of a preparatory kickoff response
- [x] 1.2 Preserve the existing interactive `/assess` operator experience by keeping follow-up prompting isolated to the interactive path
- [x] 1.3 Ensure bridged assessment lifecycle fields are derived from the completed result for the current implementation snapshot

## 2. Preserve bridge contract semantics
<!-- specs: harness/slash-commands -->

- [x] 2.1 Keep the normalized slash-command bridge envelope explicit about synchronous completion semantics for bridged `/assess`
- [x] 2.2 Preserve the full original tokenized invocation in `result.args` while carrying completed assessment metadata in structured fields

## 3. Add regression coverage for bridged and interactive assess flows
<!-- specs: harness/slash-commands -->

- [x] 3.1 Add regression tests proving bridged `/assess spec <change>` returns a completed result in the initial structured response
- [x] 3.2 Add regression tests proving interactive `/assess spec <change>` can still use follow-up prompting without corrupting bridged behavior
- [x] 3.3 Add regression tests for trustworthy lifecycle metadata and preserved `result.args`
