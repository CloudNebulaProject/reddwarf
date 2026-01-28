# Reddwarf Development Guide

## Prerequisites

- Rust 1.84 or later
- Docker (for multi-arch builds)
- kubectl (for testing)

## Development Workflow

### Building

```bash
# Build all crates
cargo build --workspace

# Build specific crate
cargo build -p reddwarf-core

# Build release
cargo build --release
```

### Testing

```bash
# Run all tests
cargo test --workspace

# Run tests for specific crate
cargo test -p reddwarf-storage

# Run with output
cargo test --workspace -- --nocapture

# Run specific test
cargo test test_resource_key
```

### Code Quality

```bash
# Run clippy (linting)
cargo clippy --workspace -- -D warnings

# Format code
cargo fmt --all

# Check formatting
cargo fmt --all -- --check
```

## Project Structure

### Core Crates

#### reddwarf-core
Foundation types and traits:
- `error.rs` - Error types with miette diagnostics
- `types.rs` - ResourceKey, GroupVersionKind, ResourceVersion
- `resources/mod.rs` - Resource trait and implementations
- `lib.rs` - Public API and serialization helpers

#### reddwarf-storage
Storage abstraction and redb backend:
- `error.rs` - Storage error types
- `kv.rs` - KVStore and Transaction traits
- `encoding.rs` - Key encoding and indexing
- `redb_backend.rs` - redb implementation
- `lib.rs` - Public API

#### reddwarf-versioning
DAG-based versioning:
- `error.rs` - Versioning error types
- `commit.rs` - Commit and Change types
- `conflict.rs` - Conflict detection
- `store.rs` - VersionStore implementation
- `lib.rs` - Public API

## Design Principles

### Error Handling
- Use `miette` for all user-facing errors
- Include helpful diagnostic messages
- Suggest fixes in error messages
- Example:
```rust
#[error("Pod validation failed: {details}")]
#[diagnostic(
    code(reddwarf::validation_failed),
    help("Ensure the pod spec has at least one container")
)]
ValidationFailed { details: String }
```

### Type Safety
- Leverage Rust's type system
- Use `k8s-openapi` types where possible
- Implement custom traits for extensions
- Avoid stringly-typed APIs

### Testing
- Unit tests in each module
- Integration tests in `tests/` directory
- Test edge cases and error conditions
- Use `tempfile` for temporary test data

### Documentation
- Document all public APIs
- Include examples in doc comments
- Keep README.md up to date
- Document architectural decisions

## Coding Standards

### Naming Conventions
- `snake_case` for functions and variables
- `PascalCase` for types and traits
- `SCREAMING_SNAKE_CASE` for constants
- Descriptive names over abbreviations

### Module Organization
- One major type per file
- Group related functionality
- Keep files under 500 lines
- Use sub-modules for complex features

### Dependencies
- Minimize dependencies
- Prefer pure Rust crates
- Pin versions for stability
- Document dependency rationale

## Testing Guidelines

### Unit Tests
- Test each function independently
- Mock external dependencies
- Cover happy path and error cases
- Keep tests fast (<100ms)

### Integration Tests
- Test component interactions
- Use real storage backend
- Test end-to-end workflows
- Can be slower but thorough

### Test Organization
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_functionality() {
        // Arrange
        let input = setup_test_data();

        // Act
        let result = function_under_test(input);

        // Assert
        assert_eq!(result, expected);
    }
}
```

## Debugging

### Enable Logging
```bash
# Debug level
RUST_LOG=debug cargo test

# Trace level
RUST_LOG=trace cargo run

# Specific module
RUST_LOG=reddwarf_storage=debug cargo test
```

### Using redb Inspector
```bash
# Install redb-inspector
cargo install redb-inspector

# Inspect database
redb-inspector /path/to/database.redb
```

## Performance

### Profiling
```bash
# CPU profiling
cargo flamegraph

# Memory profiling
cargo valgrind
```

### Benchmarking
```bash
# Run benchmarks
cargo bench

# Compare with baseline
cargo bench --bench storage_bench
```

## Release Process

### Version Bumping
1. Update version in `Cargo.toml`
2. Update CHANGELOG.md
3. Create git tag
4. Build release artifacts

### Cross-Compilation
```bash
# Add target
rustup target add aarch64-unknown-linux-gnu

# Build for target
cargo build --release --target aarch64-unknown-linux-gnu
```

## Troubleshooting

### Common Issues

#### Compilation Errors
- Ensure Rust toolchain is up to date: `rustup update`
- Clear build cache: `cargo clean`
- Check for incompatible dependencies: `cargo tree`

#### Test Failures
- Run specific test: `cargo test test_name -- --exact`
- Show test output: `cargo test -- --nocapture`
- Run serially: `cargo test -- --test-threads=1`

#### Database Issues
- Check file permissions
- Ensure sufficient disk space
- Use fresh database for tests

## Resources

- [Rust Book](https://doc.rust-lang.org/book/)
- [k8s-openapi Docs](https://docs.rs/k8s-openapi/)
- [redb Docs](https://docs.rs/redb/)
- [miette Docs](https://docs.rs/miette/)
- [Kubernetes API Conventions](https://github.com/kubernetes/community/blob/master/contributors/devel/sig-architecture/api-conventions.md)
