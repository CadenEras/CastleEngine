// Offline structural checks for THIS kit's deliberately small frontmatter subset.
// This is not a general YAML parser or Claude Code's official settings validator.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const errors = [];
const fail = value => errors.push(value);
const read = file => fs.readFileSync(path.join(root, file), 'utf8');
function walk(directory = '') {
  return fs.readdirSync(path.join(root, directory), { withFileTypes: true })
    .filter(entry => !['.git', 'node_modules', 'target', 'artifacts'].includes(entry.name))
    .flatMap(entry => {
      const item = path.posix.join(directory, entry.name);
      return entry.isDirectory() ? walk(item) : [item];
    });
}
function frontmatter(file, allowed, required) {
  const match = read(file).match(/^---\r?\n([\s\S]*?)\r?\n---\r?\n/);
  if (!match) { fail(`${file}: missing frontmatter`); return {}; }
  const fields = {}; let list;
  for (const line of match[1].split(/\r?\n/)) {
    const item = line.match(/^  - (".*")$/);
    if (item && list) {
      try { fields[list].push(JSON.parse(item[1])); }
      catch { fail(`${file}: invalid quoted list value`); }
      continue;
    }
    const pair = line.match(/^([a-zA-Z][a-zA-Z-]*):(?: (.*))?$/);
    if (!pair) { fail(`${file}: unsupported frontmatter syntax`); continue; }
    const [, key, value] = pair;
    if (!allowed.includes(key) || key in fields) fail(`${file}: unknown or duplicate field ${key}`);
    if (value === undefined) { fields[key] = []; list = key; }
    else { fields[key] = value; list = undefined; }
  }
  for (const key of required) if (!fields[key]) fail(`${file}: missing ${key}`);
  return fields;
}

if (Number(process.versions.node.split('.')[0]) < 22) fail('Node.js 22+ required');
const files = walk();
for (const file of ['CLAUDE.md', 'README.md', '.claude/settings.json',
  '.claude/hooks/pre-tool-use.mjs', '.claude/hooks/session-start.mjs',
  '.github/workflows/agent-config.yml', 'docs/SOURCES.md', 'docs/VALIDATION.md']) {
  if (!files.includes(file)) fail(`Missing ${file}`);
}
let settings;
try { settings = JSON.parse(read('.claude/settings.json')); }
catch { fail('Invalid settings JSON'); }
if (settings) {
  const permissions = settings.permissions;
  if (permissions?.defaultMode !== 'acceptEdits') fail('Unexpected default permission mode');
  for (const group of ['allow', 'ask', 'deny']) {
    if (!Array.isArray(permissions?.[group])) { fail(`Missing permissions.${group}`); continue; }
    if (new Set(permissions[group]).size !== permissions[group].length) fail(`Duplicate ${group} rules`);
    for (const rule of permissions[group]) {
      if (typeof rule !== 'string' || !/^[\w]+\(.+\)$/.test(rule)) fail(`Invalid permission rule in ${group}`);
      if (/^(Write|NotebookEdit)\(/.test(rule)) fail('Unsupported file permission tool');
      if (group === 'allow' && /^(Bash|PowerShell)\(\*\)$/.test(rule)) fail('Blanket shell grant');
    }
  }
  for (const event of ['SessionStart', 'PreToolUse']) {
    const groups = settings.hooks?.[event];
    if (!Array.isArray(groups) || groups.length !== 1) { fail(`Missing hook ${event}`); continue; }
    for (const group of groups) for (const hook of group.hooks ?? []) {
      const match = hook.command?.match(/^node "\$\{CLAUDE_PROJECT_DIR\}\/([^"\n]+)"$/);
      if (hook.type !== 'command' || !match || !files.includes(match[1])) fail(`Invalid handler ${event}`);
      if (hook.timeout !== 5) fail(`Unexpected timeout ${event}`);
    }
  }
  if (settings.hooks?.PreToolUse?.[0]?.matcher !== 'Read|Edit|Write') fail('Wrong file-hook matcher');
}
for (const file of files.filter(file => file.startsWith('.claude/rules/') && file.endsWith('.md'))) {
  if (!read(file).startsWith('---')) continue;
  const fields = frontmatter(file, ['paths'], ['paths']);
  if (!Array.isArray(fields.paths) || !fields.paths.length) fail(`${file}: empty path scope`);
}
const names = new Set();
for (const file of files.filter(file => /^\.claude\/skills\/[^/]+\/SKILL\.md$/.test(file))) {
  const fields = frontmatter(file, ['name', 'description'], ['name', 'description']);
  if (!/^[a-z0-9-]{1,64}$/.test(fields.name) || fields.name !== file.split('/')[2]) fail(`${file}: invalid skill name`);
  if (names.has(fields.name)) fail(`${file}: duplicate skill`);
  names.add(fields.name);
}
let agents = 0;
for (const file of files.filter(file => /^\.claude\/agents\/[^/]+\.md$/.test(file))) {
  const fields = frontmatter(file, ['name', 'description', 'tools', 'model', 'permissionMode'],
    ['name', 'description', 'tools', 'model', 'permissionMode']);
  if (fields.name !== path.basename(file, '.md')) fail(`${file}: mismatched agent name`);
  const tools = String(fields.tools).split(/, */);
  if (tools.some(tool => !['Read', 'Glob', 'Grep', 'Edit', 'Write', 'Bash', 'PowerShell'].includes(tool))) fail(`${file}: unknown tool`);
  if (fields.permissionMode !== 'default' || fields.model !== 'inherit') fail(`${file}: unexpected model or permissions`);
  if (['architect', 'pdf-specialist', 'reviewer'].includes(fields.name)
    && tools.some(tool => !['Read', 'Glob', 'Grep'].includes(tool))) fail(`${file}: reviewer can mutate`);
  agents++;
}
if (agents !== 6 || names.size !== 4) fail('Expected 6 agents and 4 skills');
for (const file of files.filter(file => file.endsWith('.md'))) {
  const content = read(file);
  for (const [, destination] of content.matchAll(/\[[^\]\n]*\]\(([^)\s]+)\)/g)) {
    if (/^(https?:|#|mailto:)/.test(destination)) continue;
    const target = path.resolve(root, path.dirname(file), destination.split('#')[0]);
    const relative = path.relative(root, target);
    if (relative.startsWith('..') || path.isAbsolute(relative) || !fs.existsSync(target)) fail(`${file}: broken local link ${destination}`);
  }
}
if (errors.length) {
  process.stderr.write(errors.map(error => `ERROR: ${error}`).join('\n') + '\n');
  process.exitCode = 1;
} else {
  process.stdout.write(`OK: ${files.length} files; settings, scoped rules, ${agents} agents, `
    + `${names.size} skills, hook registration and local Markdown links.\n`);
}
