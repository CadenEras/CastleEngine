// Defence in depth for Claude file tools, not an OS boundary or a shell filter.
import fs from 'node:fs';
import path from 'node:path';

function reply(decision, reason) {
  process.stdout.write(JSON.stringify({ hookSpecificOutput: {
    hookEventName: 'PreToolUse', permissionDecision: decision,
    permissionDecisionReason: reason
  } }) + '\n');
}
function normalized(value) {
  const result = value.replaceAll('\\', '/');
  return process.platform === 'win32' ? result.toLowerCase() : result;
}
function inside(root, file) {
  const relative = path.relative(root, file);
  return relative === '' || (!path.isAbsolute(relative)
    && relative !== '..' && !relative.startsWith(`..${path.sep}`));
}
// Resolve an existing parent too, so a new file underneath a symlink is checked.
function resolvedWithParents(file) {
  let cursor = file;
  const missing = [];
  while (true) {
    try { return path.join(fs.realpathSync.native(cursor), ...missing.reverse()); }
    catch (error) {
      if (error.code !== 'ENOENT') throw error;
      // A dangling symlink is not an ordinary absent file: don't trust its target.
      try {
        if (fs.lstatSync(cursor).isSymbolicLink()) throw new Error('unresolved symlink');
      } catch (statError) {
        if (statError.code !== 'ENOENT') throw statError;
      }
      const parent = path.dirname(cursor);
      if (parent === cursor) throw new Error('unresolved root');
      missing.push(path.basename(cursor)); cursor = parent;
    }
  }
}
function secret(file) {
  const parts = normalized(file).split('/');
  const name = parts.at(-1);
  return parts.some(part => ['secrets', '.ssh', '.aws'].includes(part))
    || name === '.env' || name.startsWith('.env.')
    || /\.(pem|key|p12|pfx)$/.test(name);
}
function needsReview(relative) {
  const value = normalized(relative);
  return ['.mcp.json', '.claude/settings.json', '.claude/settings.local.json',
    'scripts/validate-agent-kit.mjs'].includes(value)
    || value === '.claude/hooks' || value.startsWith('.claude/hooks/')
    || value === '.github/workflows' || value.startsWith('.github/workflows/')
    || value === 'scripts/tests' || value.startsWith('scripts/tests/');
}

try {
  const chunks = []; let length = 0;
  for await (const chunk of process.stdin) {
    length += chunk.length;
    if (length > 1024 * 1024) throw new Error('input too large');
    chunks.push(chunk);
  }
  const input = JSON.parse(Buffer.concat(chunks).toString('utf8'));
  if (!input || input.hook_event_name !== 'PreToolUse'
    || typeof input.tool_name !== 'string') throw new Error('invalid event');
  if (['Read', 'Edit', 'Write'].includes(input.tool_name)) {
    const value = input.tool_input?.file_path;
    const project = process.env.CLAUDE_PROJECT_DIR;
    if (typeof value !== 'string' || !value.trim() || value.includes('\0')
      || typeof input.cwd !== 'string' || !path.isAbsolute(input.cwd)
      || !project || !path.isAbsolute(project)) throw new Error('invalid path');
    const root = fs.realpathSync.native(project);
    // Claude supplies native absolute paths. Also handle backslash-relative paths.
    const lexical = path.resolve(input.cwd, value.replaceAll('\\', path.sep));
    let actual;
    try { actual = resolvedWithParents(lexical); }
    catch {
      reply('ask', 'Chemin impossible à résoudre sûrement : revue nécessaire.');
      process.exit(0);
    }
    const candidates = [lexical, actual];
    const edit = input.tool_name !== 'Read';
    if (candidates.some(secret)) {
      reply('deny', 'Accès à un emplacement de secrets refusé par le kit.');
    } else if (edit && candidates.some(file => normalized(file).split('/').includes('.git'))) {
      reply('deny', 'Écriture directe des métadonnées Git refusée ; utiliser Git.');
    } else if (edit && candidates.some(file => !inside(root, file))) {
      reply('ask', 'Écriture hors de la racine du projet : revue nécessaire.');
    } else if (edit && candidates.some(file => needsReview(path.relative(root, file)))) {
      reply('ask', 'Modification de politique ou de contrôle exécutable : revue nécessaire.');
    }
    // Otherwise no permission decision: Claude's normal rules still apply.
  }
} catch {
  // Never echo untrusted input, paths, document data or secret values.
  process.stderr.write('Hook input rejected: invalid event or file path.\n');
  process.exitCode = 2;
}
