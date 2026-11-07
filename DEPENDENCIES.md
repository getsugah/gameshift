# Dépendances

Compilation : Rust/Cargo ≥ 1.92, compilateur C, pkg-config, GTK4 ≥ 4.10,
libadwaita ≥ 1.4, GLib/GIO compatibles gtk-rs 0.22, chargeur et en-têtes Vulkan,
glslc. Cargo.lock verrouille les crates. Réutiliser le cache avec --offline lorsqu’il
est déjà disponible ; aucune installation n’est déclenchée par GameShift.

Exécution : pilotes et API graphiques de l’hôte, session GTK pour l’interface.
La base AppImage demande glibc ≥ 2.41. ROCm, AMD-SMI et LACT ne sont pas requis.
Les couches de validation Vulkan et vainfo sont facultatifs ; leurs absences sont
signalées. Le scan des processus utilise procfs et DRM fdinfo sans bibliothèque ajoutée.

Contrôles Radeon facultatifs : Polkit, systemd et GLib/GIO hôte pour l’agent léger.
Un environnement SNI/AppIndicator permet l’icône ; aucune extension n’est installée.
Le helper est inutile pour la surveillance et les benchmarks. L’installation
privilégiée impérative n’est pas proposée sur NixOS dans cette alpha.

Les tests standards ne demandent pas de GPU physique. Les tests Vulkan matériels
et le test D-Bus isolé sont opt-in. Voir docs/PUBLIC_BUILD.md et docs/ALPHA.md.
