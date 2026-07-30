use anyhow::Result;
use serde::Serialize;

/// Print any serializable report as stable pretty JSON.
pub fn print_json(value: &impl Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
