//! Fixed read-only host operations. The wire never carries executable scripts.

use serde::{Deserialize, Serialize};

pub const MAX_QUERY_BYTES: usize = 512 * 1024;
pub const MAX_QUERY_ITEMS: usize = 2048;
pub const MAX_QUERY_TIMEOUT_MS: u64 = 30_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryCapability {
    LoginInfo,
    FriendList,
    GroupList,
    GroupInfo,
    UinToUid,
    UidToUin,
    MessagesById,
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
            Self::UinToUid { .. } => QueryCapability::UinToUid,
            Self::UidToUin { .. } => QueryCapability::UidToUin,
            Self::MessagesById { .. } => QueryCapability::MessagesById,
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::GroupInfo { group_id } if !positive_decimal(group_id) => Err("invalid group id"),
            Self::UinToUid { uin } if !positive_decimal(uin) => Err("invalid UIN"),
            Self::UidToUin { uid } if !native_uid(uid) => Err("invalid UID"),
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
    Uid(String),
    Uin(String),
    Messages(Vec<crate::wrapper_message::WrapperMessage>),
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
            (HostQuery::UidToUin { .. }, Self::Uin(uin)) => positive_decimal(uin),
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
