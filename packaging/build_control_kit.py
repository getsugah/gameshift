#!/usr/bin/env python3
"""Assemble the manually installed Polkit helper from an existing release binary."""
import argparse
import hashlib
import shutil
import subprocess
import tarfile
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def main():
    version = tomllib.loads((ROOT / 'Cargo.toml').read_text())['workspace']['package']['version']
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=ROOT / 'target/debian13-migrated/release/gameshift-control')
    parser.add_argument('--output', type=Path, default=ROOT / f'dist/GameShift-control-helper-{version}')
    args = parser.parse_args()
    output = args.output.resolve()
    archive = output.with_name(output.name + '.tar.gz')
    if output.exists() or archive.exists():
        parser.error('Kit existant conservé ; choisir un autre --output.')
    license_source = next((p for p in [ROOT / 'LICENSE', ROOT / 'target/debian13/packaging-root/usr/share/common-licenses/GPL-3', Path('/usr/share/common-licenses/GPL-3')] if p.is_file()), None)
    if license_source is None:
        parser.error('Texte GPL-3 existant absent ; fournir les notices avant assemblage.')
    binary = args.binary.resolve(strict=True)
    actual = subprocess.check_output([str(binary), '--version'], text=True).strip()
    if actual != f'gameshift-control {version} protocol 1':
        parser.error('Binaire helper incompatible : construire la version courante.')
    agent = binary.with_name('gameshift-profiles')
    if subprocess.check_output([str(agent), '--version'], text=True).strip() != f'gameshift-profiles {version} protocol 1':
        parser.error('Agent de profils incompatible.')
    # Verify ABI before copying; this does not launch a privileged operation.
    subprocess.run(['python3', str(ROOT / 'scripts/check-glibc-baseline.py'), str(binary), str(agent)], check=True, stdout=subprocess.DEVNULL)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.mkdir()
    shutil.copy2(binary, output / 'gameshift-control')
    shutil.copy2(binary.with_name('gameshift-profiles'), output / 'gameshift-profiles')
    for name in ['install-helper.sh', 'io.github.gameshift.control.policy']:
        shutil.copy2(ROOT / 'packaging/control' / name, output / name)
    shutil.copy2(ROOT / 'docs/AMDGPU_CONTROLS.md', output / 'AMDGPU_CONTROLS.md')
    shutil.copy2(ROOT / 'docs/GPU_PROFILES.md', output / 'GPU_PROFILES.md')
    shutil.copy2(license_source, output / 'LICENSE')
    files = sorted(output.iterdir())
    (output / 'SHA256SUMS').write_text(''.join(f'{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.name}\n' for p in files))
    with tarfile.open(archive, 'x:gz') as bundle:
        bundle.add(output, arcname=output.name)
    archive.with_name(archive.name + '.sha256').write_text(f'{hashlib.sha256(archive.read_bytes()).hexdigest()}  {archive.name}\n')
    print(archive)


if __name__ == '__main__':
    main()
