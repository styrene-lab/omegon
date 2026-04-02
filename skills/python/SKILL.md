---
name: python
description: Python development guidance. Covers project setup (pyproject.toml, src/ layout), testing (pytest), linting (ruff), type checking (mypy), packaging, venv management, and CI/CD patterns. Use when creating, modifying, or debugging Python code.
---

# Python Development Skill

Conventions, tooling, and patterns for Python development.

## Core Conventions

- **Python 3.11+** minimum
- **src/ layout** (PEP 517) for all packages
- **pyproject.toml** is the single config file — no `.cfg`, `.ini`, or separate `.toml`
- **venv + pip** for environment management — no poetry, no conda
- **Makefile** (or justfile) wraps all dev commands
- **Editable install**: `pip install -e ".[dev]"` for development

## Project Scaffold

```
<project>/
├── pyproject.toml          # All config: build, deps, ruff, mypy, pytest
├── Makefile                # Dev workflow: test, lint, format, typecheck, validate
├── src/<package>/          # Source code (src/ layout)
│   ├── __init__.py         # __version__ = "0.1.0"
│   └── ...
├── tests/
│   ├── conftest.py         # Shared fixtures
│   └── test_*.py
└── .github/workflows/ci.yml
```

**Build backend choice:**
| Project Type | Backend |
|-------------|---------|
| Library / CLI tool | hatchling |
| Daemon / application | setuptools |

## Tooling Quick Reference

### Ruff (Linting + Formatting)

```toml
[tool.ruff]
line-length = 100
target-version = "py311"

[tool.ruff.lint]
select = ["E", "F", "I", "UP", "B", "SIM", "RUF"]
ignore = ["E501"]

[tool.ruff.lint.per-file-ignores]
"__init__.py" = ["F401"]

[tool.ruff.lint.isort]
known-first-party = ["<package>"]
```

```bash
ruff check .              # Lint
ruff check --fix .        # Lint + auto-fix
ruff format .             # Format (replaces black)
ruff format --check .     # Format check (CI)
```

### Mypy (Type Checking)

**New projects** — use `strict = true`.
**Projects with untyped deps** — use `ignore_missing_imports = true`, `disallow_untyped_defs = false`.

```toml
[tool.mypy]
python_version = "3.11"
strict = true
warn_return_any = true
warn_unused_ignores = true
```

### Pytest

```toml
[tool.pytest.ini_options]
testpaths = ["tests"]
addopts = "-v --strict-markers"
asyncio_mode = "auto"
markers = [
    "slow: marks tests as slow",
    "smoke: quick validation tests",
    "integration: requires external services",
]
```

**Key commands:**
```bash
pytest                          # All tests
pytest -m smoke                 # Quick validation only
pytest -m "not slow"            # Skip slow tests
pytest -x                       # Stop on first failure
pytest --lf                     # Rerun last failures
pytest -k "test_connect"        # Name pattern
pytest --cov=src --cov-report=term-missing  # Coverage
```

## Virtual Environment

```bash
python3 -m venv .venv
source .venv/bin/activate
pip install --upgrade pip
pip install -e ".[dev]"
```

## Testing Patterns

### Async Tests

With `asyncio_mode = "auto"`, async tests work without decorators:

```python
async def test_connection():
    client = await connect()
    assert client.is_connected
```

## Python Idioms

| Pattern | Convention |
|---------|-----------|
| Paths | `pathlib.Path`, never `os.path` |
| Data models | `dataclasses` for internal, Pydantic at validation boundaries |
| Imports | stdlib / third-party / local (ruff `I` rule enforces) |
| Async | `asyncio.gather` for concurrency, `asyncio.wait_for` for timeouts |
| Logging | `logging.getLogger(__name__)` |
| Entry points | `def main() -> int:` registered via `[project.scripts]` |
| Version | Single source in `__init__.py`, read by build backend |

## Packaging

```bash
python -m build       # Produces dist/*.whl + dist/*.tar.gz
twine upload dist/*   # Publish to PyPI
```

Use trusted publishing (OIDC) via `pypa/gh-action-pypi-publish` in CI.

## CI/CD Template

```yaml
name: CI
on: [push, pull_request]
jobs:
  test:
    runs-on: ubuntu-latest
    strategy:
      matrix:
        python-version: ["3.11", "3.12"]
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-python@v5
        with:
          python-version: ${{ matrix.python-version }}
      - run: pip install -e ".[dev]"
      - run: ruff format --check .
      - run: ruff check .
      - run: mypy src/
      - run: pytest --cov=src --cov-report=term-missing
```

## Common Gotchas

| Issue | Fix |
|-------|-----|
| Import from src/ fails | `pip install -e ".[dev]"` |
| `ModuleNotFoundError` in tests | Check `testpaths`, verify conftest.py exists |
| Async test hangs | Missing `await`, or add `asyncio.wait_for` timeout |
| Type stubs missing | Add `types-<pkg>` to dev deps, or `ignore_missing_imports` |
| Version mismatch | Single source: `__version__` in `__init__.py`, read by build backend |
</content>
</invoke>