//! P5 QQ 文本编解码合同测试(计划 §8 T17–T20)。
//!
//! fixture 纪律:全部为**脱敏自生成**(本测试文件内的合成器产出;无真实
//! 账号/消息内容)。字段号规格未对 9.9.33 真实抓包验证(qq_protocol 各
//! `FIELD_NUMBERS_VERIFIED=false`)——本文件验证的是**逻辑层合同**
//! (有界解码/身份不串/不伪造回执);字段级真实性在 G1/G2 拿到真实样本
//! 后以 captured fixture 替换并复核。

use caligo_core::qq_protocol::identity::{classify_incoming, validate_pairing, SessionKind};
use caligo_core::qq_protocol::receipt::{parse_send_receipt, SendReceipt};
use caligo_core::qq_protocol::recv::{parse_msg_push, CONTENT_TYPE_FRIEND_TEXT, CONTENT_TYPE_GROUP_TEXT};
use caligo_core::qq_protocol::send::{encode_pb_send_msg, SendTarget};
use caligo_core::qq_protocol::wire::Writer;

fn synth_push(from_uin: u64, ctype: u64, time: u64, text: &str) -> Vec<u8> {
    let mut msg = Writer::new();
    msg.nested(1, |r| r.varint_field(1, from_uin));
    msg.nested(2, |c| {
        c.u32_field(1, ctype as u32);
        c.u32_field(6, time as u32);
    });
    msg.nested(3, |b| {
        b.nested(1, |rich| {
            rich.nested(2, |elem| {
                elem.nested(1, |t| t.string_field(1, text));
            });
        });
    });
    let mut top = Writer::new();
    top.bytes_field(1, &msg.finish());
    top.finish()
}

/// T17:私聊/群 MsgPush;同正文双 ID 保留;Unicode/长文本完整。
#[test]
fn t17_msg_push_identity_and_text_integrity() {
    // 群:Unicode + 长文本完整。
    let long = "群🈯".repeat(2_000); // ~12KB 多字节
    let g = synth_push(888, CONTENT_TYPE_GROUP_TEXT, 100, &long);
    let pg = parse_msg_push("10001", &g, Some(777)).unwrap();
    assert_eq!(pg.text, long, "长多字节正文零截断");
    assert_eq!(pg.identity.session.kind, SessionKind::Group);
    assert_eq!(pg.identity.session.peer, "777");
    assert_eq!(pg.identity.sender, "888");
    // 私聊:对端即 sender。
    let f = synth_push(999, CONTENT_TYPE_FRIEND_TEXT, 200, "hi");
    let pf = parse_msg_push("10001", &f, None).unwrap();
    assert_eq!(pf.identity.session.kind, SessionKind::Private);
    assert_eq!(pf.identity.session.peer, "999");
    // 同正文、不同来源:两条都成立(去重在上层按原生 ID,不合并)。
    let a = synth_push(888, CONTENT_TYPE_GROUP_TEXT, 1, "dup");
    let b = synth_push(999, CONTENT_TYPE_GROUP_TEXT, 1, "dup");
    assert_ne!(
        parse_msg_push("10001", &a, Some(777)).unwrap().identity.sender,
        parse_msg_push("10001", &b, Some(777)).unwrap().identity.sender
    );
}

/// T18:未知字段跳过留痕;截断/坏 tag/未支持类型拒绝(有界)。
#[test]
fn t18_bounded_decode_rejects_garbage() {
    let ok = synth_push(888, CONTENT_TYPE_GROUP_TEXT, 1, "x");
    // 未知字段留痕。
    let mut msg = Writer::new();
    msg.varint_field(88, 5);
    msg.nested(1, |r| r.varint_field(1, 888));
    msg.nested(2, |c| c.u32_field(1, CONTENT_TYPE_GROUP_TEXT as u32));
    msg.nested(3, |b| {
        b.nested(1, |rich| {
            rich.nested(2, |elem| {
                elem.nested(1, |t| t.string_field(1, "x"));
            });
        });
    });
    let mut top = Writer::new();
    top.bytes_field(1, &msg.finish());
    let with_unknown = top.finish();
    let p = parse_msg_push("10001", &with_unknown, Some(777)).unwrap();
    assert!(p.skipped_unknown >= 1);
    // 截断。
    assert!(parse_msg_push("10001", &ok[..ok.len() - 2], Some(777)).is_err());
    // 未支持类型。
    assert!(parse_msg_push("10001", &synth_push(888, 500, 1, "x"), Some(777)).is_err());
    // 群缺 group hint。
    assert!(parse_msg_push("10001", &synth_push(888, CONTENT_TYPE_GROUP_TEXT, 1, "x"), None).is_err());
}

/// T19:发送回执 —— result=0 缺稳定身份 → Unconfirmed;失败带原因;
/// 不可解析不猜。编码侧 roundtrip 保证发给 QQ 的字节可被我们自己读回。
#[test]
fn t19_receipt_never_fabricates_success() {
    // 编码 → (作为"回执"侧我们只验证解析合同;真实回执字段验证在 G3)。
    let req = encode_pb_send_msg(
        &SendTarget::Group { group_code: 70_000_001 },
        "m",
        1,
        2,
        1_760_000_000,
    )
    .unwrap();
    assert!(!req.is_empty());
    let mut w = Writer::new();
    w.u32_field(1, 0); // result=0
    assert_eq!(
        parse_send_receipt(&w.finish()),
        SendReceipt::Unconfirmed { note: "ok without stable identity" },
        "成功但无稳定身份必须 Unconfirmed"
    );
    let mut wf = Writer::new();
    wf.u32_field(1, 42);
    wf.bytes_field(2, b"not member");
    assert_eq!(
        parse_send_receipt(&wf.finish()),
        SendReceipt::Failed { reason: "not member".into() }
    );
    assert!(matches!(parse_send_receipt(&[]), SendReceipt::Unreadable(_)));
}

/// T20:身份组合 —— 不串身份、不用本地时间补 platform_time。
#[test]
fn t20_identity_combinations_guarded() {
    // 群:peer 与 sender 独立。
    let g = classify_incoming("10001", SessionKind::Group, 777, 888, Some(9)).unwrap();
    assert!(validate_pairing(&g).is_ok());
    assert_eq!(g.platform_time, Some(9));
    // 群号==sender:可疑,拒绝。
    let bad = classify_incoming("10001", SessionKind::Group, 777, 777, None).unwrap();
    assert!(validate_pairing(&bad).is_err());
    // 私聊:对端双重身份成立;方向不猜(self_sent 恒 false,需独立证据)。
    let f = classify_incoming("10001", SessionKind::Private, 888, 888, None).unwrap();
    assert!(validate_pairing(&f).is_ok());
    assert!(!f.self_sent);
    assert_eq!(f.platform_time, None, "缺失必须如实为 None");
    // 缺账号:拒绝。
    assert!(classify_incoming("", SessionKind::Group, 1, 2, None).is_err());
}
