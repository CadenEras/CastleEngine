---
paths:
  - "crates/**"
  - "apps/**"
  - "tests/**"
  - "fuzz/**"
  - "benches/**"
  - "scripts/**"
---
# Tests

- Lire `docs/QUALITY.md` et exécuter les commandes disponibles de `docs/COMMANDS.md`.
- Un correctif PDF comporte un cas minimal reproductible autorisé et un résultat attendu.
- Tester limites exactes, limite + 1, troncature, références cycliques, débordements,
  annulation et échec de sauvegarde ; pas seulement le chemin heureux.
- Propriétés utiles : lecture/écriture/lecture sémantiquement stable pour le sous-ensemble
  supporté ; pas d'accès hors borne ni de croissance non bornée sur entrée arbitraire.
- Comparer à plusieurs oracles indépendants lorsque pertinent ; un oracle peut se tromper.
  Un hash binaire identique n'est pas une preuve de fidélité documentaire.
- Fuzzing borné en temps et mémoire, seed conservée, crash minimisé puis régression.
- Les goldens visuels ont des polices et profils couleur fixés, une tolérance motivée
  et une revue humaine des écarts. Ne pas régénérer les références pour masquer un défaut.
- Ne pas prétendre avoir exécuté une suite indisponible. Énoncer le blocage et le
  périmètre réellement vérifié ; la validation du kit n'est pas un test du moteur.
