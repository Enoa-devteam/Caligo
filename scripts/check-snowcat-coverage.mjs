#!/usr/bin/env node
/** Caligo capability mapping validator and refresh. Node builtins only.
 * Reference inventory is immutable input. Never runs reference code or QQ.
 * Default: run the real Rust registry example, validate coverage and semantics.
 * --registry-json <path>: validate a previously captured registry (offline only).
 * --refresh: regenerate derived mappings and registry/counts, preserving reviews.
 * --init: create the initial map; refuses an existing output file.
 * --output <path>: override map path (useful for review/test artifacts).
 * Rows use normalized assessment profiles: resolveEntry() yields every required
 * field, without repeating thousands of identical source/acceptance objects.
 */
import { readFileSync, writeFileSync, existsSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
export const INVENTORY_PATH = 'docs/contracts/snowcat-capability-inventory.json';
export const MAP_PATH = 'docs/contracts/caligo-snowcat-capability-map.json';
export const INVENTORY_SHA256 = '89de4711e2577f5531ba38b282bc47dea38c5a4ed0da4d416867c320fc45bc25';
const COMMIT = '7cf025fe1e7f9c609e02a8643c6e6398eeec0d64';
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const fail = message => { throw new Error(message); };
const json = p => JSON.parse(readFileSync(p, 'utf8'));
const clone = value => structuredClone(value);
const src = (file, symbol) => ({ path: file, symbol });
const ONEBOT = 'crates/caligo-core/src/onebot.rs';
const SERVER = 'crates/caligo-core/src/onebot_server.rs';
const DAEMON = 'crates/caligo-core/src/daemon.rs';
const CLI = 'crates/caligo-core/src/bin/caligod.rs';
const EVENTS = 'crates/caligo-core/src/onebot_events.rs';
const TRANSPORT = 'crates/caligo-core/src/onebot_transport.rs';
const BROKER = 'crates/caligo-core/src/capability_broker.rs';
const QUERY = 'crates/caligo-bridge/src/qq_query.rs';
const RESIDENT = 'crates/caligo-bridge/src/resident.rs';
const MANAGER = 'crates/caligo-core/src/account_manager.rs';
const MANAGEMENT_HTTP = 'crates/caligo-core/src/management_http.rs';
const MEDIA = 'crates/caligo-core/src/media_tool.rs';
const BASELINE_LEDGER = 'docs/execution/2026-11-13-snowcat-capability-integration.md';
const LEDGER = 'docs/execution/2026-11-13-snowcat-capability-integration-continued.md';
export const STATUSES = ['missing', 'unknown', 'partial_offline', 'registered_backend_blocked', 'wired_offline', 'implemented_pending_validation', 'research_lab'];
export const CATEGORIES = ['actions', 'aliases', 'decorators', 'events', 'event_schemas', 'message_segments', 'protocols', 'components',
  'component_surface_exports', 'native_components', 'component_exports', 'native_surfaces', 'wrapper_interfaces', 'webui_routes', 'tools', 'named_contract_gaps'];

// Canonicalize the registry JSON as a lossless decimal-token stream. JSON.parse
// rounds u64::MAX in schemas, so never use JSON.stringify for the raw dump hash.
export function canonicalJsonTokens(raw) {
  let out = ''; let quoted = false; let escaped = false;
  for (const c of raw) {
    if (quoted) {
      out += c;
      if (escaped) escaped = false;
      else if (c === '\\') escaped = true;
      else if (c === '"') quoted = false;
    } else if (c === '"') { quoted = true; out += c; }
    else if (!/\s/.test(c)) out += c;
  }
  JSON.parse(out);
  return out;
}
export function registryDigest(raw) { return hash(canonicalJsonTokens(raw)); }

export function runRegistry() {
  const args = ['run', '--quiet', '--manifest-path', path.join(ROOT, 'Cargo.toml'), '-p', 'caligo-core', '--example',
    'dump_onebot_registry', '--locked', '--offline', '--target-dir', path.join(ROOT, 'target-harness')];
  const result = spawnSync('cargo', args, { cwd: ROOT, encoding: 'utf8', maxBuffer: 16 * 1024 * 1024, windowsHide: true });
  if (result.error || result.status !== 0) fail(`Rust registry export failed (${result.status ?? 'spawn'}); no coverage success claimed.`);
  const registry = JSON.parse(result.stdout);
  if (!Array.isArray(registry)) fail('Registry dump must be an array');
  return { raw: result.stdout, registry, command: 'cargo run --quiet -p caligo-core --example dump_onebot_registry --locked --offline --target-dir target-harness' };
}

export function referenceEntries(inventory) {
  const entries = [];
  for (const category of CATEGORIES) {
    if (category === 'aliases') {
      for (const action of inventory.actions) for (const alias of action.aliases) entries.push({
        id: `onebot.alias.${alias.name}`, category, name: alias.name, base_id: action.id, reference: alias });
    } else for (const item of inventory[category] ?? []) {
      const id = category === 'component_exports' ? `native_export.${item.component}.${item.ordinal}` : item.id;
      if (!id) fail(`Reference item lacks stable ID in ${category}`);
      entries.push({ id, category, name: item.name ?? item.class_name ?? id, reference: item });
    }
  }
  if (new Set(entries.map(e => e.id)).size !== entries.length) fail('Reference identity collision');
  return entries;
}

const noAcceptance = () => ({ declared: false, implemented: false, wired: false, offline: 'not_run', native: 'not_verified', field: 'not_verified',
  production: 'not_accepted', evidence: [], limits: 'No equivalent Caligo behavior has been established for this reference item.' });
const scoped = (status, reason, implementation, normal_entry, permission, backend_dependencies, evidence, limits) => ({
  status, reason, implementation, normal_entry, permission, backend_dependencies,
  supported_versions: null,
  acceptance: { declared: true, implemented: true, wired: status === 'wired_offline' || status === 'partial_offline',
    offline: status === 'implemented_pending_validation' ? 'pending_final_validation' : 'recorded_lab_only', native: 'not_verified', field: 'not_verified',
    production: 'not_accepted', evidence, limits },
});

export function initialProfiles() {
  const sharedEntry = [src(CLI, 'fn main()'), src(SERVER, 'pub fn action_handler('), src(ONEBOT, 'pub fn handle_action(')];
  const actionPermission = { requirement: 'Transport authentication; read/write classification is discovery metadata only.',
    enforcement: [src(ONEBOT, 'pub enum ActionPermission'), src('crates/caligo-core/src/onebot_http.rs', 'fn token_ok(')],
    limits: 'No per-action role/administrator policy is inferred. HTTP/WS token enforcement differs by configured transport.' };
  const registryLab = [src(LEDGER, '共享动作与查询'), src(ONEBOT, 'fn registry_has_unique_names_and_resolvable_canonical_actions()')];
  const transportLab = [src(LEDGER, 'default/research 全仓 PASS'), src(TRANSPORT, 'mod tests')];
  const missing = { status: 'missing', reason: 'No audited Caligo equivalent implementation/normal entry is mapped; reference declaration or name similarity is insufficient.',
    implementation: null, normal_entry: null, permission: { requirement: null, enforcement: [], limits: 'Missing handler; capability permission contract unimplemented.' },
    backend_dependencies: [], supported_versions: null, acceptance: noAcceptance() };
  const actionMissing = { ...clone(missing), reason: 'Absent from executable ACTION_REGISTRY; the shared action handler returns unsupported for this name.',
    rejection_evidence: [src(ONEBOT, 'let Some(definition) = resolve_action(action) else')],
    acceptance: { ...noAcceptance(), limits: 'Unsupported response/parameter validation is not a backend implementation.' } };
  const directory = { ...clone(actionMissing), reason: 'Directory actions are present in ACTION_REGISTRY, but host query execution and end-to-end validation are not final; no directory backend is credited yet.',
    acceptance: { ...noAcceptance(), limits: 'Registration and query plumbing are not end-to-end backend validation.' } };
  const directoryPending = scoped('implemented_pending_validation', 'Action registry, typed core query path, and fixed host-query implementation exist; host integration and final end-to-end validation remain pending.',
    [src(ONEBOT, 'pub fn handle_action_with_queries('), src(BROKER, "pub(crate) fn begin<'a>("), src(QUERY, 'pub const SUPPORTED_CAPABILITIES: &[QueryCapability]')],
    [src(CLI, 'fn main()'), src(SERVER, 'pub fn action_handler('), src(ONEBOT, 'pub fn handle_action_with_queries(')], actionPermission,
    [{ name: 'Scoped query broker and fixed wrapper query executor', state: 'implemented_pending_validation', evidence: [src(BROKER, "pub(crate) fn begin<'a>("), src(QUERY, 'pub const SUPPORTED_CAPABILITIES: &[QueryCapability]')] }],
    [src(LEDGER, '固定 6 项查询'), src('crates/caligo-core/tests/capability_query_lab.rs', '#[test]'), src('crates/caligo-bridge/tests/wrapper_query_contract.rs', '#[test]')],
    'Host integration is not final. No QQ field acceptance; LoginInfo is unsupported and directory OneBot routes remain pending full end-to-end validation.');
  const send = scoped('partial_offline', 'Registered text handler and daemon route exist; reference send surface is broader than literal/text segments and native acceptance remains open.',
    [src(ONEBOT, 'fn send_payload('), src(ONEBOT, 'fn dispatch_send('), src(SERVER, 'impl SendSink for DaemonSendSink')], sharedEntry, actionPermission,
    [{ name: 'Daemon/AccountActor/NativeSession plus ready session, strong receipt and persisted OneBot ID', state: 'offline_path_only', evidence: [src(SERVER, 'fn send(&mut self, payload: &SendPayload)')] },
      { name: 'Private UID identity and full message-segment support', state: 'partial_offline', evidence: [src(LEDGER, '发送能力协商与查询能力分开保存')] }], registryLab,
    'Group text uses the durable daemon path. Private targets require a current capability announcement. Offline tests do not prove native callback lifetime or peer delivery.');
  const privateSend = { ...clone(send), reason: 'Private send is gated by an advertised host capability and requires a verified UIN-to-UID lookup plus exact message identity readback before reporting success.',
    implementation: [src(SERVER, 'fn send(&mut self, payload: &SendPayload)'), src(BROKER, 'pub(crate) fn supports_private_text('), src('crates/caligo-bridge/src/daemon_client.rs', 'fn capability_messages')],
    backend_dependencies: [{ name: 'Advertised private-text capability, scoped UIN-to-UID query, exact readback and persisted identity', state: 'implemented_pending_validation', evidence: [src(BROKER, 'pub(crate) fn supports_private_text('), src(QUERY, 'HostQuery::MessagesById')] }],
    acceptance: { ...clone(send.acceptance), wired: true, limits: 'The fail-closed gate and exact identity path have targeted tests; host end-to-end integration and field acceptance remain open.' } };
  const query = scoped('wired_offline', 'Local persistent get_msg, daemon status, and version actions have normal offline paths; host-backed identity/list/detail queries are tracked separately.',
    [src(ONEBOT, 'pub fn handle_action_with_queries('), src(DAEMON, 'impl StoreReader for Daemon'), src('crates/caligo-core/src/message_store.rs', 'impl StoreReader for MessageStore')], sharedEntry, actionPermission,
    [{ name: 'Daemon StoreReader and persisted MessageStore/session state', state: 'wired_offline', evidence: [src(DAEMON, 'impl StoreReader for Daemon'), src('crates/caligo-core/src/message_store.rs', 'impl StoreReader for MessageStore')] }],
    [src(LEDGER, '消息修订：default/research Store 各 6/6'), src(ONEBOT, 'fn get_msg_returns_complete_standard_fields_and_int32_id()')],
    'get_msg reads locally persisted known IDs, not arbitrary native QQ history. Host-backed login/list/detail behavior is not credited by this assessment.');
  const capabilityQuery = scoped('implemented_pending_validation', 'Bounded scoped broker and fixed wrapper query implementations exist for six advertised query kinds; host integration and end-to-end validation remain pending.',
    [src(BROKER, "pub(crate) fn begin<'a>("), src(QUERY, 'pub const SUPPORTED_CAPABILITIES: &[QueryCapability]'), src(RESIDENT, 'pub fn drain_queries(&self)')],
    [src(DAEMON, 'pub fn query_host('), src('crates/caligo-bridge/src/daemon_client.rs', 'for (request_id, scope, outcome) in resident.take_query_results()')], actionPermission,
    [{ name: 'Per-account/epoch scoped broker and owner-thread query queue', state: 'implemented_pending_validation', evidence: [src(BROKER, "pub(crate) fn begin<'a>("), src(RESIDENT, 'pub fn drain_queries(&self)')] }],
    [src(LEDGER, '固定 6 项查询'), src('crates/caligo-core/tests/capability_query_lab.rs', '#[test]'), src('crates/caligo-bridge/tests/wrapper_query_contract.rs', '#[test]')],
    'Advertised host query set excludes LoginInfo. No QQ field acceptance or completed host end-to-end integration is claimed.');
  const accountManager = scoped('partial_offline', 'AccountManager provides ordinary multi-account configuration, isolated daemon lifecycle, and persistence paths with targeted offline evidence.',
    [src(MANAGER, 'pub struct AccountManager'), src(MANAGER, 'pub fn start_all(&mut self)'), src('crates/caligo-core/src/bin/caligo-manager.rs', 'fn main()')],
    [src('crates/caligo-core/src/bin/caligo-manager.rs', 'fn main()')], actionPermission,
    [{ name: 'Account-scoped directories, credentials, daemon and pipe', state: 'partial_offline', evidence: [src(MANAGER, 'pub fn start(&mut self, account: u64)'), src('crates/caligo-core/tests/account_manager_lab.rs', '#[test]')] }],
    [src(LEDGER, '账号管理：default/research 各 5/5 PASS'), src('crates/caligo-core/tests/manager_smoke.rs', '#[test]')],
    'This is a Caligo manager capability, not equivalence for every reference WebUI route or native surface. No production or QQ field acceptance.');
  const managementUi = scoped('partial_offline', 'Loopback authenticated management HTTP/UI exposes a bounded account/config/lifecycle surface through the normal manager binary.',
    [src(MANAGEMENT_HTTP, 'pub struct ManagementHttpServer'), src(MANAGEMENT_HTTP, 'fn route(')],
    [src('crates/caligo-core/src/bin/caligo-manager.rs', 'ManagementHttpServer::start('), src('crates/caligo-core/src/bin/caligo-manager.rs', '"--management-bind"')], actionPermission,
    [{ name: 'Loopback bearer-authenticated management server over AccountManager', state: 'partial_offline', evidence: [src(MANAGEMENT_HTTP, 'pub fn start('), src('crates/caligo-core/src/bin/caligo-manager.rs', 'ManagementHttpServer::start(')] }],
    [src(LEDGER, '管理 HTTP 修正后 default/research 各 7/7 PASS'), src('crates/caligo-core/tests/management_http_lab.rs', '#[test]')],
    'Only explicitly implemented management routes are mapped; this is not parity with all 129 reference WebUI routes. Host query integration and production acceptance are not claimed.');
  const mediaTool = scoped('partial_offline', 'A normal caligo-media CLI and bounded local ffmpeg/ffprobe process contract exist with fake-runner and owned-child evidence.',
    [src(MEDIA, 'pub struct MediaTool'), src(MEDIA, 'pub fn audio_to_pcm('), src(MEDIA, 'pub fn pcm_to_container('), src(MEDIA, 'pub fn thumbnail(')],
    [src('crates/caligo-core/src/bin/caligo-media.rs', 'fn main()'), src('crates/caligo-core/src/bin/caligo-media.rs', 'fn run(cli: Cli)')], actionPermission,
    [{ name: 'Configured local ffmpeg/ffprobe executables with hash, timeout and output bounds', state: 'partial_offline', evidence: [src(MEDIA, 'pub fn verify_tools('), src(MEDIA, 'impl MediaProcessRunner for OsProcessRunner')] }],
    [src(LEDGER, '媒体工具：default `media_tool_lab` 6 PASS'), src('crates/caligo-core/tests/media_tool_lab.rs', '#[test]')],
    'No real FFmpeg conversion, QQ upload, voice/Silk path, or native acceptance was verified; the entire FFmpeg/native surface is not implemented.');
  const voiceDownload = scoped('partial_offline', 'The exact-profile Caligo bridge downloads bounded private and group Silk or ordinary single PNG images through scoped daemon host queries.',
    [src(QUERY, 'function startMediaDownload('), src('crates/caligo-bridge/src/qq_v8.rs', 'onRichMediaDownloadComplete:')],
    [src(DAEMON, 'pub fn query_host('), src(RESIDENT, 'pub fn drain_queries(&self)')],
    { requirement: 'Account-scoped Caligo bridge IPC and exact host profile.', enforcement: [src(QUERY, 'fn matches_voice_profile('), src(BROKER, "pub(crate) fn begin<'a>(")], limits: 'Available through the daemon Rust host-query API.' },
    [{ name: 'Wrapper exact message readback, matching callback, and bounded file integrity validation', state: 'offline_only', evidence: [src(QUERY, 'function startMediaDownload(')] }],
    [src(LEDGER, 'A2 私聊语音有界下载'), src('crates/caligo-bridge/tests/wrapper_query_contract.rs', 'fn voice_download_waits_for_correlated_callback_and_verifies_owned_file()'), src('crates/caligo-core/tests/capability_query_lab.rs', 'fn voice_download_requires_scoped_advertisement_and_exact_owned_response()'), src('crates/caligo-bridge/tests/wrapper_image_contract.rs', 'fn image_download_binds_callback_file_scope_integrity_and_lifetime()')],
    'Private and group Silk; ordinary PNG images only through Rust host-query API. OneBot image segments, upload, other image formats, and Caligo QQ field acceptance remain open.');
  voiceDownload.supported_versions = { declared: ['9.9.36-53644 pilot'], accepted: [], reason: 'Exact loaded-module files are hash checked. Caligo has only offline evidence.' };
  const record = scoped('partial_offline', 'Private and group record segments support bounded Tencent SILK sends and durable message readback. get_record retrieves original Silk and optionally converts it to WAV or OGG.',
    [src(ONEBOT, 'fn get_record('), src(ONEBOT, 'fn send_payload('), src(EVENTS, 'voice_file_token('), src('crates/caligo-core/src/media_tool.rs', 'pub fn silk_to_container(')], sharedEntry, actionPermission,
    [...clone(voiceDownload.backend_dependencies),
      { name: 'WAV and OGG conversion through explicitly configured local tools', state: 'offline_only', evidence: [src(SERVER, 'fn voice_media(')] },
      { name: 'Scoped private and group voice dispatch and verified native readback', state: 'offline_only', evidence: [src(DAEMON, 'pub fn submit_voice('), src('crates/caligo-bridge/src/qq_v8.rs', 'pub fn build_voice_send_js(')] }],
    [src(LEDGER, 'A2 OneBot 语音读取'), src(LEDGER, 'A2 独立 SILK 转码'), src(LEDGER, 'A2 OneBot 私聊语音发送'), src(LEDGER, 'A2 OneBot 群语音发送'), src('crates/caligo-core/tests/wrapper_message_chain.rs', 'fn d8_owned_records_reach_journal_and_only_current_incoming_messages_fan_out()'), src('crates/caligo-core/tests/onebot_private_send_lab.rs', 'fn private_record_waits_for_exact_readback_to_be_persisted()'), src('crates/caligo-core/tests/onebot_private_send_lab.rs', 'fn group_record_requires_group_capability_and_exact_group_projection()')],
    'Only short Tencent SILK v3 Base64 can be sent. WAV and OGG retrieval requires a pinned decoder and FFmpeg. QQ field acceptance remains open.');
  record.supported_versions = clone(voiceDownload.supported_versions);
  const protocolPermission = { requirement: 'Loopback binding and configured token/secret per transport.', enforcement: [src(SERVER, 'pub fn bind_loopback(')],
    limits: 'Protocol wiring does not grant QQ administrator permissions or add missing actions.' };
  const login = { ...clone(query), status: 'partial_offline',
    reason: 'Registry and StoreReader route exist, but normal-session login identity/nickname binding is incomplete; get_login_info can return not-ready.',
    acceptance: { ...clone(query.acceptance), limits: 'Offline fake-store login response only; actual normal-path nickname/account result is not established.' } };
  const profiles = { missing, action_missing: actionMissing, directory_missing: directory, directory_pending: directoryPending, send_text: send, private_send_blocked: privateSend, query, capability_query: capabilityQuery, login_partial: login, account_manager: accountManager, management_ui: managementUi, media_tool: mediaTool, voice_download: voiceDownload };
  for (const [id, symbol, file, flag, dep] of [
    ['http', 'pub fn serve_http(', SERVER, '--onebot-http-bind', 'DaemonSendSink and shared registry'],
    ['http_post', 'pub fn run_http_post_ready(', TRANSPORT, '--onebot-http-post-url', 'Daemon ordered HTTP POST event bus and quick operations'],
    ['ws_server', 'pub fn serve_forward_ws(', TRANSPORT, '--onebot-ws-bind', 'Daemon event bus, shared action handler and forward WS authentication'],
    ['ws_client', 'pub fn run_reverse_ws(', TRANSPORT, '--onebot-ws-reverse-url', 'Daemon event bus, reverse endpoint/token and reconnect loop'],
  ]) profiles[id] = scoped('wired_offline', 'Concrete transport is assembled by normal caligod and shares the real action/event layer; offline transport evidence only.',
    [src(file, symbol)], [src(CLI, flag), src(file, symbol)], protocolPermission,
    [{ name: dep, state: 'wired_offline', evidence: [src(file, symbol)] }], transportLab,
    'Only implemented actions/events are available; reference behavioral parity and QQ/native acceptance are not established.');
  for (const [id, file, symbol, flag, test] of [
    ['sse', 'crates/caligo-core/src/onebot_sse.rs', 'pub fn serve_sse(', '--onebot-sse-bind', 'crates/caligo-core/tests/onebot_sse_lab.rs'],
    ['mcp', 'crates/caligo-core/src/onebot_mcp.rs', 'pub fn serve_mcp(', '--onebot-mcp-bind', 'crates/caligo-core/tests/onebot_mcp_lab.rs'],
  ]) profiles[id] = scoped('wired_offline', 'Concrete transport is assembled by normal caligod; final targeted, default/research workspace and parallel research validation passed.',
    [src(file, symbol)], [src(CLI, flag)], { requirement: 'Configured Bearer token; loopback binding.', enforcement: [src(file, symbol)], limits: 'Transport authentication covers implemented actions/events only; no QQ role or native capability is granted.' },
    [{ name: 'Shared daemon bus/registry and validated normal caligod transport assembly', state: 'wired_offline', evidence: [src(file, symbol)] }],
    [src(test, '#[test]'), src(BASELINE_LEDGER, '最终全仓双构建 PASS')], 'Only implemented actions/events are available. Final offline transport/workspace checks do not establish reference behavioral parity, QQ/native, field or production acceptance.');
  profiles.text_segment = scoped('partial_offline', 'Literal text/text-segment input and text-only event projection exist; broad message conversion/media support is absent.',
    [src('crates/caligo-model/src/onebot_traits.rs', 'pub fn extract_text('), src(EVENTS, 'pub fn message_event_json(')], sharedEntry,
    actionPermission, clone(send.backend_dependencies), registryLab,
    'Input supports text only. Incoming event requires explicit projection/time/identity/subtype/font; native producer completeness and reference parity remain open.');
  profiles.message_event = scoped('partial_offline', 'OneBot incoming private/group text projection and daemon fanout exist with explicit required-field gates.',
    [src(EVENTS, 'pub fn message_event_json('), src('crates/caligo-core/src/message_store.rs', 'pub fn projection_for_event(')], [src(DAEMON, 'publish_onebot_message')],
    { requirement: 'Authenticated event consumer; explicit account/session identity.', enforcement: [src(EVENTS, 'pub fn message_event_json(')], limits: 'No notice/request/meta-event production is inferred.' },
    [{ name: 'Current wrapper binding, durable message identity and incoming receive event', state: 'offline_only', evidence: [src(DAEMON, 'actor.wrapper_message_projection(&event)'), src(EVENTS, 'event.onebot_projection.as_ref()?')] }],
    [src(LEDGER, '普通私聊与群聊的事件接线'), src('crates/caligo-core/tests/wrapper_message_chain.rs', 'fn d8_owned_records_reach_journal_and_only_current_incoming_messages_fan_out()')],
    'Coverage includes ordinary private/group incoming text and single private/group incoming voice on the fixed profile. Self-sent notifications, temporary chat and real QQ acceptance remain open.');
  profiles.record = record;
  profiles.profile_service = scoped('research_lab', 'Research-only exact-key ProfileService UIN/UID round-trip exists; normal persistent backend wiring and field result remain open.',
    [src('crates/caligo-bridge/src/uid_map_probe.rs', 'pub(crate) fn run_profile('), src('crates/caligo-bridge/src/qq_v8.rs', 'fn build_profile_uid_map_query_js(')],
    [src('crates/caligo-cli/src/qqentry.rs', '--uid-map-profile-service')],
    { requirement: 'research build plus exact-hash pilot and explicit field admission; no live call in this task.', enforcement: [src('crates/caligo-bridge/src/uid_map_probe.rs', 'pub(crate) fn run_profile(')], limits: 'Research feature does not authorize field interaction.' },
    [{ name: 'Owner pump/V8/session/ProfileService Map exact-key contract', state: 'research_lab', evidence: [src('crates/caligo-bridge/src/qq_v8.rs', 'fn profile_uid_map_checks_exact_keys_and_rejects_unexpected_results()')] }],
    [src(BASELINE_LEDGER, '4/4 PASS')], '9.9.36-53644 pilot contract only; native runtime result, stable identity linkage and lifetime remain unverified.');
  profiles.profile_service.supported_versions = { declared: ['9.9.36-53644 pilot'], accepted: [], reason: 'Exact local module hashes required; no blanket native acceptance.' };
  profiles.component_partial = scoped('partial_offline', 'Caligo owns a narrower Rust core/store/OneBot implementation; this is functional overlap, not replacement of the full reference component.',
    [src(ONEBOT, 'pub static ACTION_REGISTRY'), src(DAEMON, 'pub struct Daemon'), src('crates/caligo-core/src/message_store.rs', 'pub struct MessageStore')],
    [src(CLI, 'fn main()')], actionPermission, clone(query.backend_dependencies), [src(LEDGER, 'default/research 全仓 PASS')],
    'No full component API/export/feature equivalence is claimed; all unmatched reference exports remain separately missing.');
  return profiles;
}

export function chooseAssessment(entry, registry) {
  const r = registry.find(r => r.name === entry.name);
  if (entry.category === 'actions' || entry.category === 'aliases') {
    if (!r) return ['get_group_info', 'get_group_list', 'get_friend_list', 'get_group_member_info', 'get_group_member_list'].includes(entry.name.replace(/_(async|rate_limited)$/, '')) ? 'directory_missing' : 'action_missing';
    if (['get_friend_list', 'get_group_list', 'get_group_info'].includes(entry.name)) return 'directory_pending';
    if (r.handler.kind === 'send') return r.handler.target === 'private' ? 'private_send_blocked' : 'send_text';
    if (r.handler.kind === 'get_login_info') return 'login_partial';
    if (r.handler.kind === 'get_message') return 'query';
    if (r.handler.kind === 'get_record') return 'record';
    if (['get_status', 'get_version_info'].includes(entry.name)) return 'query';
    return 'action_missing';
  }
  if (entry.category === 'protocols') return ({ 'OB11HttpServerAdapter': 'http', 'OB11HttpClientAdapter': 'http_post',
    'OB11WebSocketServerAdapter': 'ws_server', 'OB11WebSocketClientAdapter': 'ws_client', 'OB11HttpSSEServerAdapter': 'sse', 'OB11McpServerAdapter': 'mcp' })[entry.name] ?? 'missing';
  if (entry.id === 'message_segment.text') return 'text_segment';
  if (entry.id === 'message_segment.record') return 'record';
  if (['event.OB11BaseMessageEvent', 'event_schema.OB11MessageSchema'].includes(entry.id)) return 'message_event';
  if (entry.category === 'wrapper_interfaces') {
    if (entry.name === 'downloadRichMedia' && entry.reference.interface === 'NodeIKernelMsgService') return 'voice_download';
    if (entry.name === 'getUidByUin' && entry.reference.interface === 'NodeIKernelProfileService') return 'profile_service';
    if (entry.name === 'getUinByUid' && entry.reference.interface === 'NodeIKernelProfileService') return 'profile_service';
    if (entry.name === 'getUidByUins' && entry.reference.interface === 'NodeIKernelGroupService') return 'capability_query';
    if (entry.name === 'getUinByUids' && entry.reference.interface === 'NodeIKernelGroupService') return 'capability_query';
    if (entry.name === 'getMsgsByMsgId' && entry.reference.interface === 'NodeIKernelMsgService') return 'capability_query';
    if (entry.name === 'getBuddyListV2' && entry.reference.interface === 'NodeIKernelBuddyService') return 'capability_query';
    if (entry.name === 'getCoreAndBaseInfo' && entry.reference.interface === 'NodeIKernelProfileService') return 'capability_query';
    if (entry.name === 'getGroupList' && entry.reference.interface === 'NodeIKernelGroupService') return 'capability_query';
    if (entry.name === 'getGroupDetailInfo' && entry.reference.interface === 'NodeIKernelGroupService') return 'capability_query';
  }
  if (entry.id === 'native_surface.FFmpeg.decodeAudioToPCM') return 'media_tool';
  if (entry.category === 'webui_routes' && ['webui_route.OB11Config.ts.15', 'webui_route.OB11Config.ts.17'].includes(entry.id)) return 'management_ui';
  if (['component.snowcat-core', 'component.snowcat-onebot', 'component.snowcat-database'].includes(entry.id)) return 'component_partial';
  return 'missing';
}

export function resolveEntry(map, row) {
  const profile = map.assessments[row.assessment];
  if (!profile) fail(`Unknown assessment ${row.assessment}`);
  return { ...clone(profile), ...clone(row), ...(row.review ?? {}) };
}
export function summarize(map) {
  const byCategory = {};
  for (const category of CATEGORIES) {
    const rows = map.entries.filter(e => e.category === category);
    const statuses = Object.fromEntries(STATUSES.map(s => [s, rows.filter(r => r.status === s).length]));
    byCategory[category] = { denominator: rows.length, statuses,
      native_accepted: rows.filter(r => resolveEntry(map, r).acceptance.native === 'verified').length,
      field_accepted: rows.filter(r => resolveEntry(map, r).acceptance.field === 'verified').length };
  }
  return { registry_declared_keys: map.registry.entries.length,
    registry_base_keys: map.registry.entries.filter(e => e.name === e.canonical).length,
    registry_alias_keys: map.registry.entries.filter(e => e.name !== e.canonical).length,
    mapped_reference_items: map.entries.length, by_category: byCategory };
}

export function buildMap(inventory, dump, previous = null) {
  const entries = referenceEntries(inventory);
  const profiles = initialProfiles();
  const map = previous ? clone(previous) : {
    schema_version: 1,
    scope: 'A0 reference-to-Caligo mapping; explicit normalized item assessments, no QQ/native/production acceptance asserted.',
    reference: { path: INVENTORY_PATH, sha256: INVENTORY_SHA256, commit: COMMIT },
    schema_rules: {
      resolution: 'Each entries row has reference_id/category/status/reason/assessment. implementation, normal_entry, permission, backend_dependencies, supported_versions and acceptance resolve through assessments[row.assessment]; optional row.review fields override reviewed values.',
      aliases: 'Aliases are separate reference IDs and require their own actual registry key; no automatic inheritance of base support.',
      exports: 'Native export reference IDs are native_export.<module>.<ordinal>; all other IDs come from the immutable inventory, aliases use onebot.alias.<key>.',
      acceptance: 'declared/implemented/wired/offline/native/field/production are separate. Registered, partial_offline, research_lab and implemented_pending_validation never imply native acceptance.',
      missing: 'No audited equivalent handler/normal entry; no credit for placeholders, file existence, learned-only caches or reference-only evidence.',
    },
    assessments: profiles,
    entries: [],
    extra_gaps: [
      { id: 'reference_inventory_semantic_granularity', status: 'unknown', reason: 'Inventory declarations are finite denominators, not a full semantic decomposition of every internal helper or runtime effect.' },
      { id: 'native_acceptance', status: 'unknown', reason: 'No QQ launched; native callback identity, strong message IDs, lifetime and per-item production gates remain open.' },
      { id: 'sse_mcp_final_validation', status: 'wired_offline', reason: 'SSE/MCP normal assembly and final default/research targeted validation passed; this remains offline evidence only.' },
    ],
  };
  for (const [id, profile] of Object.entries(profiles)) {
    // Generated assessments track current code; explicitly reviewed profiles stay intact.
    if (!map.assessments[id]?.reviewed) map.assessments[id] = profile;
  }
  map.extra_gaps = (map.extra_gaps ?? []).filter(gap => gap.id !== 'sse_mcp_final_validation');
  map.schema_rules.refresh = 'Regenerate derived rows and assessment profiles. Preserve rows with a manual review and profiles with reviewed: true. Reject missing reference IDs and validate all resulting rows.';
  const existing = new Map(map.entries.map(e => [e.reference_id, e]));
  map.entries = entries.map(entry => {
    const previousRow = existing.get(entry.id);
    if (previousRow?.review) return previousRow;
    const assessment = chooseAssessment(entry, dump.registry);
    return { reference_id: entry.id, category: entry.category, status: map.assessments[assessment].status,
      reason: map.assessments[assessment].reason, assessment };
  });
  const currentIds = new Set(entries.map(entry => entry.id));
  const orphaned = [...existing.keys()].filter(id => !currentIds.has(id));
  if (orphaned.length) fail('Refresh refuses to discard previous mapping rows whose reference IDs vanished');
  map.scope = 'A0 reference-to-Caligo mapping; explicit normalized item assessments, no QQ/native/field/production acceptance asserted. Host query and newly integrated manager/UI/media capabilities remain partial or pending validation.';
  map.review_basis = [src(LEDGER, '共享动作与查询'), src(LEDGER, '账号管理：default/research 各 5/5 PASS'), src(LEDGER, '媒体工具：default `media_tool_lab` 6 PASS')];
  map.registry = { semantic_sha256: registryDigest(dump.raw), digest_format: 'UTF-8 JSON tokens with external whitespace removed; integer literals preserved',
    command: dump.command, source: src(ONEBOT, 'pub static ACTION_REGISTRY'),
    exporter: src('crates/caligo-core/examples/dump_onebot_registry.rs', 'caligo_core::onebot::ACTION_REGISTRY'),
    entries: dump.registry.map(r => ({ name: r.name, canonical: r.canonical, permission: r.permission, handler: r.handler,
      input_schema_source: { exporter: 'dump_onebot_registry', key: r.name },
      output_contract_source: src(ONEBOT, 'pub fn handle_action(') })) };
  map.counts = summarize(map);
  return map;
}

export function validateMap(inventory, map, dump, { checkSources = true, root = ROOT } = {}) {
  const reference = referenceEntries(inventory);
  const errors = [];
  const check = (ok, message) => { if (!ok) errors.push(message); };
  check(map.reference.sha256 === INVENTORY_SHA256 && map.reference.commit === COMMIT, 'Reference pin mismatch');
  check(inventory.reference.commit === COMMIT, 'Inventory commit mismatch');
  const wanted = new Map(reference.map(e => [e.id, e]));
  const ids = map.entries.map(r => r.reference_id);
  check(new Set(ids).size === ids.length, 'Duplicate mapping reference IDs');
  check(ids.length === wanted.size, `Mapping denominator mismatch (${ids.length}/${wanted.size})`);
  for (const id of wanted.keys()) check(ids.includes(id), `Missing reference mapping: ${id}`);
  for (const id of ids) check(wanted.has(id), `Unknown reference mapping: ${id}`);
  check(map.registry.semantic_sha256 === registryDigest(dump.raw), 'Executable registry semantic hash drift');
  const current = dump.registry.map(r => ({ name: r.name, canonical: r.canonical, permission: r.permission, handler: r.handler }));
  const recorded = map.registry.entries.map(({ name, canonical, permission, handler }) => ({ name, canonical, permission, handler }));
  check(JSON.stringify(current) === JSON.stringify(recorded), 'Registry name/canonical/permission/handler drift');
  check(new Set(current.map(r => r.name)).size === current.length, 'Duplicate executable registry names');
  const byName = new Map(current.map(r => [r.name, r]));
  for (const row of map.entries) {
    const item = wanted.get(row.reference_id);
    if (!item) continue;
    const profile = map.assessments[row.assessment];
    check(Boolean(profile), `Unknown assessment: ${row.reference_id}`);
    if (!profile) continue;
    const resolved = resolveEntry(map, row);
    check(row.category === item.category, `Category drift: ${row.reference_id}`);
    check(STATUSES.includes(row.status), `Invalid actual status: ${row.reference_id}`);
    check(row.status === (row.review?.status ?? profile.status), `Assessment status mismatch: ${row.reference_id}`);
    if (!row.review) {
      const expectedAssessment = chooseAssessment(item, dump.registry);
      check(row.assessment === expectedAssessment, `Reference mapping does not match current evidence: ${row.reference_id}`);
    }
    check(typeof row.reason === 'string' && row.reason.length > 10, `Missing reason: ${row.reference_id}`);
    for (const field of ['implementation', 'normal_entry', 'permission', 'backend_dependencies', 'acceptance', 'supported_versions'])
      check(field in resolved, `Missing resolved ${field}: ${row.reference_id}`);
    check(resolved.permission && 'requirement' in resolved.permission, `Missing permission contract: ${row.reference_id}`);
    check(Array.isArray(resolved.backend_dependencies), `Missing backend dependencies: ${row.reference_id}`);
    const acceptance = resolved.acceptance;
    for (const field of ['declared', 'implemented', 'wired', 'offline', 'native', 'field', 'production', 'evidence', 'limits'])
      check(acceptance && field in acceptance, `Missing acceptance ${field}: ${row.reference_id}`);
    if (row.status === 'missing' || row.status === 'unknown') {
      check(!acceptance.implemented && !acceptance.wired, `Missing/unknown item credited as implemented: ${row.reference_id}`);
      check(resolved.implementation === null && resolved.normal_entry === null, `Missing/unknown item has positive implementation claim: ${row.reference_id}`);
    } else {
      check(Array.isArray(resolved.implementation) && resolved.implementation.length > 0, `Positive mapping lacks handler/source: ${row.reference_id}`);
      check(Array.isArray(resolved.normal_entry) && resolved.normal_entry.length > 0, `Positive mapping lacks normal entry: ${row.reference_id}`);
    }
    check(acceptance.native === 'not_verified' && acceptance.field === 'not_verified' && acceptance.production === 'not_accepted', `Unreviewed native/field acceptance promotion: ${row.reference_id}`);
    if (item.category === 'actions' || item.category === 'aliases') {
      const registered = byName.has(item.name);
      check(registered === !['missing', 'unknown'].includes(row.status), `Registry/mapping support mismatch: ${item.name}`);
      if (registered && byName.get(item.name).handler.target === 'private')
        check(['partial_offline', 'registered_backend_blocked'].includes(row.status), `Private backend status is invalid: ${item.name}`);
      if (registered && ['get_friend_list', 'get_group_list', 'get_group_info'].includes(item.name))
        check(row.status === 'implemented_pending_validation', `Directory query was promoted before end-to-end validation: ${item.name}`);
      if (item.name.endsWith('_rate_limited') && !registered) check(row.status === 'missing', `Unregistered rate_limited alias credited: ${item.name}`);
    }
    if (row.assessment === 'capability_query') {
      check(row.status === 'implemented_pending_validation', `Host query promoted before end-to-end validation: ${row.reference_id}`);
      check(resolved.supported_versions === null, `Query capability has unsupported version claim: ${row.reference_id}`);
    }
    if (['account_manager', 'management_ui', 'media_tool'].includes(row.assessment)) {
      check(row.status === 'partial_offline', `Local tool capability may not be promoted by unknown host/native state: ${row.reference_id}`);
      check(acceptance.native === 'not_verified' && acceptance.field === 'not_verified', `Local tool has native/field acceptance: ${row.reference_id}`);
    }
  }
  check(JSON.stringify(map.counts) === JSON.stringify(summarize(map)), 'Recorded coverage counts drift');
  check(map.counts.by_category.actions.denominator === 206, 'Expected 206 reference base actions');
  check(map.counts.by_category.aliases.denominator === 412, 'Expected 412 aliases');
  if (checkSources) {
    const cache = new Map();
    const visit = value => {
      if (!value || typeof value !== 'object') return;
      if (value.path && value.symbol) {
        const absolute = path.resolve(root, value.path);
        const relative = path.relative(root, absolute);
        check(!relative.startsWith('..') && !path.isAbsolute(relative), `Source escapes workspace: ${value.path}`);
        if (!cache.has(value.path)) cache.set(value.path, existsSync(absolute) ? readFileSync(absolute, 'utf8') : null);
        const text = cache.get(value.path);
        check(text !== null && text.includes(value.symbol), `Source anchor missing: ${value.path} :: ${value.symbol}`);
      }
      for (const child of Object.values(value)) visit(child);
    };
    visit(map.assessments); visit(map.registry); visit(map.review_basis);
    for (const row of map.entries) if (row.review) visit(row.review);
  }
  if (errors.length) fail(errors.slice(0, 40).join('\n') + (errors.length > 40 ? `\n... ${errors.length} errors total` : ''));
  return map.counts;
}

export function main(argv) {
  let output = path.join(ROOT, MAP_PATH); let init = false; let refresh = false; let registryPath;
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === '--init') init = true;
    else if (argv[i] === '--refresh') refresh = true;
    else if (argv[i] === '--registry-json') registryPath = argv[++i];
    else if (argv[i] === '--output') output = path.resolve(ROOT, argv[++i]);
    else fail(`Unknown option ${argv[i]}`);
  }
  if (init && refresh) fail('Choose --init or --refresh');
  const bytes = readFileSync(path.join(ROOT, INVENTORY_PATH));
  if (hash(bytes) !== INVENTORY_SHA256) fail('Immutable reference inventory SHA-256 changed; stop and review');
  const inventory = JSON.parse(bytes);
  const dump = registryPath ? { raw: readFileSync(path.resolve(ROOT, registryPath), 'utf8'), command: 'captured dump_onebot_registry JSON (current Rust execution not checked)' } : runRegistry();
  dump.registry ??= JSON.parse(dump.raw);
  let map;
  if (init) {
    if (existsSync(output)) fail('Initial map already exists');
    map = buildMap(inventory, dump);
  } else if (refresh) map = buildMap(inventory, dump, json(output));
  else map = json(output);
  const counts = validateMap(inventory, map, dump);
  if (init || refresh) writeFileSync(output, JSON.stringify(map, null, 2) + '\n', { flag: init ? 'wx' : 'w' });
  console.log(JSON.stringify({ result: 'coverage_structure_passed', registry_source: registryPath ? 'captured_json_only' : 'executed_rust_registry',
    registry_semantic_sha256: map.registry.semantic_sha256, counts }, null, 2));
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { main(process.argv.slice(2)); } catch (error) { console.error(error.message); process.exitCode = 1; }
}
