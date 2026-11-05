// Static context only: no subprocess, network, environment dump or document access.
process.stdout.write(JSON.stringify({
  hookSpecificOutput: {
    hookEventName: 'SessionStart',
    additionalContext: 'Lire CLAUDE.md et docs/COMMANDS.md. Ce kit ne contient pas encore '
      + 'le moteur PDF. Vérifications immédiates : node scripts/validate-agent-kit.mjs ; '
      + 'node --test scripts/tests/hooks.test.mjs. Pour le produit, commencer par P0 '
      + 'dans docs/ROADMAP.md. Ne pas annoncer des tests non exécutés.'
  }
}) + '\n');
