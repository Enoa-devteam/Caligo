//! OneBot 事件 fanout(N4;计划 §7-N4)。
//!
//! 有界订阅者(独立背压,计划 §9):每个订阅者一条有界 mpsc;发布时
//! try_send,满则**丢弃该订阅者的本条事件并计数**(不阻塞发布者、不阻塞
//! QQ 线程;丢弃数对订阅者可见)。订阅者断开(send 失败)自动注销。
//!
//! 事件内容 = JSON 文本(WS 推送 / HTTP POST 上报共用同一 fanout)。

use std::collections::HashMap;
use std::sync::mpsc;

use caligo_model::{Direction, EventSource, MsgEvent, OneBotMessageProjection, SessionKind};
use serde_json::{json, Map, Value};

/// 单订阅者队列上限(条)。
pub const SUBSCRIBER_QUEUE_CAP: usize = 256;

#[derive(Default)]
pub struct EventBus {
    next_id: u64,
    subscribers: HashMap<u64, (mpsc::SyncSender<String>, String, u64)>,
    /// 各订阅者累计丢弃数(诊断;随注销清除)。
    pub dropped_total: u64,
}

impl EventBus {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册订阅者(名称供诊断);返回接收端。
    pub fn subscribe(&mut self, name: &str) -> (u64, mpsc::Receiver<String>) {
        let (tx, rx) = mpsc::sync_channel::<String>(SUBSCRIBER_QUEUE_CAP);
        let id = self.next_id;
        self.next_id += 1;
        self.subscribers.insert(id, (tx, name.to_string(), 0));
        (id, rx)
    }

    pub fn unsubscribe(&mut self, id: u64) -> bool {
        self.subscribers.remove(&id).is_some()
    }

    pub fn subscriber_count(&self) -> usize {
        self.subscribers.len()
    }

    /// Queue losses for one current subscription; absent after unsubscribe.
    pub fn subscriber_dropped(&self, id: u64) -> Option<u64> {
        self.subscribers.get(&id).map(|(_, _, dropped)| *dropped)
    }

    /// 发布事件(JSON 文本)到全部订阅者;满队列丢弃并计数。
    /// 返回 (送达数, 丢弃数)。
    pub fn publish(&mut self, event_json: &str) -> (usize, u64) {
        let mut delivered = 0usize;
        let mut dropped = 0u64;
        let mut dead = Vec::new();
        for (id, (tx, _name, drops)) in self.subscribers.iter_mut() {
            match tx.try_send(event_json.to_string()) {
                Ok(()) => delivered += 1,
                Err(mpsc::TrySendError::Full(_)) => {
                    *drops += 1;
                    dropped += 1;
                }
                Err(mpsc::TrySendError::Disconnected(_)) => dead.push(*id),
            }
        }
        for id in dead {
            self.subscribers.remove(&id);
        }
        self.dropped_total += dropped;
        (delivered, dropped)
    }
}

/// Build a OneBot V11 message event only when every required standard field
/// has an explicit source. In particular, do not infer subtype/font or
/// identities from the local sequence, text, or opaque sender UID.
pub fn message_event_json(self_id: u64, message_id: i32, event: &MsgEvent) -> Option<String> {
    if self_id == 0
        || message_id <= 0
        || event.direction != Direction::Incoming
        || event.source != EventSource::Recv
    {
        return None;
    }
    let projection: &OneBotMessageProjection = event.onebot_projection.as_ref()?;
    if projection.peer_id == 0 || projection.sender_user_id == 0 || event.text.is_empty() {
        return None;
    }
    let (message_type, sub_type) = match (event.session.kind, projection.sub_type.as_deref()?) {
        (SessionKind::Private, "friend") => ("private", "friend"),
        (SessionKind::Group, "normal") => ("group", "normal"),
        _ => return None,
    };
    let font = projection.font?;
    let mut sender = Map::new();
    sender.insert("user_id".into(), Value::from(projection.sender_user_id));
    if let Some(nickname) = projection.sender_nickname.as_ref() {
        sender.insert("nickname".into(), Value::String(nickname.clone()));
    }
    let mut body = json!({
        "time": projection.time,
        "self_id": self_id,
        "post_type": "message",
        "message_type": message_type,
        "sub_type": sub_type,
        "message_id": message_id,
        "user_id": projection.sender_user_id,
        "message": [{"type":"text", "data":{"text": event.text.as_str()}}],
        "raw_message": event.text.as_str(),
        "font": font,
        "sender": Value::Object(sender),
    });
    if event.session.kind == SessionKind::Group {
        body["group_id"] = Value::from(projection.peer_id);
        body["anonymous"] = Value::Null;
    }
    serde_json::to_string(&body).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use caligo_model::{Direction, EventSource, NativeMessageId, SessionKey};

    #[test]
    fn publish_reaches_all_subscribers_with_echo_of_payload() {
        let mut bus = EventBus::new();
        let (id1, mut rx1) = bus.subscribe("ws-1");
        let (_id2, mut rx2) = bus.subscribe("http-post-1");
        let (d, dr) = bus.publish(r#"{"post_type":"message","raw_message":"hi"}"#);
        assert_eq!((d, dr), (2, 0));
        assert!(rx1.recv().unwrap().contains("hi"));
        assert!(rx2.recv().unwrap().contains("hi"));
        bus.unsubscribe(id1);
        assert_eq!(bus.subscriber_count(), 1);
    }

    #[test]
    fn slow_subscriber_drops_without_blocking_and_dead_unsubscribes() {
        let mut bus = EventBus::new();
        let (_slow_id, _slow_rx) = bus.subscribe("slow");
        // slow 订阅者的接收端已 move 走但未读 → 填满后丢弃。
        let (_fast_id, mut fast_rx) = bus.subscribe("fast");
        let mut dropped_total = 0u64;
        for i in 0..(SUBSCRIBER_QUEUE_CAP + 10) {
            let ev = format!(r#"{{"i":{i}}}"#);
            let (_, dr) = bus.publish(&ev);
            dropped_total += dr;
            // fast 订阅者随取随清,不应丢弃。
            let _ = fast_rx.try_recv().unwrap();
        }
        assert!(dropped_total > 0, "慢订阅者超限必须丢弃并计数");
        assert_eq!(bus.dropped_total, dropped_total);
        // 断开的订阅者:发布时自动注销。
        drop(_slow_rx);
        let (d, _) = bus.publish(r#"{"i":x}"#);
        assert_eq!(d, 1, "仅 fast 收到");
        assert_eq!(bus.subscriber_count(), 1);
    }

    fn incoming_event(kind: SessionKind) -> MsgEvent {
        MsgEvent {
            event_seq: 1,
            session_generation: 1,
            session: SessionKey::new("10001", kind, "20002"),
            direction: Direction::Incoming,
            sender: "opaque uid".into(),
            native_id: NativeMessageId("native-1".into()),
            text: "hello [CQ:at,qq=1]".into(),
            platform_time: None,
            wrapper_message: None,
            onebot_projection: Some(OneBotMessageProjection {
                peer_id: 20002,
                sender_user_id: 30003,
                real_id: 4,
                time: 123,
                sender_nickname: None,
                sub_type: Some(
                    if kind == SessionKind::Group {
                        "normal"
                    } else {
                        "friend"
                    }
                    .into(),
                ),
                font: Some(0),
            }),
            observed_at_unix_ms: 999,
            source: EventSource::Recv,
        }
    }

    #[test]
    fn message_event_requires_complete_explicit_v11_fields_and_keeps_text_literal() {
        let event = incoming_event(SessionKind::Group);
        let value: Value =
            serde_json::from_str(&message_event_json(10001, 9, &event).unwrap()).unwrap();
        assert_eq!(value["post_type"], "message");
        assert_eq!(value["message_type"], "group");
        assert_eq!(value["sub_type"], "normal");
        assert_eq!(value["message_id"], 9);
        assert_eq!(value["message"][0]["type"], "text");
        assert_eq!(value["message"][0]["data"]["text"], "hello [CQ:at,qq=1]");
        assert_eq!(value["raw_message"], "hello [CQ:at,qq=1]");
        assert_eq!(value["anonymous"], Value::Null);
        assert!(value["sender"].get("nickname").is_none());

        let mut incomplete = event;
        incomplete.onebot_projection.as_mut().unwrap().font = None;
        assert!(message_event_json(10001, 9, &incomplete).is_none());
        incomplete.onebot_projection.as_mut().unwrap().font = Some(0);
        incomplete.onebot_projection.as_mut().unwrap().sub_type = None;
        assert!(message_event_json(10001, 9, &incomplete).is_none());
    }
}
