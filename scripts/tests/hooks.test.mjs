import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, writeFileSync, symlinkSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../../', import.meta.url));
const hook = path.join(root, '.claude/hooks/pre-tool-use.mjs');
function run(input, project = root) {
  const result = spawnSync(process.execPath, [hook], {
    input: typeof input === 'string' ? input : JSON.stringify(input),
    encoding: 'utf8', env: { ...process.env, CLAUDE_PROJECT_DIR: project },
    timeout: 5000
  });
  assert.ifError(result.error);
  return { ...result, output: result.stdout.trim() ? JSON.parse(result.stdout) : null };
}
function decision(input, project) {
  const result = run(input, project);
  assert.equal(result.status, 0, result.stderr);
  return result.output?.hookSpecificOutput?.permissionDecision ?? null;
}
const call = (tool, file, cwd = root) => ({
  hook_event_name: 'PreToolUse', tool_name: tool, tool_input: { file_path: file }, cwd
});

test('ordinary source reads and edits leave the normal permission flow intact', () => {
  for (const tool of ['Read', 'Edit', 'Write']) {
    assert.equal(decision(call(tool, 'crates/pdf-syntax/src/lib.rs')), null);
  }
});
test('secrets are blocked on read and write without exposing their content', () => {
  for (const file of ['.env', 'apps/desktop/.env.local', 'secrets/token.json', 'keys/signing.p12']) {
    for (const tool of ['Read', 'Edit', 'Write']) {
      const result = run(call(tool, file));
      assert.equal(result.status, 0);
      assert.equal(result.output.hookSpecificOutput.permissionDecision, 'deny');
      assert.doesNotMatch(result.stdout, /token\.json|signing\.p12/);
    }
  }
});
test('Windows separators and traversal do not evade file policy', () => {
  assert.equal(decision(call('Read', 'apps\\desktop\\.env.production')), 'deny');
  assert.equal(decision(call('Edit', 'crates/../.git/config')), 'deny');
  if (process.platform === 'win32') {
    assert.equal(decision(call('Read', 'APPS\\.ENV.LOCAL')), 'deny');
  }
});
test('direct Git metadata writes are denied but ordinary source paths named git are allowed', () => {
  assert.equal(decision(call('Write', '.git/config')), 'deny');
  assert.equal(decision(call('Write', '.git')), 'deny');
  assert.equal(decision(call('Read', '.git/HEAD')), null);
  assert.equal(decision(call('Edit', 'docs/git.md')), null);
});
test('agent policy and executable checks require review on edits', () => {
  for (const file of ['.claude/settings.json', '.claude/hooks/pre-tool-use.mjs', '.mcp.json',
    '.github/workflows/agent-config.yml', 'scripts/validate-agent-kit.mjs', 'scripts/tests/hooks.test.mjs']) {
    assert.equal(decision(call('Edit', file)), 'ask');
    assert.equal(decision(call('Read', file)), null);
  }
});
test('outside-project edits ask, including when cwd is another directory', () => {
  assert.equal(decision(call('Write', '../outside.rs')), 'ask');
  assert.equal(decision(call('Edit', 'outside.rs', path.dirname(root))), 'ask');
  assert.equal(decision(call('Edit', path.join(root, 'docs', 'GIT.md'))), null);
});
test('symlinks cannot hide protected files or outside-project writes', t => {
  const temp = mkdtempSync(path.join(tmpdir(), 'pdf-agent-kit-'));
  try {
    const project = path.join(temp, 'project');
    const outside = path.join(temp, 'outside');
    mkdirSync(project); mkdirSync(outside); mkdirSync(path.join(project, 'secrets'));
    writeFileSync(path.join(project, 'secrets', 'key.txt'), 'synthetic fixture');
    try {
      // Directory junctions work without developer-mode symlink privileges on Windows.
      symlinkSync(path.join(project, 'secrets'), path.join(project, 'alias'),
        process.platform === 'win32' ? 'junction' : 'dir');
      symlinkSync(outside, path.join(project, 'export'),
        process.platform === 'win32' ? 'junction' : 'dir');
    } catch (error) {
      if (['EPERM', 'EACCES', 'ENOSYS'].includes(error.code)) {
        t.skip(`OS disallows symlink fixtures: ${error.code}`); return;
      }
      throw error;
    }
    assert.equal(decision(call('Read', 'alias/key.txt', project), project), 'deny');
    assert.equal(decision(call('Write', 'export/new.txt', project), project), 'ask');
  } finally {
    // This verified temporary directory is the only cleanup target; no shell interpolation.
    assert.equal(path.dirname(temp), tmpdir());
    assert.ok(path.basename(temp).startsWith('pdf-agent-kit-'));
    rmSync(temp, { recursive: true, force: true });
  }
});
test('unrecognized tools do not receive a fabricated permission grant', () => {
  assert.equal(decision(call('Bash', '.env')), null);
});
test('malformed file events fail with code 2 instead of being accepted', () => {
  for (const value of ['not JSON', '{}', 'null', call('Write', ''),
    { ...call('Edit', 'x'), cwd: undefined }, call('Read', 'bad\0path')]) {
    const result = run(value);
    assert.equal(result.status, 2);
    assert.equal(result.output, null);
    assert.match(result.stderr, /Hook input rejected/);
  }
});
test('session hook emits only a SessionStart context envelope', () => {
  const result = spawnSync(process.execPath, [path.join(root, '.claude/hooks/session-start.mjs')], {
    input: '{}', encoding: 'utf8', timeout: 5000
  });
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stderr);
  const output = JSON.parse(result.stdout);
  assert.equal(output.hookSpecificOutput.hookEventName, 'SessionStart');
  assert.match(output.hookSpecificOutput.additionalContext, /docs\/COMMANDS\.md/);
  assert.equal(output.hookSpecificOutput.permissionDecision, undefined);
});
