//! The production Cocoa setter and ownership check on an isolated cloud runner.
//! This fixture is never run on the user's machine.
#![cfg(target_os = "macos")]
#[allow(dead_code)]
#[path = "../../src/insertion/owned_clipboard.rs"]
mod owned_clipboard;
