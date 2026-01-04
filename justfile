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
    @MEMEX_CONFIG_PATH=./.memex cargo run --bin memex -- {{args}}

# Clean build artifacts and dev database
clean:
    cargo clean
    rm -rf ./.memex/db ./.memex/*.sock ./.memex/*.pid
