// Tools exported from this module:
// - tool_known_facts_add
// - tool_known_facts_get

use std::str::FromStr;

use serde_json::Value;
use ulid::Ulid;

use crate::models::{FactScope, FactType, KnownFact};
use crate::store::CanopyStore;

use super::{ToolResult, get_str, validate_required_string};

pub fn tool_known_facts_add(
    store: &(impl CanopyStore + ?Sized),
    _agent_id: &str,
    args: &Value,
) -> ToolResult {
    let key = match validate_required_string(args, "key") {
        Ok(k) => k,
        Err(e) => return e,
    };
    let summary = match validate_required_string(args, "summary") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let established_by = match validate_required_string(args, "established_by") {
        Ok(a) => a,
        Err(e) => return e,
    };

    let fact_type = get_str(args, "fact_type")
        .and_then(|s| FactType::from_str(s).ok())
        .unwrap_or(FactType::Other);

    let scope = get_str(args, "scope")
        .and_then(|s| FactScope::from_str(s).ok())
        .unwrap_or(FactScope::Project);

    let hyphae_id = get_str(args, "hyphae_id");
    let task_id = get_str(args, "task_id");
    #[allow(clippy::cast_possible_truncation)]
    let confidence = args
        .get("confidence")
        .and_then(Value::as_f64)
        .map_or(1.0_f32, |v| v.clamp(0.0, 1.0) as f32);

    let fact_id = Ulid::new().to_string();
    match store.insert_known_fact(
        &fact_id,
        key,
        &fact_type,
        &scope,
        summary,
        hyphae_id,
        established_by,
        task_id,
        confidence,
    ) {
        Ok(fact) => ToolResult::json(&fact),
        Err(e) => ToolResult::error(format!("failed to store known fact: {e}")),
    }
}

pub fn tool_known_facts_get(
    store: &(impl CanopyStore + ?Sized),
    _agent_id: &str,
    args: &Value,
) -> ToolResult {
    let keys: Option<Vec<String>> = args.get("keys").and_then(Value::as_array).map(|arr| {
        arr.iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect()
    });

    let scope = get_str(args, "scope").and_then(|s| FactScope::from_str(s).ok());
    let task_id = get_str(args, "task_id");

    match store.query_known_facts(keys.as_deref(), scope.as_ref(), task_id) {
        Ok(facts) => {
            let response = KnownFactsResponse {
                count: facts.len(),
                facts,
            };
            ToolResult::json(&response)
        }
        Err(e) => ToolResult::error(format!("failed to query known facts: {e}")),
    }
}

#[derive(serde::Serialize)]
struct KnownFactsResponse {
    count: usize,
    facts: Vec<KnownFact>,
}
