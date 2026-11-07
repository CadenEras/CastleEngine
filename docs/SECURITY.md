# Modèle de menace et exigences

Un attaquant fournit un PDF, une police, une image, un profil couleur, une pièce jointe
ou un chemin. Il peut chercher un crash, une consommation sans borne, une exécution,
une lecture de fichier, une sortie réseau ou une sauvegarde destructive. Un rendu
plausible peut aussi masquer une perte de contenu importante.

## Cœur et worker

Chaque budget est configurable, cumulatif et vérifié avant le travail coûteux :
octets lus/décodés, nombre d'objets/références, profondeur, pixels, allocations et tâches.
Éviter l'expansion répétée des mêmes flux via des références ; limiter les tentatives
de réparation. La lecture d'un gros fichier reste possible avec un index et des fenêtres
bornés ; la limite de mémoire ne se résume pas à une taille de fichier maximale.

Récursion, boucles de références, offsets incohérents, tailles négatives, débordements,
décompression et codecs doivent échouer proprement. Les annulations interrompent les
étapes longues ; si un codec ne peut pas être interrompu, le worker doit pouvoir être
tué sans bloquer l'application. Définir droits et limites de processus pour chaque OS.

Aucun JavaScript PDF, action Launch, programme joint ou accès distant implicite.
Les URL externes passent par un geste explicite, une validation de schéma et la politique
du shell. Les mots de passe ne sont ni journalisés ni persistés par défaut ; ne pas
réinventer les primitives cryptographiques.

## Tauri et UI

Appliquer le moindre privilège selon les [capabilities Tauri](https://v2.tauri.app/security/capabilities/)
et son [modèle de sécurité](https://v2.tauri.app/security/). Définir une CSP adaptée
au build réel, sans script distant, évaluation dynamique ou permissions globales de
fichiers/shell. Ne pas charger de contenu distant dans une WebView privilégiée.

Définir explicitement les permissions des commandes propres à l'application et les
inclure dans les capabilities des fenêtres concernées ; ne pas supposer que les
permissions des plugins protègent automatiquement les commandes applicatives.
Valider types, tailles, chemins et handles côté Rust. Une requête IPC bien typée dans
TypeScript peut toujours être falsifiée. Éviter que des chemins libres ou identifiants
de documents permettent de traverser un répertoire ou d'accéder à un document fermé.

Le texte extrait reste du texte. Nettoyer les noms d'export et interdire traversée,
écriture arbitraire et ouverture d'attachement automatique. Tester le nettoyage de
ressources à la fermeture d'un document et de la fenêtre.

## Sauvegarde et fonctions sensibles

Sauvegarde transactionnelle, même volume et politique de remplacement par OS ; pas
de promesse d'atomicité universelle sur tous les systèmes de fichiers. Une modification
simultanée du source doit être détectée ou interdite. Éviter d'écraser un fichier visé
par un lien symbolique inattendu ; tester les courses pertinentes.

Un caviardage réel exige suppression des contenus concernés, ressources et métadonnées,
réécriture complète sans anciennes révisions récupérables et tests d'extraction forensic.
Un rectangle noir ou une mise à jour incrémentale ne suffit pas.
Un dessin de signature n'est pas une signature cryptographique. Validation de certificats,
horodatage, révocation et politique de confiance forment une capacité séparée.
Ces fonctions ne sont pas commercialisées comme sûres avant leurs jalons dédiés.

## Chaîne de dépendances et distribution

Verrouiller dépendances et toolchains ; suivre RustSec/npm, licences, transitive/native,
sources de polices et codecs. Ne pas adopter automatiquement une bibliothèque à licence
incompatible. Les exceptions ont un responsable, une justification et une échéance.

En place : `deny.toml` (cargo-deny) refuse toute licence hors liste, toute source hors
crates.io et toute dépendance joker ; contrôle requis `deny` sur chaque PR. Les avis
RustSec tournent dans le workflow `advisories` (hebdomadaire, sur `main` et quand
`Cargo.lock` change), non bloquant pour les PR sans rapport. Exceptions actuelles,
toutes atteintes par le shell Tauri et jamais par `crates/*` : MPL-2.0 pour `cssparser`,
`cssparser-macros`, `dtoa-short`, `selectors` et `option-ext` ; RUSTSEC-2024-0370
(`proc-macro-error`, macro de compilation du backend GTK Linux), à revoir avec Tauri 3.
Codec dans le cœur : `miniz_oxide` 0.9.1 (Flate, `#![forbid(unsafe_code)]`, version
exacte épinglée) et sa dépendance `adler2`, sans code natif (ADR-0003) ; sortie facturée
au budget fenêtre par fenêtre. Dependabot propose chaque semaine les mises à jour Cargo, npm et actions ; le cache
Rust de la CI n'est écrit que depuis `main`, jamais depuis une PR.

CI de PR sans secrets, jeton en lecture seule, actions épinglées à un commit revu,
pas de `pull_request_target` exécutant du code d'une contribution non fiable.
Les runners de signature sont séparés ; artefacts et mises à jour sont vérifiés avant
distribution. Ne pas utiliser l'agent pour afficher ou récupérer des clés de signature.

Pour un incident : conserver un reproducer autorisé hors canaux publics si sensible,
désactiver la capacité touchée si nécessaire, corriger et publier un avis proportionné.
Créer une politique publique de signalement avec un contact réel avant la diffusion ;
ce kit n'invente pas d'adresse de sécurité.
