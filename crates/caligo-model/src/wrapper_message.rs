//! Owned QQ wrapper text and private voice records. Contract: SnowCat 7cf025f types/msg.ts,
//! NodeIKernelMsgListener and business-message/service-entry.ts:36–40.
//! msgTime is Unix seconds; MSF getServerTime is a different millisecond clock.
//! Neither a native ID nor a wrapper extension `id` proves OneBot real_id/font.

use serde::{Deserialize, Serialize};

pub const MAX_BATCH_BYTES: usize = 512 * 1024;
pub const MAX_RECORD_BYTES: usize = 64 * 1024;
pub const MAX_BATCH_ITEMS: usize = 128;
pub const MAX_TEXT_BYTES: usize = 48 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WrapperSource {
    Recv,
    Update,
    AddSend,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WrapperText {
    pub content: String,
    pub at_type: u32,
    #[serde(default)]
    pub at_uid: String,
    #[serde(default)]
    pub at_nt_uid: String,
    #[serde(default)]
    pub at_tiny_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WrapperElement {
    pub element_type: u32,
    pub text_element: Option<WrapperText>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<WrapperVoice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WrapperVoice {
    pub element_id: String,
    pub size: u32,
    pub md5: String,
}

impl WrapperVoice {
    pub fn is_valid(&self) -> bool {
        positive_decimal(&self.element_id)
            && (1..=crate::capability::MAX_VOICE_BYTES).contains(&self.size)
            && self.md5.len() == 32
            && self
                .md5
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WrapperMessage {
    pub msg_id: String,
    pub msg_seq: String,
    pub msg_random: String,
    pub msg_time: String,
    pub client_seq: Option<String>,
    pub guild_id: String,
    pub chat_type: u32,
    pub msg_type: u32,
    pub sub_msg_type: u32,
    pub peer_uid: String,
    pub peer_uin: String,
    pub sender_uid: String,
    pub sender_uin: String,
    pub send_status: Option<u32>,
    pub elements: Vec<WrapperElement>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WrapperBinding {
    pub account: String,
    pub session_generation: u64,
    pub host_nonce: u64,
    pub wrapper_session: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WrapperMessageEnvelope {
    pub binding: WrapperBinding,
    pub wrapper_session: String,
    pub source: WrapperSource,
    pub observed_at_unix_ms: u64,
    pub record: WrapperMessage,
}

fn decimal(s: &str) -> bool {
    !s.is_empty() && s.len() <= 128 && s.bytes().all(|b| b.is_ascii_digit())
}
fn positive_decimal(s: &str) -> bool {
    decimal(s) && s.bytes().any(|b| b != b'0')
}
fn uid(s: &str) -> bool {
    s.starts_with("u_")
        && s.len() > 2
        && s.len() <= 130
        && s[2..]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

impl WrapperMessageEnvelope {
    /// Strictly validate a complete record before it can enter resident/IPC.
    pub fn validate(&self) -> Result<(), &'static str> {
        let r = &self.record;
        if !positive_decimal(&self.binding.account)
            || self.binding.session_generation == 0
            || self.binding.host_nonce == 0
            || self.wrapper_session != self.binding.wrapper_session
            || self.wrapper_session.is_empty()
            || self.wrapper_session.len() > 128
            || self.observed_at_unix_ms == 0
        {
            return Err("binding");
        }
        if !positive_decimal(&r.msg_id)
            || !decimal(&r.msg_seq)
            || !decimal(&r.msg_random)
            || !decimal(&r.msg_time)
            || r.msg_time.parse::<u64>().is_err()
            || r.client_seq.as_ref().is_some_and(|s| !decimal(s))
            || r.guild_id.len() > 128
        {
            return Err("identity_or_time");
        }
        if !uid(&r.sender_uid)
            || !positive_decimal(&r.sender_uin)
            || !positive_decimal(&r.peer_uin)
            || !matches!(r.chat_type, 1 | 2)
            || (r.chat_type == 1 && !uid(&r.peer_uid))
            || (r.chat_type == 2 && (!positive_decimal(&r.peer_uid) || r.peer_uid != r.peer_uin))
            || r.send_status.is_some_and(|s| s > 3)
        {
            return Err("peer_or_sender");
        }
        if self.source == WrapperSource::AddSend && r.sender_uin != self.binding.account {
            return Err("add_send_identity");
        }
        if self.voice().is_none() {
            self.text()?;
        }
        if serde_json::to_vec(r).map_err(|_| "record_encoding")?.len() > MAX_RECORD_BYTES {
            return Err("record_size");
        }
        Ok(())
    }

    pub fn text(&self) -> Result<String, &'static str> {
        if self.record.elements.is_empty() || self.record.elements.len() > 128 {
            return Err("elements");
        }
        let mut text = String::new();
        for element in &self.record.elements {
            let Some(t) = element.text_element.as_ref() else {
                return Err("non_text");
            };
            if element.element_type != 1
                || element.voice.is_some()
                || t.at_type != 0
                || !t.at_uid.is_empty()
                || !t.at_nt_uid.is_empty()
                || !t.at_tiny_id.is_empty()
            {
                return Err("unsupported_element");
            }
            if text.len().saturating_add(t.content.len()) > MAX_TEXT_BYTES {
                return Err("text_size");
            }
            text.push_str(&t.content);
        }
        if text.is_empty() {
            return Err("empty_text");
        }
        Ok(text)
    }

    /// Single voice admitted by D8. Self-sent rows come from D9 readback.
    pub fn voice(&self) -> Option<&WrapperVoice> {
        let [element] = self.record.elements.as_slice() else {
            return None;
        };
        let voice = element.voice.as_ref()?;
        (element.element_type == 4
            && element.text_element.is_none()
            && match self.source {
                WrapperSource::Recv => self.record.sender_uin != self.binding.account
                    && match self.record.chat_type {
                        1 => self.record.sender_uid == self.record.peer_uid
                            && self.record.sender_uin == self.record.peer_uin,
                        2 => self.record.peer_uid == self.record.peer_uin,
                        _ => false,
                    },
                WrapperSource::AddSend => self.record.chat_type == 1
                    && self.record.sender_uin == self.binding.account
                    && self.record.peer_uin != self.binding.account
                    && self.record.send_status == Some(2),
                WrapperSource::Update => false,
            }
            && voice.is_valid())
        .then_some(voice)
    }

    pub fn platform_time_seconds(&self) -> Option<u64> {
        // Unit evidence is fixed service-entry.ts:39–40 + RESULTS.md seconds precision.
        self.record.msg_time.parse().ok()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WrapperDrainStats {
    pub rejected: u64,
    pub overflow: u64,
}

#[derive(Debug, Clone)]
pub struct WrapperDrain {
    pub events: Vec<WrapperMessageEnvelope>,
    pub stats: WrapperDrainStats,
    // Existing D9 seam remains separate from the typed receive records.
    pub send_results: serde_json::Map<String, serde_json::Value>,
}

/// The production owner hook and offline chain tests use this exact decoder.
/// A bad record is counted and skipped; it cannot poison the next valid record.
pub fn decode_d8_batch(json: &str, binding: &WrapperBinding) -> Result<WrapperDrain, &'static str> {
    if json.len() > MAX_BATCH_BYTES {
        return Err("batch_size");
    }
    let v: serde_json::Value = serde_json::from_str(json).map_err(|_| "batch_json")?;
    if v.get("schema").and_then(|v| v.as_u64()) != Some(1) {
        return Err("batch_schema");
    }
    let session = v
        .get("sid")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty() && s.len() <= 128)
        .ok_or("batch_session")?;
    if session != binding.wrapper_session {
        return Err("batch_session_mismatch");
    }
    let items = v
        .get("items")
        .and_then(|v| v.as_array())
        .ok_or("batch_items")?;
    let mut out = WrapperDrain {
        events: Vec::new(),
        stats: WrapperDrainStats {
            rejected: v
                .get("rejected")
                .and_then(|v| v.as_u64())
                .ok_or("batch_stats")?,
            overflow: v
                .get("overflow")
                .and_then(|v| v.as_u64())
                .ok_or("batch_stats")?,
        },
        send_results: v
            .get("sendResults")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default(),
    };
    for (i, item) in items.iter().enumerate() {
        if i >= MAX_BATCH_ITEMS {
            out.stats.overflow = out.stats.overflow.saturating_add(1);
            continue;
        }
        #[derive(Deserialize)]
        struct Item {
            source: WrapperSource,
            observed_at_unix_ms: u64,
            record: WrapperMessage,
        }
        let parsed =
            serde_json::from_value::<Item>(item.clone()).map(|item| WrapperMessageEnvelope {
                binding: binding.clone(),
                wrapper_session: session.to_owned(),
                source: item.source,
                observed_at_unix_ms: item.observed_at_unix_ms,
                record: item.record,
            });
        match parsed {
            Ok(event) if event.validate().is_ok() => out.events.push(event),
            _ => out.stats.rejected = out.stats.rejected.saturating_add(1),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn string_ids_and_seconds_are_lossless_and_bad_records_do_not_block_batch() {
        let r = serde_json::json!({"msgId":"90071992547409931234","msgSeq":"1","msgRandom":"2",
            "msgTime":"1794567890","guildId":"","chatType":1,"msgType":2,"subMsgType":0,"peerUid":"u_friend","peerUin":"20002",
            "senderUid":"u_friend","senderUin":"20002","elements":[{"elementType":1,
            "textElement":{"content":"hello","atType":0}}]});
        let binding = WrapperBinding {
            account: "10001".into(),
            session_generation: 1,
            host_nonce: 9,
            wrapper_session: "nt_fixture".into(),
        };
        let batch = serde_json::json!({"schema":1,"sid":"nt_fixture","rejected":0,"overflow":0,
            "items":[{"source":"recv","observed_at_unix_ms":1,"record":{}},
                {"source":"recv","observed_at_unix_ms":1,"record":r}]});
        let out = decode_d8_batch(&batch.to_string(), &binding).unwrap();
        assert_eq!(out.stats.rejected, 1);
        assert_eq!(out.events.len(), 1);
        assert_eq!(out.events[0].record.msg_id, "90071992547409931234");
        assert_eq!(out.events[0].platform_time_seconds(), Some(1794567890));
        let mut bad = out.events[0].clone();
        bad.record.msg_time = "90071992547409931234".into();
        assert!(bad.validate().is_err());
        bad = out.events[0].clone();
        bad.record.elements[0].element_type = 2;
        assert!(bad.validate().is_err());
        assert!(decode_d8_batch(&"x".repeat(MAX_BATCH_BYTES + 1), &binding).is_err());
    }
}
