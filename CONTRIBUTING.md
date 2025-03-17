# Contributing to LilGuy

Thank you for considering contributing to LilGuy! This document provides guidelines and information to make the contribution process smooth and effective.

## Code of Conduct

We are committed to providing a friendly, safe, and welcoming environment for all contributors. By participating in this project, you agree to abide by our [Code of Conduct](CODE_OF_CONDUCT.md). Please read it before contributing.

## Development Setup

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (stable channel)

### Building from Source

1. Clone the repository:
   ```bash
   git clone https://github.com/dylanwh/lilguy.git
   cd lilguy
   ```

2. Build the project:
   ```bash
   cargo build
   ```

3. Run tests:
   ```bash
   cargo test
   ```

4. Run the application:
   ```bash
   cargo run
   ```

## Development Guidelines

### Code Style

We use `rustfmt` to maintain consistent code formatting. Before submitting a PR, please format your code:

```bash
cargo fmt
```

We also use `clippy` to catch common mistakes and improve code quality:

```bash
cargo clippy
```

### Commit Messages

- Use clear, descriptive commit messages
- Start with a capitalized verb in the present tense (e.g., "Add feature" not "Added feature")
- Keep the first line under 72 characters
- Reference issue numbers if applicable

### Pull Requests

1. Fork the repository
2. Create a new branch for your feature or bugfix
3. Make your changes
4. Run tests and ensure they pass
5. Format your code with `cargo fmt`
6. Submit a pull request with a clear description of the changes

### Documentation

If you're adding new features or modifying existing ones, please update the relevant documentation.

## Release Process

The maintainers will handle the release process. If you're interested in helping with releases, please contact the project maintainers.

## Getting Help

If you have questions about contributing to LilGuy, feel free to:

- Open an issue on GitHub

Thank you for contributing to LilGuy!
