# Tests et définition de terminé

## Matrice de contrôles

| Niveau | Couverture attendue | Fréquence |
| --- | --- | --- |
| Kit agentique | JSON, règles, liens, hooks et cas négatifs | Chaque PR |
| Rust | fmt, Clippy, unitaires, intégration, doctests, features supportées | Chaque PR P0+ |
| UI | lint, types, tests interactions, build | Chaque PR P0+ |
| Desktop | compilation native, IPC, ouverture/fermeture/sauvegarde | Chaque PR pertinente, trois OS avant release |
| Parsing | corpus normal/malformé, limites, propriétés | Chaque changement de parseur |
| Rendu | goldens et comparaison à des moteurs indépendants | Chaque changement de rendu |
| Fuzzing | entrées arbitraires avec budgets, crashs minimisés | Smoke court en PR, campagne planifiée |
| Sécurité | dépendances, licences, secrets, capabilities et `unsafe` | Chaque PR concernée + périodique |
| Performance | temps, mémoire et allocations sur corpus versionné | Chaque changement critique + avant release |

## Corpus

Conserver des fixtures synthétiques minimales et des cas publics redistribuables.
Chaque entrée a un identifiant, hash, origine, licence, fonctionnalités et résultat
attendu. Les documents privés restent hors Git, hors logs et hors artefacts publics.
Un PDF « public sur le web » n'est pas nécessairement redistribuable.

Inclure progressivement : objets directs/indirects, xref classique et en flux,
object streams, révisions incrémentales, Unicode, arbres cycliques, filtres imbriqués,
images gigantesques, polices endommagées, chiffrement non supporté, fichiers tronqués
et incohérences de longueur. Un fichier non supporté doit échouer proprement.

## Principes de tests

Vérifier les comportements, pas les détails de l'implémentation. Un bug reproductible
devient un test qui échoue avant la correction, quand techniquement possible.
Pour les règles documentaires, citer la clause PDF applicable dans le test ou sa notice.
Les propriétés de round-trip portent sur la sémantique du sous-ensemble supporté ;
l'ordre des objets, les offsets et la compression peuvent changer sans perte visible.

Une comparaison qpdf ne vérifie pas le rendu ; une image de référence ne prouve pas
la présence du texte ni son accessibilité. Croiser structure, contenu, rendu et
manipulations. Gérer les variations de polices/antialiasing avec un environnement
fixé et des tolérances déclarées, sans accepter aveuglément de nouveaux goldens.

Tests de sauvegarde : disque plein simulé, permission refusée, interruption, fichier
verrouillé et remplacement selon OS. Vérifier que l'original reste lisible et inchangé
après un échec. Tester undo/redo et l'invalidation des caches après édition.

Les fuzzers et oracles tournent sans réseau et avec temps/mémoire bornés. Au crash :
isoler, minimiser, consigner seed/version/limites, corriger puis conserver un test sûr.
Miri couvre les composants compatibles ; les sanitizers et les codecs natifs ont
des environnements dédiés. Une campagne finie ne prouve jamais l'absence de défaut.

## Sortie d'une tâche

Résultat demandé couvert, diff relu, tests pertinents réussis, limites et erreurs
documentées, aucune donnée privée, contrats mis à jour. Pour une modification critique,
joindre une revue distincte et les mesures avant/après. Mentionner explicitement les
contrôles indisponibles ou en échec, sans les transformer en succès.

Ne pas imposer un pourcentage de couverture universel : suivre les branches de parsing,
erreurs et limites réellement exercées. La couverture aide à trouver un manque ; elle
ne remplace ni assertions utiles ni corpus représentatif.
