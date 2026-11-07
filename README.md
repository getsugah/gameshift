# GameShift

**Un cockpit hardware pour Linux.**

Garde un œil sur ton CPU et tes GPU, lance un bench, compare tes runs et ajuste
les réglages disponibles sur ta Radeon. GameShift rassemble tout ça dans une
interface GTK4, avec **Aurelia** pour les tests de performance.

Version actuelle : **0.5.0-alpha.2**. Ça tourne, ça mesure, mais ça reste une alpha.

## Au menu

- **Monitoring CPU/GPU** : fréquences, températures, charge et mémoire, selon
  les capteurs accessibles.
- **Processus GPU** : voir quelles applis utilisent la carte et suivre les
  compteurs que le pilote expose.
- **Aurelia** : benches CPU, système et scène Vulkan, avec historique et
  comparaison des résultats à protocole identique.
- **Profils Radeon** : enregistrer ses réglages, les associer à des applis et
  basculer depuis l’icône de notification.
- **Quitter GameShift** depuis cette icône ferme la fenêtre et arrête aussi
  l’agent en arrière-plan. Tes profils restent enregistrés.

Les mesures restent en local. Pas de compte à créer, pas d’envoi automatique.

## Lancer la bête

Récupère l’[AppImage x86_64 et son SHA-256](releases/0.5.0-alpha.2/), puis dans
le dossier de téléchargement :

```sh
sha256sum -c GameShift-0.5.0-alpha.2-x86_64.AppImage.sha256
chmod +x GameShift-0.5.0-alpha.2-x86_64.AppImage
./GameShift-0.5.0-alpha.2-x86_64.AppImage
```

Il faut **glibc ≥ 2.41**, tes pilotes graphiques et **Vulkan** pour le bench GPU.
Lance l’appli avec ton compte habituel, sans `sudo`.

Le monitoring et les benches fonctionnent sans le composant de contrôle Radeon.
Pour toucher aux réglages AMD, active ce composant depuis l’appli. Si tu l’avais
déjà installé, utilise **Mettre à jour les contrôles GPU** pour récupérer l’agent
avec le nouveau menu Quitter.

## Encore en rodage

Les capteurs et réglages disponibles dépendent du matériel et du pilote. La prise
en charge des différentes cartes reste à valider sur matériel réel ; le suivi des
processus avec le pilote propriétaire NVIDIA n’est pas implémenté.

Les détails techniques sont dans le [guide de l’alpha](docs/ALPHA.md).

## À toi de bidouiller

Projet perso, distribué tel quel : **aucun support, aucune maintenance ni ajout
prévu**. Les demandes d’aide, de correction ou d’évolution et les contributions
externes ne sont pas prises en charge. Le logiciel est fourni **sans garantie**.

Envie de reprendre le code, de le modifier ou de faire ton propre fork ? Les
sources sont disponibles sous **GPL-3.0-or-later** : garde les mentions des auteurs
et respecte les conditions de [LICENSE](LICENSE). Les composants tiers gardent
[leurs propres licences](docs/THIRD_PARTY.md).

Sous le capot : Rust, GTK4/libadwaita et Vulkan.
Pour compiler toi-même, direction le [guide de build](docs/PUBLIC_BUILD.md).
