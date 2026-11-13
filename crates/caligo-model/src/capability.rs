//! Fixed read-only host operations. The wire never carries executable scripts.

use serde::{Deserialize, Serialize};

pub const MAX_QUERY_BYTES: usize = 512 * 1024;
pub const MAX_QUERY_ITEMS: usize = 2048;
pub const MAX_QUERY_TIMEOUT_MS: u64 = 30_000;
pub const MAX_VOICE_BYTES: u32 = 16 * 1024 * 1024;
/// Hex encoding plus the typed response envelope must fit one IPC frame.
pub const MAX_VOICE_DOWNLOAD_BYTES: u32 = 128 * 1024;
pub const MAX_IMAGE_BYTES: u32 = 16 * 1024 * 1024;
pub const MAX_IMAGE_DOWNLOAD_BYTES: u32 = 128 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryCapability {
    LoginInfo,
    FriendList,
    GroupList,
    GroupInfo,
    GroupMembers,
    UinToUid,
    UidToUin,
    MessagesById,
    VoiceInfo,
    VoiceDownload,
    ExactVoiceDownload,
    ImageInfo,
    ImageDownload,
}

impl QueryCapability {
    /// Single authority for the broker's bounded advertisement size.
    pub const ALL: &'static [Self] = &[
        Self::LoginInfo,
        Self::FriendList,
        Self::GroupList,
        Self::GroupInfo,
        Self::GroupMembers,
        Self::UinToUid,
        Self::UidToUin,
        Self::MessagesById,
        Self::VoiceInfo,
        Self::VoiceDownload,
        Self::ExactVoiceDownload,
        Self::ImageInfo,
        Self::ImageDownload,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryScope {
    pub account: String,
    pub host_nonce: u64,
    pub session_generation: u64,
    pub connection_epoch: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "params",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum HostQuery {
    LoginInfo,
    FriendList,
    GroupList,
    GroupInfo {
        group_id: String,
    },
    GroupMembers {
        group_id: String,
        force_refresh: bool,
    },
    UinToUid {
        uin: String,
    },
    UidToUin {
        uid: String,
    },
    MessagesById {
        chat_type: u32,
        peer_uid: String,
        message_ids: Vec<String>,
    },
    /// Exact private incoming voice element. IDs are native decimal strings.
    VoiceInfo {
        peer_uid: String,
        message_id: String,
        element_id: String,
    },
    /// Fetch one identified incoming private Silk element into bounded owned bytes.
    VoiceDownload {
        peer_uid: String,
        message_id: String,
        element_id: String,
    },
    /// Download by durable sender, conversation and element identity.
    ExactVoiceDownload {
        chat_type: u32,
        peer_uid: String,
        sender_uin: String,
        message_id: String,
        element_id: String,
    },
    /// One ordinary PNG element, identified by its exact conversation and sender.
    ImageInfo {
        chat_type: u32,
        peer_uid: String,
        sender_uin: String,
        message_id: String,
        element_id: String,
    },
    ImageDownload {
        chat_type: u32,
        peer_uid: String,
        sender_uin: String,
        message_id: String,
        element_id: String,
    },
}

fn native_message_id(value: &str) -> bool {
    value.len() <= 128
        && value.bytes().all(|b| b.is_ascii_digit())
        && value.bytes().any(|b| b != b'0')
}

pub fn positive_decimal(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 20
        && value.bytes().all(|b| b.is_ascii_digit())
        && value.parse::<u64>().is_ok_and(|n| n > 0)
}

fn native_uid(value: &str) -> bool {
    value.starts_with("u_")
        && value.len() > 2
        && value.len() <= 130
        && value[2..]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

impl QueryScope {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !positive_decimal(&self.account)
            || self.host_nonce == 0
            || self.session_generation == 0
            || self.connection_epoch == 0
        {
            return Err("invalid query scope");
        }
        Ok(())
    }
}

impl HostQuery {
    pub fn capability(&self) -> QueryCapability {
        match self {
            Self::LoginInfo => QueryCapability::LoginInfo,
            Self::FriendList => QueryCapability::FriendList,
            Self::GroupList => QueryCapability::GroupList,
            Self::GroupInfo { .. } => QueryCapability::GroupInfo,
            Self::GroupMembers { .. } => QueryCapability::GroupMembers,
            Self::UinToUid { .. } => QueryCapability::UinToUid,
            Self::UidToUin { .. } => QueryCapability::UidToUin,
            Self::MessagesById { .. } => QueryCapability::MessagesById,
            Self::VoiceInfo { .. } => QueryCapability::VoiceInfo,
            Self::VoiceDownload { .. } => QueryCapability::VoiceDownload,
            Self::ExactVoiceDownload { .. } => QueryCapability::ExactVoiceDownload,
            Self::ImageInfo { .. } => QueryCapability::ImageInfo,
            Self::ImageDownload { .. } => QueryCapability::ImageDownload,
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::GroupInfo { group_id } | Self::GroupMembers { group_id, .. } if !positive_decimal(group_id) => Err("invalid group id"),
            Self::UinToUid { uin } if !positive_decimal(uin) => Err("invalid UIN"),
            Self::UidToUin { uid } if !native_uid(uid) => Err("invalid UID"),
            Self::VoiceInfo {
                peer_uid,
                message_id,
                element_id,
            }
            | Self::VoiceDownload {
                peer_uid,
                message_id,
                element_id,
            } if !native_uid(peer_uid)
                || !native_message_id(message_id)
                || !native_message_id(element_id) =>
            {
                Err("invalid voice element identity")
            }
            Self::MessagesById {
                chat_type,
                peer_uid,
                message_ids,
            } => {
                if !matches!(chat_type, 1 | 2)
                    || (*chat_type == 1 && !native_uid(peer_uid))
                    || (*chat_type == 2 && !positive_decimal(peer_uid))
                    || message_ids.is_empty()
                    || message_ids.len() > 64
                    || message_ids.iter().any(|id| {
                        id.is_empty() || id.len() > 128 || !id.bytes().all(|b| b.is_ascii_digit())
                    })
                {
                    return Err("invalid exact-message query");
                }
                let mut ids = std::collections::HashSet::new();
                if message_ids.iter().any(|id| !ids.insert(id)) {
                    return Err("duplicate message id");
                }
                Ok(())
            }
            Self::ExactVoiceDownload {
                chat_type,
                peer_uid,
                sender_uin,
                message_id,
                element_id,
            }
            | Self::ImageInfo {
                chat_type,
                peer_uid,
                sender_uin,
                message_id,
                element_id,
            }
            | Self::ImageDownload {
                chat_type,
                peer_uid,
                sender_uin,
                message_id,
                element_id,
            } if !match *chat_type {
                1 => native_uid(peer_uid),
                2 => positive_decimal(peer_uid),
                _ => false,
            } || !positive_decimal(sender_uin)
                || !native_message_id(message_id)
                || !native_message_id(element_id) =>
            {
                Err("invalid exact media identity")
            }
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoginInfo {
    pub user_id: String,
    pub nickname: String,
    pub online: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FriendInfo {
    pub user_id: String,
    pub nickname: String,
    pub remark: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupInfo {
    pub group_id: String,
    pub group_name: String,
    pub member_count: u32,
    pub max_member_count: u32,
}

/// A complete bounded member snapshot for one requested group. Native UID keys
/// and private profile fields are consumed in the bridge and never cross IPC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupMembers {
    pub group_id: String,
    pub members: Vec<GroupMember>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupMember {
    pub user_id: String,
    pub nickname: String,
    pub card: String,
    pub sex: MemberSex,
    pub age: u32,
    pub level: String,
    pub qq_level: u32,
    pub join_time: String,
    pub last_sent_time: String,
    pub role: MemberRole,
    pub title: String,
    pub is_robot: bool,
    pub shut_up_timestamp: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberSex {
    Male,
    Female,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberRole {
    Owner,
    Admin,
    Member,
}

impl GroupMembers {
    pub fn matches(&self, group_id: &str) -> bool {
        let timestamp = |v: &str| {
            !v.is_empty()
                && v.len() <= 20
                && v.bytes().all(|b| b.is_ascii_digit())
                && v.parse::<u64>().is_ok()
        };
        self.group_id == group_id
            && positive_decimal(group_id)
            && self.members.len() <= MAX_QUERY_ITEMS
            && self.members.iter().all(|m| {
                positive_decimal(&m.user_id)
                    && m.nickname.len() <= 4096
                    && m.card.len() <= 4096
                    && m.title.len() <= 4096
                    && timestamp(&m.level)
                    && timestamp(&m.join_time)
                    && timestamp(&m.last_sent_time)
            })
            && self
                .members
                .iter()
                .map(|m| &m.user_id)
                .collect::<std::collections::HashSet<_>>()
                .len()
                == self.members.len()
    }
}

/// Owned metadata only. The wrapper's local file path and media credentials
/// are excluded from the bridge protocol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceInfo {
    pub peer_uid: String,
    pub message_id: String,
    pub element_id: String,
    pub size: u32,
    pub md5: String,
}

impl VoiceInfo {
    fn matches(&self, peer_uid: &str, message_id: &str, element_id: &str, limit: u32) -> bool {
        self.peer_uid == peer_uid
            && (native_uid(peer_uid) || positive_decimal(peer_uid))
            && self.message_id == message_id
            && native_message_id(message_id)
            && self.element_id == element_id
            && native_message_id(element_id)
            && (1..=limit).contains(&self.size)
            && self.md5.len() == 32
            && self
                .md5
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceDownload {
    pub info: VoiceInfo,
    pub silk_hex: String,
}

/// Only owned, non-secret metadata crosses IPC. PNG is the first admitted format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageInfo {
    pub chat_type: u32,
    pub peer_uid: String,
    pub sender_uin: String,
    pub message_id: String,
    pub element_id: String,
    pub size: u32,
    pub md5: String,
    pub width: u32,
    pub height: u32,
    pub pic_type: u32,
}

impl ImageInfo {
    fn matches(&self, query: &HostQuery, limit: u32) -> bool {
        let (HostQuery::ImageInfo {
            chat_type,
            peer_uid,
            sender_uin,
            message_id,
            element_id,
        }
        | HostQuery::ImageDownload {
            chat_type,
            peer_uid,
            sender_uin,
            message_id,
            element_id,
        }) = query
        else {
            return false;
        };
        query.validate().is_ok()
            && self.chat_type == *chat_type
            && self.sender_uin == *sender_uin
            && self.size >= 33
            && self.peer_uid == *peer_uid
            && self.message_id == *message_id
            && self.element_id == *element_id
            && self.size <= limit
            && self.md5.len() == 32
            && self
                .md5
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            && matches!(self.pic_type, 1000 | 1001)
            && (1..=32768).contains(&self.width)
            && (1..=32768).contains(&self.height)
            && u64::from(self.width) * u64::from(self.height) <= 100_000_000
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageDownload {
    pub info: ImageInfo,
    pub png_hex: String,
}

impl ImageDownload {
    fn valid_png_header(&self) -> bool {
        self.png_hex.len() >= 66
            && self.png_hex.starts_with("89504e470d0a1a0a0000000d49484452")
            && u32::from_str_radix(&self.png_hex[32..40], 16) == Ok(self.info.width)
            && u32::from_str_radix(&self.png_hex[40..48], 16) == Ok(self.info.height)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum QueryData {
    LoginInfo(LoginInfo),
    FriendList(Vec<FriendInfo>),
    GroupList(Vec<GroupInfo>),
    GroupInfo(GroupInfo),
    GroupMembers(GroupMembers),
    Uid(String),
    Uin(String),
    Messages(Vec<crate::wrapper_message::WrapperMessage>),
    VoiceInfo(VoiceInfo),
    VoiceDownload(VoiceDownload),
    ImageInfo(ImageInfo),
    ImageDownload(ImageDownload),
}

impl QueryData {
    pub fn validate_for(&self, query: &HostQuery, scope: &QueryScope) -> Result<(), &'static str> {
        let group_ok = |g: &GroupInfo| {
            positive_decimal(&g.group_id)
                && g.group_name.len() <= 4096
                && g.member_count <= g.max_member_count
        };
        let valid = match (query, self) {
            (HostQuery::LoginInfo, Self::LoginInfo(info)) => {
                info.user_id == scope.account && info.nickname.len() <= 4096
            }
            (HostQuery::FriendList, Self::FriendList(list)) => {
                list.len() <= MAX_QUERY_ITEMS
                    && list.iter().all(|f| {
                        positive_decimal(&f.user_id)
                            && f.nickname.len() <= 4096
                            && f.remark.len() <= 4096
                    })
                    && list
                        .iter()
                        .map(|f| &f.user_id)
                        .collect::<std::collections::HashSet<_>>()
                        .len()
                        == list.len()
            }
            (HostQuery::GroupList, Self::GroupList(list)) => {
                list.len() <= MAX_QUERY_ITEMS
                    && list.iter().all(group_ok)
                    && list
                        .iter()
                        .map(|g| &g.group_id)
                        .collect::<std::collections::HashSet<_>>()
                        .len()
                        == list.len()
            }
            (HostQuery::GroupInfo { group_id }, Self::GroupInfo(info)) => {
                &info.group_id == group_id && group_ok(info)
            }
            (HostQuery::UinToUid { .. }, Self::Uid(uid)) => native_uid(uid),
            (HostQuery::GroupMembers { group_id, .. }, Self::GroupMembers(list)) => list.matches(group_id),
            (HostQuery::UidToUin { .. }, Self::Uin(uin)) => positive_decimal(uin),
            (HostQuery::ImageInfo { .. }, Self::ImageInfo(info)) => {
                info.matches(query, MAX_IMAGE_BYTES) && (info.pic_type == 1001 || info.sender_uin == scope.account)
            }
            (HostQuery::ImageDownload { .. }, Self::ImageDownload(download)) => {
                download.info.matches(query, MAX_IMAGE_DOWNLOAD_BYTES)
                    && (download.info.pic_type == 1001 || download.info.sender_uin == scope.account)
                    && download.png_hex.len() == download.info.size as usize * 2
                    && download
                        .png_hex
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                    && download.valid_png_header()
            }
            (
                HostQuery::VoiceInfo {
                    peer_uid,
                    message_id,
                    element_id,
                },
                Self::VoiceInfo(info),
            ) => info.matches(peer_uid, message_id, element_id, MAX_VOICE_BYTES),
            (
                HostQuery::VoiceDownload {
                    peer_uid,
                    message_id,
                    element_id,
                }
                | HostQuery::ExactVoiceDownload {
                    peer_uid,
                    message_id,
                    element_id,
                    ..
                },
                Self::VoiceDownload(download),
            ) => {
                let info = &download.info;
                query.validate().is_ok()
                    && info.matches(peer_uid, message_id, element_id, MAX_VOICE_DOWNLOAD_BYTES)
                    && download.silk_hex.len() == info.size as usize * 2
                    && download
                        .silk_hex
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                    && (download.silk_hex.starts_with("232153494c4b5f5633")
                        || download.silk_hex.starts_with("02232153494c4b5f5633"))
            }
            (
                HostQuery::MessagesById {
                    chat_type,
                    peer_uid,
                    message_ids,
                },
                Self::Messages(list),
            ) => {
                let mut seen = std::collections::HashSet::new();
                list.len() == message_ids.len()
                    && list.iter().all(|record| {
                        record.chat_type == *chat_type
                            && &record.peer_uid == peer_uid
                            && message_ids.contains(&record.msg_id)
                            && seen.insert(&record.msg_id)
                    })
            }
            _ => false,
        };
        if !valid {
            return Err("query response contract mismatch");
        }
        if let Self::Messages(records) = self {
            for record in records {
                let envelope = crate::wrapper_message::WrapperMessageEnvelope {
                    binding: crate::wrapper_message::WrapperBinding {
                        account: scope.account.clone(),
                        session_generation: scope.session_generation,
                        host_nonce: scope.host_nonce,
                        wrapper_session: "query-result".into(),
                    },
                    wrapper_session: "query-result".into(),
                    source: crate::wrapper_message::WrapperSource::Update,
                    observed_at_unix_ms: 1,
                    record: record.clone(),
                };
                envelope
                    .validate()
                    .map_err(|_| "invalid exact-message record")?;
            }
        }
        if serde_json::to_vec(self)
            .map_err(|_| "query encoding")?
            .len()
            > MAX_QUERY_BYTES
        {
            return Err("query response over budget");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "status",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum QueryOutcome {
    Success(QueryData),
    Unsupported,
    NotReady,
    PermissionDenied,
    Rejected { code: i64, reason: String },
    Timeout,
    Disconnected,
    InvalidResponse,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_operations_validate_identity_and_response_shape() {
        let scope = QueryScope {
            account: "10001".into(),
            host_nonce: 1,
            session_generation: 1,
            connection_epoch: 2,
        };
        assert!(scope.validate().is_ok());
        assert!(HostQuery::UinToUid { uin: "0".into() }.validate().is_err());
        assert!(HostQuery::UidToUin {
            uid: "arbitrary script".into()
        }
        .validate()
        .is_err());
        let query = HostQuery::GroupInfo {
            group_id: "20002".into(),
        };
        let info = GroupInfo {
            group_id: "30003".into(),
            group_name: "group".into(),
            member_count: 1,
            max_member_count: 2,
        };
        assert!(QueryData::GroupInfo(info)
            .validate_for(&query, &scope)
            .is_err());
        assert!(serde_json::from_str::<HostQuery>(r#"{"kind":"eval","params":"script"}"#).is_err());
        let empty = QueryData::FriendList(vec![]);
        assert!(empty.validate_for(&HostQuery::FriendList, &scope).is_ok());
        assert!(empty.validate_for(&HostQuery::GroupList, &scope).is_err());
    }
}
