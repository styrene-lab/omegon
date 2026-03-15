#!/usr/bin/env bash
# Publish pi-mono fork packages to npm if their versions don't match what's on the registry.
# Called by CI before publishing omegon itself.
# Also rewrites file: refs in both omegon's package.json and pi-mono's internal cross-deps.
set -euo pipefail

PACKAGES=("ai" "tui" "agent" "coding-agent")
SCOPED_NAMES=("@cwilson613/pi-ai" "@cwilson613/pi-tui" "@cwilson613/pi-agent-core" "@cwilson613/pi-coding-agent")
BASE="vendor/pi-mono/packages"

# Phase 1: Rewrite file: refs in pi-mono packages to pinned versions (for npm publish)
echo "Phase 1: Rewriting internal file: refs to pinned versions..."
for i in "${!PACKAGES[@]}"; do
  pkg="${PACKAGES[$i]}"
  dir="$BASE/$pkg"
  [ ! -d "$dir" ] && continue

  node -e "
    const fs = require('fs');
    const pkgJson = JSON.parse(fs.readFileSync('$dir/package.json', 'utf8'));
    let changed = false;
    for (const section of ['dependencies', 'peerDependencies']) {
      if (!pkgJson[section]) continue;
      for (const [name, ver] of Object.entries(pkgJson[section])) {
        if (typeof ver === 'string' && ver.startsWith('file:')) {
          // Resolve the target package.json to get its version
          const targetDir = require('path').resolve('$dir', ver.replace('file:', ''));
          try {
            const targetPkg = JSON.parse(fs.readFileSync(targetDir + '/package.json', 'utf8'));
            pkgJson[section][name] = targetPkg.version;
            changed = true;
            console.log('  ' + name + ': file: → ' + targetPkg.version);
          } catch (e) {
            console.error('  WARNING: could not resolve ' + name + ' at ' + targetDir);
          }
        }
      }
    }
    if (changed) {
      fs.writeFileSync('$dir/package.json', JSON.stringify(pkgJson, null, '\t') + '\n');
    }
  "
done

# Phase 2: Publish packages in dependency order (ai, tui, agent-core, then coding-agent)
echo ""
echo "Phase 2: Publishing packages..."
for i in "${!PACKAGES[@]}"; do
  pkg="${PACKAGES[$i]}"
  name="${SCOPED_NAMES[$i]}"
  dir="$BASE/$pkg"

  if [ ! -d "$dir" ]; then
    echo "⚠ Skipping $name — $dir not found"
    continue
  fi

  local_ver=$(node -p "require('./$dir/package.json').version")
  npm_ver=$(npm view "$name" version 2>/dev/null || echo "0.0.0")

  if [ "$local_ver" = "$npm_ver" ]; then
    echo "✓ $name@$local_ver already published"
  else
    echo "→ Publishing $name@$local_ver (registry has $npm_ver)"
    (cd "$dir" && npm publish --access public)
  fi
done

# Phase 3: Rewrite omegon package.json — file: refs → registry versions
echo ""
echo "Phase 3: Rewriting omegon package.json for registry publish..."
for i in "${!PACKAGES[@]}"; do
  pkg="${PACKAGES[$i]}"
  name="${SCOPED_NAMES[$i]}"
  dir="$BASE/$pkg"
  ver=$(node -p "require('./$dir/package.json').version")

  node -e "
    const fs = require('fs');
    const pkg = JSON.parse(fs.readFileSync('package.json', 'utf8'));
    if (pkg.dependencies['$name']?.startsWith('file:')) {
      pkg.dependencies['$name'] = '$ver';
      fs.writeFileSync('package.json', JSON.stringify(pkg, null, '\t') + '\n');
      console.log('  ✓ $name → $ver');
    } else {
      console.log('  - $name already pinned');
    }
  "
done

echo ""
echo "Done. pi-mono packages published and package.json updated for omegon publish."
