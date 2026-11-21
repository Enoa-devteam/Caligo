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
    let generation = batch.get("textSendTraceGeneration")?.as_u64()?;
    if generation > 2_147_483_647 { return None; }
    let overflow = batch.get("textSendTraceOverflow")?.as_bool()?;
    let allowed = ["registered", "submit_enter", "submit_return", "promise_accepted",
        "promise_rejected", "promise_unknown", "matched_add", "matched_update",
        "readback_matched", "readback_mismatch", "settled"];
    let mut incomplete = overflow || (generation == 0 && !stages.is_empty()) || stages.len() > 32;
    let mut events = Vec::new();
    for event in stages.iter().take(32) {
        let seq = event.get("seq").and_then(Value::as_u64);
        let stage = event.get("stage").and_then(Value::as_str);
        match (seq, stage) {
            (Some(seq @ 1..=2_147_483_647), Some(stage)) if allowed.contains(&stage) => {
                events.push(json!({"seq":seq,"stage":stage}));
            }
            _ => incomplete = true,
        }
    }
    (!events.is_empty() || incomplete).then(|| json!({
        "generation":generation,"events":events,"incomplete":incomplete
    }).to_string())
}

/// Accept only listener-scoped metadata. Invalid or missing rows describe an
/// incomplete observation, independent of the business result.
#[cfg(feature = "research")]
pub(crate) fn sanitize_callback_observations(input: &str, generation: u32) -> String {
    let sanitize = || -> Option<Value> {
        if input.len() > 24_576 || generation > 2_147_483_647 {
            return None;
        }
        let batch: Value = serde_json::from_str(input).ok()?;
        if batch.get("generation")?.as_u64()? != u64::from(generation) {
            return None;
        }
        let rows = batch.get("events")?.as_array()?;
        let mut incomplete = generation == 0 || batch.get("overflow")?.as_bool()? || rows.len() > 128;
        let mut events = Vec::new();
        let mut previous = 0;
        for row in rows.iter().take(129) {
            let event = (|| {
                let order = row.get("order")?.as_u64()?;
                let seq = row.get("seq")?.as_u64()?;
                let phase = row.get("phase")?.as_str()?;
                let kind = row.get("kind")?.as_str()?;
                let association = row.get("association")?.as_str()?;
                let current = row.get("current")?;
                let callback_tid = row.get("callback_tid");
                let text_state = row.get("text_state");
                if text_state.is_some_and(|value| phase != "stopped" || association != "closure"
                    || !matches!(value.as_str(), Some("unchanged" | "changed" | "unverified")))
                {
                    return None;
                }
                if callback_tid.is_some_and(|tid| !tid.is_null() && !tid.as_u64().is_some_and(|tid| (1..=u64::from(u32::MAX)).contains(&tid))) {
                    return None;
                }
                if generation == 0 || row.get("generation")?.as_u64()? != u64::from(generation)
                    || !((previous + 1)..=128).contains(&order)
                    || !matches!(phase, "active" | "stopping" | "stopped" | "remove_failed")
                {
                    return None;
                }
                let valid = match kind {
                    "recv" | "add" | "update" => association == "none",
                    "stop_begin" => association == "none" && phase == "stopping",
                    "stop_end" => association == "none" && phase == "stopped",
                    "stop_remove_failed" => association == "none" && phase == "remove_failed",
                    "matched_add" | "matched_update" => association == "matched" && phase == "active" && current == true,
                    "promise_fulfilled" | "promise_rejected" | "readback_fulfilled" | "readback_rejected" => association == "closure",
                    _ => false,
                } && if association == "none" {
                    seq == 0 && current.is_null()
                } else {
                    (1..=2_147_483_647).contains(&seq) && current.is_boolean()
                };
                valid.then(|| {
                    let mut event = json!({"generation":generation,"order":order,"seq":seq,
                        "kind":kind,"phase":phase,"association":association,"current":current});
                    // Absent preserves older sanitized fixtures; null means the
                    // synchronous getter was unavailable for this observation.
                    if let Some(tid) = callback_tid { event["callback_tid"] = tid.clone(); }
                    if let Some(state) = text_state { event["text_state"] = state.clone(); }
                    (order, event)
                })
            })();
            if let Some((order, event)) = event {
                incomplete |= order != previous + 1;
                previous = order;
                events.push(event);
            } else {
                incomplete = true;
            }
        }
        Some(json!({"generation":generation,"events":events,"incomplete":incomplete}))
    };
    sanitize().unwrap_or_else(|| json!({"generation":generation,"events":[],"incomplete":true})).to_string()
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
        let input = json!({"textSendTraceGeneration":3,"textSendTraceOverflow":false,
            "textSendTrace":[{"seq":1,"stage":"registered","secret":"private"},
                {"seq":2,"stage":"promise_accepted"},{"seq":0,"stage":"matched_add"},
                {"seq":1,"stage":"private"}],"message":"secret"}).to_string();
        let output = sanitize_text_send_trace(&input).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&output).unwrap(), json!({"generation":3,
            "events":[{"seq":1,"stage":"registered"},{"seq":2,"stage":"promise_accepted"}],
            "incomplete":true}));
        assert!(!output.contains("private") && !output.contains("secret"));
        let many = json!({"textSendTraceGeneration":3,"textSendTraceOverflow":false,
            "textSendTrace":vec![json!({"seq":1,"stage":"matched_add"});40]}).to_string();
        let value: Value = serde_json::from_str(&sanitize_text_send_trace(&many).unwrap()).unwrap();
        assert_eq!(value["events"].as_array().unwrap().len(),32);
        assert_eq!(value["incomplete"],true);
        let overflow = json!({"textSendTraceGeneration":3,"textSendTraceOverflow":true,
            "textSendTrace":[]}).to_string();
        assert_eq!(serde_json::from_str::<Value>(&sanitize_text_send_trace(&overflow).unwrap()).unwrap(),
            json!({"generation":3,"events":[],"incomplete":true}));
        let disabled = json!({"textSendTraceGeneration":0,"textSendTraceOverflow":false,
            "textSendTrace":[]}).to_string();
        assert_eq!(sanitize_text_send_trace(&disabled),None);
        let unavailable = json!({"textSendTraceGeneration":0,"textSendTraceOverflow":true,
            "textSendTrace":[]}).to_string();
        assert_eq!(serde_json::from_str::<Value>(&sanitize_text_send_trace(&unavailable).unwrap()).unwrap(),
            json!({"generation":0,"events":[],"incomplete":true}));
        let missing_generation = json!({"textSendTrace":[],"textSendTraceOverflow":false}).to_string();
        assert_eq!(sanitize_text_send_trace(&missing_generation),None);
        assert_eq!(sanitize_text_send_trace(&" ".repeat(524_289)),None);
    }

    #[cfg(feature = "research")]
    #[test]
    fn callback_observations_reject_gaps_duplicates_and_foreign_metadata() {
        let row = |order| json!({"generation":3,"order":order,"seq":2,
            "kind":"promise_fulfilled","phase":"stopped","association":"closure","current":false});
        let mut first = row(1);
        first["body"] = json!("PRIVATE");
        let mut unknown = row(2);
        unknown["kind"] = json!("PRIVATE");
        let mut foreign = row(4);
        foreign["generation"] = json!(4);
        let mut bad_seq = row(5);
        bad_seq["seq"] = json!(0);
        let mut bad_current = row(6);
        bad_current["current"] = json!("false");
        let input = json!({"generation":3,"overflow":false,"events":[first,
            unknown,row(1),row(3),foreign,bad_seq,bad_current],"cookie":"PRIVATE"});
        let output = sanitize_callback_observations(&input.to_string(), 3);
        assert!(!output.contains("PRIVATE"));
        assert_eq!(serde_json::from_str::<Value>(&output).unwrap(), json!({
            "generation":3,"events":[row(1),row(3)],"incomplete":true
        }));
        let oversized = json!({"generation":3,"overflow":false,
            "events":(1..=140).map(row).collect::<Vec<_>>()});
        let output: Value = serde_json::from_str(&sanitize_callback_observations(&oversized.to_string(), 3)).unwrap();
        assert_eq!(output["events"].as_array().unwrap().len(), 128);
        assert_eq!(output["incomplete"], true);
        for (input, generation) in [("UNAVAILABLE", 3), ("{}", 3),
            (r#"{"generation":3,"overflow":false,"events":[]}"#, 4),
            (r#"{"generation":0,"overflow":false,"events":[]}"#, 0)] {
            let output = sanitize_callback_observations(input, generation);
            assert_eq!(serde_json::from_str::<Value>(&output).unwrap(), json!({
                "generation":generation,"events":[],"incomplete":true
            }));
        }
    }

    #[cfg(feature = "research")]
    #[test]
    fn callback_text_state_requires_a_stopped_closure_and_a_fixed_label() {
        let row = json!({"generation":1,"order":1,"seq":7,
            "kind":"readback_fulfilled","phase":"stopped","association":"closure","current":true});
        let sanitize = |event| {
            let input = json!({"generation":1,"overflow":false,"events":[event]});
            serde_json::from_str::<Value>(&sanitize_callback_observations(&input.to_string(), 1)).unwrap()
        };
        for label in ["unchanged", "changed", "unverified"] {
            let mut observed = row.clone();
            observed["text_state"] = json!(label);
            observed["secret"] = json!("PRIVATE_SENTINEL");
            let result = sanitize(observed);
            assert_eq!(result["events"][0]["text_state"], label);
            assert_eq!(result["incomplete"], false);
            assert!(!result.to_string().contains("PRIVATE_SENTINEL"));
        }
        let mut invalid = row.clone();
        invalid["text_state"] = json!("PRIVATE_SENTINEL");
        assert_eq!(sanitize(invalid), json!({"generation":1,"events":[],"incomplete":true}));
        let mut active = row;
        active["phase"] = json!("active");
        active["text_state"] = json!("unchanged");
        assert_eq!(sanitize(active)["incomplete"], true);
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
