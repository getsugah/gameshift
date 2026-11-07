# GameShift 0.5.0-alpha.2 — guide de validation de l’alpha

Cette alpha vise les retours de configurations variées. La compilation et les
tests logiciels sont effectués dans une VM Debian 13 sans Radeon accessible.
**Les écritures Radeon et la compatibilité matérielle AMD, NVIDIA et Intel ne sont pas déclarées validées.**
Machine physique prévue : Fedora 44, Ryzen 7 7800X3D, RX 7800 XT, 32 Go de RAM.

## Périmètre

- Aurelia : accueil par domaine, matériel détecté, dernier résultat système,
  parcours système avec Lancer / Suivi / Résultats / Historique et comparaison,
  CPU avancé et scène Vulkan conservée. Paramètres techniques dépliables,
  télémétrie ponctuelle, temps écoulé, progression des étapes réelles, annulation.
- Résultats : mesures physiques par cartes, variation entre répétitions, durée,
  protocole et détails dépliables. Aucun nouveau score global. `.gsbench` et
  comparaisons compatibles conservés ; les imports et références restent locaux.
- GPU → Surveillance → Processus GPU : sélection PCI multi-constructeurs,
  tableau triable, cadence 1/2/5 s, VRAM/GTT résidentes attribuées, compteurs de
  moteurs, activité calculée par différence, exécutable/PID et 60 derniers relevés.
- Diagnostic partageable : Paramètres → Exporter un rapport de diagnostic.
  Aperçu obligatoire avant export ; résultats système joints sur sélection.
  Aucun envoi, hostname, numéro de série, environnement complet ou liste des
  processus. Le contenu exporté est exactement celui affiché dans l’aperçu.
- Profils, helper AMDGPU et icône SNI conservés. Le contrôleur de cette alpha
  utilise le protocole 3 pour geler les changements automatiques pendant Aurelia.
  Mettre à jour un ancien agent actif, ou l’arrêter, avant une mesure.

Surveillance et benchmarks ne nécessitent **ni activation du helper, ni root**.
Les couches de validation Vulkan deviennent facultatives dans le lanceur ; leur
état réel est enregistré. Pour comparer, conserver les mêmes paramètres et le
même état de validation. La scène et les charges du moteur restent inchangées.

## Ce que les mesures de processus signifient

Source : `/proc/PID/fdinfo/FD`, spécification DRM du noyau. Le couple périphérique
PCI/client DRM déduplique les descripteurs d’un processus. Un client partagé
entre processus est signalé : **ne pas additionner les lignes**, ni considérer
la somme des mémoires comme une consommation physique globale sans doublons.
La mémoire résidente est distincte de la mémoire demandée/allouée. Les anciens
champs AMD `drm-memory-*` sont des alias, jamais ajoutés aux champs résidents.

Chaque moteur conserve son compteur cumulé en ns. Le taux est la différence de
deux compteurs du même client divisée par le temps monotone et la capacité exposée
(1 si absente selon l’ABI). Les régressions temporaires gardent le maximum précédent.
Premier relevé, pause, changement de PID/client ou compteur invalide : indisponible.
Le taux de la colonne principale est celui du **moteur le plus occupé**, pas un
pourcentage physique global du GPU. Les durées des moteurs peuvent se chevaucher.

Les processus disparus et les permissions insuffisantes sont tolérés et comptés.
Les clients sans identité attribuable ne sont pas mélangés à un GPU arbitraire.
La collecte est bornée (300 ms de budget vérifié entre lectures, 32 768 descripteurs)
et suspendue hors de la vue et pendant les benchmarks. Une lecture noyau bloquée
ne peut pas être interrompue par ce budget, mais reste hors du thread GTK.

Limites explicites : PCI seulement ; certains compteurs Xe exprimés uniquement
en cycles ne sont pas décodés ; pas de backend NVML/NVIDIA propriétaire dans cette
alpha. Des clients DRM peuvent exister sans compteurs exploitables. Pas de
puissance exacte par processus, nombre de CU mobilisées ou codec réellement utilisé.
Les capteurs globaux standard hwmon sont affichés selon ce que le pilote expose.

Le bouton vidéo utilise **vainfo, seulement s’il existe déjà**, pour interroger
VA-API explicitement. Rien n’est installé. Ce sont les profils exposés par VA-API,
pas une liste exhaustive du silicium ni une observation du codec actif. L’absence
de vainfo ou de support VA-API ne désactive aucune autre fonction.

## Profils pendant une mesure

Un garde de benchmark utilisateur et une barrière avec l’agent bloquent les
basculements automatiques avant le début des charges. La fermeture du benchmark
libère le garde ; un marqueur orphelin ne bloque pas après disparition/réutilisation
du PID. Sans agent, les tests fonctionnent normalement. Pendant le garde, le
contrôleur diffère aussi ses réapplications automatiques ; les protections
thermiques et les changements manuels explicites restent prioritaires.

Le profil GPU observé est conservé avant/après les évaluations système et le rendu
Vulkan, avec les réglages disponibles. Les rapports CPU Architecture restent dans
leur format existant. Une association ne démontre pas la cause d’un écart.

## NixOS et distributions basées sur Nix

**Compatibilité NixOS non testée.** L’AppImage contient une base Debian 13 et
demande glibc ≥ 2.41. NixOS n’expose pas nécessairement le chargeur ELF et les
chemins FHS attendus. L’extraction seule ne résout pas ces différences.

Solution à essayer depuis un environnement Nix adapté, avec son accord local :

```sh
nix shell nixpkgs#appimage-run --command appimage-run ./GameShift-0.5.0-alpha.2-x86_64.AppImage
```

L’outil et son environnement FHS doivent fournir une glibc assez récente, ainsi
que l’accès aux pilotes Vulkan/GL et à `/dev/dri`. Ne pas mettre à jour un canal
Nix automatiquement pour contourner un échec. Une compilation native des sources
est une autre voie. Documenter précisément le lanceur et les bibliothèques réellement
chargées : une mesure de bibliothèques dans un FHS n’est pas une mesure du système
Nix nu. Les versions et empreintes restent indispensables aux comparaisons.

L’installation privilégiée impérative est **désactivée sur NixOS** : `/usr` et
les unités doivent être gérés déclarativement. Aucun module Nix de contrôle GPU
n’est fourni dans cette alpha ; ne pas exécuter l’AppImage en root. L’agent GTK,
les benchmarks et l’observation restent utilisables sans helper si le lancement
de l’AppImage fonctionne. Aucun ROCm obligatoire ni extension GNOME installée.

Références : [wiki officiel AppImage NixOS](https://wiki.nixos.org/wiki/Appimage/en),
[ABI DRM fdinfo](https://docs.kernel.org/gpu/drm-usage-stats.html).
Étude ciblée d’[amdgpu_top, commit 70fd1e1](https://github.com/Umio-Yasuno/amdgpu_top/tree/70fd1e13bd7453180dd343265d12f4d09cb412b0),
notamment le lecteur `amdgpu_fdinfo.rs`. Aucun code de ce projet intégré,
aucune dépendance obligatoire ajoutée.

## Retour de test utile

1. Vérifier l’empreinte de l’AppImage et garder la version précédente.
2. Tester d’abord sans helper : détection de toutes les cartes, choix du GPU,
   processus, valeurs absentes, tri, détails, cadence et disparition de processus.
3. Tester Aurelia avec le même protocole : CPU, système puis rendu Vulkan.
   Vérifier progression, annulation, sauvegarde, détails, comparaison et imports.
4. Vérifier qu’un lancement d’application pendant Aurelia ne change pas
   automatiquement le profil ; un changement manuel reste une action explicite.
5. Exporter le diagnostic après relecture. Joindre seulement les résultats voulus,
   préciser la session graphique, le lancement natif/FHS et les erreurs observées.
6. Sur Radeon/Fedora seulement, valider séparément activation Polkit, profils,
   ventilation, restauration, veille et réglages avancés décidés explicitement.
   Ne pas provoquer un reset matériel pour tester cette alpha.

Le surcoût de la télémétrie pendant les mesures reste à quantifier sur matériel.
La présentation GTK, l’icône sous les différents bureaux et la compatibilité
réelle AMD/NVIDIA/Intel doivent être validées par les testeurs.
