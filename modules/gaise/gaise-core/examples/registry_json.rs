//! Dump the bundled model registry as JSON, with each entry's classified
//! capabilities attached. Used to regenerate the model tables in `wiki/`.
//!
//! ```powershell
//! cargo run -p gaise --example registry_json > registry.json
//! ```

use gaise_core::registry::ModelRegistry;

fn main() {
    let registry = ModelRegistry::bundled();
    let models: Vec<serde_json::Value> = registry
        .models
        .iter()
        .map(|m| {
            let mut value = serde_json::to_value(m).expect("registry entry serializes");
            if let Ok(classified) = m.classified() {
                value["classified"] = serde_json::json!({
                    "input": classified.input,
                    "output": classified.output,
                    "operations": classified.operations,
                    "tools": classified.tools,
                    "reasoning": classified.reasoning,
                    "features": classified.features,
                });
            }
            value["mapped_status"] = serde_json::to_value(m.status()).unwrap();
            value
        })
        .collect();
    let out = serde_json::json!({
        "schema_version": registry.schema_version,
        "audited_on": registry.audited_on,
        "providers": registry.providers,
        "models": models,
    });
    println!("{}", serde_json::to_string_pretty(&out).expect("json"));
}
