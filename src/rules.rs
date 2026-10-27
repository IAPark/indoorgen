//! STUB (config worker): the rules worker replaces this module with the real furniture-rules layer.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Editable furniture rules.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct RuleSpec {}

/// apply the rules to the loaded furnishing program json
pub fn apply(_program: &mut serde_json::Value, _spec: &RuleSpec) {}
