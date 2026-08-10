//! Dummy implementation for append_random.
use liquid_core::Result;
use liquid_core::Runtime;
use liquid_core::{Display_filter, Filter, FilterReflection, ParseFilter};
use liquid_core::{Value, ValueView};

#[derive(Clone, ParseFilter, FilterReflection)]
#[filter(
    name = "append_random",
    description = "Does nothing",
    parsed(AppendRandomFilter)
)]
pub struct AppendRandom;

#[derive(Default, Debug, Display_filter)]
#[name = "append_random"]
struct AppendRandomFilter {}

impl Filter for AppendRandomFilter {
    fn evaluate(&self, input: &dyn ValueView, _runtime: &dyn Runtime) -> Result<Value> {
        Ok(input.to_value())
    }
}
