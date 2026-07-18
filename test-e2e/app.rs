// Codasaurus E2E Test — Rust

// 1. HALLUCINATED IMPORTS (crates that don't exist on crates.io)
use non_existent_crate_xyz::MagicTrait;
use completely_made_up_framework::prelude::*;

// 2. PHANTOM DEPS
use serde::{Deserialize, Serialize}; // serde NOT in Cargo.toml
use anyhow::Result;                   // anyhow NOT in Cargo.toml

// 3. SECRETS
const API_KEY: &str = "sk-or-v1-abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";
const DB_URL: &str = "postgresql://admin:SuperSecret123@prod-db.internal:5432/main";

// 4. TODO / FIXME
// TODO: implement error handling for edge cases
fn process(data: &str) -> String {
    // FIXME: unchecked unwrap
    data.to_string()
}

// 5. OVER-ENGINEERING
trait Serializer {}
struct JsonSerializer;
impl Serializer for JsonSerializer {}
struct XmlSerializer;
impl Serializer for XmlSerializer {}
fn create_serializer(t: &str) -> Box<dyn Serializer> {
    match t {
        "json" => Box::new(JsonSerializer),
        _ => Box::new(XmlSerializer),
    }
}

// 6. BOILERPLATE
fn validate_user(user: &std::collections::HashMap<String, String>) -> Result<(), String> {
    if !user.contains_key("name") { return Err("name required".into()); }
    if !user.contains_key("email") { return Err("email required".into()); }
    if !user.contains_key("age") { return Err("age required".into()); }
    if !user.contains_key("phone") { return Err("phone required".into()); }
    if !user.contains_key("city") { return Err("city required".into()); }
    if !user.contains_key("zip") { return Err("zip required".into()); }
    if !user.contains_key("country") { return Err("country required".into()); }
    Ok(())
}
fn validate_order(order: &std::collections::HashMap<String, String>) -> Result<(), String> {
    if !order.contains_key("id") { return Err("id required".into()); }
    if !order.contains_key("sku") { return Err("sku required".into()); }
    if !order.contains_key("qty") { return Err("qty required".into()); }
    if !order.contains_key("total") { return Err("total required".into()); }
    if !order.contains_key("status") { return Err("status required".into()); }
    if !order.contains_key("address") { return Err("address required".into()); }
    Ok(())
}

fn main() {
    println!("Codasaurus E2E Test");
}
