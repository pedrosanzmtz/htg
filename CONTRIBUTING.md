# Contributing to HTG

Thank you for your interest in contributing to HTG!

## Branch Rules

> **IMPORTANT**: Direct pushes to `main` are not allowed.

All changes must go through a Pull Request, even for repository owners.

## Workflow

### 1. Create an Issue First

Before starting work, create a GitHub issue describing:
- What you want to add/fix
- Why it's needed
- Proposed approach (optional)

### 2. Create a Feature Branch

```bash
# Sync with main
git checkout main
git pull origin main

# Create a new branch
git checkout -b feature/issue-123-short-description
# or
git checkout -b fix/issue-123-short-description
```

**Branch naming convention:**
- `feature/issue-{number}-{description}` - for new features
- `fix/issue-{number}-{description}` - for bug fixes
- `docs/issue-{number}-{description}` - for documentation
- `refactor/issue-{number}-{description}` - for refactoring

### 3. Make Your Changes

- Write clean, documented code
- Add tests for new functionality
- Follow Rust conventions

### 4. Verify Your Changes

```bash
# Format code
cargo fmt

# Run clippy
cargo clippy -- -D warnings

# Run tests
cargo test

# Build release
cargo build --release
```

### 5. Commit Your Changes

Write clear commit messages:

```bash
git commit -m "feat(tile): add bilinear interpolation support

- Implement interpolation between 4 nearest points
- Add tests for edge cases
- Update documentation

Closes #123"
```

**Commit message format:**
```
type(scope): short description

Longer description if needed.

Closes #issue-number
```

**Types:** `feat`, `fix`, `docs`, `refactor`, `test`, `chore`

### 6. Push and Create PR

```bash
git push -u origin feature/issue-123-short-description
```

Then create a Pull Request on GitHub:
- Reference the issue: `Closes #123`
- Describe what changed and why
- Add screenshots if UI-related

### 7. Address Review Feedback

- Respond to comments
- Make requested changes
- Push additional commits

### 8. Merge

After approval, the PR will be merged to `main`.

## Code Standards

### Rust Style

- Use `cargo fmt` for formatting
- No clippy warnings (`cargo clippy -- -D warnings`)
- Document public APIs with `///` comments
- Use `Result` and proper error handling (no `.unwrap()` in library code)

### Testing

- Unit tests go in the same file as the code
- Integration tests go in `tests/` directory
- Aim for good coverage of edge cases

### Documentation

- Update README if adding user-facing features
- Add doc comments to public functions
- Update CHANGELOG for notable changes

## Questions?

Open an issue with the `question` label.
