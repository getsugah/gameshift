# Compilation et distribution des sources

Prérequis : Linux x86_64, Rust ≥ 1.92 avec Cargo, compilateur C et pkg-config,
GTK4 ≥ 4.10, libadwaita ≥ 1.4, GLib/GIO compatibles avec gtk-rs 0.22,
chargeur/en-têtes Vulkan et `glslc`. Les bibliothèques hôte optionnelles des
évaluations système sont découvertes ; une absence est rapportée sans valeur fictive.
Les couches de validation Vulkan et `vainfo` sont facultatifs. Pas de ROCm requis.

```sh
cargo build --release --locked --workspace --jobs 2
cargo test --workspace --locked --jobs 2
cargo clippy --workspace --all-targets --locked -- -D warnings
python3 scripts/verify-v3.py
```

Un cache déjà disponible permet `--offline`. Aucune procédure n’installe de
dépendances automatiquement. Les tests matériels/Vulkan opt-in sont explicitement
ignorés par défaut. Le test d’agent s’exécute sans GTK ni écriture matérielle :

```sh
dbus-run-session -- cargo test -p gameshift-profiles --test agent -- --ignored
```

Lancer `target/release/gameshift-gui`. Les cinq exécutables doivent rester ensemble
pour les évaluations et le bootstrap des contrôles. Sans helper installé, les
lectures et benchmarks restent accessibles ; ne jamais lancer GTK avec sudo.

## Construire une AppImage

La base portable est **Debian 13 / glibc ≥ 2.41**, avec les dépendances installées
et un appimagetool local déjà disponible. Après compilation sur Debian 13 :

```sh
python3 packaging/build_appimage.py --skip-build --sysroot / \
  --binary-dir target/release --appimagetool /chemin/appimagetool-x86_64.AppImage \
  --output dist/GameShift-alpha-x86_64.AppImage
```

On peut fournir un sysroot Debian exporté plutôt que `/`. Les dépendances ELF
sont résolues dans ce sysroot, sans repli vers celles d’un autre hôte. Le paquet
inclut GTK/libadwaita et leurs notices, les contrôles de symboles glibc et les
empreintes. Il laisse glibc, les pilotes et Vulkan/GL/Wayland à l’environnement
d’exécution. Les bibliothèques graphiques de l’hôte restent nécessaires.
`--native-fedora` est une variante explicitement liée à la glibc Fedora utilisée,
elle n’est pas équivalente à la base portable. Les sorties existantes sont conservées.

Voir [les contraintes Nix et la recette](ALPHA.md) avant un test sur un système basé sur Nix.
Le helper impératif n’est pas installable sur NixOS via le bouton de cette alpha.

## Archive publique

```sh
python3 packaging/build_source_archive.py
```

La recette exige un état Git propre, inclut les sources, shaders, tests, fichiers
Cargo verrouillés, recettes et documents publics sélectionnés. Elle exclut
l’historique Git, les remotes, caches, captures matérielles, exports personnels,
images de travail et scripts de migration propres au développeur. Un contrôle
de motifs privés précède la création ; les propriétaires/timestamps du tar ne
révèlent pas le compte de construction. `SOURCE-MANIFEST.json` donne le commit
et les empreintes des fichiers. Cela décrit une provenance, pas une signature.

La licence du projet était déjà déclarée **GPL-3.0-or-later** dans Cargo ; son
texte est fourni dans `LICENSE`. Les dépendances conservent leurs licences.
Aucun dépôt distant n’est poussé et aucune publication n’est faite par ces recettes.

## Distribution sans historique

`python3 packaging/build_source_archive.py --snapshot` exporte explicitement les
fichiers actuels, même avec des changements locaux. Le manifeste donne leurs
empreintes sans identité Git ni historique de développement. L’archive existante
n’est jamais écrasée ; utiliser `--output` pour choisir une nouvelle destination.

Pour une construction destinée au public, ajouter avant Cargo :

```sh
export RUSTFLAGS="--remap-path-prefix=$PWD=/usr/src/gameshift --remap-path-prefix=$HOME=/usr/src/build-home"
export PKG_CONFIG="$PWD/packaging/public-pkg-config"
```

Le wrapper garde les répertoires de compilation dans le sysroot mais empêche X11
d’enregistrer son chemin local comme répertoire de recherche à l’exécution.
Vérifier le contenu final après assemblage, y compris les binaires et les notices.
