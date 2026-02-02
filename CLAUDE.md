# CLAUDE.md - COSMIC Connect Development Guidelines

##  MANDATORY Pre-Commit Checks

### REQUIRED: Two-Step Pre-Commit Process

Before creating **ANY** git commit, you **MUST** run both checks:

#### Step 1: COSMIC Code Review (REQUIRED)

```bash
@cosmic-code-reviewer /pre-commit-check
```

This verifies:

- No hard-coded colors, dimensions, or radii
- No `.unwrap()` or `.expect()` calls
- Proper error handling and logging
- Theme integration correctness
- COSMIC Desktop best practices
- Architecture patterns

#### Step 2: Code Simplification (REQUIRED)

```bash
Run code-simplifier:code-simplifier agent on the changes we made
```

This ensures:

- Code clarity and consistency
- Removal of redundant patterns
- Better Rust idioms
- Improved maintainability
- Alignment with codebase conventions

**Exception:** Skip only if changes are trivial (typo fixes, comments only).

### Why Both Checks?

- **@cosmic-code-reviewer**: Catches COSMIC-specific issues (theming, widgets, patterns)
- **code-simplifier**: Optimizes Rust code quality and idioms
- Together they ensure high-quality, maintainable COSMIC Desktop code

## Development Standards

### Code Style

- Follow Rust idioms and conventions
- Use existing patterns from the codebase
- Prefer clarity over cleverness
- Keep functions focused and single-purpose

### Testing

- Write comprehensive unit tests for new plugins
- Test both success and error paths
- Use `create_test_device()` helper for consistency

### Documentation

- Document public APIs with doc comments
- Include protocol specifications in module docs
- Add usage examples for complex features
- Keep TODO comments with clear descriptions

### Commit Messages

- Use conventional commit format: `feat(scope): description`
- Include detailed body for complex changes
- Add `Co-Authored-By: Claude Sonnet 4.5 <noreply@anthropic.com>`

## Project Architecture

### Plugin Development

- Each plugin in `cosmic-connect-core/src/plugins/`
- Implement both `Plugin` and `PluginFactory` traits
- Add config flag in `cosmic-connect-daemon/src/config.rs`
- Register factory in `cosmic-connect-daemon/src/main.rs`
- Follow existing plugin patterns (ping, battery, etc.)

### Testing Strategy

- Unit tests in plugin modules
- Integration tests for daemon components
- Test with real devices when possible
- Document manual testing procedures

## Protocol References

- [Valent Protocol Documentation](https://valent.andyholmes.ca/documentation/protocol.html)
- [KDE Connect Community Wiki](https://community.kde.org/KDEConnect)
- [MPRIS2 Specification](https://specifications.freedesktop.org/mpris/latest/)

---

_This project implements COSMIC Connect - a device connectivity solution for COSMIC Desktop_
