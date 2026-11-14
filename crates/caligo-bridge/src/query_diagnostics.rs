//! Bounded, value-free diagnostics for failed directory query script output.
//!
//! This module only sanitizes diagnostics; it does not validate query results or
//! change their outcome. Callers may append the returned JSON to an internal
//! stage report. Never append the original script output or parser error.

use caligo_model::capability::MAX_QUERY_BYTES;
use serde_json::{json, Map, Value};

const MAX_DIAGNOSTIC_BYTES: usize = 2048;
const MAX_DIAGNOSTICS: usize = 8;
const MAX_FIELDS: usize = 16;
const MAX_COUNT: u64 = 2048;

const STAGES: &[&str] = &[
    "buddy_result",
    "buddy_category",
    "buddy_uid",
    "profile_result",
    "profile_item",
    "group_list_result",
    "group_list_event",
    "group_item",
    "group_detail_result",
    "group_detail_event",
    "group_detail_registration",
    "member_result",
    "member_snapshot",
    "member_item",
    "finish",
];

const TYPE_FIELDS: &[&str] = &[
    "value",
    "result",
    "errCode",
    "data",
    "buddyUids",
    "coreInfo",
    "uin",
    "nick",
    "remark",
    "groupCode",
    "groupName",
    "memberCount",
    "maxMember",
    "memberNum",
    "maxMemberNum",
    "finish",
    "infos",
    "ids",
    "uid",
    "role",
    "isDelete",
    "isRobot",
    "joinTime",
    "lastSpeakTime",
    "shutUpTime",
    "cardName",
    "sex",
    "age",
    "qqLevel",
    "memberRealLevel",
    "memberSpecialTitle",
    "max_member_count",
];

const COUNT_FIELDS: &[&str] = &[
    "count",
    "length",
    "size",
    "category_count",
    "buddy_count",
    "profile_count",
    "group_count",
    "member_count",
    "id_count",
    "key_count",
];

const TYPE_LABELS: &[&str] = &[
    "undefined",
    "null",
    "boolean",
    "number",
    "string",
    "object",
    "array",
    "map",
    "function",
    "bigint",
    "symbol",
];

// Exact matching only. In particular, never retain suffixes from JS exceptions.
const DIRECTORY_REASONS: &[(&str, &str)] = &[
    ("buddy result shape", "buddy_result_shape"),
    ("buddy category shape", "buddy_category_shape"),
    ("buddy uid shape", "buddy_uid_shape"),
    ("buddy uid type", "buddy_uid_type"),
    ("buddy list over budget", "buddy_list_over_budget"),
    ("profile result is not Map", "profile_result_not_map"),
    ("profile item missing", "profile_item_missing"),
    ("group shape", "group_shape"),
    ("group list result", "group_list_result"),
    ("group list over budget", "group_list_over_budget"),
    ("invalid group id", "invalid_group_id"),
    ("duplicate group id", "duplicate_group_id"),
    ("group name over budget", "group_name_over_budget"),
    ("group member count out of range", "group_member_count_out_of_range"),
    ("group capacity out of range", "group_capacity_out_of_range"),
    ("group capacity zero with members", "group_capacity_zero_with_members"),
    ("group member count exceeds capacity", "group_member_count_exceeds_capacity"),
    ("group detail shape", "group_detail_shape"),
    ("group detail result shape", "group_detail_result_shape"),
    ("group detail listener registration", "group_detail_listener_registration"),
    ("group detail listener cleanup", "group_detail_listener_cleanup"),
    ("member response shape", "member_response_shape"),
    ("incomplete member snapshot", "incomplete_member_snapshot"),
    ("invalid member count", "invalid_member_count"),
    ("invalid member timestamp", "invalid_member_timestamp"),
    ("invalid member text", "invalid_member_text"),
    ("invalid member key", "invalid_member_key"),
    ("invalid member identity", "invalid_member_identity"),
    ("unknown member role", "unknown_member_role"),
    ("response depth over budget", "response_depth_over_budget"),
    ("non-owned response", "non_owned_response"),
    ("response array over budget", "response_array_over_budget"),
    ("response object over budget", "response_object_over_budget"),
    ("response over budget", "response_over_budget"),
];

/// Sanitize raw query script output into at most 2,048 UTF-8 bytes of JSON.
///
/// Input schema: `{status:"invalid_response", reason?:string,
/// diagnostics?:[{stage:string, fields:{name:type_label_or_count}}]}`.
/// Output schema: `{failure:static_code, diagnostics:[{stage, fields}]}`.
/// Only allowlisted stages, field names, and type labels survive. Counts must
/// be unsigned JSON integers and saturate at 2,048. `member_count` is count-only;
/// native `memberCount` and `memberNum` are type-only. Unknown extras are ignored.
/// Up to eight input entries and sixteen accepted fields per entry are inspected
/// for inclusion; whole entries are omitted once the output byte cap is reached.
///
/// Known non-failure statuses return `None`. Oversize input is rejected before
/// parsing; invalid JSON and missing/unknown status return static decode codes.
/// Raw exception messages, values, identifiers and parser errors never escape.
/// This is not a substitute for the caller's query-result decoder.
pub(crate) fn sanitized_query_diagnostic(output: &str) -> Option<String> {
    if output.len() > MAX_QUERY_BYTES {
        return Some(render("input_over_budget", &[]));
    }
    let value: Value = match serde_json::from_str(output) {
        Ok(value) => value,
        Err(_) => return Some(render("decode_json", &[])),
    };
    match value.get("status").and_then(Value::as_str) {
        Some("invalid_response") => {}
        Some(
            "pending" | "success" | "unsupported" | "not_ready" | "timeout" | "cancelled"
            | "rejected",
        ) => {
            return None;
        }
        _ => return Some(render("decode_status", &[])),
    }

    let reason = value.get("reason").and_then(Value::as_str).unwrap_or("");
    let reason = reason.strip_prefix("Error: ").unwrap_or(reason);
    let failure = DIRECTORY_REASONS
        .iter()
        .find_map(|&(text, code)| (reason == text).then_some(code))
        .unwrap_or("invalid_response");
    let mut diagnostics = Vec::new();
    let mut result = render(failure, &diagnostics);
    if let Some(entries) = value.get("diagnostics").and_then(Value::as_array) {
        for entry in entries.iter().take(MAX_DIAGNOSTICS) {
            let Some(stage) = entry
                .get("stage")
                .and_then(Value::as_str)
                .and_then(|stage| allowlisted(STAGES, stage))
            else {
                continue;
            };
            let Some(fields) = entry.get("fields").and_then(Value::as_object) else {
                continue;
            };
            let fields: Map<String, Value> = fields
                .iter()
                .filter_map(|(name, value)| {
                    if let Some(name) = allowlisted(TYPE_FIELDS, name) {
                        let label = value.as_str().and_then(|v| allowlisted(TYPE_LABELS, v))?;
                        Some((name.to_owned(), Value::String(label.to_owned())))
                    } else {
                        let name = allowlisted(COUNT_FIELDS, name)?;
                        let count = value.as_u64()?.min(MAX_COUNT);
                        Some((name.to_owned(), Value::from(count)))
                    }
                })
                .take(MAX_FIELDS)
                .collect();
            diagnostics.push(json!({"stage": stage, "fields": fields}));
            let candidate = render(failure, &diagnostics);
            if candidate.len() > MAX_DIAGNOSTIC_BYTES {
                break;
            }
            result = candidate;
        }
    }
    Some(result)
}

fn allowlisted(allowed: &'static [&'static str], value: &str) -> Option<&'static str> {
    allowed.iter().copied().find(|&item| item == value)
}

fn render(failure: &str, diagnostics: &[Value]) -> String {
    json!({"failure": failure, "diagnostics": diagnostics}).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sanitized(value: Value) -> Value {
        let output = sanitized_query_diagnostic(&value.to_string()).unwrap();
        assert!(output.len() <= MAX_DIAGNOSTIC_BYTES);
        serde_json::from_str(&output).unwrap()
    }

    #[test]
    fn directory_reasons_are_exact_static_codes_with_optional_error_prefix() {
        for &(reason, code) in DIRECTORY_REASONS {
            for reason in [reason.to_owned(), format!("Error: {reason}")] {
                let result = sanitized(json!({"status":"invalid_response", "reason":reason}));
                assert_eq!(result, json!({"failure":code, "diagnostics":[]}));
            }
        }
    }

    #[test]
    fn known_other_statuses_have_no_failure_diagnostic() {
        for status in [
            "pending",
            "success",
            "unsupported",
            "not_ready",
            "timeout",
            "cancelled",
            "rejected",
        ] {
            assert!(sanitized_query_diagnostic(
                &json!({"status":status,"reason":"buddy result shape","diagnostics":[
                    {"stage":"buddy_result","fields":{"data":"array"}}
                ]})
                .to_string()
            )
            .is_none());
        }
    }

    #[test]
    fn shape_metadata_survives_without_directory_values() {
        let result = sanitized(
            json!({"status":"invalid_response", "reason":"Error: buddy result shape",
                "diagnostics":[{"stage":"buddy_result", "fields":{
                    "value":"object", "result":"number", "errCode":"undefined",
                    "data":"array", "category_count":3
                }}]
            }),
        );
        assert_eq!(
            result,
            json!({"failure":"buddy_result_shape", "diagnostics":[
                {"stage":"buddy_result", "fields":{
                    "value":"object", "result":"number", "errCode":"undefined",
                    "data":"array", "category_count":3
                }}
            ]})
        );
    }

    #[test]
    fn secrets_in_reasons_keys_values_extras_and_stages_never_escape() {
        let secret = "PRIVATE_TOKEN_u_synthetic_secret C:\\private\\identity.txt\nforged stage";
        let result = sanitized(json!({"status":"invalid_response", "reason":secret,
            "data":{"uid":secret}, "path":secret,
            "diagnostics":[
                {"stage":secret, "fields":{"uin":"string"}},
                {"stage":"profile_item", "extra":secret, "fields":{
                    (secret):"string", "uid":secret, "uin":"10000000000001",
                    "nick":secret, "remark":{"secret":secret}, "count":secret,
                    "coreInfo":"object"
                }}
            ]
        }));
        assert_eq!(
            result,
            json!({"failure":"invalid_response", "diagnostics":[
                {"stage":"profile_item", "fields":{"coreInfo":"object"}}
            ]})
        );
        assert!(!result.to_string().contains("PRIVATE_TOKEN"));
        assert!(!result.to_string().contains("10000000000001"));
    }

    #[test]
    fn injected_reason_suffix_and_forged_type_labels_are_rejected() {
        for reason in [
            "buddy result shape\nPRIVATE_TOKEN",
            "Error: buddy result shape PRIVATE_TOKEN",
            "Error: Error: buddy result shape",
        ] {
            let result = sanitized(json!({"status":"invalid_response", "reason":reason,
                "diagnostics":[{"stage":"buddy_result", "fields":{
                    "data":"array\nPRIVATE_TOKEN", "__proto__":"object",
                    "constructor":"function", "value":"map"
                }}]
            }));
            assert_eq!(
                result,
                json!({"failure":"invalid_response", "diagnostics":[
                    {"stage":"buddy_result", "fields":{"value":"map"}}
                ]})
            );
        }
    }

    #[test]
    fn counts_are_unsigned_integers_and_clamped_and_native_fields_are_types() {
        let result = sanitized(json!({"status":"invalid_response", "diagnostics":[
            {"stage":"member_snapshot", "fields":{
                "count":u64::MAX, "length":0, "size":2049, "member_count":2048,
                "id_count":-1, "key_count":1.5, "profile_count":"7",
                "buddy_count":true, "group_count":null,
                "memberCount":"number", "memberNum":12345,
                "max_member_count":"number"
            }}
        ]}));
        assert_eq!(
            result["diagnostics"][0]["fields"],
            json!({
                "count":2048, "length":0, "size":2048, "member_count":2048,
                "memberCount":"number", "max_member_count":"number"
            })
        );
    }

    #[test]
    fn every_approved_stage_type_field_count_field_and_label_is_supported() {
        for &stage in STAGES {
            for &name in TYPE_FIELDS {
                for &label in TYPE_LABELS {
                    let result = sanitized(json!({"status":"invalid_response", "diagnostics":[
                        {"stage":stage, "fields":{(name):label}}
                    ]}));
                    assert_eq!(result["diagnostics"][0]["stage"], stage);
                    assert_eq!(result["diagnostics"][0]["fields"][name], label);
                }
            }
        }
        for &name in COUNT_FIELDS {
            let result = sanitized(json!({"status":"invalid_response", "diagnostics":[
                {"stage":"finish", "fields":{(name):1}}
            ]}));
            assert_eq!(result["diagnostics"][0]["fields"][name], 1);
        }
    }

    #[test]
    fn malformed_metadata_is_ignored_without_preventing_static_failure() {
        for diagnostics in [
            Value::Null,
            json!("PRIVATE_TOKEN"),
            json!({}),
            json!([
                null, "PRIVATE_TOKEN", {}, {"stage":17,"fields":{}},
                {"stage":"buddy_result","fields":[]},
                {"stage":"buddy_result\nPRIVATE_TOKEN","fields":{}},
                {"stage":"finish","fields":{"value":"undefined"}}
            ]),
        ] {
            let result = sanitized(json!({"status":"invalid_response", "diagnostics":diagnostics}));
            assert_eq!(result["failure"], "invalid_response");
            assert!(!result.to_string().contains("PRIVATE_TOKEN"));
        }
    }

    #[test]
    fn entry_and_field_caps_and_total_byte_cap_preserve_valid_json() {
        let fields: Map<String, Value> = TYPE_FIELDS
            .iter()
            .map(|&name| (name.to_owned(), json!("undefined")))
            .collect();
        let diagnostics: Vec<_> = (0..100)
            .map(|_| {
                json!({
                    "stage":"group_detail_result", "fields":fields
                })
            })
            .collect();
        let result = sanitized(json!({"status":"invalid_response", "diagnostics":diagnostics}));
        let entries = result["diagnostics"].as_array().unwrap();
        assert!(!entries.is_empty());
        assert!(entries.len() < MAX_DIAGNOSTICS); // Long entries hit the byte cap first.
        for entry in entries {
            assert_eq!(entry["fields"].as_object().unwrap().len(), MAX_FIELDS);
        }
        let result = sanitized(json!({"status":"invalid_response", "diagnostics":
            vec![json!({"stage":"finish","fields":{}}); 100]
        }));
        assert_eq!(
            result["diagnostics"].as_array().unwrap().len(),
            MAX_DIAGNOSTICS
        );
    }

    #[test]
    fn input_byte_cap_is_checked_before_parsing_and_accepts_exact_limit() {
        let mut input = String::from(r#"{"status":"invalid_response"}"#);
        input.push_str(&" ".repeat(MAX_QUERY_BYTES - input.len()));
        assert_eq!(input.len(), MAX_QUERY_BYTES);
        assert_eq!(
            sanitized_query_diagnostic(&input).unwrap(),
            render("invalid_response", &[])
        );
        input.push(' ');
        assert_eq!(
            sanitized_query_diagnostic(&input).unwrap(),
            render("input_over_budget", &[])
        );
        // The cap counts UTF-8 bytes, not Unicode scalar values.
        let input = "密".repeat(MAX_QUERY_BYTES / 3 + 1);
        assert!(input.chars().count() < MAX_QUERY_BYTES);
        assert_eq!(
            sanitized_query_diagnostic(&input).unwrap(),
            render("input_over_budget", &[])
        );
    }

    #[test]
    fn decode_failures_are_static_and_never_echo_parser_input() {
        for input in ["", "PRIVATE_TOKEN", "{\"status\":\"PRIVATE_TOKEN", "{} {}"] {
            assert_eq!(
                sanitized_query_diagnostic(input).unwrap(),
                render("decode_json", &[])
            );
        }
        for input in [
            "null",
            "[]",
            "{}",
            r#"{"status":7}"#,
            r#"{"status":"PRIVATE_TOKEN"}"#,
        ] {
            assert_eq!(
                sanitized_query_diagnostic(input).unwrap(),
                render("decode_status", &[])
            );
        }
        let input = format!("{}0{}", "[".repeat(200), "]".repeat(200));
        assert_eq!(
            sanitized_query_diagnostic(&input).unwrap(),
            render("decode_json", &[])
        );
    }
}
