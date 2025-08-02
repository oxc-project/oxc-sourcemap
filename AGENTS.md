# AI Agents

This document tracks AI agents and their contributions to the oxc-sourcemap project.

## Purpose

This file serves to:

- Document AI agent contributions to the project
- Provide transparency about automated contributions
- Track the evolution of AI-assisted development in this codebase

## Build Instructions

To build and develop this project, you'll need:

### Prerequisites

- [Rust toolchain](https://rustup.rs/) (version specified in `rust-toolchain.toml`)
- [Just command runner](https://github.com/casey/just) for task automation

### Initial Setup

```bash
# Install development tools
just init
```

This installs the following tools via `cargo binstall`:
- `watchexec-cli` - File watcher for development
- `typos-cli` - Spell checker for code
- `cargo-shear` - Unused dependency remover
- `dprint` - Code formatter

### Development Workflow

```bash
# Run all checks (recommended before committing)
just ready

# Format code
just fmt

# Individual commands
cargo check --all-targets --all-features  # Check compilation
cargo test                                # Run tests
cargo clippy --all-targets --all-features # Run linter
typos                                     # Check for typos
```

The `just ready` command runs a comprehensive check including git status verification, spell checking, compilation checks, tests, linting, and formatting.

## Agent Contributions

### GitHub Copilot

- **Type**: Code completion and generation assistant
- **Usage**: Assists developers with code suggestions and automated fixes
- **Contributions**: Various code improvements and maintenance tasks

## Guidelines for AI Agent Contributions

When AI agents contribute to this project:

1. **Transparency**: All AI-generated contributions should be clearly documented
2. **Review**: AI contributions must undergo the same review process as human contributions
3. **Quality**: AI contributions must meet the same quality standards as human contributions
4. **Attribution**: Significant AI contributions should be properly attributed

## Contact

For questions about AI agent contributions or this documentation, please open an issue in the repository.
