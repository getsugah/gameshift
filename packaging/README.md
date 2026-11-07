# Distribution GameShift

Voir [la compilation publique](../docs/PUBLIC_BUILD.md) et [la recette alpha](../docs/ALPHA.md).
La base portable est Debian 13 / glibc ≥ 2.41. Un lancement normal n’installe aucun
composant et les benchmarks ne dépendent pas du helper Radeon.

Après une compilation sur Debian 13 :

```sh
python3 packaging/build_appimage.py --skip-build --sysroot / \
  --binary-dir target/release --appimagetool /chemin/appimagetool-x86_64.AppImage \
  --output dist/GameShift-alpha-x86_64.AppImage
```

Fournir un outil local, ou GAMESHIFT_APPIMAGETOOL ; aucun téléchargement automatique.
Un sysroot Debian exporté est accepté. Le compilateur et les caches déjà configurés
peuvent être réutilisés dans l’environnement de développement migré, sans Podman.
Les artefacts précédents sont conservés. L’assemblage contrôle les dépendances,
notices et symboles glibc, avec empreintes et chemins relatifs dans son manifeste.
Les pilotes graphiques et le chargeur Vulkan restent fournis par l’hôte.

Le paquet fournit cinq exécutables, dont un agent sans GTK et un contrôleur sans
GTK/GLib. L’activation facultative utilise Polkit et prépare les binaires hors du
montage FUSE ; aucune dépendance à un autre projet local. Le bootstrap impératif
n’est pas proposé sur NixOS. Le kit extrait reste une alternative facultative via
`build_control_kit.py`, après compilation du contrôleur et de l’agent.

`build_source_archive.py` prépare une archive publique sélectionnée, exige Git
propre, vérifie les motifs privés et ajoute un manifeste des sources. Les caches,
remotes, historiques Git, captures matérielles et exports personnels sont exclus.
Aucune publication ni aucun push ne sont effectués par ces recettes.
