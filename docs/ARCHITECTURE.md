# Architecture cible

Décision proposée pour démarrer : monorepo, workspace Cargo à la racine, application
Tauri 2 dans `apps/desktop`, frontend React/TypeScript. Windows prioritaire, mêmes
contrats et jeux de tests sur macOS et Linux. Les versions exactes et OS minimums
seront verrouillés en P0 d'après les prérequis réellement testés.

```text
React/TypeScript : pages visibles, sélection, outils, état UI
        |
        | commandes typées, petits DTO, erreurs et annulation
        v
Tauri : fichiers autorisés, documents ouverts, IPC, orchestration
        |
        | protocole borné et versionné, buffers limités
        v
Worker Rust : lecture / rendu / édition / écriture
        |
        +-- pdf-syntax      objets, tokenizer, xref, flux et sources
        +-- pdf-document    graphes, pages, ressources et opérations
        +-- pdf-render      interprétation graphique et rendu en tuiles
        +-- pdf-write       sérialisation, réécriture et sauvegarde
pdf-cli : accès au même cœur pour tests et diagnostics
```

`pdf-document` dépend de `pdf-syntax` ; `pdf-render` et `pdf-write` dépendent du modèle.
Le modèle ne dépend pas de ses consommateurs. CLI, worker et shell dépendent du cœur.
Un type partagé réellement transversal peut être extrait après constat d'un besoin,
sans créer un conteneur générique de toute l'application.

## Contrats à figer tôt

- Source à accès aléatoire, tailles vérifiées en `u64`, conversion bornée vers les tailles
  de plateforme, lectures paresseuses et politique de modification du fichier source.
- `ResourceLimits`, cumul de budgets, cancellation token et échéance par requête.
  Limites sur objets visités, profondeur, octets décodés, pixels et travail en vol.
- Erreurs typées : malformé, tronqué, limite, annulé, IO, mot de passe requis,
  fonctionnalité non supportée. Diagnostic utile sans contenu privé ni mot de passe.
- Identifiants opaques document/page, révision, propriétaire des handles, fermeture
  explicite et rejet des handles périmés ou appartenant à un autre document.
- Tuile : page, rectangle, rotation, zoom, profil couleur, révision, génération de requête.
  Les réponses anciennes ne remplacent jamais un état plus récent de l'interface.
- Écriture transactionnelle vers un fichier temporaire du même volume, puis remplacement
  maîtrisé selon l'OS ; conserver l'original et nettoyer les temporaires après erreur.

## « De zéro » et dépendances

Le parseur, les références, le modèle, l'interpréteur et l'écriture PDF sont développés
dans le projet. Utiliser une bibliothèque de décompression, de police, de couleur ou
de rasterisation est possible après ADR : réécrire ces domaines par principe n'améliore
pas automatiquement sûreté ou empreinte. Ne pas intégrer un moteur PDF complet en
production en présentant ensuite l'assemblage comme un moteur maison.

## Processus et isolation

Le worker doit pouvoir être arrêté sans bloquer l'UI et sans perdre un fichier source.
En P0, définir et prototyper cette frontière ; avant ouverture de PDF externes dans
une release, appliquer limites de ressources et droits OS adaptés à chaque plateforme.
Un sous-processus sans restrictions de fichiers/réseau reste un processus privilégié.
Le workspace ne prétend pas fournir une isolation complète avant ces vérifications.

## Portabilité

Tester chemins Unicode/longs, séparateurs, casse, permissions, fichiers verrouillés,
remplacement, affichages HiDPI, clavier, presse-papiers et impression. Éviter API Windows
dans le cœur. Les builds et signatures sont propres à chaque OS ; un build Windows
ne démontre pas la compatibilité macOS/Linux. ARM64 est une extension à valider,
pas une promesse incluse dans le simple choix de Rust/Tauri.
