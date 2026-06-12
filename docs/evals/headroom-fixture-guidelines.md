# Headroom Fixture Guidance

Headroom fixtures are evidence for compression behavior. Treat every fixture as a small regression contract: it should prove one preservation/savings property and avoid carrying private or irrelevant data.

## Fixture classes

- `canonical_smoke`: small built-in fixtures that prove the evaluator still works.
- `adversarial`: crafted cases for known compressor failure modes.
- `dogfood`: generated local fixtures from real commands, docs, or repos under `.tmp/headroom/`.
- `regression`: minimized committed fixtures for bugs already found and fixed.

## What may be committed

Committed fixtures must be:

- small enough to review in a diff
- deterministic
- free of secrets, personal data, private URLs, private paths, access tokens, credentials, and proprietary content
- reduced to the minimum input needed to reproduce the behavior
- explicit about `required_facts`
- explicit about `min_savings_percent`
- explicit about `expected_compressed` when the behavior matters

Prefer committing a minimized synthetic fixture over a raw dogfood artifact.

## What must stay local

Keep these under ignored `.tmp/headroom/` paths:

- cloned public repositories
- raw command outputs
- full build logs
- converted PDF/EPUB text
- generated dogfood fixtures
- generated corpus manifests
- baseline comparison outputs

Do not move `.tmp/headroom/fixtures/*.json` directly into the repository without minimization and redaction.

## Minimization process

1. Generate local dogfood fixtures:

   ```bash
   just headroom-dogfood-eval
   ```

   or from a collector manifest:

   ```bash
   just headroom-corpus-collect docs/evals/headroom-corpus-local.example.json
   python3 scripts/headroom_dogfood.py .tmp/headroom/generated-corpus.json
   just headroom-eval --text --fixtures .tmp/headroom/fixtures --max-restored-facts 0
   ```

2. Identify the failing fixture and the missing/restored facts.
3. Copy only the minimum surrounding lines needed to reproduce the failure into a new committed regression fixture.
4. Replace private paths, hostnames, usernames, ids, and tokens with neutral placeholders.
5. Keep exact required facts that encode the behavior under test.
6. Run the strict gate:

   ```bash
   just headroom-regression
   ```

## Redaction checklist

Before committing a fixture, check:

- [ ] no credentials, API keys, OAuth tokens, cookies, or authorization headers
- [ ] no private hostnames, customer/project names, or internal repository URLs
- [ ] no personal emails, phone numbers, home directories, or usernames
- [ ] no absolute machine-local paths unless intentionally replaced with placeholders
- [ ] no vendored third-party corpus text beyond short fair-use snippets needed for the regression
- [ ] required facts are behavior-significant, not incidental noise
- [ ] the fixture fails before the intended fix or documents a previously observed dogfood failure

## Naming

Use kebab-case and include the failure shape:

- `json-schema-signal-critical-row.json`
- `threshold-plaintext-required-decision.json`
- `diff-function-anchor.json`

## Evidence quality

A good fixture makes the compressor choose correctly without evaluator repair:

```text
restored facts: 0
raw missing facts: <empty>
evaluated missing facts: <empty>
```

Fixtures that pass only because evaluator restoration appended required facts should usually become compressor tuning targets before they are accepted as regression evidence.
