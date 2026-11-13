//! K4-D3 请求 journal 与事件账本(计划 §6.6/§7-D3)。
//!
//! 设计约束(全部来自计划 §6.6):
//! - append-only;core 在确认 accepted 前先持久记录请求,派发前记录
//!   dispatch_intent —— "意图已记录、桥执行确认未收到"的窗口显式化;
//! - 记录带版本、长度、CRC32 校验;损坏处理分级:**尾部**截断/半条可恢复
//!   (截到最近完好记录并如实报告),**中段**损坏 → `Err(Corrupt)`,
//!   调用方(core runtime)必须进入 Degraded 并拒绝发送,不得静默重建;
//! - 总量上限(默认 1 GiB)到达或写失败 → `CapacityExhausted`/`Io`,
//!   发送入口关闭;未实现可证明归档前不自动删除旧记录;
//! - fsync 策略显式配置(`Always` / `PerRecord`)。
//!
//! 序列化用 JSON(与仓库现有 serde 栈一致);二进制外框做长度与校验。
//! 帧格式:`[u32 magic][u16 rec_ver][u16 kind][u32 len][u32 crc32][payload]`。

use std::io::{Read, Seek, SeekFrom, Write};

use serde::{Deserialize, Serialize};

/// 帧魔数 "CLGJ"(Caligo Journal)。
pub const RECORD_MAGIC: u32 = 0x4A_47_4C_43;
/// 记录外框版本(外框布局变化时递增;payload 内另有自描述版本)。
pub const RECORD_VERSION: u16 = 1;
/// journal 默认总量上限:1 GiB(计划 §6.6 建议值;实测后可在 D11 调整)。
pub const DEFAULT_MAX_BYTES: u64 = 1024 * 1024 * 1024;

/// journal 记录体。`v` 字段是 payload 自身版本,新增字段必须带默认值以保
/// 旧记录可读;语义不兼容时递增 `v` 并由读取侧拒绝。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "t", content = "d")]
pub enum JournalRecord {
    /// 请求状态迁移(含首条 accepted 记录)。
    Request(RequestRecord),
    /// 接收事件持久化(EventAck 的前提:先落账,后前移 ACK)。
    Event(EventRecord),
    /// 事件交付游标(core 已把事件交付给下游的确认点)。
    EventDelivered { event_seq: u64 },
    /// 观察缺口(缓存不足/断线窗口/溢出),范围或精度限制如实记录。
    Gap(GapRecord),
    /// 迟到/重复/旧代次回执的幂等审计(不改变动作状态)。
    ReceiptAudit(ReceiptAuditRecord),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReceiptAuditRecord {
    pub v: u32,
    pub request_id: String,
    pub session_generation: u64,
    pub detail: String,
    pub at_unix_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RequestRecord {
    pub v: u32,
    pub request_id: String,
    pub payload_hash: u64,
    /// 迁移后的状态名(与 caligo_model::ActionState 的 JSON 对应)。
    pub state: String,
    /// ConfirmedSuccess 的稳定原生消息 ID;旧 ConfirmedSuccess 空值只在
    /// 恢复时解释为 AcceptedUnconfirmed,原始记录保持不变。
    pub native_id: Option<String>,
    pub at_unix_ms: u64,
    /// 记录该状态所属会话代次(旧代次回执只更新旧代次审计)。
    pub session_generation: u64,
    /// core 分配并在派发前持久化的 QQ client_seq；旧记录没有此字段。
    #[serde(default)]
    pub wire_seq: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EventRecord {
    pub v: u32,
    pub event_seq: u64,
    pub session_generation: u64,
    /// 会话键三元的 JSON(caligo_model::SessionKey)。
    pub session: serde_json::Value,
    pub direction: String,
    pub sender: String,
    pub native_id: String,
    pub text: String,
    pub platform_time: Option<u64>,
    /// Retain old-generation observations without indexing them as messages.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub audit_only: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wrapper_message: Option<caligo_model::wrapper_message::WrapperMessageEnvelope>,
    /// Old journal rows remain valid and never gain guessed OneBot fields.
    #[serde(default)]
    pub onebot_projection: Option<caligo_model::OneBotMessageProjection>,
    pub observed_at_unix_ms: u64,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GapRecord {
    pub v: u32,
    /// 起点(含)与终点(含);终点未知填 None —— 不得编造缺失条数。
    pub from_event_seq: u64,
    pub to_event_seq: Option<u64>,
    pub reason: String,
    pub at_unix_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncMode {
    /// 每条记录 `sync_data`(最保守;性能换持久性)。
    Always,
    /// 每条 `flush`,仅在显式 checkpoint 时 `sync_data`。
    FlushOnly,
}

#[derive(Debug)]
pub enum JournalError {
    Io(std::io::Error),
    /// 中段损坏(完好记录之后跟有非法数据)。原文件保持不动。
    Corrupt {
        offset: u64,
        reason: String,
    },
    /// 总量上限到达:拒绝新写入,不删除旧记录。
    CapacityExhausted,
    RecordTooLarge {
        len: usize,
    },
}

impl core::fmt::Display for JournalError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            JournalError::Io(e) => write!(f, "journal io: {e}"),
            JournalError::Corrupt { offset, reason } => {
                write!(f, "journal corrupt at offset {offset}: {reason}")
            }
            JournalError::CapacityExhausted => write!(f, "journal capacity exhausted"),
            JournalError::RecordTooLarge { len } => write!(f, "journal record too large: {len}"),
        }
    }
}

impl std::error::Error for JournalError {}

/// 打开时的恢复报告。
#[derive(Debug, Default, Clone, PartialEq)]
pub struct RecoveryReport {
    /// 成功读回的记录(按序)。
    pub records: Vec<JournalRecord>,
    /// 尾部不完整/坏记录被截去的字节数(0 = 无尾部损伤)。
    pub tail_truncated_bytes: u64,
}

/// append-only journal。单 owner(core runtime)串行使用;`&mut self` 即所有权。
pub struct Journal {
    file: std::fs::File,
    path: std::path::PathBuf,
    size: u64,
    max_bytes: u64,
    sync: SyncMode,
}

impl Journal {
    /// 打开(不存在则创建)。读回全部记录做恢复:
    /// - 尾部半条/坏 CRC → 截断到最近完好记录,`tail_truncated_bytes` 如实报告;
    /// - **中段**损坏(完好记录后跟非法帧)→ `Err(Corrupt)`,**不修改文件**。
    ///
    /// 返回可写 journal 与恢复报告(记录按序)。
    pub fn open(
        path: impl Into<std::path::PathBuf>,
        max_bytes: u64,
        sync: SyncMode,
    ) -> Result<(Journal, RecoveryReport), JournalError> {
        let path = path.into();
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(JournalError::Io)?;
        let size_before = file.metadata().map_err(JournalError::Io)?.len();
        let mut buf = Vec::with_capacity(size_before.min(64 * 1024 * 1024) as usize);
        file.seek(SeekFrom::Start(0)).map_err(JournalError::Io)?;
        file.read_to_end(&mut buf).map_err(JournalError::Io)?;

        let mut records = Vec::new();
        let mut off: usize = 0;
        let mut tail_truncated: u64 = 0;
        loop {
            if buf.len() - off == 0 {
                break;
            }
            match decode_record(&buf[off..]) {
                Ok((rec_len, rec)) => {
                    records.push(rec);
                    off += rec_len;
                }
                Err(DecodeError::Incomplete) => {
                    // 尾部半条(写入中途断电/崩溃):截断到最近完好记录。
                    // 防御:若"半条"之后仍有完好记录,说明是坏长度字段而非
                    // 真实尾部 → 按中段损坏拒绝。
                    if has_decodable_after(&buf[off + 1..]) {
                        return Err(JournalError::Corrupt {
                            offset: off as u64,
                            reason: "torn record followed by valid records".into(),
                        });
                    }
                    tail_truncated = (buf.len() - off) as u64;
                    break;
                }
                Err(DecodeError::Bad(reason)) => {
                    if off == 0 {
                        // 首记录即坏:视为整文件损坏,不动文件。
                        return Err(JournalError::Corrupt { offset: 0, reason });
                    }
                    // 坏帧之后仍有可解码记录 → 中段损坏,拒绝;
                    // 否则按尾部截断处理(可能只是最后一条写坏)。
                    if has_decodable_after(&buf[off + 1..]) {
                        return Err(JournalError::Corrupt {
                            offset: off as u64,
                            reason,
                        });
                    }
                    tail_truncated = (buf.len() - off) as u64;
                    break;
                }
            }
        }
        let good_len = (buf.len() - tail_truncated as usize) as u64;
        if tail_truncated > 0 {
            file.set_len(good_len).map_err(JournalError::Io)?;
        }
        file.seek(SeekFrom::Start(good_len))
            .map_err(JournalError::Io)?;
        let journal = Self {
            file,
            path,
            size: good_len,
            max_bytes,
            sync,
        };
        Ok((
            journal,
            RecoveryReport {
                records,
                tail_truncated_bytes: tail_truncated,
            },
        ))
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn max_bytes(&self) -> u64 {
        self.max_bytes
    }

    /// 追加一条记录(写 + flush,按 [`SyncMode`] 决定是否 sync_data)。
    pub fn append(&mut self, rec: &JournalRecord) -> Result<(), JournalError> {
        let frame = encode_record(rec)?;
        if self.size + frame.len() as u64 > self.max_bytes {
            return Err(JournalError::CapacityExhausted);
        }
        self.file.write_all(&frame).map_err(JournalError::Io)?;
        self.file.flush().map_err(JournalError::Io)?;
        if self.sync == SyncMode::Always {
            self.file.sync_data().map_err(JournalError::Io)?;
        }
        self.size += frame.len() as u64;
        Ok(())
    }

    /// 显式 checkpoint(FlushOnly 模式下强制落盘)。
    pub fn checkpoint(&mut self) -> Result<(), JournalError> {
        self.file.sync_data().map_err(JournalError::Io)
    }
}

// —— 编解码 ——

fn encode_record(rec: &JournalRecord) -> Result<Vec<u8>, JournalError> {
    let payload = serde_json::to_vec(rec).map_err(|e| JournalError::Io(e.into()))?;
    if payload.len() > u32::MAX as usize {
        return Err(JournalError::RecordTooLarge { len: payload.len() });
    }
    let mut out = Vec::with_capacity(16 + payload.len());
    out.extend_from_slice(&RECORD_MAGIC.to_le_bytes());
    out.extend_from_slice(&RECORD_VERSION.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // kind 保留位
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&crc32(&payload).to_le_bytes());
    out.extend_from_slice(&payload);
    Ok(out)
}

#[derive(Debug)]
enum DecodeError {
    Incomplete,
    Bad(String),
}

/// `buf` 从一条记录的起点开始解码。
fn decode_record(buf: &[u8]) -> Result<(usize, JournalRecord), DecodeError> {
    if buf.len() < 16 {
        // 也可能头部本身就是坏帧:长度不足 16 无法判别 → 按半条等待处理。
        // (open 场景文件不会再增长,这里返回 Incomplete 由上层按尾部截断。)
        return if buf.iter().all(|b| *b == 0) {
            Err(DecodeError::Incomplete)
        } else {
            Err(DecodeError::Bad("record header truncated".into()))
        };
    }
    let magic = u32::from_le_bytes(buf[0..4].try_into().unwrap());
    if magic != RECORD_MAGIC {
        return Err(DecodeError::Bad(format!("magic mismatch: {magic:#010x}")));
    }
    let ver = u16::from_le_bytes(buf[4..6].try_into().unwrap());
    if ver != RECORD_VERSION {
        return Err(DecodeError::Bad(format!("record version {ver}")));
    }
    let len = u32::from_le_bytes(buf[8..12].try_into().unwrap()) as usize;
    if buf.len() < 16 + len {
        return Err(DecodeError::Incomplete);
    }
    let payload = &buf[16..16 + len];
    let want = u32::from_le_bytes(buf[12..16].try_into().unwrap());
    let got = crc32(payload);
    if want != got {
        return Err(DecodeError::Bad(format!(
            "crc mismatch: want {want:#010x} got {got:#010x}"
        )));
    }
    let rec: JournalRecord = serde_json::from_slice(payload)
        .map_err(|e| DecodeError::Bad(format!("payload json: {e}")))?;
    Ok((16 + len, rec))
}

/// `buf` 之后是否存在任何可解码记录(区分中段损坏与尾部损伤)。
/// 恢复期一次性扫描,上限 1 MiB 防失控。
fn has_decodable_after(buf: &[u8]) -> bool {
    let limit = buf.len().min(1024 * 1024);
    for start in 0..limit {
        if decode_record(&buf[start..]).is_ok() {
            return true;
        }
    }
    false
}

/// CRC32(IEEE 802.3,无外部依赖)。
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for b in data {
        crc ^= u32::from(*b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_event_rows_decode_without_onebot_projection() {
        let row: EventRecord = serde_json::from_value(serde_json::json!({
            "v": 1,
            "event_seq": 4,
            "session_generation": 2,
            "session": {"account":"10001", "kind":"group", "peer":"123456"},
            "direction": "incoming",
            "sender": "opaque-sender",
            "native_id": "native-1",
            "text": "hello",
            "platform_time": null,
            "observed_at_unix_ms": 5,
            "source": "recv"
        }))
        .unwrap();
        assert!(row.onebot_projection.is_none());
    }

    fn sample_req(id: &str) -> JournalRecord {
        JournalRecord::Request(RequestRecord {
            v: 1,
            request_id: id.into(),
            payload_hash: 42,
            state: "Queued".into(),
            native_id: None,
            at_unix_ms: 1_000,
            session_generation: 7,
            wire_seq: None,
        })
    }

    #[test]
    fn crc32_known_vector() {
        // 标准测试向量:"123456789" → 0xCBF43926。
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn roundtrip_record() {
        let rec = sample_req("r1");
        let frame = encode_record(&rec).unwrap();
        let (n, back) = decode_record(&frame).unwrap();
        assert_eq!(n, frame.len());
        assert_eq!(back, rec);
    }

    #[test]
    fn open_append_recover_roundtrip() {
        let dir = std::env::temp_dir().join(format!("caligo-journal-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("j1.log");
        let _ = std::fs::remove_file(&path);
        {
            let (mut j, report) =
                Journal::open(&path, DEFAULT_MAX_BYTES, SyncMode::Always).unwrap();
            assert!(report.records.is_empty());
            j.append(&sample_req("a")).unwrap();
            j.append(&JournalRecord::Gap(GapRecord {
                v: 1,
                from_event_seq: 3,
                to_event_seq: None,
                reason: "test".into(),
                at_unix_ms: 2_000,
            }))
            .unwrap();
        }
        let (_, report) = Journal::open(&path, DEFAULT_MAX_BYTES, SyncMode::Always).unwrap();
        assert_eq!(report.records.len(), 2);
        assert_eq!(report.tail_truncated_bytes, 0);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn tail_half_record_is_truncated_and_reported() {
        let dir = std::env::temp_dir().join(format!("caligo-journal-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("j2.log");
        let _ = std::fs::remove_file(&path);
        let good = encode_record(&sample_req("g")).unwrap();
        let mut bytes = good.clone();
        bytes.extend_from_slice(&good[..10]); // 半条
        std::fs::write(&path, &bytes).unwrap();
        let (_, report) = Journal::open(&path, DEFAULT_MAX_BYTES, SyncMode::Always).unwrap();
        assert_eq!(report.records.len(), 1);
        assert_eq!(report.tail_truncated_bytes, 10);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn mid_file_corruption_is_rejected_and_file_untouched() {
        let dir = std::env::temp_dir().join(format!("caligo-journal-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("j3.log");
        let _ = std::fs::remove_file(&path);
        let mut bytes = encode_record(&sample_req("g1")).unwrap();
        let good2 = encode_record(&sample_req("g2")).unwrap();
        let good3 = encode_record(&sample_req("g3")).unwrap();
        let corrupt_at = bytes.len();
        bytes.extend_from_slice(&good2);
        bytes.extend_from_slice(&good3);
        // 翻坏第二条(g2)的魔数 → 其后仍有完好记录(g3)→ 中段损坏。
        bytes[corrupt_at] ^= 0xFF;
        std::fs::write(&path, &bytes).unwrap();
        let before = std::fs::read(&path).unwrap();
        let err = match Journal::open(&path, DEFAULT_MAX_BYTES, SyncMode::Always) {
            Err(e) => e,
            Ok(_) => panic!("mid-file corruption must be rejected"),
        };
        assert!(matches!(err, JournalError::Corrupt { .. }), "{err}");
        let after = std::fs::read(&path).unwrap();
        assert_eq!(before, after, "损坏文件必须原样保留");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn capacity_exhausted_refuses_without_truncating() {
        let dir = std::env::temp_dir().join(format!("caligo-journal-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("j4.log");
        let _ = std::fs::remove_file(&path);
        let (mut j, _) = Journal::open(&path, 4096, SyncMode::FlushOnly).unwrap();
        let mut appended = 0;
        for i in 0..100 {
            match j.append(&sample_req(&format!("request-{i}"))) {
                Ok(()) => appended += 1,
                Err(JournalError::CapacityExhausted) => break,
                Err(e) => panic!("unexpected: {e}"),
            }
        }
        assert!(appended > 0, "至少一条能写入");
        let size_after = j.size();
        assert!(size_after <= 4096);
        // 显著超限的记录必须被拒,且 size 不变(未写入半条)。
        assert!(matches!(
            j.append(&sample_req(&"x".repeat(8192))),
            Err(JournalError::CapacityExhausted)
        ));
        assert_eq!(j.size(), size_after);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }
}
