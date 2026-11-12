//! Account-scoped message-ID journal used by OneBot get_msg.
//!
//! Message identity is `(account, chat type, peer, native identity)`. Native
//! `seq`, request IDs, cookies, and text are never used to invent a message ID.
//! The append is flushed before the int32 OneBot ID is returned to a caller.

use crate::onebot::StoreReader;
use caligo_model::{onebot_traits::StoredMessageView, EventSource, MsgEvent, SessionKind};
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
            if entry.version != 1
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
            if store.by_id.contains_key(&entry.message.message_id)
                || store.by_identity.contains_key(&entry.message.identity)
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "message journal line {} duplicates an ID or identity",
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
        if identity.account_id != self.account_id
            || identity.peer_id.is_empty()
            || identity.native_id.is_empty()
        {
            return Err("message identity is incomplete or belongs to another account".into());
        }
        if let Some(id) = self.by_identity.get(&identity) {
            return Ok(*id);
        }
        let id = self.next_id;
        if id <= 0 {
            return Err("OneBot int32 message ID space exhausted".into());
        }
        let next_id = id
            .checked_add(1)
            .ok_or("OneBot int32 message ID space exhausted")?;
        let stored = StoredMessage {
            message_id: id,
            identity: identity.clone(),
            real_id,
            sender,
            message,
            time,
            native_seq,
        };
        if let Some(path) = &self.path {
            let entry = JournalEntry {
                version: 1,
                message: stored.clone(),
            };
            let mut bytes = serde_json::to_vec(&entry).map_err(|e| e.to_string())?;
            bytes.push(b'\n');
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .map_err(|e| format!("open message journal: {e}"))?;
            file.write_all(&bytes)
                .map_err(|e| format!("append message journal: {e}"))?;
            file.sync_all()
                .map_err(|e| format!("flush message journal: {e}"))?;
        }
        self.next_id = next_id;
        self.by_identity.insert(identity, id);
        self.by_id.insert(id, stored);
        Ok(id)
    }

    /// Persist an incoming event only when the bridge supplied a complete,
    /// separately verified OneBot projection. Opaque native fields never fill
    /// numeric OneBot IDs or timestamps.
    pub fn insert_event(&mut self, event: &MsgEvent) -> Result<Option<i32>, String> {
        if event.source == EventSource::Update {
            return Ok(None);
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
        self.insert_confirmed(
            identity,
            projection.real_id,
            Value::Object(sender),
            Value::String(event.text.clone()),
            projection.time,
            None,
        )
        .map(Some)
    }

    pub fn get(&self, message_id: i32) -> Option<&StoredMessage> {
        self.by_id.get(&message_id)
    }

    /// Resolve a OneBot ID only for an identity already persisted from a
    /// complete native message projection.
    pub fn message_id_for_identity(&self, identity: &MessageIdentity) -> Option<i32> {
        self.by_identity.get(identity).copied()
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}

impl StoreReader for MessageStore {
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
