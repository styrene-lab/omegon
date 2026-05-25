---
id: session-resume-degradation-visibility
title: "Resume Degradation Visibility"
status: seed
tags: [session, resume, context, upgrade]
open_questions: []
dependencies: []
related: []
---

# Resume Degradation Visibility

## Overview

Make lossy resume behavior visible and actionable. Current resume intentionally keeps only a recent tail and folds older messages into a compact synthetic summary; this protects context budget but can look like dropped context after upgrades. Track follow-up work to surface dropped counts, preserved evidence, and recovery affordances without blocking the atomic persistence patch.
