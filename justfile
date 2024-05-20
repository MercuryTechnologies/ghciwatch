# Build `ghciwatch`
build:
    cargo build

# Run tests, including integration tests
test *OPTIONS:
    cargo nextest run

# Generate `docs/cli.md`
_docs_cli_md:
    # It would be really nice if `mdbook` supported running commands before
    # rendering.
    cargo run --features clap-markdown -- --generate-markdown-help > docs/cli.md

# Build the user manual to `docs/book`
docs: _docs_cli_md
    mdbook build docs

# Serve the user manual on `http://localhost:3000`
serve-docs: _docs_cli_md
    mdbook serve docs

# Generate API documentation with rustdoc (like CI)
api-docs:
    cargo doc --document-private-items --no-deps --workspace

# Lint Rust code with clippy
lint:
    cargo clippy

# Format Rust code
format:
    cargo fmt
