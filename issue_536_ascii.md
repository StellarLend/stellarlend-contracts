<!-- ghit#filepath: /Users/jagadeesh/1nonly/stellarlend/stellarlend-contracts/stellar-lend/contracts -->

## Description

Run once per org/repo so labels are **not default gray**. Ghit seeds issue **names** only; label **colors** live on GitHub.

### Commands (example ??? adjust org/repo)

```bash
# Area (blues / purples / green / yellow)
gh label create "contracts:hello-world" --color 1D76DB --force
gh label create "contracts:lending" --color 5319E7 --force
gh label create "contracts:amm" --color 0052CC --force
gh label create "contracts:common" --color 0E8A16 --force
gh label create "contracts:meta" --color FBCA04 --force

# Type
gh label create "type:feature" --color A2EEEF --force
gh label create "type:test" --color BFDADC --force
gh label create "type:docs" --color 0075CA --force
gh label create "type:task" --color C5DEF5 --force

# Theme
gh label create "theme:security" --color B60205 --force
gh label create "theme:governance" --color D93F0B --force
gh label create "theme:oracle" --color F9D0C4 --force
```

### Then seed

```bash
python3 ~/.local/bin/ghit_bootstrap_from_gh.py OWNER/REPO
ghit issues:seed stellar-lend/contracts/GHIT_CONTRACT_ISSUES.md --dry-run
ghit issues:seed stellar-lend/contracts/GHIT_CONTRACT_ISSUES.md
```

**Note:** This issue can be closed immediately after labels exist; it is only a checklist for maintainers.

## Requirements and context

- Must be secure, tested, and documented.
- This is a **process** task, not contract code.

## Suggested execution

### Fork the repo and create a branch

```bash
git checkout -b chore/github-labels-colored
```

### Implement changes

- Execute `gh label create` with the colors above (or your palette).
- Re-run seed after labels exist.

### Validate security assumptions

- Document trust boundaries, admin/guardian powers, and token transfer flows.
- Check reentrancy and authorization on every external call path.
- Prefer checked arithmetic and explicit bounds on all protocol parameters.

### Test and commit

- No code changes required.
- Paste `gh label list` output snippet in PR or comment.

## Example commit message

```
chore: document colored labels for ghit seeding
```

## Guidelines

- **Minimum 95% test coverage** for new or materially changed Rust modules.
- **Clear documentation** (module-level docs, user-facing `.md` in the contract crate, and security notes).
- **Timeframe:** 96 hours from assignment unless agreed otherwise.
- This issue is optional for tracking; you may close it without merging code.
