# Contributing to icetable

Thank you for your interest in contributing to icetable! This document provides guidelines and information for contributors.

## Getting Started

### Prerequisites

- Rust 1.75+ (uses edition 2024 features like `let-else`)
- Git

### Setup

```bash
git clone https://github.com/laketoolkit/icetable.git
cd icetable
cargo build
cargo test
```

### Project Structure

See [ARCHITECTURE.md](ARCHITECTURE.md) for a detailed overview of the codebase.

## Development Workflow

### 1. Find or Create an Issue

- Check [existing issues](https://github.com/laketoolkit/icetable/issues)
- For new features, open an issue first to discuss the approach
- For bugs, include reproduction steps and expected behavior

### 2. Create a Branch

```bash
git checkout -b feature/your-feature-name
# or
git checkout -b fix/your-bug-fix
```

### 3. Make Your Changes

- Follow the code style (enforced by `cargo clippy`)
- Add tests for new functionality
- Update documentation if needed

### 4. Run Quality Checks

```bash
# Format code
cargo fmt

# Run linter (must pass with no warnings)
cargo clippy -- -D warnings

# Run tests
cargo test

# Build release to catch optimization issues
cargo build --release
```

### 5. Commit Your Changes

Write clear commit messages:

```
fix: handle UTF-8 paths in vacuum command

The vacuum command failed when table paths contained non-ASCII
characters. This fix properly handles UTF-8 encoded paths by
using OsStr instead of assuming ASCII.
```

Commit message format:
- `feat:` - New feature
- `fix:` - Bug fix
- `docs:` - Documentation only
- `refactor:` - Code change that neither fixes a bug nor adds a feature
- `test:` - Adding or updating tests
- `chore:` - Maintenance tasks

### 6. Submit a Pull Request

- Push your branch to your fork
- Open a PR against `main`
- Fill out the PR template
- Wait for review

## Code Guidelines

### Rust Style

- Use `cargo fmt` for formatting
- All code must pass `cargo clippy -- -D warnings`
- Prefer `expect()` over `unwrap()` with descriptive messages
- Use `?` for error propagation
- Document public APIs with `///` doc comments

### Error Handling

```rust
// Good: Descriptive expect message
let timestamp = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .expect("system time is after UNIX epoch");

// Good: Proper error propagation
let metadata = self.storage.get(path, &opts).await?;

// Good: Convert errors with context
let content = String::from_utf8(bytes)
    .map_err(|e| Error::General(format!("Invalid UTF-8 in {}: {}", path, e)))?;

// Bad: Bare unwrap in production code
let data = file.read().unwrap();
```

### Architecture Principles

1. **Thin CLI, Fat Core**: Keep CLI code minimal, put logic in `src/core/`
2. **Trait-Based Design**: Use traits for extensibility
3. **Format-Agnostic**: Don't leak format-specific details to CLI
4. **No Over-Engineering**: Only add complexity when needed

### Testing

- Add unit tests for new functions
- Tests go in `#[cfg(test)]` modules at the bottom of files
- Use `unwrap()` freely in test code
- Name tests descriptively: `test_vacuum_removes_orphan_files`

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_timestamp_from_string() {
        let result = parse_timestamp("2024-01-15");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_timestamp_invalid_format() {
        let result = parse_timestamp("not-a-date");
        assert!(result.is_err());
    }
}
```

### Documentation

- Add doc comments to public items
- Include examples in doc comments when helpful
- Update ARCHITECTURE.md for structural changes

```rust
/// Parse a timestamp string into a DateTime.
///
/// Supports formats:
/// - Relative: `7d`, `24h`, `30m`, `2w`
/// - Absolute: `YYYY-MM-DD` or `YYYY-MM-DDTHH:MM:SS`
///
/// # Examples
///
/// ```
/// let dt = parse_timestamp("7d")?;  // 7 days ago
/// let dt = parse_timestamp("2024-01-15")?;
/// ```
pub fn parse_timestamp(s: &str) -> Result<DateTime<Utc>> {
    // ...
}
```

## Adding New Features

### New Command

1. Create `src/cli/commands/mycommand.rs`:

```rust
use clap::Args;
use crate::error::Result;

#[derive(Args)]
pub struct MyCommandArgs {
    /// Table path or identifier
    #[arg(required = true)]
    pub table: String,
}

pub struct MyCommand;

impl MyCommand {
    pub async fn execute(args: MyCommandArgs) -> Result<()> {
        // Implementation
        Ok(())
    }
}
```

2. Register in `src/cli/commands/mod.rs`
3. Add to CLI in `src/main.rs`

### New Table Format

1. Implement `PhysicalInspector` trait
2. Create factory implementing `PhysicalInspectorFactory`
3. Register in `PhysicalInspectorRegistry`
4. Add detection logic

### New Storage Backend

1. Implement `StorageBackend` trait
2. Add to `StorageBackendFactory`
3. Update URL scheme handling

## Pull Request Checklist

Before submitting:

- [ ] Code compiles without warnings (`cargo clippy -- -D warnings`)
- [ ] All tests pass (`cargo test`)
- [ ] Code is formatted (`cargo fmt`)
- [ ] No `unwrap()` in production code (use `expect()` with message)
- [ ] New public APIs are documented
- [ ] ARCHITECTURE.md updated if structure changed
- [ ] Commit messages follow convention

## Getting Help

- Open an issue for questions
- Tag `@midnattsol` for maintainer attention
- Check existing issues and PRs for similar topics

## Code of Conduct

Be respectful and constructive. We're all here to build something useful.

## License

By contributing, you agree that your contributions will be licensed under the same license as the project (see LICENSE file).

---

Questions? Contact the maintainer at juanjo@delasheras.dev
