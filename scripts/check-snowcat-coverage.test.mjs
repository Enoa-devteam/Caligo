import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import path from 'node:path';
import { ROOT, INVENTORY_PATH, MAP_PATH, INVENTORY_SHA256, CATEGORIES, canonicalJsonTokens, registryDigest,
  referenceEntries, buildMap, resolveEntry, validateMap, summarize } from './check-snowcat-coverage.mjs';

const inventoryBytes = readFileSync(path.join(ROOT, INVENTORY_PATH));
const inventory = JSON.parse(inventoryBytes);
const artifact = JSON.parse(readFileSync(path.join(ROOT, MAP_PATH), 'utf8'));
// Unit tests use the recorded structural registry; they never pretend to run Rust.
// The final integration test invokes the real exporter when COVERAGE_RUN_RUST=1.
const registry = artifact.registry.entries.map(({ name, canonical, permission, handler }) => ({
  name, canonical, permission, handler,
}));
const raw = JSON.stringify(registry);
function fixture() {
  const dump = { raw, registry, command: 'unit-test structural registry fixture' };
  return { map: buildMap(inventory, dump, artifact), dump };
}
function check(map, dump) { return validateMap(inventory, map, dump, { checkSources: false }); }
function row(map, id) { return map.entries.find(e => e.reference_id === id); }

test('immutable reference SHA and all 16 category denominators are preserved', () => {
  assert.equal(createHash('sha256').update(inventoryBytes).digest('hex'), INVENTORY_SHA256);
  const entries = referenceEntries(inventory);
  assert.equal(entries.length, 5711);
  assert.equal(CATEGORIES.length, 16);
  assert.equal(entries.filter(e => e.category === 'actions').length, 206);
  assert.equal(entries.filter(e => e.category === 'aliases').length, 412);
  assert.equal(entries.filter(e => e.category === 'component_exports').length, 3297);
  const { map, dump } = fixture();
  assert.doesNotThrow(() => check(map, dump));
});

test('registry token digest preserves u64 integer literals and ignores formatting only', () => {
  const text = '[ { "maximum": 18446744073709551615, "text": "a b" } ]';
  assert.equal(canonicalJsonTokens(text), '[{"maximum":18446744073709551615,"text":"a b"}]');
  assert.equal(registryDigest(text), registryDigest('[{"maximum":18446744073709551615,"text":"a b"}]'));
  assert.notEqual(registryDigest(text), registryDigest('[{"maximum":18446744073709551614,"text":"a b"}]'));
});

test('base and alias support come from independent actual registry keys', () => {
  const { map } = fixture();
  assert.equal(map.counts.registry_declared_keys, 17);
  assert.equal(map.counts.registry_base_keys, 14);
  assert.equal(map.counts.registry_alias_keys, 3);
  assert.equal(row(map, 'onebot.action.get_status').status, 'wired_offline');
  assert.equal(row(map, 'onebot.alias.get_status_async').status, 'missing');
  assert.equal(row(map, 'onebot.alias.send_group_msg_async').status, 'partial_offline');
  assert.equal(row(map, 'onebot.alias.send_group_msg_rate_limited').status, 'missing');
  assert.equal(row(map, 'onebot.action.send_private_msg').status, 'partial_offline');
  for (const name of ['get_group_info', 'get_group_list', 'get_friend_list']) {
    const entry = row(map, `onebot.action.${name}`);
    assert.equal(entry.status, 'implemented_pending_validation');
    assert.equal(entry.assessment, 'directory_pending');
  }
});

test('every item resolves required implementation/entry/permission/backend/acceptance fields', () => {
  for (const entry of artifact.entries) {
    const resolved = resolveEntry(artifact, entry);
    for (const field of ['status', 'reason', 'implementation', 'normal_entry', 'permission', 'backend_dependencies', 'acceptance', 'supported_versions'])
      assert.ok(field in resolved, `${entry.reference_id}: ${field}`);
    assert.equal(resolved.acceptance.native, 'not_verified');
    assert.equal(resolved.acceptance.field, 'not_verified');
    assert.equal(resolved.acceptance.production, 'not_accepted');
    if (entry.status === 'missing') {
      assert.equal(resolved.acceptance.implemented, false);
      assert.equal(resolved.implementation, null);
    }
  }
});

test('checker rejects missing, duplicate, extraneous and recategorized rows', () => {
  for (const mutation of [
    map => map.entries.pop(),
    map => map.entries.push(structuredClone(map.entries[0])),
    map => { map.entries[0].reference_id = 'invented.capability'; },
    map => { map.entries[0].category = 'tools'; },
  ]) {
    const { map, dump } = fixture(); mutation(map); map.counts = summarize(map);
    assert.throws(() => check(map, dump), /mapping|Category drift|reference/i);
  }
});

test('checker rejects changed registry semantics, permissions and digest', () => {
  const { map, dump } = fixture();
  const changed = structuredClone(dump);
  changed.registry[0].permission = 'read';
  changed.raw = JSON.stringify(changed.registry);
  assert.throws(() => check(map, changed), /registry.*drift/i);
  map.registry.semantic_sha256 = '0'.repeat(64);
  assert.throws(() => check(map, dump), /semantic hash drift/);
});

test('checker refuses false rate_limited/private/directory backend credit', () => {
  for (const [id, profile] of [
    ['onebot.alias.send_group_msg_rate_limited', 'query'],
    ['onebot.action.send_private_msg', 'send_text'],
    ['onebot.action.get_friend_list', 'query'],
  ]) {
    const { map, dump } = fixture();
    Object.assign(row(map, id), { assessment: profile, status: map.assessments[profile].status });
    map.counts = summarize(map);
    assert.throws(() => check(map, dump), /Reference mapping does not match current evidence/);
  }
});

test('checker separates declared/lab/native/field acceptance', () => {
  const { map, dump } = fixture();
  map.assessments.query.acceptance.native = 'verified';
  map.counts = summarize(map);
  assert.throws(() => check(map, dump), /acceptance promotion/);
  assert.equal(row(artifact, 'protocol.OB11HttpSSEServerAdapter').status, 'wired_offline');
  assert.equal(row(artifact, 'protocol.OB11McpServerAdapter').status, 'wired_offline');
  const profile = artifact.entries.filter(e => e.assessment === 'profile_service');
  assert.equal(profile.length, 2);
  assert.ok(profile.every(e => e.status === 'research_lab'));
});

test('checker rejects stale counts and positive mappings without entry/permission', () => {
  const { map, dump } = fixture();
  map.counts.registry_declared_keys = 999;
  assert.throws(() => check(map, dump), /counts drift/);
  map.counts = summarize(map);
  map.assessments.query.normal_entry = null;
  assert.throws(() => check(map, dump), /normal entry/);
});

test('refresh regenerates derived rows and preserves manual reviews', () => {
  const { map, dump } = fixture();
  const id = 'onebot.action.get_status';
  row(map, id).review = { reason: 'Reviewed manually: daemon state must be ready.', reviewer_note: 'Keep this exact reviewed note.' };
  Object.assign(map.assessments.query, {
    reviewed: true,
    reason: 'Reviewed query limitations must survive refresh.',
  });
  const privateSend = row(map, 'onebot.action.send_private_msg');
  privateSend.status = 'registered_backend_blocked';
  const target = map.entries.find(e => e.status === 'missing');
  map.entries = map.entries.filter(e => e !== target);
  const refreshed = buildMap(inventory, dump, map);
  assert.equal(refreshed.entries.length, 5711);
  assert.equal(row(refreshed, id).review.reviewer_note, 'Keep this exact reviewed note.');
  assert.equal(refreshed.assessments.query.reason, 'Reviewed query limitations must survive refresh.');
  assert.equal(row(refreshed, privateSend.reference_id).status, 'partial_offline');
  assert.doesNotThrow(() => check(refreshed, dump));
});

test('all current source/normal-entry and offline evidence anchors are locatable', () => {
  const { map, dump } = fixture();
  assert.doesNotThrow(() => validateMap(inventory, map, dump));
});

test('current executable Rust registry matches pinned semantic export', { skip: process.env.COVERAGE_RUN_RUST !== '1' }, async () => {
  const { runRegistry } = await import('./check-snowcat-coverage.mjs');
  const dump = runRegistry();
  assert.doesNotThrow(() => validateMap(inventory, artifact, dump));
});
