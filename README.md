# GameShift

GameShift est une application Linux pour observer son ordinateur et mesurer ses
performances. Elle rassemble les informations du processeur et des cartes
graphiques dans une interface, avec des tests de performance appelés **Aurelia**.

Cette première version publique, **0.5.0-alpha.2**, est expérimentale.

## À quoi sert l’application ?

- **Observer le matériel** : consulter les fréquences, températures et taux
  d’utilisation que le système permet de lire.
- **Voir l’activité graphique** : identifier les cartes et, lorsque le pilote
  le permet, les applications qui les utilisent.
- **Mesurer les performances** : lancer les tests Aurelia, conserver les résultats
  localement et comparer des mesures réalisées avec le même protocole.
- **Régler certaines cartes AMD Radeon** : utiliser les réglages exposés par le
  pilote, enregistrer des profils et les associer à des applications. Cette partie
  est facultative et dépend du matériel et du système.

Une information indisponible est signalée comme telle. GameShift ne prétend pas
que toutes les cartes offrent les mêmes capteurs ou les mêmes réglages.

## Télécharger et lancer

La version pour Linux **x86_64** est fournie sous forme d’**AppImage** : un fichier
exécutable qui regroupe l’application et une partie de ses bibliothèques.
Télécharger l’AppImage et son fichier `.sha256` depuis la distribution de cette
version, puis ouvrir un terminal dans le dossier de téléchargement :

```sh
sha256sum -c GameShift-0.5.0-alpha.2-x86_64.AppImage.sha256
chmod +x GameShift-0.5.0-alpha.2-x86_64.AppImage
./GameShift-0.5.0-alpha.2-x86_64.AppImage
```

Le premier contrôle vérifie que le fichier correspond à l’empreinte fournie.
Lancer l’application avec son compte habituel, sans `sudo`.

L’observation du matériel et les tests ne demandent pas d’installer le composant
privilégié Radeon. Celui-ci sert uniquement aux réglages AMD et s’active séparément
avec une demande d’autorisation du système.

Depuis le menu de l’icône de notification, **Quitter GameShift** ferme la fenêtre
et arrête aussi l’agent en arrière-plan. Les profils enregistrés sont conservés.

## Ce qu’il faut savoir avant de l’essayer

La version AppImage utilise une base **Debian 13**, avec **glibc 2.41 ou plus
récente**. Les pilotes graphiques doivent déjà être installés sur l’ordinateur.
Le test graphique Aurelia demande un environnement Vulkan fonctionnel.

Sous NixOS ou un système dérivé, un lanceur comme `appimage-run` ou une compilation
adaptée peut être nécessaire. La compatibilité NixOS n’est pas encore validée.
L’installation du composant de réglage Radeon est désactivée sur NixOS.

Cette alpha a été construite et vérifiée logiciellement dans une machine virtuelle
Debian. Les essais matériels prévus sur Fedora 44, avec un Ryzen 7 7800X3D et une
Radeon RX 7800 XT, restent à effectuer. Le fonctionnement sur les différentes
cartes AMD, Intel et NVIDIA n’est donc pas garanti. La mesure des processus avec
le pilote NVIDIA propriétaire n’est pas implémentée.

Les résultats restent sur l’ordinateur. Aucun compte ni envoi automatique de
données n’est nécessaire. Un diagnostic peut être relu puis exporté manuellement.

Les détails et limites se trouvent dans le [guide de l’alpha](docs/ALPHA.md).

## Un projet fourni tel quel

GitHub sert uniquement à distribuer cette version et ses sources.
**Aucun support, aucune maintenance ni ajout de fonctionnalités n’est prévu.**
Les demandes d’aide, de correction ou d’évolution et les contributions externes
ne sont pas prises en charge.

Chacun est libre de récupérer les sources, de les étudier, de les modifier et de
redistribuer sa propre version, dans le respect de la licence et des mentions des
auteurs tiers. GameShift est fourni **sans garantie**, dans les limites de sa licence.

## Sources et licence

GameShift est actuellement distribué sous **GPL-3.0-or-later** ; le texte complet
se trouve dans [LICENSE](LICENSE). Les composants tiers gardent leurs propres
licences : voir [les logiciels utilisés et leurs licences](docs/THIRD_PARTY.md).

Pour compiler, il faut notamment Rust ≥ 1.92, GTK4 ≥ 4.10, libadwaita ≥ 1.4,
GLib/GIO, un compilateur C, pkg-config, Vulkan et `glslc`.
Les instructions détaillées sont dans le [guide de compilation](docs/PUBLIC_BUILD.md).

```sh
cargo build --release --locked --workspace --jobs 2
./target/release/gameshift-gui
```
