#!/usr/bin/env python3
"""Offline x86_64 AppImage packaging from a Debian sysroot or native build."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]

def run(args, **kwargs):
    return subprocess.run([str(a) for a in args], check=True, **kwargs)

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--appimagetool', type=Path, default=Path(os.environ.get('GAMESHIFT_APPIMAGETOOL') or shutil.which('appimagetool') or ROOT/'tools/appimagetool-x86_64.AppImage'))
    parser.add_argument('--skip-build', action='store_true')
    parser.add_argument('--native-fedora', action='store_true', help='Legacy Fedora-only build, explicitly requested')
    parser.add_argument('--sysroot', type=Path, help='Debian 13 library/resource root; never resolve libraries against Fedora')
    parser.add_argument('--binary-dir', type=Path)
    parser.add_argument('--appdir-only', action='store_true')
    parser.add_argument('--output', type=Path, help='New output AppImage; existing files are preserved')
    args = parser.parse_args()
    output_target=(args.output or ROOT/'dist/GameShift-x86_64.AppImage').resolve()
    if not args.appdir_only and output_target.exists():
        parser.error('AppImage existante conservée ; choisir un nouveau chemin --output : '+str(output_target))
    if args.native_fedora and args.sysroot:
        parser.error('--native-fedora et --sysroot sont incompatibles.')
    portable_default = not args.sysroot and not args.native_fedora
    migrated_environment = ROOT.parent/'migration-tools/gameshift-env.sh'
    migrated = migrated_environment.is_file() and 'ID=debian' in Path('/etc/os-release').read_text().splitlines()
    if portable_default:
        if migrated:
            if not args.skip_build:
                run(['bash','-c','set -e; source "$1"; exec cargo build --release --jobs 2 --workspace --offline --locked','gameshift-build',migrated_environment],cwd=ROOT)
            args.sysroot=ROOT/'target/debian13/packaging-root'
            args.binary_dir=args.binary_dir or ROOT/'target/debian13-migrated/release'
        elif 'ID=debian' in Path('/etc/os-release').read_text().splitlines():
            if not args.skip_build:
                run(['cargo','build','--release','--jobs','2','--workspace','--locked'],cwd=ROOT)
            args.sysroot=Path('/')
            args.binary_dir=args.binary_dir or ROOT/'target/release'
        else:
            parser.error('Construire sur Debian 13, fournir --sysroot/--binary-dir ou choisir explicitement --native-fedora.')
        args.skip_build=True
    sysroot = args.sysroot.resolve() if args.sysroot else None
    if sysroot and not sysroot.is_dir():
        parser.error('SDK Debian absent : provisionner scripts/debian13-sdk.sh ; aucun remplacement automatique par Fedora.')
    binary_dir = args.binary_dir or ROOT / ('target/debian13/release' if sysroot else 'target/release')
    if sysroot and not args.skip_build:
        parser.error('Construire dans le SDK Debian puis utiliser --skip-build avec --sysroot.')
    tool = args.appimagetool.resolve()
    if not tool.is_file():
        parser.error('Fournir --appimagetool /chemin/appimagetool-x86_64.AppImage ; aucun téléchargement automatique.')
    if os.uname().machine != 'x86_64':
        parser.error('Cette recette est actuellement x86_64 uniquement.')
    if not args.skip_build:
        run(['cargo', 'build', '--release', '-p', 'gameshift-gui', '-p', 'gameshift-control', '-p', 'gameshift-profiles', '--offline', '--locked'], cwd=ROOT)
    work_root = ROOT / 'target/appimage-build'
    work_root.mkdir(parents=True, exist_ok=True)
    # Preserve previous assembly directories and all existing artifacts.
    work = Path(tempfile.mkdtemp(prefix='build-',dir=work_root))
    appdir = work / 'GameShift.AppDir'
    bindir, libdir, share = appdir/'usr/bin', appdir/'usr/lib', appdir/'usr/share'
    for directory in [bindir, libdir, share]:
        directory.mkdir(parents=True, exist_ok=True)
    binary = binary_dir/'gameshift-gui'
    binaries = [binary, binary_dir/'gameshift-control', binary_dir/'gameshift-profiles']
    if sysroot:
        binaries += [binary_dir/'aurelia-host-bench', binary_dir/'gameshift-system-bench']
    for executable in binaries:
        shutil.copy2(executable, bindir/executable.name)
    control_share=share/'gameshift-control'
    shutil.copytree(ROOT/'packaging/control',control_share)
    # Driver dependencies and C/C++ runtime come from the same host as its Mesa.
    excluded = re.compile(r'^(ld-linux|libc\.so|libm\.so|libpthread\.so|libdl\.so|librt\.so|libresolv\.so|libutil\.so|libnss_|libstdc\+\+|libgcc_s|libvulkan|libEGL|libGL|libOpenGL|libdrm|libgbm|libva|libvdpau|libcuda|libnvidia|libwayland|libxkbcommon)')
    bundled = []
    if sysroot:
        search = [sysroot/p for p in ['usr/lib/x86_64-linux-gnu','lib/x86_64-linux-gnu','usr/lib','lib']]
        pending = list(binaries)
        seen = set()
        while pending:
            executable = pending.pop()
            dynamic = run(['readelf','-d',executable], text=True, capture_output=True).stdout
            for soname in re.findall(r'\(NEEDED\).*?\[([^]]+)\]', dynamic):
                if soname in seen or excluded.match(soname):
                    continue
                seen.add(soname)
                source = next((directory/soname for directory in search if (directory/soname).is_file()), None)
                if source is None:
                    raise RuntimeError('Dépendance absente du sysroot Debian : '+soname)
                source = source.resolve()
                if not source.is_relative_to(sysroot):
                    raise RuntimeError('Résolution hors du sysroot interdite : '+str(source))
                shutil.copy2(source, libdir/soname)
                bundled.append(source)
                pending.append(source)
    else:
        # ldd only on our own trusted build; never use host ldd for Debian resolution.
        output = run(['ldd', binary], text=True, capture_output=True).stdout
        for line in output.splitlines():
            if 'not found' in line:
                raise RuntimeError('Dépendance native absente : '+line)
            match = re.match(r'\s*(\S+)\s+=>\s+(/\S+)', line)
            if match and not excluded.match(match[1]):
                source = Path(match[2]).resolve()
                shutil.copy2(source, libdir/match[1])
                bundled.append(source)
    resource_root = sysroot if sysroot else Path('/')
    for directory in ['icons/Adwaita', 'glib-2.0/schemas']:
        source = resource_root/'usr/share'/directory
        if not source.is_dir():
            raise RuntimeError('Ressource hôte absente : '+str(source))
        shutil.copytree(source, share/directory)
    shutil.copy2(ROOT/'packaging/AppRun', appdir/'AppRun')
    (appdir/'AppRun').chmod(0o755)
    shutil.copy2(ROOT/'packaging/gameshift.desktop', appdir/'gameshift.desktop')
    shutil.copy2(ROOT/'packaging/gameshift.svg', appdir/'gameshift.svg')
    shutil.copy2(ROOT/'packaging/gameshift.svg', appdir/'.DirIcon')
    applications=share/'applications'; applications.mkdir()
    shutil.copy2(appdir/'gameshift.desktop', applications/'gameshift.desktop')
    if sysroot:
        # Keep installed Debian copyright notices and referenced common licenses.
        for source in (sysroot/'usr/share/doc').glob('*/copyright'):
            resolved = source.resolve()
            if resolved.is_file() and resolved.is_relative_to(sysroot):
                destination = share/'licenses'/source.parent.name/'copyright'
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(resolved, destination)
        common = sysroot/'usr/share/common-licenses'
        shutil.copytree(common, share/'common-licenses', symlinks=False)
    else:
        owners = run(['rpm','-qf','--qf','%{NAME}\n',*bundled], text=True, capture_output=True).stdout.splitlines()
        for owner in set(owners):
            source=Path('/usr/share/licenses')/owner
            if source.is_dir():
                shutil.copytree(source, share/'licenses'/owner, dirs_exist_ok=True)
    docs=share/'doc/gameshift'; docs.mkdir(parents=True)
    for name in ['README.md','DEPENDENCIES.md','CHANGELOG.md']:
        shutil.copy2(ROOT/name, docs/name)
    shutil.copy2(ROOT/'packaging/README.md', docs/'APPIMAGE.md')
    shutil.copy2(ROOT/'LICENSE', docs/'LICENSE')
    # Preserve upstream notices for statically linked Rust dependencies as well.
    cargo_home = Path(os.environ.get('CARGO_HOME', Path.home()/'.cargo'))
    registry_roots = list((cargo_home/'registry/src').glob('*'))
    rust_notices = share/'licenses/cargo'
    inventory = []
    metadata = json.loads(run(['cargo', 'metadata', '--offline', '--locked',
                               '--filter-platform', 'x86_64-unknown-linux-gnu',
                               '--format-version', '1'], cwd=ROOT, text=True,
                              capture_output=True).stdout)
    active_ids = {node['id'] for node in metadata['resolve']['nodes']}
    active = {(package['name'], package['version']) for package in metadata['packages']
              if package['id'] in active_ids and package['source']}
    for package in tomllib.loads((ROOT/'Cargo.lock').read_text())['package']:
        if (package['name'], package['version']) not in active:
            continue
        if 'source' not in package:
            continue
        name = f"{package['name']}-{package['version']}"
        source = next((root/name for root in registry_roots if (root/name/'Cargo.toml').is_file()), None)
        if source is None:
            raise RuntimeError('Notices Cargo absentes du cache : '+name)
        metadata = tomllib.loads((source/'Cargo.toml').read_text())['package']
        notices = []
        for path in source.rglob('*'):
            if not path.is_file() or path.is_symlink():
                continue
            if path.name.upper().startswith(('LICENSE', 'LICENCE', 'COPYING', 'NOTICE', 'COPYRIGHT', 'AUTHORS')) or str(path.relative_to(source)) == metadata.get('license-file'):
                relative = path.relative_to(source)
                destination = rust_notices/name/relative
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(path, destination)
                notices.append(str(relative))
        if not notices:
            raise RuntimeError('Texte de licence Cargo absent : '+name)
        inventory.append({'name': package['name'], 'version': package['version'],
                          'license': metadata.get('license'), 'notices': sorted(notices)})
    (rust_notices/'inventory.json').write_text(json.dumps(inventory, indent=2)+'\n')
    for relative in ['docs/ALPHA.md','docs/PUBLIC_BUILD.md','docs/GPU_PROFILES.md','docs/AMDGPU_CONTROLS.md','docs/THIRD_PARTY.md']:
        source=ROOT/relative
        if source.is_file():
            destination=docs/relative
            destination.parent.mkdir(parents=True,exist_ok=True)
            shutil.copy2(source,destination)
    sdk = None
    if sysroot and (sysroot/'sdk-provenance.json').exists():
        sdk = json.loads((sysroot/'sdk-provenance.json').read_text())
    elif sysroot and (ROOT/'target/debian13/sdk-image.json').exists():
        image = json.loads((ROOT/'target/debian13/sdk-image.json').read_text())[0]
        sdk = {'image_id':image['Id'], 'image_digests':image.get('RepoDigests',[])}
    repository=subprocess.run(['git','rev-parse','--show-toplevel'],cwd=ROOT,text=True,capture_output=True)
    if repository.returncode==0 and Path(repository.stdout.strip()).resolve()==ROOT:
        source_commit=run(['git','rev-parse','HEAD'],cwd=ROOT,text=True,capture_output=True).stdout.strip()
        source_modified=subprocess.run(['git','diff','--quiet','HEAD'],cwd=ROOT).returncode!=0
    else:
        source=ROOT/'SOURCE-MANIFEST.json'
        provenance=json.loads(source.read_text()) if source.is_file() else {}
        source_commit=provenance.get('source_commit','unversioned source')
        source_modified=not provenance or any(not (ROOT/name).is_file() or hashlib.sha256((ROOT/name).read_bytes()).hexdigest()!=digest for name,digest in provenance.get('files',{}).items())
    if sdk is not None:
        sdk={'provenance_sha256':hashlib.sha256(json.dumps(sdk,sort_keys=True).encode()).hexdigest()}
    manifest={'source_commit':source_commit,'source_modified':source_modified,'tool_sha256':hashlib.sha256(tool.read_bytes()).hexdigest(), 'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),
              'bundled_libraries':sorted(p.name for p in libdir.iterdir()), 'host_baseline': 'Debian 13 x86_64; glibc >=2.41; host Vulkan/GL/Wayland/C++ runtime' if sysroot else 'Fedora 44 x86_64; glibc 2.43; host Vulkan/GL/Wayland drivers',
              'binaries_sha256':{p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in binaries},
              'libraries_sha256':{p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in libdir.iterdir()},
              'sdk':sdk,
              'drivers_bundled':False}
    (docs/'packaging.json').write_text(json.dumps(manifest, indent=2)+'\n')
    if sysroot:
        run(['python3', ROOT/'scripts/check-glibc-baseline.py', *bindir.iterdir(), *libdir.iterdir()], stdout=(docs/'abi-audit.json').open('w'))
        audit=json.loads((docs/'abi-audit.json').read_text())
        for record in audit['artifacts']:record['artifact']=str(Path(record['artifact']).relative_to(appdir))
        (docs/'abi-audit.json').write_text(json.dumps(audit,indent=2)+'\n')
    if args.appdir_only:
        print(appdir)
        return
    # Reuse the trusted local tool's embedded runtime, entirely offline, without FUSE.
    offset = int(run([tool,'--appimage-offset'], capture_output=True, text=True).stdout.strip())
    runtime=work/'runtime-x86_64'
    with tool.open('rb') as stream:
        runtime.write_bytes(stream.read(offset))
    run([tool,'--appimage-extract'], cwd=work, stdout=subprocess.DEVNULL)
    target=output_target
    if target.exists():
        raise RuntimeError('AppImage existante conservée ; choisir un nouveau chemin --output : '+str(target))
    target.parent.mkdir(parents=True,exist_ok=True)
    staging=target.parent/('.'+target.name+'.'+str(os.getpid())+'.building')
    env=dict(os.environ, ARCH='x86_64')
    run([work/'squashfs-root/AppRun','--no-appstream','--runtime-file',runtime,
         '--mksquashfs-opt','-processors','--mksquashfs-opt','2',appdir,staging], env=env)
    staging.chmod(0o755)
    if target.exists():
        raise RuntimeError('AppImage créée entre-temps ; staging conservé : '+str(staging))
    os.replace(staging, target)
    digest=hashlib.sha256(target.read_bytes()).hexdigest()
    target.with_suffix('.AppImage.sha256').write_text(f'{digest}  {target.name}\n')
    print(target)

if __name__=='__main__':
    main()
