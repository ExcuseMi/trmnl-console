//! Dummy implementation for raw.
use liquid_core::Result;
use liquid_core::Runtime;
use liquid_core::{Display_filter, Filter, FilterReflection, ParseFilter};
use liquid_core::{Value, ValueView};

#[derive(Clone, ParseFilter, FilterReflection)]
#[filter(name = "raw", description = "Does nothing", parsed(RawFilter))]
pub struct Raw;

#[derive(Default, Debug, Display_filter)]
#[name = "raw"]
struct RawFilter {}

impl Filter for RawFilter {
    fn evaluate(&self, input: &dyn ValueView, _runtime: &dyn Runtime) -> Result<Value> {
        Ok(input.to_value())
    }
}
