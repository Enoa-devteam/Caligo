//! Export the executable action registry for capability coverage checks.
fn main() {
    println!(
        "{}",
        serde_json::to_string_pretty(caligo_core::onebot::ACTION_REGISTRY)
            .expect("action registry is serializable")
    );
}
