# GameShift

**Surveillance matérielle, benchmarks et profils GPU sous Linux.**

GameShift rassemble le suivi du CPU et des GPU dans une interface GTK4.
Son moteur de benchmark, **Aurelia**, permet de mesurer les performances et de
comparer les résultats. Des profils permettent également de gérer les réglages
pris en charge par les cartes AMD Radeon.

Version actuelle : **0.5.0-alpha.2**, expérimentale.

## Fonctionnalités

- **Monitoring CPU/GPU** : fréquences, températures, utilisation et mémoire,
  selon les capteurs accessibles.
- **Processus GPU** : affichage des applications utilisant la carte et suivi
  des compteurs exposés par le pilote.
- **Aurelia** : benchmarks CPU, système et scène Vulkan, avec historique et
  comparaison des résultats à protocole identique.
- **Profils Radeon** : enregistrement des réglages, association à des applications
  et sélection depuis l’icône de notification.
- **Quitter GameShift** : fermeture de la fenêtre et arrêt de l’agent depuis
  l’icône de notification, avec conservation des profils enregistrés.

Les mesures restent locales. Aucun compte ni envoi automatique de données.

## Téléchargement et lancement

Télécharger l’[AppImage x86_64 et son SHA-256](https://github.com/getsugah/gameshift/releases/tag/v0.5.0-alpha.2), puis
exécuter ces commandes dans le dossier de téléchargement :

```sh
sha256sum -c GameShift-0.5.0-alpha.2-x86_64.AppImage.sha256
chmod +x GameShift-0.5.0-alpha.2-x86_64.AppImage
./GameShift-0.5.0-alpha.2-x86_64.AppImage
```

L’AppImage nécessite **glibc ≥ 2.41**, des pilotes graphiques fonctionnels et
**Vulkan** pour le benchmark GPU. L’application se lance avec un compte utilisateur
habituel, sans `sudo`.

Le monitoring et les benchmarks fonctionnent sans le composant de contrôle Radeon.
Les réglages AMD nécessitent son activation depuis l’application. Si une version
précédente est déjà installée, **Mettre à jour les contrôles GPU** permet d’obtenir
l’agent avec le nouveau menu Quitter.

## État de l’alpha

Les capteurs et réglages disponibles dépendent du matériel et du pilote. La prise
en charge des différentes cartes reste à valider sur matériel réel. Le suivi des
processus avec le pilote propriétaire NVIDIA n’est pas implémenté.

Les détails techniques figurent dans le [guide de l’alpha](docs/ALPHA.md).

## Sources et licence

GameShift est un projet personnel distribué tel quel, **sans garantie**.
**Aucun support, aucune maintenance ni ajout de fonctionnalités n’est prévu.**
Les demandes d’aide, de correction ou d’évolution et les contributions externes
ne sont pas prises en charge.

Les sources sont disponibles sous **GPL-3.0-or-later**. Elles peuvent être étudiées,
modifiées et redistribuées dans le respect de [LICENSE](LICENSE) et des mentions
des auteurs. Les composants tiers conservent [leurs propres licences](docs/THIRD_PARTY.md).

Le projet utilise Rust, GTK4/libadwaita et Vulkan. Les instructions de compilation
se trouvent dans le [guide de compilation](docs/PUBLIC_BUILD.md).
