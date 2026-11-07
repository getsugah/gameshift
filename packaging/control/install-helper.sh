#!/bin/sh
# Optional extracted-kit fallback. GameShift now offers this directly in the GUI.
set -eu
if [ "$(id -u)" = 0 ]; then
    echo "Lancer avec le compte normal ; Polkit demandera l’autorisation." >&2
    exit 1
fi
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
if [ -x "$here/gameshift-control" ]; then
    binary=$here/gameshift-control
else
    binary=$here/../../bin/gameshift-control
fi
[ -x "$binary" ] || { echo "Composant d’installation absent." >&2; exit 1; }
if [ -f "$here/SHA256SUMS" ]; then
    (cd -- "$here" && sha256sum -c SHA256SUMS)
fi
unset LD_LIBRARY_PATH LD_PRELOAD LD_AUDIT
exec pkexec --disable-internal-agent "$binary" --install
