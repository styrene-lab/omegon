# Fresh startup — Delta Spec

## ADDED Requirements

### Requirement: Profile-free startup uses the entrypoint defaults

Interactive startup must reach the editor without a blocking posture questionnaire,
a detected-tool inventory, or an implicit saved posture choice. Connection setup
remains available through /connect and configuration through /settings.

#### Scenario: Fresh lightweight entry
Given an isolated home and project without profile files or OMEGON_CHILD
When the operator launches om
Then the inline editor becomes usable without setup input
And its detail level is Active
And startup does not persist a defaultPosture selection

#### Scenario: Fresh workspace entry
Given an isolated home and project without profile files or OMEGON_CHILD
When the operator launches omegon
Then the fullscreen workspace becomes usable without setup input
And its detail level is Full
And no Fabricator, Architect, Explorator, or Devastator menu is printed
