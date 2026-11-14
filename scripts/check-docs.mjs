import { readFile, readdir, stat } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const failures = [];
const check = (ok, message) => { if (!ok) failures.push(message); };
const read = (file) => readFile(path.join(root, file), 'utf8');
const exists = async (file) => { try { await stat(file); return true; } catch { return false; } };
const guides = (await readdir(path.join(root, 'docs/guides'))).filter((file) => file.endsWith('.md'));
const live = ['AGENTS.md', 'README.md', 'progress.md', 'session-handoff.md', 'docs/README.md',
    ...guides.map((file) => `docs/guides/${file}`)];
const linked = new Set();
let linkCount = 0;

for (const file of live) {
    const text = await read(file);
    const lines = text.trimEnd().split(/\r?\n/).length;
    const limit = file === 'AGENTS.md' ? 100 : file.startsWith('docs/guides/') ? 150 : null;
    if (limit) check(lines <= limit, `${file}: ${lines} lines exceeds budget ${limit}`);
    // Inspect inline Markdown file destinations outside code fences, not historical prose.
    const prose = text.replace(/^```[^\n]*\n[\s\S]*?^```\s*$/gm, '');
    for (const match of prose.matchAll(/\[[^\]\n]+\]\(([^)\n]+)\)/g)) {
        const target = match[1].trim().replace(/^<|>$/g, '');
        if (/^[a-z][a-z\d+.-]*:/i.test(target)) continue;
        const destination = decodeURIComponent(target.split('#')[0]);
        if (!destination) continue;
        const resolved = path.resolve(root, path.dirname(file), destination);
        const relative = path.relative(root, resolved);
        const withinRoot = relative !== '..' && !relative.startsWith(`..${path.sep}`) && !path.isAbsolute(relative);
        check(withinRoot, `${file}: link escapes repository: ${target}`);
        check(await exists(resolved), `${file}: missing destination: ${target}`);
        linked.add(relative.split(path.sep).join('/'));
        linkCount++;
    }
}

// Each top-level historical Markdown document must remain discoverable from the index.
const index = await read('docs/README.md');
let historyCount = 0;
for (const folder of ['plans', 'research', 'acceptance']) {
    for (const entry of await readdir(path.join(root, 'docs', folder))) {
        if (!entry.endsWith('.md')) continue;
        check(index.includes(`(${folder}/${entry})`), `docs/README.md: unindexed ${folder}/${entry}`);
        historyCount++;
    }
}

const state = JSON.parse(await read('feature_list.json'));
check(Array.isArray(state.features) && state.features.length > 0, 'features must be a non-empty array');
const ids = new Set();
for (const feature of state.features ?? []) {
    check(/^feat-\d+$/.test(feature.id) && !ids.has(feature.id), `invalid/duplicate feature id: ${feature.id}`);
    ids.add(feature.id);
    for (const field of ['name', 'description', 'evidence']) {
        check(typeof feature[field] === 'string', `${feature.id}: ${field} must be a string`);
    }
    check(['not-started', 'in-progress', 'blocked', 'done'].includes(feature.status), `${feature.id}: invalid status`);
    check(Array.isArray(feature.dependencies), `${feature.id}: dependencies must be an array`);
    check(Array.isArray(feature.done_criteria) && feature.done_criteria.length > 0,
        `${feature.id}: missing done criteria`);
    check(Array.isArray(feature.evidence_paths), `${feature.id}: evidence_paths must be an array`);
    if (feature.status === 'done') {
        check(Boolean(feature.evidence?.trim()) && feature.evidence_paths?.length > 0,
            `${feature.id}: done requires evidence and evidence_paths`);
    }
    for (const file of feature.evidence_paths ?? []) {
        check(linked.has(file) || await exists(path.join(root, file)), `${feature.id}: missing evidence path ${file}`);
    }
}
const features = new Map(state.features.map((feature) => [feature.id, feature]));
for (const feature of state.features) {
    for (const dependency of feature.dependencies ?? []) {
        check(ids.has(dependency) && dependency !== feature.id, `${feature.id}: invalid dependency ${dependency}`);
        if (feature.status === 'done') check(features.get(dependency)?.status === 'done',
            `${feature.id}: done while dependency ${dependency} is incomplete`);
    }
}
const visiting = new Set();
const visited = new Set();
function visit(id) {
    if (visiting.has(id)) { failures.push(`dependency cycle at ${id}`); return; }
    if (visited.has(id) || !features.has(id)) return;
    visiting.add(id);
    for (const dependency of features.get(id).dependencies ?? []) visit(dependency);
    visiting.delete(id);
    visited.add(id);
}
for (const id of ids) visit(id);
check(state.active_feature === null || features.get(state.active_feature)?.status === 'in-progress',
    'active_feature must be null or refer to an in-progress feature');

const lock = JSON.parse(await read('skills-lock.json'));
check(lock.skills?.['harness-creator']?.source === 'walkinglabs/learn-harness-engineering',
    'harness-creator source is missing from skills-lock.json');
// Skills are optional workstation tools, not tracked clone/CI dependencies.
check(lock.skills?.['harness-creator']?.skillPath === 'skills/harness-creator/SKILL.md',
    'harness-creator skillPath is missing from skills-lock.json');
for (const required of ['init.ps1', 'init.sh', 'Cargo.lock', 'rust-toolchain.toml']) {
    check(await exists(path.join(root, required)), `required entrypoint missing: ${required}`);
}
if (failures.length) {
    for (const failure of failures) console.error(`FAIL ${failure}`);
    process.exitCode = 1;
} else {
    console.log(`PASS ${live.length} live docs, ${linkCount} local links, ${historyCount} historical entries, ${ids.size} features; instruction budgets and skill lock checked.`);
}
