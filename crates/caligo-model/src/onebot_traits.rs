//! OneBot 动作层共用类型(N4 下沉;core 与 bridge 共用,防依赖环)。

use serde_json::Value;

/// OneBot send target. Private `user_id` is a UIN and must not be treated as a
/// resolved QQ UID by the native encoder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendTarget {
    Group { group_id: u64 },
    Private { user_id: u64 },
}

/// 发送载荷(已通过参数校验;wire 编码与原生派发由实现方承担)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendPayload {
    pub target: SendTarget,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendDisposition {
    /// Durable queue acceptance only; no OneBot message ID is known.
    AcceptedUnconfirmed,
    /// A single dispatch ended without a stable public message identity.
    /// Callers must report an unknown result and must not retry automatically.
    DeliveryUnknown { reason: String },
    /// This transport/target pair has no proven implementation path.
    Unsupported { reason: String },
    /// Terminal receipt and persistent OneBot message ID are available.
    Confirmed { message_id: i32 },
}

/// 发送汇(N3 NativeSession 实现;此处解耦以便测试)。
pub trait SendSink {
    /// Dispatch once. A queued result is not a successful standard OneBot send.
    fn send(&mut self, payload: &SendPayload) -> Result<SendDisposition, String>;
    fn send_image(&mut self, _target: SendTarget, _png: Vec<u8>) -> Result<SendDisposition, String> {
        Ok(SendDisposition::Unsupported {reason:"当前宿主未支持图片发送".into()})
    }
    fn send_private_voice(&mut self, _user_id: u64, _silk: Vec<u8>) -> Result<SendDisposition, String> {
        Ok(SendDisposition::Unsupported {
            reason: "当前宿主未支持私聊语音发送".into(),
        })
    }
    fn send_group_voice(&mut self, _group_id: u64, _silk: Vec<u8>) -> Result<SendDisposition, String> {
        Ok(SendDisposition::Unsupported {
            reason: "当前宿主未支持群语音发送".into(),
        })
    }
}

/// Complete message fields required by OneBot get_msg.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredMessageView {
    pub message_id: i32,
    pub message_type: String,
    pub real_id: i32,
    pub sender: Value,
    pub message: Value,
    pub time: i32,
}

/// 消息查询源(消息存储实现)。
pub trait StoreReader {
    /// 按已持久化的 OneBot int32 message_id 查询完整标准消息。
    fn get_msg(&self, message_id: i32) -> Option<StoredMessageView>;
    /// Resolve a local voice file token within this reader's account.
    fn voice_info(&self, _file: &str) -> Option<crate::capability::VoiceInfo> {
        None
    }
    fn voice_record(&self, file: &str) -> Option<(crate::capability::VoiceInfo, crate::capability::HostQuery)> {
        let info = self.voice_info(file)?;
        let query = crate::capability::HostQuery::VoiceDownload {
            peer_uid: info.peer_uid.clone(),
            message_id: info.message_id.clone(),
            element_id: info.element_id.clone(),
        };
        Some((info, query))
    }
    fn image_info(&self, _file: &str) -> Option<crate::capability::ImageInfo> { None }
    /// Login identity is absent until the host provides both account and nickname.
    fn login_info(&self) -> Option<(u64, String)>;
    /// Actual daemon/session state, absent until wired to the live actor.
    fn online_status(&self) -> Option<bool>;
    /// Group ids learned from persisted group messages; absent until observed.
    /// Empty by default so readers without a message store stay truthful.
    fn group_peers(&self) -> Vec<String> {
        Vec::new()
    }
}

pub fn voice_file_token(account_id: u64, message_id: i32) -> String {
    format!("caligo-voice:{account_id}:{message_id}")
}

pub fn image_file_token(account_id: u64, message_id: i32) -> String {
    format!("caligo-image:{account_id}:{message_id}")
}

/// 解析 message 段:字符串或标准数组,提取纯文本;未支持段显式失败。
pub fn extract_text(message: &Value) -> Result<String, String> {
    match message {
        Value::String(s) => Ok(s.clone()),
        Value::Array(items) => {
            let mut out = String::new();
            for item in items {
                let ty = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
                match ty {
                    "text" => {
                        out.push_str(
                            item.get("data")
                                .and_then(|d| d.get("text"))
                                .and_then(|v| v.as_str())
                                .unwrap_or(""),
                        );
                    }
                    "" => return Err("message 段缺少 type".into()),
                    other => {
                        return Err(format!("未支持消息段 {other}(N6 后续;显式拒绝不静默)"));
                    }
                }
            }
            if out.is_empty() {
                return Err("message 为空".into());
            }
            Ok(out)
        }
        _ => Err("message 须为字符串或段数组".into()),
    }
}
