set positional-arguments

# List available commands
default:
    @just --list --unsorted

# Build the project
build:
    cargo build

# Run tests
test:
    cargo test

# Run dev CLI with local config
memex *args='-h':
    #!/usr/bin/env bash
    # OBJC_DISABLE_INITIALIZE_FORK_SAFETY works around macOS fork-after-init crash
    # CLAUDE_BINARY captures path before daemon fork (daemon loses PATH from nix shell)
    OBJC_DISABLE_INITIALIZE_FORK_SAFETY=YES \
    MEMEX_CONFIG_PATH=./.memex \
    CLAUDE_BINARY="$(which claude)" \
    cargo run --bin memex -- "$@"

# Clean build artifacts and dev database
clean:
    cargo clean
    rm -rf ./.memex/db ./.memex/*.sock ./.memex/*.pid
