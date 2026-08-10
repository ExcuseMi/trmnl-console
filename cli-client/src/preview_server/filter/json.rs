//! `json` filter
use liquid_core::Result;
use liquid_core::Runtime;
use liquid_core::{Display_filter, Filter, FilterReflection, ParseFilter};
use liquid_core::{Value, ValueView};

#[derive(Clone, ParseFilter, FilterReflection)]
#[filter(
    name = "json",
    description = "Converts liquid value to JSON",
    parsed(JsonFilter)
)]
pub struct Json;

#[derive(Default, Debug, Display_filter)]
#[name = "json"]
struct JsonFilter {}

impl Filter for JsonFilter {
    fn evaluate(&self, input: &dyn ValueView, _runtime: &dyn Runtime) -> Result<Value> {
        let json_value = input.to_value();

        let json_string = serde_json::to_string(&json_value)
            .map_err(|e| liquid_core::Error::with_msg(e.to_string()))?;

        Ok(Value::scalar(json_string))
    }
}
