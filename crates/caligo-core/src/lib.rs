//! Caligo core:进程外核心(library 形态)。
//!
//! K1 范围:IPC 帧编解码与握手校验([`ipc`])。
//! K4-D3 范围:请求 journal 与事件账本([`journal`])、账号 actor
//! ([`runtime`]) —— 生命周期、代次身份、请求状态机与恢复;不实现任何
//! QQ 调用,native 侧由 bridge/夹具经显式 API 驱动。

pub mod account_manager;
pub mod capability_broker;
pub mod daemon;
pub mod ipc;
pub mod journal;
pub mod management_http;
pub mod media_tool;
pub mod message_store;
pub mod onebot;
pub mod onebot_events;
pub mod onebot_http;
pub mod onebot_mcp;
pub mod onebot_server;
pub mod onebot_sse;
pub mod onebot_transport;
pub mod qq_protocol;
pub mod runtime;
pub mod transport;

/// core 构建标识,进入握手 HelloAck。
pub const CORE_BUILD: &str = concat!("caligo-core ", env!("CARGO_PKG_VERSION"));
