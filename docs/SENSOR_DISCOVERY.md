# Découverte des capteurs et lectures de monitoring

## Diagnostic et périmètre

La redécouverte à chaque cycle était confirmée dans le code :

- `environment::Snapshot::read` parcourait hwmon et lisait les noms/labels CPU.
- `overview::collect` appelait les découvertes DRM/PCI, `DeviceSensors::discover_all`
  et `home_sensors::discover` à chaque relevé.
- `processes::Tracker` redécouvrait les identités PCI à chaque scan fdinfo.
- La vue GPU, la vue Processus GPU et le monitoring des benchmarks redécouvraient aussi les
  capteurs génériques. La lecture de courbe de l’accueil appelait `Sysfs::open`
  à chaque cycle, relisant l’identité PCI.

La modification conserve les collecteurs, conversions, cadences et conditions de
suspension existants. Elle ne modifie aucun widget ni aucune règle de profil.
Les fréquences cpufreq continuent d’être lues directement aux chemins CPU connus ;
la filière de puissance CPU RAPL, son intervalle et son helper autorisé sont
inchangés. Les opérations de contrôle gardent leurs découvertes et vérifications
d’identité fraîches. Aucun descripteur sysfs n’est gardé ouvert.

## Cache

`gameshift-core::discovery::Cache` conserve des chemins et métadonnées. Le
collecteur CPU et le collecteur de températures sélectionnables l’utilisent.
`gameshift-telemetry::inventory` partage un inventaire CPU-indépendant entre les
workers GPU et fdinfo : identités PCI, capteurs AMD existants, chemins génériques
et objet Sysfs validé pour la seule lecture de courbe. Les racines injectables
restent disponibles pour les tests.

La découverte est paresseuse au premier relevé de démarrage. Ensuite, chaque
relevé rouvre les fichiers de mesure et lit leurs valeurs actuelles ; aucun
échantillon numérique ancien ne remplace une lecture échouée. Les identités,
labels, unités et facteurs de conversion restent mémorisés.

Les métadonnées des répertoires et des liens de pilote surveillés sont vérifiées
au plus une fois par seconde de monitoring actif, sans parcours ni ouverture de
labels. Une topologie modifiée déclenche une redécouverte, avec au moins cinq
secondes entre deux découvertes. Trois relevés en erreur consécutifs déclenchent
une nouvelle tentative, espacée progressivement de 5, 10, 20, 40 puis 60 secondes
si l’erreur persiste. Une lecture saine rétablit le délai initial. Une vérification
complète au premier relevé après 60 secondes sans découverte couvre les pilotes sysfs qui ne mettent
pas à jour les dates des répertoires. Les ajouts silencieux peuvent donc prendre
jusqu’à 60 secondes à apparaître ; cela ne ralentit pas les mesures existantes.

Les chemins disparus donnent immédiatement une mesure indisponible, puis sont
remplacés lors de la redécouverte. Les GPU restent séparés par adresse PCI ; les
identifiants des températures sélectionnées restent indépendants de hwmonN.
Aucune dépendance externe ajoutée : seule la dépendance interne existante
`gameshift-core` est déclarée directement dans `gameshift-telemetry` pour partager
la politique de cache.

## Vérification locale sans interface

```sh
# Utiliser un environnement Rust/GTK/Vulkan déjà configuré.
cargo test -p gameshift-core -p gameshift-telemetry -p gameshift-control -p gameshift-profiles --offline
cargo test -p gameshift-cpu --offline environment::tests
cargo test -p gameshift-cpu --offline power::tests
cargo test -p gameshift-gui --offline --bin gameshift-gui
# IPC réel dans une session D-Bus isolée, sans GTK ni écriture matérielle :
dbus-run-session -- cargo test -p gameshift-profiles --offline --test agent -- --ignored
```

Les tests sysfs simulés couvrent : valeurs fraîches et unités identiques,
plusieurs GPU AMD et un autre pilote, invalidité/absence des valeurs, disparition,
ajout, changement de pilote, renumérotation hwmon, stabilité des sélections et
absence de relecture des labels entre deux mesures. Les tests du cache injectent
le temps et vérifient les délais sans attente réelle.

Comparaison contrôlée sur six relevés espacés logiquement de 500 ms, avec un CPU,
deux GPU AMD et un troisième GPU utilisant le collecteur générique :

| Opération | Redécouverte forcée à chaque relevé | Cache |
| --- | ---: | ---: |
| Tentatives d’ouverture des fichiers statiques | 282 | 47 |
| Ouvertures de répertoires sysfs | 126 | 21 |
| Tentatives d’ouverture des fichiers de mesure | 138 | 118 |

Comptage local des appels libc `open`/`open64`/`opendir`, filtrés sur la fixture.
Les tentatives incluent les labels optionnels absents. Le cache n’ouvre les
fichiers statiques et les répertoires qu’à la découverte initiale de ces six
cycles. La baisse des lectures de mesure provient de la suppression des lectures
initiales redondantes des températures : chaque cycle lit toujours les valeurs.
Cette comparaison isole le coût de découverte ; elle ne constitue pas une mesure
de charge CPU de l’interface réelle ni un strace du binaire historique.

Sur un système équipé de strace, le même probe se reproduit ainsi (la compilation
est préchauffée pour les deux commandes ; seuls les chemins de fixture comptent) :

```sh
cargo test -p gameshift-gui --offline --bin gameshift-gui --no-run
strace -f -qq -e trace=openat,openat2,getdents64 -o /tmp/discovery-before.trace \
  env GAMESHIFT_DISCOVERY_BASELINE=1 cargo test -p gameshift-gui --offline --bin gameshift-gui \
  sensor_discovery_tests::discovery_io_probe -- --exact --ignored --nocapture --test-threads=1
strace -f -qq -e trace=openat,openat2,getdents64 -o /tmp/discovery-after.trace \
  cargo test -p gameshift-gui --offline --bin gameshift-gui \
  sensor_discovery_tests::discovery_io_probe -- --exact --ignored --nocapture --test-threads=1
```

Le mode `GAMESHIFT_DISCOVERY_BASELINE` n’existe que dans le test ignoré ; il ne
change aucun comportement de l’application distribuée.

## Validation Fedora sur le matériel réel

La VM Debian de développement n’expose ni sondes hwmon ni Radeon. La validation
matérielle et la charge CPU visible/minimisée restent donc à effectuer sur Fedora.
Ne pas confondre un binaire debug avec une AppImage ou un binaire release.
Comparer deux binaires construits avec le même profil, le même réglage de cadence,
le même GPU sélectionné et la même charge des autres applications.

1. Lancer chaque version sous strace, laisser la première découverte se terminer,
   puis relever l’accueil pendant au moins dix secondes, et pendant plus d’une
   minute pour observer l’audit borné. Changer les vues manuellement.

   ```sh
   strace -f -qq -tt -yy -e trace=openat,openat2,getdents64,readlink,readlinkat,newfstatat,statx \
     -o /tmp/gameshift-monitoring.trace /chemin/vers/gameshift-gui
   ```

   Les `*_input`, `*_average`, cpufreq, occupations, VRAM et compteurs de courbe
   doivent rester lus à la cadence configurée. Les noms/labels et identités ne
   doivent plus être réouverts à chaque cycle. `statx`/`newfstatat` sur les chemins
   surveillés sont attendus ; ils ne sont pas des relectures de labels. L’agent
   de profils peut toujours lire une identité avant une opération de contrôle.

2. Vérifier températures, horloges, puissance, occupation, VRAM, ventilateur,
   moteurs et processus pour chaque GPU. Contrôler que les valeurs suivent la
   charge et restent cohérentes avec les fichiers sources affichés par l’app.
   Vérifier les profils et contrôles habituels, sans modifier la configuration
   pour les besoins de la mesure de performance.

3. Déterminer le PID exact du processus GUI puis mesurer séparément, sans strace :

   ```sh
   python3 scripts/measure-monitoring-cpu.py PID --seconds 30 --state visible
   # Minimiser manuellement, attendre cinq secondes, puis :
   python3 scripts/measure-monitoring-cpu.py PID --seconds 30 --state minimized
   ```

   Refaire avec chaque version, idéalement trois fois. La sortie JSON indique
   le CPU en pourcentage d’un cœur et la durée réelle. `--state` est une annotation :
   le script ne déplace, ne minimise et ne clique aucune fenêtre. Il détecte un
   processus disparu ou un PID réutilisé.


Binaires de développement conservés pour comparaison locale :

- Avant optimisation : `target/debian13-migrated/monitoring-comparison/before-gzkmk6ip/gameshift-gui`.
- Après optimisation : `target/debian13-migrated/debug/gameshift-gui`.

Ils sont tous deux des binaires debug. Aucun empaquetage AppImage n’est effectué
par cette validation.
