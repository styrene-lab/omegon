+++
id = "39e5741d-c1ac-414d-90d2-6f412a15e2a9"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# 0.6.9 stability step — runaway resource use, lingering pi processes, and usability hardening

## Overview

Investigate and harden recurring 0.6.9 stability/usability issues observed during normal Omegon operation, especially runaway resource consumption, lingering `pi` subprocesses, and clipboard/temp-path related runtime noise. The goal is to identify the concrete leak/spawn failure modes, bound resource usage, and ensure the operator-facing product remains stable and understandable during long-running sessions.

## Research

### Initial operator report

Operator reports multiple observed episodes of runaway resource use and lingering `pi` processes while running Omegon, plus runtime noise involving clipboard temp artifacts under `/var/folders/.../T/pi-clipboard-*`. This suggests at least one still-leaky subprocess or temp-artifact path remains despite prior stability work in 0.6.6–0.6.8, and 0.6.9 should explicitly audit subprocess lifecycle, clipboard/temp-file cleanup, and operator-facing recovery behavior.

### Concrete clipboard temp artifact evidence

Observed concrete temp artifact path during the failure mode: `/var/folders/vl/w3m4rq616c9gv9cmbj99kz_80000gn/T/pi-clipboard-ab2029f7-4c45-4ce2-98aa-7f53f834ad97.png`. This strengthens the hypothesis that clipboard image handling and temp-file lifecycle are part of the 0.6.9 stability/usability problem surface, either through leaked files, repeated references surfacing in operator output, or clipboard-related subprocess paths failing to clean up correctly.

### Activity Monitor evidence of many concurrent pi processes

The provided screenshot of Activity Monitor is directly relevant: it shows a large number of concurrent `pi` processes running at once, many with very similar CPU usage (~4–7% each), similar thread counts (mostly 23 threads), and similar short CPU-time totals (~2.2 CPU time for many entries), alongside at least one longer-lived `pi` instance (~4:38 CPU time, 33 threads, 24 idle wake-ups). This pattern looks more like repeated spawned worker/child processes that are not exiting cleanly than a single runaway main process. The screenshot also captures the clipboard temp image path itself, reinforcing that clipboard artifact handling is part of the observed operator workflow when this leak is noticed.

### UX severity and thermal impact

The operator reports that the multi-`pi` process accumulation makes Omegon almost unusable from a UX perspective and can overheat the machine. This elevates the issue from mere background inefficiency to a release-blocking stability/usability defect: any fix must reduce lingering child-process count, bound CPU usage during assessment/cleave/extraction flows, and improve operator trust that long-running helpers will terminate.

### Initial spawn-path audit

Current Omegon-owned subprocess spawn sites that can recursively launch the full runtime are concentrated in three places: `extensions/cleave/dispatcher.ts` (child task execution), `extensions/cleave/index.ts` (structured spec assessment subprocess), and `extensions/project-memory/extraction-v2.ts` (memory extraction subprocesses). The extraction path is comparatively defensive: it spawns detached, tracks active/all subprocesses, and kills by process group (`process.kill(-pid, signal)`) to avoid orphans. By contrast, the Cleave child runner and spec-assessment runner spawn non-detached child Omegon processes and only call `proc.kill('SIGTERM')` / `proc.kill('SIGKILL')` on timeout or abort, with no process-group kill, no shared tracking set, and no shutdown sweep. If those child Omegon instances themselves launch additional subprocesses or fail to exit promptly after timeout, they can accumulate as lingering `pi` processes.

### Likely failure mode: timeout rejects before confirmed process teardown

The Cleave spec-assessment subprocess path in `extensions/cleave/index.ts` rejects the parent promise immediately on timeout after sending `SIGTERM` and scheduling a later `SIGKILL`, rather than waiting for the child `close` event to confirm teardown. The child runner in `extensions/cleave/dispatcher.ts` does wait for `close`, but still kills only the immediate process, not a whole detached process group. This asymmetry makes the assessment path especially suspicious for repeated orphan accumulation: every timed-out assessment can be treated as finished by the parent while the child runtime is still shutting down—or failing to shut down—off to the side. The Activity Monitor screenshot showing many similarly sized `pi` processes is consistent with repeated timeout/abort leak events in these recursive runtime paths.

## Open Questions

*No open questions.*
