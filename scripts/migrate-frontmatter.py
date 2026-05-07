#!/usr/bin/env python3
"""Migrate YAML frontmatter from pre-Flynt git state into current TOML frontmatter.

Reads old YAML from git ref (a2c7c9b6~1), merges into current TOML as:
- title, status, tags → top-level Flynt fields
- id (slug) → aliases
- parent, related, dependencies, open_questions, etc → [data] table
- kind = "design_node" auto-assigned for design/ files

Usage:
    python3 scripts/migrate-frontmatter.py [--dry-run]
"""

import subprocess
import sys
import os
import re

OLD_REF = "a2c7c9b6~1"
DRY_RUN = "--dry-run" in sys.argv

# Fields that map to top-level Flynt frontmatter
TOP_LEVEL = {"title", "status", "tags"}
# Fields that become aliases (the old slug-style id)
ALIAS_FIELD = "id"
# Everything else goes into [data]
SKIP_FIELDS = {"jj_change_id"}  # internal VCS state, not worth preserving


def get_old_yaml(filepath):
    """Read YAML frontmatter from the old git ref."""
    try:
        content = subprocess.check_output(
            ["git", "show", f"{OLD_REF}:{filepath}"],
            stderr=subprocess.DEVNULL,
        ).decode("utf-8", errors="replace")
    except subprocess.CalledProcessError:
        return None

    if not content.startswith("---\n"):
        return None

    end = content.find("\n---\n", 4)
    if end == -1:
        return None

    yaml_text = content[4:end]

    # Simple YAML parser — handles the fields we know about
    result = {}
    current_key = None
    current_list = None

    for line in yaml_text.split("\n"):
        if not line.strip():
            continue

        # List continuation
        if line.startswith("  - ") and current_key:
            val = line[4:].strip().strip('"').strip("'")
            if current_list is not None:
                current_list.append(val)
            continue

        # Key: value
        match = re.match(r'^(\w+):\s*(.*)', line)
        if match:
            key = match.group(1)
            val = match.group(2).strip()

            # Save previous list
            if current_list is not None and current_key:
                result[current_key] = current_list

            current_key = key
            current_list = None

            if val == "" or val == "[]":
                current_list = []
            elif val.startswith("[") and val.endswith("]"):
                # Inline list: [a, b, c]
                items = val[1:-1].split(",")
                result[key] = [i.strip().strip('"').strip("'") for i in items if i.strip()]
            else:
                result[key] = val.strip('"').strip("'")

    # Save last list
    if current_list is not None and current_key:
        result[current_key] = current_list

    return result


def read_current_file(filepath):
    """Read the current file and split into frontmatter + body."""
    with open(filepath, "r") as f:
        content = f.read()

    if not content.startswith("+++\n"):
        return None, None, content

    end = content.find("\n+++\n", 4)
    if end == -1:
        return None, None, content

    toml_text = content[4:end]
    body = content[end + 5:]  # after closing +++\n

    return toml_text, "+++", body


def merge_frontmatter(toml_text, yaml_data, filepath):
    """Merge YAML fields into TOML frontmatter."""
    lines = toml_text.split("\n")
    new_lines = []
    has_data_section = False
    data_lines = []

    # Existing TOML keys
    existing_keys = set()
    for line in lines:
        stripped = line.strip()
        if "=" in stripped and not stripped.startswith("["):
            key = stripped.split("=")[0].strip()
            existing_keys.add(key)
        if stripped == "[data]":
            has_data_section = True

    # Copy existing TOML lines
    for line in lines:
        new_lines.append(line)

    # Add top-level fields
    if "title" in yaml_data and "title" not in existing_keys:
        title = yaml_data["title"]
        new_lines.insert(1, f'title = "{title}"')

    if "status" in yaml_data and "status" not in existing_keys:
        status = yaml_data["status"]
        new_lines.insert(2 if "title" in yaml_data else 1, f'status = "{status}"')

    if "tags" in yaml_data and isinstance(yaml_data["tags"], list) and yaml_data["tags"]:
        # Check if current tags are empty
        for i, line in enumerate(new_lines):
            if line.strip().startswith("tags = []"):
                tag_vals = ", ".join(f'"{t}"' for t in yaml_data["tags"])
                new_lines[i] = f"tags = [{tag_vals}]"
                break

    # Add old slug id as alias
    if ALIAS_FIELD in yaml_data and isinstance(yaml_data[ALIAS_FIELD], str):
        slug = yaml_data[ALIAS_FIELD]
        for i, line in enumerate(new_lines):
            if line.strip().startswith("aliases = []"):
                new_lines[i] = f'aliases = ["{slug}"]'
                break

    # Determine kind
    kind = None
    if filepath.startswith("design/"):
        kind = "design_node"
    elif filepath.startswith("docs/"):
        kind = "document"

    if kind and "kind" not in existing_keys:
        # Insert kind after id line
        for i, line in enumerate(new_lines):
            if line.strip().startswith("id = "):
                new_lines.insert(i + 1, f'kind = "{kind}"')
                break

    # Build [data] section from remaining YAML fields
    data_fields = {}
    for key, val in yaml_data.items():
        if key in TOP_LEVEL or key == ALIAS_FIELD or key in SKIP_FIELDS:
            continue
        data_fields[key] = val

    if data_fields and not has_data_section:
        new_lines.append("")
        new_lines.append("[data]")
        for key, val in sorted(data_fields.items()):
            if isinstance(val, list):
                if val:
                    items = ", ".join(f'"{v}"' for v in val)
                    new_lines.append(f'{key} = [{items}]')
                else:
                    new_lines.append(f'{key} = []')
            elif isinstance(val, str):
                # Escape quotes in value
                escaped = val.replace('"', '\\"')
                new_lines.append(f'{key} = "{escaped}"')

    return "\n".join(new_lines)


def process_file(filepath):
    """Process a single file."""
    yaml_data = get_old_yaml(filepath)
    if yaml_data is None:
        return False

    toml_text, delimiter, body = read_current_file(filepath)
    if toml_text is None:
        return False

    merged = merge_frontmatter(toml_text, yaml_data, filepath)

    new_content = f"+++\n{merged}\n+++\n{body}"

    if DRY_RUN:
        print(f"  [dry-run] {filepath}: {len(yaml_data)} YAML fields → merged")
        return True

    with open(filepath, "w") as f:
        f.write(new_content)
    return True


def main():
    import glob

    files = sorted(glob.glob("design/*.md") + glob.glob("docs/*.md"))
    migrated = 0
    skipped = 0

    for filepath in files:
        if process_file(filepath):
            migrated += 1
            if not DRY_RUN:
                print(f"  ✓ {filepath}")
        else:
            skipped += 1

    print(f"\n{migrated} files migrated, {skipped} skipped")
    if DRY_RUN:
        print("(dry run — no files modified)")


if __name__ == "__main__":
    main()
