#!/usr/bin/env node
/**
 * Static A0 census. Only Node builtins and read-only Git object commands.
 * Never imports, evaluates, builds or launches reference code; never reads C/C++.
 * Run from Caligo (not the reference checkout):
 * node scripts/extract-snowcat-capabilities.mjs --reference E:/stella/Snowcat/Napcat-Snowluma-research --output docs/contracts/snowcat-capability-inventory.json
 * Repeat with --check to compare bytes without writing.
 * node --test scripts/extract-snowcat-capabilities.test.mjs
 */
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync, existsSync, realpathSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const COMMIT = '7cf025fe1e7f9c609e02a8643c6e6398eeec0d64';
const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const ACTION = 'packages/snowcat-onebot/action/';
const INDEX = `${ACTION}index.ts`;
const ROUTER = `${ACTION}router.ts`;
const RECOVERED = 'research/native-reconstruction/recovered/';
const sha256 = value => createHash('sha256').update(value).digest('hex');
const unique = values => [...new Set(values)];
const ordered = values => [...values].sort((a, b) => a < b ? -1 : a > b ? 1 : 0);
const fail = message => { throw new Error(message); };

// A deliberately bounded lexical reader, not a TypeScript compiler. Templates
// and regexes are opaque. Unsupported registrations fail or enter unknowns.
export function lex(text) {
  const out = [];
  let i = 0;
  let line = 1;
  const advance = () => { if (text[i] === '\n') line++; return text[i++]; };
  while (i < text.length) {
    if (/\s/.test(text[i])) { advance(); continue; }
    if (text.startsWith('//', i)) { while (i < text.length && text[i] !== '\n') advance(); continue; }
    if (text.startsWith('/*', i)) {
      advance(); advance();
      while (i < text.length && !text.startsWith('*/', i)) advance();
      if (i >= text.length) fail('Unterminated comment');
      advance(); advance(); continue;
    }
    const start = i;
    const atLine = line;
    const c = text[i];
    if (c === "'" || c === '"' || c === '`') {
      const quote = advance();
      let value = '';
      let closed = false;
      while (i < text.length) {
        const v = advance();
        if (v === quote) { closed = true; break; }
        if (v === '\\') {
          const escaped = advance();
          value += ({ n: '\n', r: '\r', t: '\t' })[escaped] ?? escaped;
        } else value += v;
      }
      if (!closed) fail(`Unterminated literal at line ${atLine}`);
      out.push({ value, kind: quote === '`' ? 'template' : 'string', start, end: i, line: atLine });
      continue;
    }
    const previous = out.at(-1)?.value;
    if (c === '/' && ['=', '(', '[', ',', ':', 'return', '=>', '!', '&&', '||', '?'].includes(previous)) {
      advance();
      let inClass = false;
      let closed = false;
      while (i < text.length) {
        const v = advance();
        if (v === '\\') { advance(); continue; }
        if (v === '[') inClass = true;
        if (v === ']') inClass = false;
        if (v === '/' && !inClass) { closed = true; break; }
      }
      if (!closed) fail(`Unterminated regex at line ${atLine}`);
      while (/[a-z]/i.test(text[i] ?? '') && i < text.length) advance();
      out.push({ value: '<regex>', kind: 'regex', start, end: i, line: atLine }); continue;
    }
    if (/[A-Za-z_$]/.test(c)) {
      let value = advance();
      while (i < text.length && /[\w$]/.test(text[i])) value += advance();
      out.push({ value, kind: 'identifier', start, end: i, line: atLine }); continue;
    }
    const pair = text.slice(i, i + 2);
    const value = ['=>', '?.', '??', '&&', '||', '==', '!='].includes(pair) ? advance() + advance() : advance();
    out.push({ value, kind: 'punctuation', start, end: i, line: atLine });
  }
  return out;
}

export function closeAt(tokens, open) {
  const pairs = { '{': '}', '[': ']', '(': ')' };
  const stack = [];
  for (let i = open; i < tokens.length; i++) {
    const t = tokens[i];
    if (t.kind !== 'punctuation') continue;
    if (pairs[t.value]) stack.push(pairs[t.value]);
    else if (['}', ']', ')'].includes(t.value)) {
      if (stack.pop() !== t.value) fail(`Unbalanced delimiters at line ${t.line}`);
      if (!stack.length) return i;
    }
  }
  fail(`Unclosed delimiter at line ${tokens[open]?.line}`);
}

export function declarations(tokens, keyword) {
  const result = [];
  for (let i = 0; i < tokens.length; i++) {
    if (tokens[i].value !== keyword || tokens[i + 1]?.kind !== 'identifier') continue;
    let j = i + 2;
    let angles = 0;
    for (; j < tokens.length; j++) {
      if (tokens[j].value === '<') angles++;
      if (tokens[j].value === '>') angles--;
      if (tokens[j].value === '{' && angles === 0) break;
    }
    if (j === tokens.length) fail(`No body for ${keyword} ${tokens[i + 1].value}`);
    const end = closeAt(tokens, j);
    const header = tokens.slice(i + 2, j);
    const extendsAt = header.findIndex(t => t.value === 'extends');
    result.push({ name: tokens[i + 1].value, line: tokens[i].line, start: tokens[i].start,
      end: tokens[end].end, header, body: tokens.slice(j + 1, end),
      base: extendsAt < 0 ? null : header[extendsAt + 1]?.value,
      abstract: tokens[i - 1]?.value === 'abstract',
      decorated: tokens.slice(Math.max(0, i - 5), i).some(t => t.value === 'ActionHandler') });
    i = end;
  }
  return result;
}

export function enumMembers(tokens, name) {
  let start = tokens.findIndex((t, i) => t.value === name && ['enum', 'const'].includes(tokens[i - 1]?.value));
  if (start < 0) fail(`Missing enum/object ${name}`);
  while (tokens[start]?.value !== '{') start++;
  return tokens.slice(start + 1, closeAt(tokens, start)).flatMap((t, i, all) =>
    t.kind === 'identifier' && ['=', ':'].includes(all[i + 1]?.value) && all[i + 2]?.kind === 'string'
      ? [{ member: t.value, value: all[i + 2].value, line: t.line }] : []);
}

export function explicitHandlers(tokens) {
  const fn = tokens.findIndex(t => t.value === 'getAllHandlers');
  const arr = tokens.findIndex((t, i) => i > fn && t.value === 'actionHandlers' && tokens[i + 1]?.value === '=');
  if (arr < 0 || tokens[arr + 2]?.value !== '[') fail('Unsupported getAllHandlers array');
  const end = closeAt(tokens, arr + 2);
  const result = [];
  let i = arr + 3;
  while (i < end) {
    if (tokens[i].value === ',') { i++; continue; }
    if (tokens[i].value !== 'new' || tokens[i + 1]?.kind !== 'identifier' || tokens[i + 2]?.value !== '(')
      fail(`Unsupported handler expression at line ${tokens[i].line}`);
    const close = closeAt(tokens, i + 2);
    result.push({ class_name: tokens[i + 1].value, line: tokens[i].line, order: result.length });
    i = close + 1;
  }
  return result;
}

function git(reference, args, input) {
  const result = spawnSync('git', ['--no-pager', '-c', 'core.hooksPath=', '-C', reference, ...args],
    { cwd: ROOT, input, encoding: null, maxBuffer: 128 * 1024 * 1024, windowsHide: true });
  if (result.error || result.status !== 0) fail(`Read-only Git ${args[0]} failed (${result.status ?? 'spawn'}); no reference output copied`);
  return result.stdout;
}

function loadSources(reference) {
  const commit = git(reference, ['rev-parse', '--verify', `${COMMIT}^{commit}`]).toString().trim();
  if (commit !== COMMIT) fail('Pinned commit unavailable');
  const tree = git(reference, ['ls-tree', '-rz', '--full-tree', COMMIT]).toString('utf8').split('\0').filter(Boolean);
  const rows = tree.map(row => {
    const match = /^(\d+) blob ([a-f0-9]+)\t(.+)$/.exec(row);
    return match ? { mode: match[1], blob: match[2], path: match[3] } : null;
  }).filter(Boolean);
  const docs = ['README.md', 'hook/EVIDENCE.md', 'ffmpeg/EVIDENCE.md', 'winmm/README.md', 'winpty/README.md',
    'qqnt/README.md', 'napiloader/RECOVERY-EVIDENCE.md', 'addons/README.md', 'addons/LIVE-ACCEPTANCE-20261113.md',
    'ACCEPTANCE.md', 'winmm/exports.tsv', 'winpty/exports.tsv', 'qqnt/exports.tsv'].map(p => RECOVERED + p);
  const selected = rows.filter(r =>
    (r.path.startsWith('packages/') && /\.(?:ts|tsx|js|mjs|cjs)$/.test(r.path)) ||
    /^packages\/[^/]+\/package\.json$/.test(r.path) ||
    docs.includes(r.path) || ['package.json', 'snowcat-launch.ps1', 'snowcat-launch-9937.ps1',
      'snowcat-portable-maker.ps1', 'snowcat-qq-unpack.ps1', 'snowcat-doctor.ps1'].includes(r.path));
  if (selected.some(r => r.mode !== '100644' && r.mode !== '100755')) fail('Non-regular reference source rejected');
  // cat-file reads immutable objects, ignoring worktree changes, filters and CRLF.
  const packed = git(reference, ['cat-file', '--batch'], selected.map(r => r.blob + '\n').join(''));
  let offset = 0;
  const sources = new Map();
  for (const row of selected) {
    const newline = packed.indexOf(10, offset);
    const header = packed.subarray(offset, newline).toString().split(' ');
    if (header[0] !== row.blob || header[1] !== 'blob') fail('Unexpected Git batch record');
    const length = Number(header[2]);
    const bytes = packed.subarray(newline + 1, newline + 1 + length);
    if (bytes.length !== length || packed[newline + 1 + length] !== 10) fail('Truncated Git batch record');
    sources.set(row.path, { ...row, text: bytes.toString('utf8'), sha256: sha256(bytes), bytes: length });
    offset = newline + 2 + length;
  }
  if (offset !== packed.length) fail('Unexpected trailing Git batch data');
  return sources;
}

function rowsForPackageDirectories(reference) {
  return git(reference, ['ls-tree', '--name-only', `${COMMIT}:packages`]).toString('utf8').trim().split('\n');
}

function imports(tokens) {
  const result = [];
  for (let i = 0; i < tokens.length; i++) {
    if (!['import', 'export'].includes(tokens[i].value) || tokens[i + 1]?.value === 'type') continue;
    if (tokens[i].value === 'import' && tokens[i + 1]?.kind === 'string') result.push(tokens[i + 1].value);
    for (let j = i + 1; j < tokens.length && ![';', 'import', 'export'].includes(tokens[j].value); j++) {
      if (tokens[j].value === 'from' && tokens[j + 1]?.kind === 'string') { result.push(tokens[j + 1].value); break; }
    }
  }
  return unique(result);
}

export function buildInventory(reference) {
  const sources = loadSources(reference);
  const consumed = new Set();
  const cache = new Map();
  const source = p => { const s = sources.get(p); if (!s) fail(`Required source missing: ${p}`); consumed.add(p); return s; };
  const tokens = p => { if (!cache.has(p)) cache.set(p, lex(source(p).text)); return cache.get(p); };
  const ref = (p, line, origin) => { source(p); return { path: p, line, origin }; };
  const findLine = (p, pattern) => {
    const lines = source(p).text.split('\n'); const index = lines.findIndex(line => pattern.test(line));
    if (index < 0) fail(`Missing expected evidence in ${p}`); return index + 1;
  };
  const nullContract = () => ({ input: null, output: null, permission: null, host_api_dependencies: null,
    supported_versions: null, caligo: { handler: null, status: 'not_assessed', normal_entry: null },
    acceptance: { reference: null, caligo: null },
    unknown_reasons: ['Input/output semantics, authorization, host binding and supported versions require per-item review.',
      'Caligo implementation, backend, normal-entry reachability and acceptance have not been assessed; placeholders are not support.'] });
  const classes = new Map();
  const actionPaths = ordered([...sources.keys()].filter(p => p.startsWith(ACTION) && p.endsWith('.ts') &&
    !p.includes('/example/') && !p.endsWith('/examples.ts')));
  for (const p of actionPaths) {
    for (const declaration of declarations(tokens(p), 'class')) {
      if (classes.has(declaration.name)) fail(`Ambiguous action class ${declaration.name}`);
      classes.set(declaration.name, { ...declaration, path: p });
    }
  }
  const chain = name => {
    const result = []; const seen = new Set(); let c = classes.get(name);
    while (c) {
      if (seen.has(c.name)) fail('Cyclic action inheritance'); seen.add(c.name); result.push(c); c = classes.get(c.base);
    }
    return result;
  };
  const router = enumMembers(tokens(ROUTER), 'ActionName');
  const routerByMember = new Map(router.map(r => [r.member, r]));
  const nameOf = name => {
    for (const c of chain(name)) {
      for (let i = 0; i < c.body.length; i++) {
        if (c.body[i].value !== 'actionName' || c.body[i + 1]?.value !== '=') continue;
        const t = c.body[i + 2];
        if (t.kind === 'string') return { name: t.value, evidence: ref(c.path, t.line, 'actionName literal') };
        if (t.value === 'ActionName' && c.body[i + 3]?.value === '.') {
          const member = c.body[i + 4].value; const r = routerByMember.get(member);
          if (!r) fail(`Unresolved ActionName.${member}`);
          return { name: r.value, member, evidence: ref(c.path, t.line, 'actionName enum reference'),
            router_evidence: ref(ROUTER, r.line, 'ActionName literal member') };
        }
        fail(`Unsupported actionName expression: ${name}`);
      }
    }
    fail(`ActionName missing: ${name}`);
  };
  // Static import closure, never a module load. Type-only imports do not register decorators.
  const resolveImport = (p, spec) => {
    let stem;
    if (spec.startsWith('.')) stem = path.posix.normalize(path.posix.join(path.posix.dirname(p), spec));
    else if (spec.startsWith('@/')) stem = 'packages/' + spec.slice(2);
    else if (spec.startsWith('snowcat-')) stem = 'packages/' + spec;
    else return null;
    return [stem, stem + '.ts', stem + '.tsx', stem + '/index.ts', stem.replace(/\.js$/, '.ts')].find(s => sources.has(s)) ?? null;
  };
  const reachable = new Set();
  const visit = p => {
    if (reachable.has(p)) return; reachable.add(p);
    for (const spec of imports(tokens(p))) { const target = resolveImport(p, spec); if (target) visit(target); }
  };
  visit(INDEX);
  const bindings = new Map();
  const indexTokens = tokens(INDEX);
  for (let i = 0; i < indexTokens.length; i++) {
    if (indexTokens[i].value !== 'import') continue;
    let from = i + 1;
    while (from < indexTokens.length && !['from', ';'].includes(indexTokens[from].value)) from++;
    if (indexTokens[from]?.value !== 'from' || indexTokens[from + 1]?.kind !== 'string') continue;
    const target = resolveImport(INDEX, indexTokens[from + 1].value);
    if (!target?.startsWith(ACTION)) continue;
    const importedClasses = declarations(tokens(target), 'class');
    if (indexTokens[i + 1]?.kind === 'identifier') {
      const targetTokens = tokens(target);
      const defaultClass = importedClasses.find(c => targetTokens.some((t, j) =>
        (t.value === 'class' && t.start === c.start && targetTokens[j - 1]?.value === 'default') ||
        (t.value === 'default' && targetTokens[j - 1]?.value === 'export' && targetTokens[j + 1]?.value === c.name)));
      if (defaultClass) bindings.set(indexTokens[i + 1].value, classes.get(defaultClass.name));
    }
    for (let j = i + 1; j < from; j++) {
      if (!['{', ','].includes(indexTokens[j - 1]?.value) || indexTokens[j]?.kind !== 'identifier') continue;
      const exported = indexTokens[j].value;
      const local = indexTokens[j + 1]?.value === 'as' ? indexTokens[j + 2].value : exported;
      const c = importedClasses.find(c => c.name === exported);
      if (c) bindings.set(local, classes.get(c.name));
    }
  }
  const occurrences = explicitHandlers(indexTokens).map(item => {
    const c = bindings.get(item.class_name); if (!c) fail(`Missing imported handler class ${item.class_name}`);
    return { ...item, imported_binding: item.class_name, class_name: c.name, name: nameOf(c.name).name, source: 'explicit', evidence: ref(INDEX, item.line, 'getAllHandlers new expression') };
  });
  const decorators = [...classes.values()].filter(c => c.decorated).map(c => ({
    id: `onebot.decorator.${c.name}`, kind: 'decorator_declaration', class_name: c.name, ...nameOf(c.name),
    action_name_evidence: nameOf(c.name).evidence, ...nullContract(), static_import_reachable: reachable.has(c.path),
    test_only: c.path.includes('/test/'), evidence: ref(c.path, c.line, '@ActionHandler declaration'),
    included_in_denominator: reachable.has(c.path),
    reason: reachable.has(c.path) ? 'Decorator declaration is in the value-import closure; runtime execution unverified.' :
      'No value-import path from action/index.ts; a decorator declaration alone does not populate AutoRegisterRouter.' }));
  for (const d of decorators.filter(d => d.included_in_denominator)) occurrences.push({
    class_name: d.class_name, name: d.name, source: 'decorator', order: occurrences.length, evidence: d.evidence });
  // Require the pinned map's known alias registration contract; do not guess suffixes.
  const mapText = source(INDEX).text;
  for (const needle of ['_map.set(h.actionName', '_map.set(`${h.actionName}_async`', '_map.set(`${h.actionName}_rate_limited`',
    '_map.set(actionName', '_map.set(`${actionName}_async`', '_map.set(`${actionName}_rate_limited`']) {
    if (!mapText.includes(needle)) fail('Unsupported createActionMap registration shape');
  }
  const aliasEvidence = {
    explicit: ['_async', '_rate_limited'].map(s => ref(INDEX, findLine(INDEX, new RegExp('h\\.actionName}'+s)), 'generated alias registration')),
    decorator: ['_async', '_rate_limited'].map(s => ref(INDEX, findLine(INDEX, new RegExp('\\$\\{actionName}'+s)), 'generated alias registration')),
  };
  const byName = new Map();
  for (const occurrence of occurrences) {
    if (!byName.has(occurrence.name)) byName.set(occurrence.name, []);
    byName.get(occurrence.name).push(occurrence);
  }
  const identifierRefs = (c, property) => {
    for (const parent of chain(c.name)) {
      const i = parent.body.findIndex((t, j, all) => t.value === property && all[j + 1]?.value === '=');
      if (i >= 0) return { expression_identifiers: unique(parent.body.slice(i + 2, parent.body.findIndex((t, j) => j > i && t.value === ';')).filter(t => t.kind === 'identifier').map(t => t.value)),
        evidence: ref(parent.path, parent.body[i].line, `${property} assignment; schema is not evaluated`), inherited_from: parent.name };
    }
    return null;
  };
  const actions = ordered(byName.keys()).map(name => {
    const regs = byName.get(name); const winner = regs.at(-1); const c = classes.get(winner.class_name);
    const ancestors = chain(c.name);
    const roleEvidence = [];
    const apiEvidence = [];
    const schemas = [];
    for (const parent of ancestors) {
      for (let i = 0; i < parent.body.length; i++) {
        const t = parent.body[i];
        if (/^(?:KOWNER|KADMIN|role|member_role|self_role|permission|permissions)$/i.test(t.value))
          roleEvidence.push(ref(parent.path, t.line, 'role/permission lexical mention; review enforcement'));
        if (/^[A-Za-z]+Api$/.test(t.value) && ['.', '?.'].includes(parent.body[i + 1]?.value) && parent.body[i + 2]?.kind === 'identifier')
          apiEvidence.push({ api: `${t.value}.${parent.body[i + 2].value}`, evidence: ref(parent.path, t.line, 'host/API lexical reference; transitive dependencies unreviewed') });
      }
      for (const match of tokens(parent.path).filter((t, i, all) => t.kind === 'identifier' && /Schema$/.test(t.value) && all[i - 1]?.value === 'const'))
        schemas.push(ref(parent.path, match.line, 'schema declaration in handler module; exact use requires review'));
    }
    const dedup = list => [...new Map(list.map(v => [JSON.stringify(v), v])).values()];
    return { id: `onebot.action.${name}`, name, kind: 'action', evidence: nameOf(c.name),
      registrations: regs, effective_class: c.name, replacement_count: regs.length - 1,
      aliases: ['_async', '_rate_limited'].map((suffix, i) => ({ name: name + suffix, evidence: aliasEvidence[winner.source][i] })),
      test_only: c.path.includes('/test/') || c.name === 'TestDownloadStream',
      test_classification_evidence: c.name === 'TestDownloadStream' ? ref(c.path, c.line, 'TestDownloadStream class; explicit production registration retained') : null,
      input: identifierRefs(c, 'payloadSchema'), output: identifierRefs(c, 'returnSchema'),
      schema_declarations: dedup(schemas), inheritance: ancestors.map(p => ({ class_name: p.name, evidence: ref(p.path, p.line, 'class declaration') })),
      permission: { requirement: null, candidate_evidence: dedup(roleEvidence), review_state: 'unreviewed' },
      host_api_dependencies: { lexical_references: dedup(apiEvidence), completeness: 'direct_lexical_only' },
      supported_versions: null, caligo: { handler: null, status: 'not_assessed', normal_entry: null },
      acceptance: { reference: null, caligo: null },
      unknown_reasons: ['Schema references are static pointers, not evaluated schemas; inherited/transitive semantics require review.',
        'Required permissions, version-specific host ABI, reference field coverage and all Caligo mappings/acceptance are unconfirmed.'] };
  });
  const effectiveKeys = new Map();
  const overwrites = [];
  for (const registration of occurrences) for (const suffix of ['', '_async', '_rate_limited']) {
    const key = registration.name + suffix;
    if (effectiveKeys.has(key)) overwrites.push({ key, previous: effectiveKeys.get(key), replacement: registration });
    effectiveKeys.set(key, registration);
  }
  const registeredNames = new Set(actions.map(a => a.name));
  const routerDiff = {
    router_only: router.filter(r => !registeredNames.has(r.value)).map(r => ({ ...r, evidence: ref(ROUTER, r.line, 'router declaration is not a registration'),
      decorator: decorators.find(d => d.name === r.value)?.class_name ?? null })),
    registered_only: actions.filter(a => !router.some(r => r.value === a.name)).map(a => a.name),
    duplicate_router_values: router.filter((r, i) => router.findIndex(v => v.value === r.value) !== i),
    map_overwrites: overwrites,
  };
  const events = [];
  for (const p of ordered([...sources.keys()].filter(p => /^packages\/snowcat-(?:onebot|protocol)\/event\/.*\.ts$/.test(p)))) {
    for (const c of declarations(tokens(p), 'class')) {
      const discriminators = [];
      for (let i = 0; i < c.body.length; i++) {
        const t = c.body[i];
        if (!['post_type', 'notice_type', 'request_type', 'message_type', 'meta_event_type', 'sub_type'].includes(t.value)) continue;
        const next = c.body[i + 1];
        if (!['=', ':'].includes(next?.value)) continue;
        const value = c.body[i + 2];
        discriminators.push({ field: t.value, form: next.value === '=' ? 'assignment' : 'type_annotation',
          literal: value?.kind === 'string' ? value.value : null,
          symbol: value?.kind === 'identifier' ? value.value + (c.body[i + 3]?.value === '.' ? '.' + c.body[i + 4]?.value : '') : null,
          evidence: ref(p, t.line, 'event discriminator; constructor values/inheritance not evaluated') });
      }
      events.push({ id: `event.${c.name}`, name: c.name, base: c.base, abstract: c.abstract,
        kind: 'event_class_declaration', evidence: ref(p, c.line, 'event class declaration; not a runtime event census'),
        discriminators, ...nullContract() });
    }
  }
  const messagePath = 'packages/snowcat-onebot/types/message.ts';
  const messageTokens = tokens(messagePath);
  const messageSegments = enumMembers(messageTokens, 'OB11MessageDataType').map(m => {
    const sites = messageTokens.flatMap((t, i, all) => t.value === 'OB11MessageDataType' && all[i + 1]?.value === '.' && all[i + 2]?.value === m.member
      ? [ref(messagePath, t.line, 'segment schema/reference; send/receive support not inferred')] : []);
    return { id: `message_segment.${m.value}`, name: m.value, member: m.member, kind: 'message_segment_enum',
      evidence: ref(messagePath, m.line, 'OB11MessageDataType enum member'), schema_references: sites, ...nullContract() };
  });
  const adapters = [];
  for (const p of ordered([...sources.keys()].filter(p => /^packages\/snowcat-onebot\/network\/[^/]+\.ts$/.test(p)))) {
    for (const c of declarations(tokens(p), 'class').filter(c => /Adapter$/.test(c.name) && !c.abstract)) {
      const wiringPath = 'packages/snowcat-onebot/index.ts';
      const wiringTokens = tokens(wiringPath);
      const initializationEnd = findLine(wiringPath, /private async reloadNetwork/);
      const wiring = wiringTokens.filter((t, i, all) => t.value === c.name &&
        (all[i - 1]?.value === 'new' || (all[i - 1]?.value === ',' && t.line >= initializationEnd)));
      const initial = wiring.filter(t => t.line < initializationEnd);
      const reload = wiring.filter(t => t.line >= initializationEnd);
      const auth = tokens(p).filter(t => /^(?:authOk|authorization|Authorization|token|access_token)$/.test(t.value));
      adapters.push({ id: `protocol.${c.name}`, name: c.name, kind: 'network_adapter_class', base: c.base,
        evidence: ref(p, c.line, 'concrete adapter declaration'), normal_entry_candidates: wiring.map(t => ref(wiringPath, t.line, 'adapter constructor/config-change reference')),
        wiring: { initial_constructor_sites: initial.map(t => ref(wiringPath, t.line, 'initialization constructor')),
          configuration_change_sites: reload.map(t => ref(wiringPath, t.line, 'reloadNetwork adapter class argument')),
          initial_constructor_found: initial.length > 0,
          note: initial.length ? 'Configuration/path conditions still apply; runtime not executed.' : 'No direct constructor found in initial registration block; config-change path is separate.' },
        ...nullContract(), permission: { requirement: null, candidate_evidence: unique(auth.map(t => t.line)).map(line => ref(p, line, 'auth/token lexical mention; includes inherited/config-conditional checks')), review_state: 'unreviewed' } });
    }
  }
  const second = 'packages/snowcat-protocol/action/index.ts';
  const secondTokens = tokens(second);
  const secondSet = secondTokens.filter((t, i, all) => t.value === 'actionMap' && all[i + 1]?.value === '.' && all[i + 2]?.value === 'set');
  adapters.push({ id: 'protocol.snowcat_second_protocol', name: 'SnowCatProtocolAdapter', kind: 'protocol_scaffold',
    evidence: ref('packages/snowcat-protocol/index.ts', 26, 'adapter declaration'), action_registration_count: secondSet.length,
    action_map_evidence: ref(second, findLine(second, /export function createActionMap/), 'empty map factory at pinned commit'), ...nullContract() });

  const components = [];
  for (const p of ordered([...sources.keys()].filter(p => /^packages\/[^/]+\/package\.json$/.test(p)))) {
    const data = JSON.parse(source(p).text);
    components.push({ id: `component.${p.split('/')[1]}`, name: data.name ?? p.split('/')[1], kind: 'package_manifest',
      evidence: ref(p, 1, 'package manifest; package presence is not normal-entry reachability'),
      declared_entrypoints: { main: data.main ?? null, module: data.module ?? null, types: data.types ?? null, exports: data.exports ?? null },
      ...nullContract() });
  }
  // Some package directories have no package.json; retain them as directory
  // components rather than silently dropping loader/native source surfaces.
  const packageDirectories = ordered(unique([...sources.keys()].filter(p => p.startsWith('packages/')).map(p => p.split('/')[1])));
  for (const directory of packageDirectories.filter(d => !components.some(c => c.id === `component.${d}`))) {
    const first = ordered([...sources.keys()].filter(p => p.startsWith(`packages/${directory}/`)))[0];
    components.push({ id: `component.${directory}`, name: directory, kind: 'source_directory_without_manifest',
      evidence: ref(first, 1, 'tracked package source exists; no package.json at pinned commit'), declared_entrypoints: null, ...nullContract() });
  }
  const componentSurfaceExports = [];
  const entrypointPaths = new Set();
  const collectEntrypoints = (directory, value) => {
    if (typeof value === 'string') {
      const p = path.posix.normalize(`packages/${directory}/${value}`);
      if (sources.has(p) && /\.(?:ts|tsx)$/.test(p)) entrypointPaths.add(p);
    } else if (value && typeof value === 'object') for (const child of Object.values(value)) collectEntrypoints(directory, child);
  };
  for (const component of components) {
    const directory = component.id.slice('component.'.length);
    if (component.declared_entrypoints) collectEntrypoints(directory, component.declared_entrypoints);
    for (const conventional of [`packages/${directory}/index.ts`, `packages/${directory}/snowcat.ts`])
      if (sources.has(conventional)) entrypointPaths.add(conventional);
  }
  for (const p of ordered(entrypointPaths)) {
    const ts = tokens(p);
    for (let i = 0; i < ts.length; i++) {
      if (ts[i].value !== 'export') continue;
      let j = i + 1;
      const typeOnly = ['type', 'interface'].includes(ts[j]?.value);
      while (['default', 'abstract', 'async', 'declare'].includes(ts[j]?.value)) j++;
      if (['class', 'function', 'const', 'let', 'enum', 'type', 'interface'].includes(ts[j]?.value) && ts[j + 1]?.kind === 'identifier') {
        componentSurfaceExports.push({ id: `component_export.${p}.${ts[i].line}`, component: p.split('/')[1],
          name: ts[j + 1].value, kind: `typescript_export_${ts[j].value}`, type_only: typeOnly,
          evidence: ref(p, ts[i].line, 'package public/conventional entrypoint export declaration'), ...nullContract() });
      } else if (['*', '{'].includes(ts[j]?.value)) {
        let end = j;
        while (end < ts.length && ts[end].value !== ';') end++;
        const specifier = ts.slice(j, end).find(t => t.kind === 'string');
        const named = ts.slice(j, end).filter(t => t.kind === 'identifier').map(t => t.value).filter(v => !['from', 'as', 'type'].includes(v));
        componentSurfaceExports.push({ id: `component_export.${p}.${ts[i].line}`, component: p.split('/')[1],
          name: ts[j].value === '*' ? '*' : named.join(','), module_specifier: specifier?.value ?? null,
          kind: ts[j].value === '*' ? 'typescript_export_star' : 'typescript_export_list', type_only: typeOnly,
          evidence: ref(p, ts[i].line, 'export edge; star/list resolution deliberately not flattened'), ...nullContract() });
      }
    }
  }
  const nativeComponents = [
    ['NapCatWinBootHook.dll', 'hook/EVIDENCE.md', /#/, 'no_export_behavioral_contract'],
    ['ffmpeg.dll', 'ffmpeg/EVIDENCE.md', /#/, 'no_export_placeholder_lifecycle'],
    ['winmm-v8.dll', 'winmm/README.md', /#/, 'export_table_and_behavioral_contract'],
    ['winpty.dll', 'winpty/README.md', /#/, 'export_table_and_behavioral_contract'],
    ['QQNT.dll', 'qqnt/README.md', /#/, 'development_host_forwarding_contract'],
    ['napiloader.dll', 'napiloader/RECOVERY-EVIDENCE.md', /#/, 'no_export_loader_contract'],
    ['ffmpegAddon', 'addons/README.md', /ffmpegAddon.*音频/, 'typescript_and_research_contract'],
    ['MoeHoo', 'addons/README.md', /MoeHoo.*观察/, 'typescript_and_research_contract'],
    ['napi2native', 'addons/README.md', /napi2native.*SSO/, 'typescript_and_research_contract'],
  ].map(([name, p, marker, contractKind]) => ({ id: `native_component.${name}`, name, kind: contractKind,
    evidence: ref(RECOVERED + p, findLine(RECOVERED + p, marker), 'research contract pointer; no decompiled implementation copied'), ...nullContract() }));
  const componentExports = [];
  for (const module of ['winmm', 'winpty', 'qqnt']) {
    const p = `${RECOVERED}${module}/exports.tsv`;
    const lines = source(p).text.trimEnd().split(/\r?\n/);
    const headers = lines[0].split('\t');
    for (let i = 1; i < lines.length; i++) {
      const cells = lines[i].split('\t'); const row = Object.fromEntries(headers.map((h, j) => [h, cells[j] ?? '']));
      if (!/^\d+$/.test(row.ordinal) || !row.name) fail(`Invalid export table row: ${p}:${i + 1}`);
      componentExports.push({ component: module, ordinal: Number(row.ordinal), name: row.name,
        forwarder: row.forwarder || null, evidence: ref(p, i + 1, 'archived PE export-table row; not a behavior equivalence assertion'),
        input: null, output: null, permission: null, host_api_dependencies: null, supported_versions: null,
        caligo: { handler: null, status: 'not_assessed', normal_entry: null }, acceptance: { reference: null, caligo: null },
        unknown_reasons: ['Export name/forwarder only; ABI, permissions, exact supported builds, Caligo backend and acceptance unreviewed.'],
        contract_defaults: 'unreviewed_export' });
    }
  }
  const nativeSurfaces = [];
  const interfaces = [
    ['packages/snowcat-core/helper/ffmpeg/ffmpeg-addon.ts', ['FFmpeg']],
    ['packages/snowcat-core/packet/handler/client.ts', ['NativePacketExportType']],
    ['packages/snowcat-core/packet/handler/napi2nativeLoader.ts', ['Napi2NativeExportType', 'BypassOptions']],
    ['packages/snowcat-pty/native.d.ts', ['IConptyNative', 'IWinptyNative', 'IUnixNative']],
  ];
  for (const [p, names] of interfaces) {
    for (const declaration of declarations(tokens(p), 'interface').filter(d => names.includes(d.name))) {
      const members = new Map(); let depth = 0;
      for (let i = 0; i < declaration.body.length; i++) {
        const t = declaration.body[i];
        if (depth === 0 && t.kind === 'identifier' && ['(', '?', ':'].includes(declaration.body[i + 1]?.value)) {
          if (!members.has(t.value)) members.set(t.value, []);
          members.get(t.value).push(ref(p, t.line, 'TypeScript interface member; declaration only'));
        }
        if (['{', '[', '('].includes(t.value) && t.kind === 'punctuation') depth++;
        if (['}', ']', ')'].includes(t.value) && t.kind === 'punctuation') depth--;
      }
      for (const [name, sites] of members) nativeSurfaces.push({ id: `native_surface.${declaration.name}.${name}`,
        name, interface: declaration.name, kind: declaration.name === 'BypassOptions' ? 'declared_bypass_option' : 'declared_native_interface_member',
        evidence: sites, ...nullContract() });
    }
  }
  const webuiRoutes = [];
  for (const p of ordered([...sources.keys()].filter(p => /^packages\/snowcat-webui-backend\/src\/router\/.*\.ts$/.test(p)))) {
    const ts = tokens(p);
    for (let i = 0; i < ts.length; i++) {
      if (ts[i].value !== 'router' || ts[i + 1]?.value !== '.' || !['get', 'post', 'put', 'delete', 'all', 'use', 'ws'].includes(ts[i + 2]?.value) ||
          ts[i + 3]?.value !== '(' || ts[i + 4]?.kind !== 'string') continue;
      webuiRoutes.push({ id: `webui_route.${p.split('/').at(-1)}.${ts[i].line}`, name: ts[i + 4].value, method: ts[i + 2].value,
        kind: ts[i + 2].value === 'use' ? 'router_mount' : 'router_registration',
        evidence: ref(p, ts[i].line, 'literal router registration; full mount path/middleware unreviewed'), ...nullContract() });
    }
  }
  const tools = [];
  for (const p of ordered([...sources.keys()].filter(p => /(?:^|\/)package\.json$/.test(p)))) {
    const data = JSON.parse(source(p).text);
    for (const name of ordered(Object.keys(data.scripts ?? {}))) tools.push({ id: `tool.${p}.${name}`, name, kind: 'package_script',
      evidence: ref(p, source(p).text.slice(0, source(p).text.indexOf(JSON.stringify(name), source(p).text.indexOf('"scripts"'))).split('\n').length,
        'package.json scripts key; command intentionally not executed or copied'), ...nullContract() });
  }
  for (const p of ordered([...sources.keys()].filter(p => /^snowcat-[^/]+\.ps1$/.test(p)))) {
    tools.push({ id: `tool.${p}`, name: p, kind: 'launcher_or_deployment_script', evidence: ref(p, 1, 'tracked launcher/tool file; not executed'), ...nullContract() });
  }
  const namedGaps = [
    ['boot_hook_signature_branch', RECOVERED + 'hook/EVIDENCE.md', /conditional branch opcode/, 'research_named_behavior'],
    ['packet_o3_report_rewrite', RECOVERED + 'addons/README.md', /其它宿主和 O3/, 'explicit_unrestored_behavior'],
    ['napi2native_hidden_interfaces', RECOVERED + 'addons/README.md', /其它宿主和隐藏接口/, 'explicit_unrestored_behavior'],
    ['electron_missing_exports', RECOVERED + 'qqnt/README.md', /(?:stub|空|缺失|存根)/i, 'development_host_missing_behavior'],
  ].map(([name, p, marker, kind]) => ({ id: `contract_gap.${name}`, name, kind,
    evidence: ref(p, findLine(p, marker), 'named contract gap; not counted as implemented'), ...nullContract() }));
  const allPackageDirs = ordered(unique(rowsForPackageDirectories(reference)));
  for (const directory of allPackageDirs.filter(d => !components.some(c => c.id === `component.${d}`))) {
    components.push({ id: `component.${directory}`, name: directory, kind: 'tracked_directory_without_selected_source',
      evidence: { path: `packages/${directory}/`, line: null, origin: 'pinned Git tree directory entry; no textual contract extracted', commit: COMMIT },
      reference_directory: `packages/${directory}/`, declared_entrypoints: null, ...nullContract() });
  }
  const wrapperInterfaces = [];
  for (const p of ordered([...sources.keys()].filter(p => /^packages\/snowcat-core\/(?:services|listeners)\/.*\.ts$/.test(p)))) {
    for (const declaration of declarations(tokens(p), 'interface')) {
      let depth = 0;
      for (let i = 0; i < declaration.body.length; i++) {
        const t = declaration.body[i];
        const next = declaration.body[i + 1];
        if (depth === 0 && t.kind === 'identifier' && (next?.value === '(' ||
            (next?.value === '?' && declaration.body[i + 2]?.value === '('))) {
          wrapperInterfaces.push({ id: `wrapper_interface.${declaration.name}.${t.value}.${t.line}`, name: t.value,
            interface: declaration.name, kind: p.includes('/listeners/') ? 'wrapper_listener_method' : 'wrapper_service_method',
            evidence: ref(p, t.line, 'TypeScript wrapper method contract; not a registered OneBot action or verified host ABI'), ...nullContract() });
        }
        if (['{', '[', '('].includes(t.value) && t.kind === 'punctuation') depth++;
        if (['}', ']', ')'].includes(t.value) && t.kind === 'punctuation') depth--;
      }
    }
  }
  // A schema-based message event does not appear in event/ class counts.
  const eventSchemas = messageTokens.filter((t, i, all) => t.value === 'OB11MessageSchema' && all[i - 1]?.value === 'const').map(t => ({
    id: 'event_schema.OB11MessageSchema', name: 'OB11MessageSchema', kind: 'event_schema',
    evidence: ref(messagePath, t.line, 'OneBot message event schema outside event class directory'), ...nullContract() }));
  const sourceFiles = ordered(consumed).map(p => {
    const s = sources.get(p); return { path: p, git_blob: s.blob, sha256: s.sha256, bytes: s.bytes };
  });
  const sections = { actions, decorators, events, event_schemas: eventSchemas, message_segments: messageSegments, protocols: adapters,
    components, component_surface_exports: componentSurfaceExports, native_components: nativeComponents, component_exports: componentExports,
    native_surfaces: nativeSurfaces, wrapper_interfaces: wrapperInterfaces, webui_routes: webuiRoutes, tools, named_contract_gaps: namedGaps };
  return {
    schema_version: 1,
    scope: 'A0 reference-source inventory; no Caligo capability or field acceptance asserted',
    reference: { repository: 'Napcat-Snowluma-research', commit: COMMIT, input_kind: 'immutable_git_blobs',
      worktree_used: false, reference_code_executed: false, decompiled_c_cpp_read_or_copied: false },
    reproduction: { command: 'node scripts/extract-snowcat-capabilities.mjs --reference <checkout> --output docs/contracts/snowcat-capability-inventory.json',
      check_command: 'node scripts/extract-snowcat-capabilities.mjs --reference <checkout> --output docs/contracts/snowcat-capability-inventory.json --check',
      tests: 'node --test scripts/extract-snowcat-capabilities.test.mjs',
      extractor_sha256: sha256(readFileSync(fileURLToPath(import.meta.url))), deterministic: true },
    counting_rules: {
      actions: 'getAllHandlers explicit new expressions plus value-import-reachable @ActionHandler declarations; duplicate Map keys use last registration.',
      aliases: 'Each actual registration inserts base, _async and _rate_limited. Aliases are keys, not independent backend implementations.',
      tests: 'Unreachable decorator test classes excluded with reasons; explicitly registered test_download_stream retained in raw denominator and flagged.',
      events: 'Class declarations under OneBot/second-protocol event directories; abstract/base classes included and flagged. This is a declaration denominator, not emitted event coverage.',
      message_segments: 'OB11MessageDataType enum members; declaration/schema evidence does not establish send/receive support.',
      exports: 'Rows of three tracked exports.tsv files; each inherits export_contract_defaults.unreviewed_export. Symbol presence alone says nothing about ABI/behavior equivalence.',
      other_sections: 'Manifest, adapter, route, interface and script declarations with distinct section denominators; no claim of exhaustive semantic decomposition of every helper.',
    },
    counts: { explicit_registration_occurrences: occurrences.filter(o => o.source === 'explicit').length,
      unique_explicit_classes: new Set(occurrences.filter(o => o.source === 'explicit').map(o => o.class_name)).size,
      decorator_declarations: decorators.length, reachable_decorator_registrations: decorators.filter(d => d.included_in_denominator).length,
      unique_registered_base_actions: actions.length, generated_alias_keys: effectiveKeys.size - actions.length,
      final_map_keys: effectiveKeys.size, map_overwrites: overwrites.length,
      base_actions_excluding_flagged_test: actions.filter(a => !a.test_only).length,
      router_members: router.length, router_unique_values: new Set(router.map(r => r.value)).size,
      router_only: routerDiff.router_only.length, registered_only: routerDiff.registered_only.length,
      event_class_declarations: events.length, concrete_event_class_declarations: events.filter(e => !e.abstract).length,
      event_schema_declarations: eventSchemas.length, wrapper_interface_method_declarations: wrapperInterfaces.length,
      message_segment_enum_members: messageSegments.length, protocols: adapters.length, package_components: components.length,
      component_entrypoint_export_declarations: componentSurfaceExports.length,
      native_components: nativeComponents.length, component_export_rows: componentExports.length,
      component_exports_by_module: Object.fromEntries(['winmm', 'winpty', 'qqnt'].map(m => [m, componentExports.filter(e => e.component === m).length])),
      native_interface_members: nativeSurfaces.length, webui_route_registrations: webuiRoutes.length, tools: tools.length,
      named_contract_gaps: namedGaps.length, source_files: sourceFiles.length,
      actions_with_unknown_permission: actions.filter(a => a.permission.requirement === null).length,
      actions_with_null_input_schema: actions.filter(a => a.input === null).length,
      actions_with_null_output_schema: actions.filter(a => a.output === null).length,
      actions_with_unknown_supported_versions: actions.filter(a => a.supported_versions === null).length,
      actions_with_unassessed_caligo_mapping: actions.filter(a => a.caligo.status === 'not_assessed').length },
    unknowns: [
      { id: 'authorization_review', reason: 'Lexical role/token mentions are evidence candidates; per-action and per-endpoint access control is not inferred.', affected: 'all sections' },
      { id: 'runtime_registration', reason: 'Reference code was not executed; static value-import closure does not model bundler tree shaking, dynamic modules or arbitrary side effects.', affected: 'decorators and runtime wiring' },
      { id: 'schema_semantics', reason: 'Schemas are pointed to, never evaluated. Conditional checks, inheritance and referenced definitions need manual review.', affected: 'inputs/outputs' },
      { id: 'events_and_segments_wiring', reason: 'Declarations and schema references do not prove event production or direction-specific message handling.', affected: 'events/message_segments' },
      { id: 'native_contracts', reason: 'Export lists and TS declarations do not prove ABI, ownership, lifetime, hash-bound host support or hidden/O3 behavior.', affected: 'native components/exports/surfaces' },
      { id: 'tool_surface_completeness', reason: 'Tool denominator covers tracked package scripts and five root launch/deployment tools; research fixtures, vendored tools and arbitrary internal helper methods are not flattened into product capabilities.', affected: 'tools/components' },
      { id: 'caligo_mapping_and_acceptance', reason: 'All Caligo handler/normal_entry values are null pending separate source/backend review. Empty lists, learned-only group caches and placeholder group_info are not verified backends.', affected: 'all sections' },
      { id: 'versions_and_field_evidence', reason: 'Research contract documents are pointers, not blanket supported-version or field-success claims. Precise module hashes and per-item acceptance remain unassigned.', affected: 'all sections' },
    ],
    export_contract_defaults: { unreviewed_export: nullContract() },
    registration_evidence: { create_action_map: ref(INDEX, 425, 'map factory'), decorator_registry: ref(`${ACTION}auto-register.ts`, 4, 'ActionHandler pushes target'),
      explicit_aliases: aliasEvidence.explicit, decorator_aliases: aliasEvidence.decorator },
    router_difference: routerDiff,
    ...sections,
    source_files: sourceFiles,
  };
}

export function main(argv) {
  const options = {};
  for (let i = 0; i < argv.length; i++) {
    const name = argv[i];
    if (name === '--check') options.check = true;
    else if (name === '--reference' || name === '--output') {
      if (!argv[i + 1] || argv[i + 1].startsWith('--')) fail(`Missing ${name} value`);
      options[name.slice(2)] = argv[++i];
    } else fail(`Unknown option ${name}`);
  }
  if (!options.reference || !options.output) fail('Required: --reference <checkout> --output <inventory.json> [--check]');
  const reference = realpathSync(path.resolve(options.reference));
  const output = path.resolve(ROOT, options.output);
  const relative = path.relative(reference, output);
  if (!relative || (!relative.startsWith('..' + path.sep) && relative !== '..' && !path.isAbsolute(relative)))
    fail('Output must be outside the reference checkout');
  const inventory = buildInventory(reference);
  const bytes = JSON.stringify(inventory, null, 2) + '\n';
  if (options.check) {
    if (!existsSync(output) || readFileSync(output, 'utf8') !== bytes) fail('Inventory differs from deterministic regeneration');
  } else {
    // Initial creation only: never overwrite an existing artifact without --check.
    writeFileSync(output, bytes, { flag: 'wx' });
  }
  console.log(JSON.stringify({ result: options.check ? 'check_passed' : 'created', counts: inventory.counts }, null, 2));
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { main(process.argv.slice(2)); } catch (error) { console.error(error.message); process.exitCode = 1; }
}
