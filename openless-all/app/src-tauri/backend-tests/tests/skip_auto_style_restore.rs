//! Exercise the production monitor replacement used by restore and rollback,
//! with RAII registrations instead of reserving a user's global keyboard keys.
#[path = "../../src/coordinator/skip_auto_style_monitor.rs"]
mod skip_auto_style_monitor;
#[path = "../../../crates/openless-core/tests/skip_auto_style.rs"]
mod core_contract;
