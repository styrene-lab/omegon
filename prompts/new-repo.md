+++
id = "83069f20-6eb8-4486-a8c6-abfb7403ea0f"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# New Repository

Guide through creating a new repository with proper identity awareness and placement.

## Pre-flight: Identity Check

```bash
echo "Git user: $(git config user.name) <$(git config user.email)>"
gh auth status
gh api user -q '.login'
gh api user/orgs -q '.[].login' 2>/dev/null || echo "(no orgs or limited scope)"
```

Present identity to user. If wrong, fix before proceeding.

## Guided Questions

Ask the user:

1. **Repo name?**
2. **Personal account or org?** List available orgs from pre-flight.
3. **Starting point?** From scratch or existing local code?
4. **Visibility?** Private (recommended) or public?
5. **Description?**

## Execution

### From Scratch

```bash
GH_USER=$(gh api user -q '.login')
OWNER=${ORG:-$GH_USER}
gh repo create ${OWNER}/$1 --private --description "$2"
git clone https://github.com/${OWNER}/$1.git
```

### From Existing Code

```bash
cd /path/to/existing/code
[[ ! -d .git ]] && git init && git add . && git commit -m "feat: initial commit"
OWNER=${ORG:-$(gh api user -q '.login')}
gh repo create ${OWNER}/$1 --private --source=. --push
```

## Post-Creation Checklist

- [ ] Verify git remote identity matches intended account
- [ ] Add README.md if not present
- [ ] Set up branch protection if org repo
- [ ] Add collaborators if needed
