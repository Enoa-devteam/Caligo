import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { lex, closeAt, declarations, enumMembers, explicitHandlers, buildInventory, COMMIT } from './extract-snowcat-capabilities.mjs';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('lexer ignores comment/quoted/template/regex fake registrations and preserves line numbers', () => {
  const text = [
    '// new Fake();',
    '/* @ActionHandler class Fake {} */',
    'const s = "class Fake { new Bogus(); }";',
    'const t = `new Fake(${value})`;',
    'const r = /class Fake \\{ [}]/g;',
    'export class Real extends Base<T<{a: string}>> { actionName = ActionName.Real; }',
  ].join('\n');
  const result = declarations(lex(text), 'class');
  assert.equal(result.length, 1);
  assert.equal(result[0].name, 'Real');
  assert.equal(result[0].base, 'Base');
  assert.equal(result[0].line, 6);
});

test('balanced reader skips delimiter text in literals and rejects malformed bodies', () => {
  const ts = lex('{ nested: [call("}")], object: { a: 1 } }');
  assert.equal(closeAt(ts, 0), ts.length - 1);
  assert.throws(() => closeAt(lex('{]'), 0), /Unbalanced/);
  assert.throws(() => lex('/* never closed'), /Unterminated/);
});

test('router census excludes commented declarations and accepts multiline object members', () => {
  const result = enumMembers(lex("export const ActionName = { A: 'a', // Fake: 'fake'\n B: 'b' } as const;"), 'ActionName');
  assert.deepEqual(result.map(r => [r.member, r.value]), [['A', 'a'], ['B', 'b']]);
});

test('explicit handler census retains duplicates, order and alias import bindings', () => {
  const result = explicitHandlers(lex('export function getAllHandlers() { const actionHandlers = [new A(obContext, core), /* new Fake() */ new Renamed(obContext, core), new A(obContext, core)]; return actionHandlers; }'));
  assert.deepEqual(result.map(r => r.class_name), ['A', 'Renamed', 'A']);
  assert.deepEqual(result.map(r => r.order), [0, 1, 2]);
  assert.throws(() => explicitHandlers(lex('function getAllHandlers() { const actionHandlers = [ ...dynamic ]; }')), /Unsupported handler expression/);
});

test('decorator declaration and abstract/base distinctions survive lexical extraction', () => {
  const result = declarations(lex('@ActionHandler\nexport default class TestA extends Base<void, string> {}\nexport abstract class Base<T,R> {}'), 'class');
  assert.equal(result[0].decorated, true);
  assert.equal(result[0].abstract, false);
  assert.equal(result[1].abstract, true);
});

const reference = process.env.SNOWCAT_REFERENCE;
test('pinned reference census, evidence integrity, unknown contracts and byte regeneration', { skip: !reference }, () => {
  const generated = buildInventory(reference);
  assert.equal(generated.reference.commit, COMMIT);
  const c = generated.counts;
  assert.equal(c.explicit_registration_occurrences, 207);
  assert.equal(c.unique_explicit_classes, 206);
  assert.equal(c.unique_registered_base_actions, 206);
  assert.equal(c.generated_alias_keys, 412);
  assert.equal(c.final_map_keys, 618);
  assert.equal(c.map_overwrites, 3);
  assert.equal(c.base_actions_excluding_flagged_test, 205);
  assert.equal(c.router_members, 210);
  assert.equal(c.decorator_declarations, 2);
  assert.equal(c.reachable_decorator_registrations, 0);
  assert.deepEqual(generated.router_difference.router_only.map(r => r.value), ['test_auto_register_01', 'test_auto_register_02', '.get_word_slices', 'unknown']);
  assert.equal(generated.router_difference.registered_only.length, 0);
  assert.equal(generated.actions.find(a => a.name === 'check_url_safely').replacement_count, 1);
  assert.equal(generated.actions.find(a => a.name === 'set_qq_avatar').effective_class, 'SetAvatar');
  assert.equal(generated.actions.find(a => a.name === 'test_download_stream').test_only, true);
  assert.equal(c.event_class_declarations, 38);
  assert.equal(c.concrete_event_class_declarations, 26);
  assert.equal(c.message_segment_enum_members, 23);
  assert.deepEqual(c.component_exports_by_module, { winmm: 31, winpty: 19, qqnt: 3247 });
  assert.equal(c.component_export_rows, 3297);
  assert.equal(generated.protocols.find(p => p.name === 'SnowCatProtocolAdapter').action_registration_count, 0);
  const mcp = generated.protocols.find(p => p.name === 'OB11McpServerAdapter');
  assert.equal(mcp.wiring.initial_constructor_found, false);
  assert.equal(mcp.wiring.configuration_change_sites.length, 1);
  assert.ok(generated.component_surface_exports.length > 0);
  for (const list of ['actions', 'decorators', 'events', 'event_schemas', 'message_segments', 'protocols', 'components', 'component_surface_exports', 'native_components', 'native_surfaces', 'wrapper_interfaces', 'webui_routes', 'tools', 'named_contract_gaps']) {
    const ids = generated[list].map(item => item.id);
    assert.equal(new Set(ids).size, ids.length, `Duplicate ids in ${list}`);
    for (const item of generated[list]) {
      assert.ok(item.evidence, `Missing evidence: ${item.id}`);
      assert.equal(item.caligo.handler, null);
      assert.equal(item.caligo.normal_entry, null);
      assert.equal(item.caligo.status, 'not_assessed');
      assert.equal(item.supported_versions, null);
      assert.ok(item.unknown_reasons.length > 0);
      assert.equal(item.acceptance.caligo, null);
    }
  }
  const manifest = new Map(generated.source_files.map(s => [s.path, s]));
  // One batch independently verifies all exported source hashes/line ranges.
  const batch = spawnSync('git', ['--no-pager', '-C', reference, 'cat-file', '--batch'], {
    cwd: ROOT, input: generated.source_files.map(s => s.git_blob + '\n').join(''), maxBuffer: 128 * 1024 * 1024,
  });
  assert.equal(batch.status, 0);
  let offset = 0;
  for (const s of generated.source_files) {
    const end = batch.stdout.indexOf(10, offset);
    const header = batch.stdout.subarray(offset, end).toString().split(' ');
    assert.equal(header[0], s.git_blob);
    const bytes = batch.stdout.subarray(end + 1, end + 1 + Number(header[2]));
    assert.equal(createHash('sha256').update(bytes).digest('hex'), s.sha256);
    assert.equal(bytes.length, s.bytes);
    s.line_count = bytes.toString('utf8').split('\n').length;
    offset = end + 2 + bytes.length;
  }
  assert.equal(offset, batch.stdout.length);
  function walk(value) {
    if (!value || typeof value !== 'object') return;
    if (value.path && value.line) {
      const record = manifest.get(value.path);
      assert.ok(record, `Missing source manifest: ${value.path}`);
      assert.ok(Number.isInteger(value.line) && value.line >= 1 && value.line <= record.line_count, `Invalid evidence line: ${value.path}:${value.line}`);
      assert.ok(value.origin, 'Missing evidence origin');
    }
    for (const child of Object.values(value)) walk(child);
  }
  walk(generated);
  for (const s of generated.source_files) delete s.line_count;
  const artifact = readFileSync(path.join(ROOT, 'docs/contracts/snowcat-capability-inventory.json'), 'utf8');
  assert.equal(JSON.stringify(generated, null, 2) + '\n', artifact);
  assert.ok(!/"\d{5,}"/.test(artifact), 'Unexpected long decimal literal; inspect potential identity data');
  assert.ok(generated.source_files.every(s => !/\.(?:c|cc|cpp|h|hpp)$/i.test(s.path)), 'C/C++ input must not be consumed');
});
