//! Bounded media diagnostics for the local research report.
//! Never forward the original D8 JSON or arbitrary property values to the report.

use serde_json::{json, Map, Value};

#[cfg(feature = "research")]
pub(crate) fn sanitize_text_send_trace(input: &str) -> Option<String> {
    if input.len() > 524_288 {
        return None;
    }
    let batch: Value = serde_json::from_str(input).ok()?;
    let stages = batch.get("textSendTrace")?.as_array()?;
    let allowed = ["registered", "submit_enter", "submit_return", "promise_accepted",
        "promise_rejected", "promise_unknown", "matched_add", "matched_update",
        "readback_matched", "readback_mismatch", "settled"];
    let trace: Vec<&str> = stages.iter().take(32).filter_map(Value::as_str)
        .filter(|stage| allowed.contains(stage)).collect();
    (!trace.is_empty()).then(|| json!({"stages":trace}).to_string())
}

pub(crate) fn sanitize_media_download(input: &str) -> Option<String> {
    if input.len() > 524_288 {
        return None;
    }
    let batch: Value = serde_json::from_str(input).ok()?;
    let entries = batch.get("mediaDiagnostics")?.as_array()?;
    let mut output = Vec::new();
    for entry in entries.iter().take(8) {
        let Some(media @ ("image" | "voice")) = entry.get("media").and_then(Value::as_str) else {
            continue;
        };
        let Some(stage) = entry.get("stage").and_then(Value::as_str) else {
            continue;
        };
        if ![
            "dispatch",
            "promise",
            "promise_rejected",
            "dispatch_threw",
            "callback",
            "validated",
            "validation_failed",
            "readback",
            "readback_matched",
        ]
        .contains(&stage)
        {
            continue;
        }
        let mut row = json!({"media":media,"stage":stage});
        if stage == "promise" {
            if let Some(result) = entry.get("result").and_then(Value::as_str) {
                if ["accepted", "rejected", "unknown"].contains(&result) {
                    row["result"] = json!(result);
                }
            }
        }
        if stage == "callback" {
            if let Some(chat) = entry.get("callback_chat").and_then(Value::as_str) {
                if [
                    "private",
                    "group",
                    "zero",
                    "string_private",
                    "string_group",
                    "missing",
                    "null",
                    "other",
                ]
                .contains(&chat)
                {
                    row["callback_chat"] = json!(chat);
                }
            }
            for field in [
                "message_matches",
                "element_matches",
                "chat_matches",
                "path_matches",
                "registered_path_matches",
                "error_is_string",
                "error_is_zero",
                "slot_matches",
                "pending",
            ] {
                if let Some(value) = entry.get(field).and_then(Value::as_bool) {
                    row[field] = json!(value);
                }
            }
        }
        output.push(row);
    }
    (!output.is_empty()).then(|| json!({"download_stages":output}).to_string())
}

pub(crate) fn sanitize_image_shapes(input: &str) -> Option<String> {
    if input.len() > 524_288 {
        return None;
    }
    let batch: Value = serde_json::from_str(input).ok()?;
    let shapes = batch.get("imageShapes")?.as_array()?;
    let mut output = Vec::new();
    for shape in shapes.iter().take(8) {
        let mut row = Map::new();
        for (field, allowed) in [
            ("source", &["recv", "update", "add_send"][..]),
            ("chat", &["private", "group", "other"][..]),
            ("arity", &["single", "multiple"][..]),
            ("pic_type", &["png", "default", "other"][..]),
            ("subtype", &["ordinary", "other"][..]),
        ] {
            if let Some(value) = shape.get(field).and_then(Value::as_str) {
                if allowed.contains(&value) {
                    row.insert(field.into(), json!(value));
                }
            }
        }
        for field in [
            "wire_strings_valid",
            "message_types_valid",
            "optional_fields_valid",
            "peer_matches",
            "self_sent",
            "element_id_valid",
            "size_valid",
            "dimensions_valid",
            "md5_valid",
            "text_present",
            "numeric_element_type",
        ] {
            if let Some(value) = shape.get(field).and_then(Value::as_bool) {
                row.insert(field.into(), json!(value));
            }
        }
        if !row.is_empty() {
            output.push(Value::Object(row));
        }
    }
    if output.is_empty() {
        return None;
    }
    let mut report = json!({"image_shapes":output});
    for field in ["rejected", "overflow"] {
        if let Some(count) = batch.get(field).and_then(Value::as_u64) {
            report[field] = json!(count.min(2048));
        }
    }
    if let Some(items) = batch.get("items").and_then(Value::as_array) {
        report["accepted_records"] = json!(items.len().min(128));
    }
    Some(report.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "research")]
    #[test]
    fn text_send_trace_accepts_only_bounded_fixed_labels() {
        let input = json!({"textSendTrace":["registered","sensitive-id",{"stage":"matched_add"},
            "promise_accepted","readback_matched","settled"],"message":"secret"}).to_string();
        let output = sanitize_text_send_trace(&input).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&output).unwrap(),
            json!({"stages":["registered","promise_accepted","readback_matched","settled"]}));
        let many = json!({"textSendTrace":vec!["matched_add";40]}).to_string();
        let value: Value = serde_json::from_str(&sanitize_text_send_trace(&many).unwrap()).unwrap();
        assert_eq!(value["stages"].as_array().unwrap().len(),32);
        assert_eq!(sanitize_text_send_trace(&" ".repeat(524_289)),None);
    }

    #[test]
    fn media_diagnostics_have_only_bounded_labels_and_comparison_flags() {
        let input = json!({"mediaDiagnostics":[
            {"media":"voice","stage":"callback","path_matches":false,"message_matches":"SECRET","path":"SECRET","callback_chat":"missing"},
            {"media":"image","stage":"callback","callback_chat":"SECRET"},
            {"media":"voice","stage":"promise","result":"accepted","message":"SECRET"},
            {"media":"voice","stage":"SECRET"}, {"media":"SECRET","stage":"callback"},
            {"media":"voice","stage":"validation_failed","error":"SECRET"},
            {"media":"voice","stage":"readback_matched","peerUid":"SECRET"}
        ]})
        .to_string();
        let output = sanitize_media_download(&input).unwrap();
        assert!(!output.contains("SECRET"));
        assert_eq!(
            serde_json::from_str::<Value>(&output).unwrap(),
            json!({"download_stages":[
                {"media":"voice","stage":"callback","path_matches":false,"callback_chat":"missing"},
                {"media":"image","stage":"callback"},
                {"media":"voice","stage":"promise","result":"accepted"},
                {"media":"voice","stage":"validation_failed"},
                {"media":"voice","stage":"readback_matched"}
            ]})
        );
        let many = json!({"mediaDiagnostics":vec![json!({"media":"voice","stage":"dispatch"});100]}).to_string();
        let output: Value = serde_json::from_str(&sanitize_media_download(&many).unwrap()).unwrap();
        assert_eq!(output["download_stages"].as_array().unwrap().len(), 8);
        assert_eq!(sanitize_media_download(&" ".repeat(524_289)), None);
        assert_eq!(sanitize_media_download("NOT_ARMED"), None);
    }

    #[test]
    fn hostile_values_paths_and_identities_never_reach_report() {
        let input = json!({"items":[{"sender":"SECRET"}],"imageShapes":[{
            "source":"recv", "chat":"private", "pic_type":"default",
            "peer_matches":false, "md5_valid":true, "text_present":"SECRET",
            "arity":"SECRET", "path":"SECRET", "senderUin":"SECRET",
            "message_types_valid":42, "element_id_valid":true
        }]})
        .to_string();
        let output = sanitize_image_shapes(&input).unwrap();
        assert!(!output.contains("SECRET"));
        assert_eq!(
            serde_json::from_str::<Value>(&output).unwrap(),
            json!({"accepted_records":1,"image_shapes":[{
                "source":"recv", "chat":"private", "pic_type":"default",
                "peer_matches":false, "md5_valid":true, "element_id_valid":true
            }]})
        );
    }

    #[test]
    fn bounds_and_unrecognized_inputs_produce_no_unbounded_output() {
        let input = json!({"imageShapes":vec![json!({"source":"recv"});100],
            "rejected":u64::MAX,"overflow":"SECRET","items":vec![json!(null);200]})
        .to_string();
        let output: Value = serde_json::from_str(&sanitize_image_shapes(&input).unwrap()).unwrap();
        assert_eq!(output["image_shapes"].as_array().unwrap().len(), 8);
        assert_eq!(output["rejected"], 2048);
        assert_eq!(output["accepted_records"], 128);
        assert!(output.get("overflow").is_none());
        for input in ["NOT_ARMED", "{}", r#"{"imageShapes":[{"secret":"value"}]}"#] {
            assert_eq!(sanitize_image_shapes(input), None);
        }
        assert_eq!(sanitize_image_shapes(&" ".repeat(524_289)), None);
    }
}
