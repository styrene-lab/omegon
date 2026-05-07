+++
id = "fa8a3de1-0780-4d6e-ab75-97ff705b3011"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Cleave: Skill-Aware Dispatch & Review Loop

## Requirement: Skill Matching

Children receive skill directives based on their scope and annotations.

#### Scenario: Auto-match skills from file scope
Given a child with scope `["src/models/*.py", "tests/test_models.py"]`
When skills are matched
Then the child receives a directive to read the `python` skill

#### Scenario: Annotation overrides auto-match
Given a task group with `<!-- skills: rust, oci -->` annotation
When the child plan is generated
Then `child.skills` contains exactly `["rust", "oci"]` regardless of scope patterns

#### Scenario: No skills matched
Given a child with scope `["README.md"]` and no annotation
When skills are matched
Then the child receives no skill directives and proceeds normally

#### Scenario: Multiple skills from mixed scope
Given a child with scope `["src/app.py", "Containerfile", "k8s/deployment.yaml"]`
When skills are matched
Then the child receives directives for `python`, `oci`, and `k8s-operations`

#### Scenario: Skills annotation parsed from tasks.md
Given a tasks.md group header followed by `<!-- skills: python, k8s-operations -->`
When `parseTasksFile` processes the markdown
Then the TaskGroup's `skills` field contains `["python", "k8s-operations"]`

## Requirement: Skill Directive Injection

Child prompts include instructions to read matched skill files.

#### Scenario: Skill paths injected into child prompt
Given a child matched to skills `["python", "rust"]`
When the child task file is generated
Then the task file contains a "Specialist Skills" section listing skill file paths

#### Scenario: Skill directive is actionable
Given a child prompt with skill directives
When the child agent starts execution
Then the directive contains absolute paths to SKILL.md files that the agent can read

## Requirement: Model Tier Routing

Children are assigned execution models based on skill complexity hints.

#### Scenario: Default tier is sonnet
Given a child with no skill-based tier hints
When the execution model is resolved
Then the child runs with sonnet-tier model

#### Scenario: Skill with elevated complexity routes to opus
Given a child matched to a skill with `preferredTier: "opus"`
When the execution model is resolved
Then the child runs with opus-tier model

#### Scenario: Local model override preserved
Given `prefer_local: true` in cleave_run params
When the execution model is resolved
Then local model takes precedence over skill tier hints

## Requirement: Review Loop

After execution, an adversarial review evaluates child work.

#### Scenario: Clean execution skips review
Given `review: false` in cleave_run config
When a child completes execution
Then no review is performed and the child status is set from task file

#### Scenario: Review passes on first attempt
Given a child that completed execution with correct implementation
When the adversarial review runs
Then the verdict is PASS and no fix iteration occurs

#### Scenario: Warning issues trigger one fix iteration
Given a review that finds only W-severity issues
When the severity gate evaluates the verdict
Then exactly 1 fix iteration is dispatched before final review

#### Scenario: Critical issues get two fix attempts
Given a review that finds C-severity issues (non-security)
When the severity gate evaluates the verdict
Then up to 2 fix iterations are attempted before escalation

#### Scenario: Critical security issues escalate immediately
Given a review that finds C-severity issues tagged as security/data-loss
When the severity gate evaluates the verdict
Then no fix is attempted and the child escalates to the orchestrator

#### Scenario: Fix agent receives review issues
Given a review with verdict FAIL and issues [C1, W1, W2]
When the fix prompt is built
Then it contains the issue list verbatim with file paths and line numbers

## Requirement: Diminishing Returns Guardrail

The review loop detects when fix iterations are not converging.

#### Scenario: Churn detection bails on repeated issues
Given review round 1 finds issues [C1, W1, W2]
And review round 2 finds issues [C1, W1, W3]
When churn is evaluated (>50% reappearance threshold)
Then the loop bails with 2/3 = 67% reappearance and escalates

#### Scenario: Genuine progress continues iteration
Given review round 1 finds issues [C1, C2, W1, W2]
And review round 2 finds issues [W3]
When churn is evaluated
Then the loop continues (0% reappearance, all original issues resolved)

#### Scenario: Review iterations tracked in child state
Given a child that goes through 2 review cycles
When the final state is persisted
Then `child.reviewIterations` equals 2 and review history is accessible
