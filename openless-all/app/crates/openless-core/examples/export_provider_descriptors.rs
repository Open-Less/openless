//! Regenerate the public provider metadata used by the browser-only UI preview.
//! Run from the app directory: cargo run --locked -p openless-core --example export_provider_descriptors

use openless_core::{domains::ProviderKind, provider_rules::provider_descriptors};
use std::{fs, path::Path};

fn main() -> anyhow::Result<()> {
    let catalog = serde_json::json!({
        "_generatedFrom": "openless-core::provider_rules::provider_descriptors",
        "asr": provider_descriptors(ProviderKind::Asr),
        "llm": provider_descriptors(ProviderKind::Llm),
        "omni": provider_descriptors(ProviderKind::Omni),
    });
    let destination = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../src/lib/ipc/provider-descriptors.generated.json");
    let mut bytes = serde_json::to_vec_pretty(&catalog)?;
    bytes.push(b'\n');
    fs::write(&destination, bytes)?;
    println!(
        "Provider preview catalog exported to {}",
        destination.display()
    );
    Ok(())
}
