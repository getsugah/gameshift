#!/usr/bin/env python3
"""Create a curated source tarball, without Git history, caches or machine reports."""
import argparse
import hashlib
import io
import json
from pathlib import Path
import re
import subprocess
import tarfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
DOCS = ['ALPHA.md', 'PUBLIC_BUILD.md', 'GPU_PROFILES.md', 'AMDGPU_CONTROLS.md', 'THIRD_PARTY.md']
PRIVATE = re.compile(rb'-----BEGIN (?:OPENSSH |RSA |EC |DSA )?PRIVATE KEY-----|(?:ghp_|github_pat_|glpat-|sk-proj-)[A-Za-z0-9_-]{20,}|/home/(?:builder|getsugah)(?:/|\b)|192\.168\.\d+\.\d+')


def main():
    version = tomllib.loads((ROOT / 'Cargo.toml').read_text())['workspace']['package']['version']
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=ROOT / f'dist/GameShift-{version}-sources.tar.gz')
    parser.add_argument('--snapshot', action='store_true', help='Export the current files without Git identity or history; hashes record the snapshot.')
    args = parser.parse_args()
    output = args.output.resolve()
    if output.exists():
        parser.error('Archive existante conservée ; choisir un autre --output.')
    files = {ROOT / name for name in ['Cargo.toml', 'Cargo.lock', 'README.md', 'LICENSE', 'DEPENDENCIES.md', 'CHANGELOG.md', '.gitignore']}
    for directory in ['crates', 'shaders']:
        files.update(p for p in (ROOT / directory).rglob('*') if p.is_file())
    files.update(ROOT / 'docs' / name for name in DOCS)
    files.update(p for p in (ROOT / 'docs/reference').rglob('*') if p.is_file())
    for name in ['build_appimage.py', 'build_source_archive.py', 'build_control_kit.py', 'AppRun', 'gameshift.desktop', 'gameshift.svg', 'README.md', 'public-pkg-config']:
        files.add(ROOT / 'packaging' / name)
    files.update(p for p in (ROOT / 'packaging/control').rglob('*') if p.is_file())
    for name in ['verify-v3.py', 'check-glibc-baseline.py']:
        files.add(ROOT / 'scripts' / name)
    payload = {}
    allowed = {'.rs', '.toml', '.css', '.vert', '.frag', '.comp', '.glsl', '.md', '.json', '.py', '.sh', '.svg', '.desktop', '.policy', '.service', '.socket'}
    for path in sorted(files):
        relative = path.relative_to(ROOT)
        if any(part in {'.git', 'target', 'dist', '__pycache__', 'exports'} for part in relative.parts):
            continue
        if path.is_symlink() or not path.resolve().is_relative_to(ROOT):
            raise RuntimeError(f'Lien ou source extérieure refusé : {relative}')
        if path.suffix not in allowed and path.name not in {'Cargo.lock', 'LICENSE', '.gitignore', 'AppRun', 'public-pkg-config'}:
            raise RuntimeError(f'Extension non revue : {relative}')
        data = path.read_bytes()
        if PRIVATE.search(data):
            raise RuntimeError(f'Donnée privée potentielle : {relative} ; archive non créée.')
        payload[str(relative)] = data
    provenance = {'source_kind': 'working-tree-snapshot'}
    if not args.snapshot:
        commit = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
        status = subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT, text=True)
        if status.strip():
            raise RuntimeError('Sécuriser les sources dans Git ou choisir explicitement --snapshot.')
        provenance = {'source_commit': commit}
    manifest = {'schema_version': 1, 'application_version': version, **provenance,
                'license': tomllib.loads((ROOT / 'Cargo.toml').read_text())['workspace']['package']['license'],
                'files': {name: hashlib.sha256(data).hexdigest() for name, data in payload.items()},
                'excluded': ['Git history/remotes', 'build caches', 'hardware captures', 'user exports', 'developer migration/SDK scripts', 'private documents']}
    payload['SOURCE-MANIFEST.json'] = (json.dumps(manifest, indent=2) + '\n').encode()
    output.parent.mkdir(parents=True, exist_ok=True)
    prefix = f'GameShift-{version}'
    with tarfile.open(output, 'x:gz') as archive:
        for name, data in payload.items():
            entry = tarfile.TarInfo(f'{prefix}/{name}')
            entry.size = len(data)
            entry.mode = 0o755 if name.endswith('.sh') or name in {'packaging/AppRun', 'packaging/public-pkg-config'} else 0o644
            entry.uid = entry.gid = entry.mtime = 0
            entry.uname = entry.gname = ''
            archive.addfile(entry, io.BytesIO(data))
    output.with_name(output.name + '.sha256').write_text(f'{hashlib.sha256(output.read_bytes()).hexdigest()}  {output.name}\n')
    print(json.dumps({'archive': str(output), 'files': len(payload), **provenance, 'private_scan': 'passed'}, indent=2))


if __name__ == '__main__':
    main()
