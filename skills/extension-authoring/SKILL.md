---
name: extension-authoring
description: Use when the task explicitly involves authoring or debugging an Omegon native extension or its SDK integration.
activation: intent_detected
profile: [infra]
triggers: [omegon extension, extension authoring, extension sdk, execute_tool rpc]
---

# Omegon extension authoring

Use the separately published `omegon-extension` SDK and its current compatibility
guidance at https://github.com/styrene-lab/omegon-extension-rs. Do not recreate an
internal SDK crate or infer SDK contracts from host implementation details.

Read `references/authoring.md` relative to the **Base directory** from `skills_get` for
the versioned RPC, manifest, environment, trust, restart, and cleanup reference.
In an Omegon checkout, `docs/extensions.md` owns host runtime behavior. Check the
actual SDK version and admitted tool contracts before applying examples.

A manifest or SDK file does not authorize installation, execution, trust grants,
or lifecycle work. Apply only the procedure authorized by the task. Installation
and enablement do not grant executable trust; process separation is not a sandbox.
