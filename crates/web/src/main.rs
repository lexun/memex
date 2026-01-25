//! Memex web - CSR-only Leptos application
//!
//! The web UI is built with wasm-pack and embedded in the daemon binary.
//! This main.rs is only needed for wasm-pack builds.

#[cfg(not(feature = "ssr"))]
fn main() {
    // This is required for wasm-pack
}
