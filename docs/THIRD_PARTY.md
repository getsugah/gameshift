# Logiciels et licences tiers

Les composants conservent leurs licences et copyrights ; la licence de GameShift
ne remplace pas les leurs. L’AppImage contient leurs notices dans
`usr/share/licenses/`, dont l’inventaire Rust `cargo/inventory.json`.

| Composants | Licences principales |
| --- | --- |
| GTK4, GLib/GIO, libadwaita, Pango, GdkPixbuf | LGPL-2.1-or-later |
| Cairo | LGPL-2.1-or-later ou MPL-1.1, selon les fichiers |
| Crates Rust (serde, gtk-rs, ash, winit, etc.) | MIT, Apache-2.0, BSD, ISC, Zlib et Unicode, selon l’inventaire |
| LZO et JBIG-KIT embarqués indirectement | GPL-2.0-or-later (option GPLv3 disponible) |
| Nettle, GMP, GnuTLS et autres bibliothèques natives | GPL/LGPL et licences permissives selon les composants et fichiers |
| Icônes Adwaita | Licences graphiques multiples, voir la notice du thème |
| Runtime AppImage | Licence propre au runtime ; distinct du code de GameShift |

LACT, amdgpu-sysfs-rs et amdgpu_top sont des références d’étude documentées ;
aucun code intégré de ces projets n’est déclaré dans cette version.
Les détails par fichier dans les notices tierces font foi.
