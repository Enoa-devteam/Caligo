//! Shared read-only account display for the local browser and terminal.

use crate::daemon::Daemon;
use crate::message_store::{ChatType, StoredMessage};
use caligo_model::capability::{HostQuery, QueryData, QueryOutcome};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub(crate) const MESSAGE_LIMIT: usize = 100;

pub(crate) fn parse_account(text: &str) -> Option<u64> {
    if text.starts_with('0') || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse::<u64>().ok().filter(|account| *account > 0)
}

#[derive(Serialize)]
pub(crate) struct MessageRow {
    pub message_id: i32,
    pub time: i32,
    pub chat_type: &'static str,
    pub peer: String,
    pub sender: Option<String>,
    pub nickname: Option<String>,
    pub preview: String,
}

fn preview(value: &str, limit: usize) -> String {
    // Some legacy rows can be very large. Bound character count before copying.
    let mut chars = value.chars();
    let mut result: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        result.push('…');
    }
    result
}

impl From<&StoredMessage> for MessageRow {
    fn from(message: &StoredMessage) -> Self {
        let content = if let Some(text) = message.message.as_str() {
            preview(text, 480)
        } else if let Some(segments) = message.message.as_array() {
            let mut result = String::new();
            for segment in segments.iter().take(16) {
                match segment["type"].as_str() {
                    Some("text") => result.push_str(&preview(
                        segment["data"]["text"].as_str().unwrap_or_default(),
                        480,
                    )),
                    Some("image") => result.push_str("[图片]"),
                    Some("record") => result.push_str("[语音]"),
                    _ => result.push_str("[消息元素]"),
                }
                if result.chars().count() > 480 {
                    break;
                }
            }
            if segments.len() > 16 && result.chars().count() <= 480 {
                result.push('…');
            }
            preview(&result, 480)
        } else {
            "[消息]".into()
        };
        Self {
            message_id: message.message_id,
            time: message.time,
            chat_type: match message.identity.chat_type {
                ChatType::Group => "group",
                ChatType::Private => "private",
            },
            peer: preview(&message.identity.peer_id, 96),
            sender: message.sender["user_id"].as_u64().map(|id| id.to_string()),
            nickname: message.sender["nickname"]
                .as_str()
                .map(|name| preview(name, 96)),
            preview: content,
        }
    }
}

/// Snapshot the active connection under the same actor lock as the phase.
pub(crate) fn connection_id(daemon: &Daemon) -> Option<String> {
    let actor = daemon.actor.lock().ok()?;
    if daemon.stop_flag().load(Ordering::Acquire) || actor.phase() != caligo_model::Phase::Active {
        return None;
    }
    let connection = actor.connection()?;
    Some(format!(
        "{:016x}-{}",
        connection.core_instance_nonce, connection.connection_epoch
    ))
}

pub(crate) fn verify_login(
    daemon: &Daemon,
    cancel: &AtomicBool,
) -> Result<(String, String), &'static str> {
    let id = connection_id(daemon).ok_or("QQ 桥接尚未连接")?;
    let outcome =
        daemon.query_host_cancellable(HostQuery::LoginInfo, Duration::from_secs(4), cancel);
    if connection_id(daemon).as_ref() != Some(&id) || cancel.load(Ordering::Acquire) {
        return Err("连接已变化，请核对当前 QQ 后重新确认");
    }
    match outcome {
        QueryOutcome::Success(QueryData::LoginInfo(info))
            if info.online && info.user_id == daemon.config.account =>
        {
            Ok((id, preview(&info.nickname, 96)))
        }
        QueryOutcome::Unsupported => Err("当前桥接不支持登录核验"),
        QueryOutcome::Timeout => Err("登录核验超时"),
        QueryOutcome::NotReady => Err("桥接尚未就绪"),
        _ => Err("未能核验 QQ 登录与所选账号一致"),
    }
}

pub(crate) fn recent_messages(daemon: &Daemon) -> Result<Vec<MessageRow>, &'static str> {
    daemon
        .actor
        .lock()
        .map(|actor| actor.recent_messages())
        .map_err(|_| "账号消息账本不可用")
}
