#!/usr/bin/env python3
"""Local consistency checks for data/model-registry.json.

This is intentionally offline and stdlib-only. Use it after quick model catalog edits
so drift is caught before CI or upstream probes run.
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REGISTRY_PATH = ROOT / "data" / "model-registry.json"
PROVIDER_RS = ROOT / "core" / "crates" / "omegon" / "src" / "providers.rs"
# Some defaults intentionally target dynamic/local/compat providers whose models
# are not exhaustively enumerated in the static registry.
DYNAMIC_PROVIDERS = {
    "cerebras",
    "google-antigravity",
    "huggingface",
    "ollama",
    "openrouter",
    "anthropic",
    "ollama-cloud",
    "openai",
    "openai-codex",
}


def load_registry() -> dict:
    try:
        return json.loads(REGISTRY_PATH.read_text())
    except Exception as exc:  # noqa: BLE001 - script prints actionable error
        raise SystemExit(f"failed to parse {REGISTRY_PATH}: {exc}") from exc


def provider_names_from_rust() -> set[str]:
    text = PROVIDER_RS.read_text()
    names = set(re.findall(r'provider_id\s*=>\s*"([a-z0-9-]+)"', text))
    names.update(re.findall(r'"([a-z0-9-]+)"\s*=>\s*Some\(', text))
    return names


def main() -> int:
    data = load_registry()
    errors: list[str] = []
    warnings: list[str] = []

    models = data.get("models", [])
    if not isinstance(models, list):
        errors.append("models must be a list")
        models = []

    model_keys: set[tuple[str, str]] = set()
    model_ids_by_provider: dict[str, set[str]] = {}
    for idx, model in enumerate(models):
        if not isinstance(model, dict):
            errors.append(f"models[{idx}] must be an object")
            continue
        provider = model.get("provider")
        model_id = model.get("id")
        if not isinstance(provider, str) or not provider:
            errors.append(f"models[{idx}] missing provider")
            continue
        if not isinstance(model_id, str) or not model_id:
            errors.append(f"models[{idx}] missing id")
            continue
        key = (provider, model_id)
        if key in model_keys:
            errors.append(f"duplicate model entry: {provider}:{model_id}")
        model_keys.add(key)
        model_ids_by_provider.setdefault(provider, set()).add(model_id)

        for field in ["contextInput", "contextOutput"]:
            value = model.get(field)
            if not isinstance(value, int) or value <= 0:
                errors.append(f"{provider}:{model_id} has invalid {field}: {value!r}")
        if model.get("supportsReasoning") and "reasoning" not in model.get("capabilities", []):
            errors.append(
                f"{provider}:{model_id} supportsReasoning=true but lacks reasoning capability"
            )

    defaults = data.get("defaults", {})
    if not isinstance(defaults, dict):
        errors.append("defaults must be an object")
        defaults = {}
    for provider, model_id in sorted(defaults.items()):
        if provider not in DYNAMIC_PROVIDERS and (provider, model_id) not in model_keys:
            errors.append(f"defaults.{provider} references unknown model {model_id}")

    tiers = data.get("tiers", {})
    if not isinstance(tiers, dict):
        errors.append("tiers must be an object")
        tiers = {}
    for tier, providers in sorted(tiers.items()):
        if not isinstance(providers, dict):
            errors.append(f"tiers.{tier} must be an object")
            continue
        for provider, model_id in sorted(providers.items()):
            if provider not in DYNAMIC_PROVIDERS and (provider, model_id) not in model_keys:
                errors.append(f"tiers.{tier}.{provider} references unknown model {model_id}")

    routes = data.get("routes", [])
    if not isinstance(routes, list):
        errors.append("routes must be a list")
        routes = []
    for idx, route in enumerate(routes):
        if not isinstance(route, dict):
            errors.append(f"routes[{idx}] must be an object")
            continue
        provider = route.get("provider")
        pattern = route.get("modelIdPattern")
        ceiling = route.get("contextCeiling")
        tier = route.get("tier")
        if not isinstance(provider, str) or not provider:
            errors.append(f"routes[{idx}] missing provider")
        if not isinstance(pattern, str) or not pattern:
            errors.append(f"routes[{idx}] missing modelIdPattern")
        if not isinstance(ceiling, int) or ceiling <= 0:
            errors.append(f"routes[{idx}] invalid contextCeiling: {ceiling!r}")
        if tier not in tiers:
            errors.append(f"routes[{idx}] references unknown tier {tier!r}")

    rust_providers = provider_names_from_rust() | DYNAMIC_PROVIDERS
    registry_providers = set(model_ids_by_provider)
    for provider in sorted(defaults):
        if provider not in DYNAMIC_PROVIDERS and provider not in registry_providers:
            errors.append(f"defaults.{provider} has no model entries")
    for provider in sorted(registry_providers - rust_providers):
        warnings.append(f"registry provider {provider!r} was not recognized in providers.rs scan")

    result = {
        "registry": str(REGISTRY_PATH.relative_to(ROOT)),
        "models": len(model_keys),
        "providers": sorted(registry_providers),
        "errors": errors,
        "warnings": warnings,
    }
    print(json.dumps(result, indent=2))
    return 1 if errors else 0


if __name__ == "__main__":
    raise SystemExit(main())
