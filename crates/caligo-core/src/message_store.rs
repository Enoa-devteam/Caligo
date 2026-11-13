//! Account-scoped message-ID journal used by OneBot get_msg.
//!
//! Message identity is `(account, chat type, peer, native identity)`. Native
//! `seq`, request IDs, cookies, and text are never used to invent a message ID.
//! The append is flushed before the int32 OneBot ID is returned to a caller.

use crate::onebot::StoreReader;
use caligo_model::{capability::{HostQuery, VoiceInfo, ImageInfo}, onebot_traits::StoredMessageView, EventSource, MsgEvent, SessionKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatType {
    Group,
    Private,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageIdentity {
    pub account_id: u64,
    pub chat_type: ChatType,
    pub peer_id: String,
    /// Native message identity received from QQ. Empty identity is rejected.
    pub native_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredMessage {
    pub message_id: i32,
    pub identity: MessageIdentity,
    pub real_id: i32,
    pub sender: Value,
    pub message: Value,
    pub time: i32,
    #[serde(default)]
    pub native_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<VoiceInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<ImageInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
struct JournalEntry {
    version: u32,
    message: StoredMessage,
}

/// Single-writer account store. A failed append leaves both the ID counter and
/// in-memory indexes unchanged, so a caller cannot report a non-durable ID.
pub struct MessageStore {
    path: Option<PathBuf>,
    account_id: u64,
    nickname: Option<String>,
    online: Option<bool>,
    by_id: BTreeMap<i32, StoredMessage>,
    by_identity: HashMap<MessageIdentity, i32>,
    next_id: i32,
}

impl MessageStore {
    pub fn in_memory(
        account_id: u64,
        nickname: Option<String>,
        online: Option<bool>,
        first_id: i32,
    ) -> Self {
        Self {
            path: None,
            account_id,
            nickname,
            online,
            by_id: BTreeMap::new(),
            by_identity: HashMap::new(),
            next_id: first_id.max(1),
        }
    }

    /// Open/replay an append-only JSONL ID journal. Any malformed record or
    /// duplicate identity with a conflicting ID stops startup for repair.
    pub fn open(
        path: impl AsRef<Path>,
        account_id: u64,
        nickname: Option<String>,
    ) -> std::io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut store = Self {
            path: Some(path.clone()),
            account_id,
            nickname,
            online: None,
            by_id: BTreeMap::new(),
            by_identity: HashMap::new(),
            next_id: 1,
        };
        if !path.exists() {
            return Ok(store);
        }
        let file = std::fs::File::open(path)?;
        for (line_no, line) in std::io::BufReader::new(file).lines().enumerate() {
            let line = line?;
            if line.is_empty() {
                continue;
            }
            let entry: JournalEntry = serde_json::from_str(&line).map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("message journal line {}: {e}", line_no + 1),
                )
            })?;
            if (entry.version != 1 && entry.version != 2)
                || entry.message.identity.account_id != account_id
                || entry.message.message_id <= 0
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "message journal line {} violates account/version/id contract",
                        line_no + 1
                    ),
                ));
            }
            let prior_id = store.by_identity.get(&entry.message.identity).copied();
            let prior_message = store.by_id.get(&entry.message.message_id);
            let is_revision = entry.version == 2
                && prior_id == Some(entry.message.message_id)
                && prior_message.is_some();
            if (entry.version != 1 && entry.version != 2)
                || (entry.version == 1
                    && (store.by_id.contains_key(&entry.message.message_id)
                        || prior_id.is_some()))
                || (entry.version == 2 && !is_revision)
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "message journal line {} duplicates or conflicts with an ID or identity",
                        line_no + 1
                    ),
                ));
            }
            store.next_id = store
                .next_id
                .max(entry.message.message_id.checked_add(1).ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "message ID space exhausted",
                    )
                })?);
            store
                .by_identity
                .insert(entry.message.identity.clone(), entry.message.message_id);
            store.by_id.insert(entry.message.message_id, entry.message);
        }
        Ok(store)
    }

    pub fn set_online_status(&mut self, online: Option<bool>) {
        self.online = online;
    }

    /// Persist one message with an observed native identity. Repeated identity
    /// is idempotent and returns the original ID without rewriting its fields.
    pub fn insert_confirmed(
        &mut self,
        identity: MessageIdentity,
        real_id: i32,
        sender: Value,
        message: Value,
        time: i32,
        native_seq: Option<u64>,
    ) -> Result<i32, String> {
        self.insert_message(StoredMessage {
            message_id: self.next_id,
            identity,
            real_id,
            sender,
            message,
            time,
            native_seq,
            voice: None,
            image: None,
        })
    }

    fn insert_message(&mut self, stored: StoredMessage) -> Result<i32, String> {
        let identity = &stored.identity;
        if identity.account_id != self.account_id
            || identity.peer_id.is_empty()
            || identity.native_id.is_empty()
        {
            return Err("message identity is incomplete or belongs to another account".into());
        }
        if let Some(id) = self.by_identity.get(identity) {
            return Ok(*id);
        }
        let id = self.next_id;
        if id <= 0 {
            return Err("OneBot int32 message ID space exhausted".into());
        }
        let next_id = id
            .checked_add(1)
            .ok_or("OneBot int32 message ID space exhausted")?;
        self.append_entry(1, &stored)?;
        self.next_id = next_id;
        self.by_identity.insert(identity.clone(), id);
        self.by_id.insert(id, stored);
        Ok(id)
    }

    /// Persist a projected message, preserving its ID when an update revises it.
    /// Opaque native fields never fill numeric OneBot IDs or timestamps.
    pub fn insert_event(&mut self, event: &MsgEvent) -> Result<Option<i32>, String> {
        if event.onebot_projection.is_none() && event.wrapper_message.is_some() {
            return self.insert_wrapper_event(event);
        }
        let Some(projection) = event.onebot_projection.as_ref() else {
            return Ok(None);
        };
        let account_id = event
            .session
            .account
            .0
            .parse::<u64>()
            .map_err(|_| "OneBot message account is not a numeric QQ ID".to_string())?;
        if account_id == 0
            || account_id != self.account_id
            || projection.peer_id == 0
            || projection.sender_user_id == 0
            || event.native_id.0.is_empty()
            || event.text.is_empty()
        {
            return Err("OneBot message projection is incomplete or mismatched".into());
        }

        let chat_type = match event.session.kind {
            SessionKind::Group => ChatType::Group,
            SessionKind::Private => ChatType::Private,
        };
        let identity = MessageIdentity {
            account_id,
            chat_type,
            peer_id: projection.peer_id.to_string(),
            native_id: event.native_id.0.clone(),
        };
        let mut sender = serde_json::Map::new();
        sender.insert("user_id".into(), Value::from(projection.sender_user_id));
        if let Some(nickname) = projection.sender_nickname.as_ref() {
            sender.insert("nickname".into(), Value::String(nickname.clone()));
        }
        if event.source != EventSource::Update {
            return self
                .insert_confirmed(
                    identity,
                    projection.real_id,
                    Value::Object(sender),
                    Value::String(event.text.clone()),
                    projection.time,
                    None,
                )
                .map(Some);
        }
        let Some(id) = self.by_identity.get(&identity).copied() else {
            return Ok(None);
        };
        let mut message = self
            .by_id
            .get(&id)
            .cloned()
            .ok_or("message identity index is inconsistent")?;
        if message.voice.is_some() || message.image.is_some() {
            return Ok(Some(id));
        }
        message.real_id = projection.real_id;
        message.sender = Value::Object(sender);
        message.message = Value::String(event.text.clone());
        message.time = projection.time;
        message.native_seq = None;

        message.message_id = id;
        if self.by_id.get(&id) != Some(&message) {
            self.append_entry(2, &message)?;
            self.by_id.insert(id, message);
        }
        Ok(Some(id))
    }

    fn insert_wrapper_event(&mut self, event: &MsgEvent) -> Result<Option<i32>, String> {
        let envelope = event.wrapper_message.as_ref().ok_or("wrapper record is missing")?;
        envelope.validate().map_err(|_| "wrapper record is invalid")?;
        let record = &envelope.record;
        let account_id = event
            .session
            .account
            .0
            .parse::<u64>()
            .map_err(|_| "wrapper account is not numeric")?;
        let bound_account = envelope.binding.account.parse::<u64>()
            .map_err(|_| "wrapper binding account is not numeric")?;
        let peer_id = record
            .peer_uin
            .parse::<u64>()
            .map_err(|_| "wrapper peer UIN is not numeric")?;
        let sender_id = record
            .sender_uin
            .parse::<u64>()
            .map_err(|_| "wrapper sender UIN is not numeric")?;
        let platform_time = record
            .msg_time
            .parse::<u64>()
            .map_err(|_| "wrapper message time is invalid")?;
        let expected_kind = match event.session.kind {
            SessionKind::Private if record.chat_type == 1 => ChatType::Private,
            SessionKind::Group if record.chat_type == 2 => ChatType::Group,
            _ => return Err("wrapper chat type does not match session".into()),
        };
        let voice = envelope.voice();
        let image = envelope.image();
        let text = if voice.is_some() || image.is_some() {
            String::new()
        } else {
            envelope.text().map_err(|_| "wrapper text is invalid")?
        };
        if account_id == 0
            || account_id != self.account_id
            || account_id != bound_account
            || event.session_generation != envelope.binding.session_generation
            || event.session.peer.0 != record.peer_uin
            || peer_id == 0
            || sender_id == 0
            || event.sender != record.sender_uin
            || event.native_id.0 != record.msg_id
            || event.text != text
            || event.platform_time != Some(platform_time)
            || (voice.is_some() || image.is_some()) && (event.source != EventSource::Recv
                || match envelope.source {
                    caligo_model::wrapper_message::WrapperSource::Recv =>
                        event.direction != caligo_model::Direction::Incoming,
                    caligo_model::wrapper_message::WrapperSource::AddSend =>
                        event.direction != caligo_model::Direction::SelfSent,
                    caligo_model::wrapper_message::WrapperSource::Update => true,
                })
            || event.direction == caligo_model::Direction::SelfSent
                && record.sender_uin != envelope.binding.account
        {
            return Err("wrapper record does not match normalized event".into());
        }
        let identity = MessageIdentity {
            account_id,
            chat_type: expected_kind,
            peer_id: peer_id.to_string(),
            native_id: record.msg_id.clone(),
        };
        let mut sender = serde_json::Map::new();
        sender.insert("user_id".into(), Value::from(sender_id));
        let body = Value::String(text);
        let time = i32::try_from(platform_time).map_err(|_| "OneBot message time exceeds int32")?;

        if event.source == EventSource::Update {
            let Some(id) = self.by_identity.get(&identity).copied() else {
                return Ok(None);
            };
            let mut revised = self
                .by_id
                .get(&id)
                .cloned()
                .ok_or("message identity index is inconsistent")?;
            if revised.voice.is_some() || revised.image.is_some() {
                return Ok(Some(id));
            }
            revised.real_id = id;
            revised.sender = Value::Object(sender);
            revised.message = body;
            revised.time = time;
            if self.by_id.get(&id) != Some(&revised) {
                self.append_entry(2, &revised)?;
                self.by_id.insert(id, revised);
            }
            return Ok(Some(id));
        }

        let id = self.next_id;
        if let Some(voice) = voice {
            let token = caligo_model::onebot_traits::voice_file_token(account_id, id);
            return self.insert_message(StoredMessage {
                message_id: id,
                identity,
                real_id: id,
                sender: Value::Object(sender),
                message: serde_json::json!([{"type":"record","data":{"file":token}}]),
                time,
                native_seq: None,
                image: None,
                voice: Some(VoiceInfo {
                    peer_uid: record.peer_uid.clone(),
                    message_id: record.msg_id.clone(),
                    element_id: voice.element_id.clone(),
                    size: voice.size,
                    md5: voice.md5.clone(),
                }),
            }).map(Some);
        }
        if let Some(image) = image {
            let token = caligo_model::onebot_traits::image_file_token(account_id, id);
            return self.insert_message(StoredMessage {
                message_id:id, identity, real_id:id, sender:Value::Object(sender),
                message:serde_json::json!([{"type":"image","data":{"file":token}}]),
                time, native_seq:None, voice:None, image:Some(image.clone()),
            }).map(Some);
        }
        self.insert_confirmed(identity, id, Value::Object(sender), body, time, None)
            .map(Some)
    }

    fn append_entry(&self, version: u32, message: &StoredMessage) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let entry = JournalEntry { version, message: message.clone() };
        let mut bytes = serde_json::to_vec(&entry).map_err(|e| e.to_string())?;
        bytes.push(b'\n');
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| format!("open message journal: {e}"))?;
        file.write_all(&bytes).map_err(|e| format!("append message journal: {e}"))?;
        file.sync_all().map_err(|e| format!("flush message journal: {e}"))
    }

    pub fn get(&self, message_id: i32) -> Option<&StoredMessage> {
        self.by_id.get(&message_id)
    }

    /// Resolve a OneBot ID only for an identity already persisted from a
    /// complete native message projection.
    pub fn message_id_for_identity(&self, identity: &MessageIdentity) -> Option<i32> {
        self.by_identity.get(identity).copied()
    }

    /// Build compatibility fields from a durable wrapper-backed local message ID.
    /// The pinned SnowCat contract maps C2C to friend and group to normal, with font 14.
    pub fn projection_for_event(&self, event: &MsgEvent) -> Option<caligo_model::OneBotMessageProjection> {
        let envelope = event.wrapper_message.as_ref()?;
        envelope.validate().ok()?;
        let voice = envelope.voice();
        let image = envelope.image();
        let account_id = event.session.account.0.parse::<u64>().ok()?;
        let peer_id = envelope.record.peer_uin.parse::<u64>().ok()?;
        let sender_user_id = envelope.record.sender_uin.parse::<u64>().ok()?;
        let (chat_type, sub_type) = match (event.session.kind, envelope.record.chat_type) {
            (SessionKind::Private, 1) => (ChatType::Private, "friend"),
            (SessionKind::Group, 2) => (ChatType::Group, "normal"),
            _ => return None,
        };
        let identity = MessageIdentity {
            account_id,
            chat_type,
            peer_id: peer_id.to_string(),
            native_id: envelope.record.msg_id.clone(),
        };
        if account_id != self.account_id
            || envelope.binding.account.parse::<u64>().ok()? != account_id
            || event.session_generation != envelope.binding.session_generation
            || event.session.peer.0 != envelope.record.peer_uin
            || event.sender != envelope.record.sender_uin
            || event.native_id.0 != envelope.record.msg_id
            || if voice.is_some() || image.is_some() { !event.text.is_empty() }
                else { event.text != envelope.text().ok()? }
            || event.platform_time != Some(envelope.record.msg_time.parse::<u64>().ok()?)
        {
            return None;
        }
        let message_id = self.by_identity.get(&identity).copied()?;
        let stored = self.by_id.get(&message_id)?;
        if stored.sender.get("user_id")?.as_u64()? != sender_user_id {
            return None;
        }
        match (voice, &stored.voice) {
            (None, None) => {}
            (Some(element), Some(info))
                if info.peer_uid == envelope.record.peer_uid
                    && info.message_id == envelope.record.msg_id
                    && info.element_id == element.element_id
                    && info.size == element.size
                    && info.md5 == element.md5 => {}
            _ => return None,
        }
        if image != stored.image.as_ref() { return None; }
        Some(caligo_model::OneBotMessageProjection {
            peer_id,
            sender_user_id,
            real_id: message_id,
            time: stored.time,
            sender_nickname: stored.sender.get("nickname").and_then(Value::as_str).map(str::to_owned),
            sub_type: Some(sub_type.into()),
            font: Some(14),
        })
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}

impl StoreReader for MessageStore {
    fn voice_info(&self, file: &str) -> Option<VoiceInfo> {
        self.voice_record(file).map(|(info, _)| info)
    }

    fn voice_record(&self, file: &str) -> Option<(VoiceInfo, HostQuery)> {
        let (account, id) = file.strip_prefix("caligo-voice:")?.split_once(':')?;
        let message = self.get(id.parse::<i32>().ok()?)?;
        let voice = message.voice.as_ref()?;
        if account != self.account_id.to_string() || id != message.message_id.to_string()
            || message.identity.native_id != voice.message_id
        {
            return None;
        }
        let sender = message.sender.get("user_id")?.as_u64()?;
        let query = if message.identity.chat_type == ChatType::Private && sender != self.account_id {
            HostQuery::VoiceDownload {
                peer_uid: voice.peer_uid.clone(),
                message_id: voice.message_id.clone(),
                element_id: voice.element_id.clone(),
            }
        } else {
            HostQuery::ExactVoiceDownload {
                chat_type: match message.identity.chat_type {
                    ChatType::Private => 1,
                    ChatType::Group => 2,
                },
                peer_uid: voice.peer_uid.clone(),
                sender_uin: sender.to_string(),
                message_id: voice.message_id.clone(),
                element_id: voice.element_id.clone(),
            }
        };
        query.validate().ok()?;
        Some((voice.clone(), query))
    }

    fn image_info(&self, file: &str) -> Option<ImageInfo> {
        let (account, id) = file.strip_prefix("caligo-image:")?.split_once(':')?;
        let message = self.get(id.parse().ok()?)?;
        let info = message.image.as_ref()?;
        if account != self.account_id.to_string() || id != message.message_id.to_string()
            || message.identity.native_id != info.message_id
            || message.sender.get("user_id")?.as_u64()?.to_string() != info.sender_uin
            || info.chat_type != match message.identity.chat_type { ChatType::Private=>1, ChatType::Group=>2 }
            || (info.chat_type == 2 && message.identity.peer_id != info.peer_uid) { return None; }
        Some(info.clone())
    }

    fn group_peers(&self) -> Vec<String> {
        let mut set = std::collections::BTreeSet::new();
        for m in self.by_id.values() {
            if matches!(m.identity.chat_type, ChatType::Group) {
                set.insert(m.identity.peer_id.clone());
            }
        }
        set.into_iter().collect()
    }

    fn get_msg(&self, message_id: i32) -> Option<StoredMessageView> {
        let m = self.by_id.get(&message_id)?;
        Some(StoredMessageView {
            message_id: m.message_id,
            message_type: match m.identity.chat_type {
                ChatType::Group => "group",
                ChatType::Private => "private",
            }
            .into(),
            real_id: m.real_id,
            sender: m.sender.clone(),
            message: m.message.clone(),
            time: m.time,
        })
    }

    fn login_info(&self) -> Option<(u64, String)> {
        self.nickname
            .clone()
            .map(|nickname| (self.account_id, nickname))
    }

    fn online_status(&self) -> Option<bool> {
        self.online
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_PATH: AtomicU64 = AtomicU64::new(0);

    fn identity(
        account_id: u64,
        chat_type: ChatType,
        peer: &str,
        native_id: &str,
    ) -> MessageIdentity {
        MessageIdentity {
            account_id,
            chat_type,
            peer_id: peer.into(),
            native_id: native_id.into(),
        }
    }

    fn journal_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "caligo-message-store-{}.jsonl",
            NEXT_PATH.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn message_id_is_durable_and_stable_after_restart() {
        let path = journal_path();
        let msg = serde_json::json!([{"type":"text","data":{"text":"hello"}}]);
        let id = MessageStore::open(&path, 100, Some("bot".into()))
            .unwrap()
            .insert_confirmed(
                identity(100, ChatType::Group, "g1", "native-A"),
                901,
                serde_json::json!({"user_id": 1001, "nickname": "nick", "sex": "unknown", "age": 0}),
                msg.clone(),
                123,
                Some(999),
            )
            .unwrap();
        let mut reopened = MessageStore::open(&path, 100, Some("bot".into())).unwrap();
        assert_eq!(reopened.get(id).unwrap().message, msg);
        assert_eq!(
            reopened
                .insert_confirmed(
                    identity(100, ChatType::Group, "g1", "native-A"),
                    0,
                    Value::Null,
                    Value::Null,
                    0,
                    None
                )
                .unwrap(),
            id
        );
        let next = reopened
            .insert_confirmed(
                identity(100, ChatType::Group, "g1", "native-B"),
                902,
                serde_json::json!({"user_id": 1002, "nickname": "n2"}),
                Value::String("next".into()),
                124,
                None,
            )
            .unwrap();
        assert_eq!(next, id + 1);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn update_appends_revision_and_keeps_the_allocated_id_after_reopen() {
        let path = journal_path();
        let mut store = MessageStore::open(&path, 100, None).unwrap();
        let mut initial = MsgEvent {
            event_seq: 1,
            session_generation: 1,
            session: caligo_model::SessionKey::new("100", SessionKind::Group, "g1"),
            direction: caligo_model::Direction::Incoming,
            sender: "1001".into(),
            native_id: caligo_model::NativeMessageId("native-A".into()),
            text: "sending".into(),
            platform_time: Some(10),
            wrapper_message: None,
            onebot_projection: Some(caligo_model::OneBotMessageProjection {
                peer_id: 200,
                sender_user_id: 1001,
                real_id: 0,
                time: 10,
                sender_nickname: None,
                sub_type: Some("normal".into()),
                font: Some(14),
            }),
            observed_at_unix_ms: 1,
            source: EventSource::Recv,
        };
        let id = store.insert_event(&initial).unwrap().unwrap();
        initial.source = EventSource::Update;
        initial.event_seq = 2;
        initial.text = "sent".into();
        initial.onebot_projection.as_mut().unwrap().real_id = id;
        initial.onebot_projection.as_mut().unwrap().time = 11;
        assert_eq!(store.insert_event(&initial).unwrap(), Some(id));
        assert_eq!(store.get(id).unwrap().message, Value::String("sent".into()));
        drop(store);

        let reopened = MessageStore::open(&path, 100, None).unwrap();
        let message = reopened.get(id).unwrap();
        assert_eq!(message.real_id, id);
        assert_eq!(message.message, Value::String("sent".into()));
        assert_eq!(message.time, 11);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn typed_wrapper_message_gets_durable_local_id_and_updates_in_place() {
        let path = journal_path();
        let mut store = MessageStore::open(&path, 10001, None).unwrap();
        let make_event = |source, text: &str, status| {
            let element = caligo_model::wrapper_message::WrapperElement {
                element_type: 1,
                voice: None,
                image: None,
                text_element: Some(caligo_model::wrapper_message::WrapperText {
                    content: text.into(), at_type: 0, at_uid: String::new(),
                    at_nt_uid: String::new(), at_tiny_id: String::new(),
                }),
            };
            let mut event = MsgEvent {
                event_seq: 0,
                session_generation: 1,
                session: caligo_model::SessionKey::new("10001", SessionKind::Private, "20002"),
                direction: caligo_model::Direction::Incoming,
                sender: "20002".into(),
                native_id: caligo_model::NativeMessageId("90071992547409931234".into()),
                text: text.into(),
                platform_time: Some(1_794_567_890),
                wrapper_message: None,
                onebot_projection: None,
                observed_at_unix_ms: 10,
                source,
            };
            event.wrapper_message = Some(caligo_model::wrapper_message::WrapperMessageEnvelope {
                binding: caligo_model::wrapper_message::WrapperBinding {
                    account: "10001".into(), session_generation: 1, host_nonce: 9,
                    wrapper_session: "nt_fixture".into(),
                },
                wrapper_session: "nt_fixture".into(),
                source: if source == EventSource::Update {
                    caligo_model::wrapper_message::WrapperSource::Update
                } else {
                    caligo_model::wrapper_message::WrapperSource::Recv
                },
                observed_at_unix_ms: 10,
                record: caligo_model::wrapper_message::WrapperMessage {
                    msg_id: "90071992547409931234".into(), msg_seq: "1".into(),
                    msg_random: "2".into(), msg_time: "1794567890".into(),
                    client_seq: None, guild_id: String::new(), chat_type: 1, msg_type: 2,
                    sub_msg_type: 0, peer_uid: "u_friend".into(), peer_uin: "20002".into(),
                    sender_uid: "u_friend".into(), sender_uin: "20002".into(),
                    send_status: status, elements: vec![element],
                },
            });
            event
        };

        let first = make_event(EventSource::Recv, "sending", Some(1));
        let id = store.insert_event(&first).unwrap().unwrap();
        let before = store.projection_for_event(&first).unwrap();
        assert_eq!(before.real_id, id);
        assert_eq!(before.font, Some(14));
        assert_eq!(before.sub_type.as_deref(), Some("friend"));

        let update = make_event(EventSource::Update, "sent", Some(2));
        assert_eq!(store.insert_event(&update).unwrap(), Some(id));
        assert_eq!(store.get(id).unwrap().message, Value::String("sent".into()));
        drop(store);
        let reopened = MessageStore::open(&path, 10001, None).unwrap();
        assert_eq!(reopened.get(id).unwrap().message, Value::String("sent".into()));
        assert_eq!(reopened.projection_for_event(&update).unwrap().real_id, id);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn identity_includes_account_chat_type_peer_and_native_id_not_sequence() {
        let mut store = MessageStore::in_memory(100, None, None, 1);
        let cases = [
            identity(100, ChatType::Group, "g1", "same-native"),
            identity(100, ChatType::Private, "g1", "same-native"),
            identity(100, ChatType::Group, "g2", "same-native"),
        ];
        let mut ids = Vec::new();
        for key in cases {
            ids.push(
                store
                    .insert_confirmed(
                        key,
                        77,
                        serde_json::json!({"user_id": 1, "nickname": "n"}),
                        Value::String("x".into()),
                        1,
                        Some(7),
                    )
                    .unwrap(),
            );
        }
        assert!(store
            .insert_confirmed(
                identity(101, ChatType::Group, "g1", "foreign"),
                77,
                Value::Null,
                Value::Null,
                1,
                None
            )
            .is_err());
        assert_eq!(ids, vec![1, 2, 3]);
    }

    #[test]
    fn incomplete_identity_never_allocates_id() {
        let mut store = MessageStore::in_memory(100, None, None, 9);
        assert!(store
            .insert_confirmed(
                identity(100, ChatType::Group, "g", ""),
                77,
                Value::Null,
                Value::Null,
                1,
                Some(1)
            )
            .is_err());
        assert_eq!(
            store
                .insert_confirmed(
                    identity(100, ChatType::Group, "g", "native"),
                    77,
                    Value::Null,
                    Value::Null,
                    1,
                    Some(1)
                )
                .unwrap(),
            9
        );
    }

    #[test]
    fn group_peers_are_learned_from_group_messages_only() {
        let mut store = MessageStore::in_memory(100, None, Some(true), 1);
        store
            .insert_confirmed(
                identity(100, ChatType::Group, "g1", "n1"),
                1,
                serde_json::json!({"user_id": 1}),
                Value::String("a".into()),
                1,
                None,
            )
            .unwrap();
        store
            .insert_confirmed(
                identity(100, ChatType::Group, "g1", "n2"),
                2,
                serde_json::json!({"user_id": 1}),
                Value::String("b".into()),
                1,
                None,
            )
            .unwrap();
        store
            .insert_confirmed(
                identity(100, ChatType::Private, "u_x", "n3"),
                3,
                serde_json::json!({"user_id": 1}),
                Value::String("c".into()),
                1,
                None,
            )
            .unwrap();
        assert_eq!(store.group_peers(), vec!["g1".to_string()]);
    }
}
