#!/usr/bin/env python3
"""prune.py APP QT_DIR QML_SRC: after macdeployqt, keeps in ryNet.app only the part of Qt the window loads, as the
Linux package does: the QML modules qmlimportscanner finds, the plugins it uses, and the frameworks they all link,
each for one processor (RYNET_ARCH, arm64 by default)."""
import json, os, pathlib, re, shutil, subprocess, sys

app, qt, src = map(pathlib.Path, sys.argv[1:4])
c = app / 'Contents'

scan = json.loads(subprocess.run([str(qt / 'libexec/qmlimportscanner'), '-rootPath', str(src), '-importPath', str(qt / 'qml')],
                                 capture_output=True, text=True, check=True).stdout)
mods = [m for m in scan if m.get('type') == 'module' and m.get('path', '').startswith(str(qt / 'qml'))]
keep_dirs = {os.path.relpath(m['path'], qt / 'qml') for m in mods}
keep_plugins = {f"lib{m['plugin']}.dylib" for m in mods if m.get('plugin')}

qml = c / 'Resources/qml'
for d, _, files in os.walk(qml, topdown=False):
    rel = os.path.relpath(d, qml)
    for f in files:
        p = pathlib.Path(d) / f
        if rel not in keep_dirs or f.endswith('.qmltypes'):
            p.unlink()
        elif p.is_symlink():
            keep_plugins.add(pathlib.Path(os.readlink(p)).name)
    if not os.listdir(d) and d != str(qml):
        os.rmdir(d)

plugins = c / 'PlugIns'
for p in (plugins / 'quick').glob('*.dylib'):
    if p.name not in keep_plugins:
        p.unlink()
for d in plugins.iterdir():
    if d.name not in ('platforms', 'tls', 'networkinformation', 'quick'):
        shutil.rmtree(d)

def links(f):
    out = subprocess.run(['otool', '-L', str(f)], capture_output=True, text=True).stdout
    return set(re.findall(r'@(?:rpath|executable_path/\.\./Frameworks)/([^/\s]+\.framework|[^/\s]+\.dylib)', out))

roots = [p for p in (c / 'MacOS').iterdir() if p.is_file()] + [p for p in plugins.rglob('*.dylib') if p.is_file()]
need, todo = set(), [r for r in roots]
while todo:
    for name in links(todo.pop()):
        if name in need:
            continue
        need.add(name)
        fw = c / 'Frameworks' / name
        if fw.suffix == '.framework':
            binary = fw / 'Versions/A' / fw.stem
            todo.append(binary if binary.exists() else fw / fw.stem)
        elif fw.exists():
            todo.append(fw)
for fw in (c / 'Frameworks').iterdir():
    if fw.name not in need:
        shutil.rmtree(fw) if fw.is_dir() and not fw.is_symlink() else fw.unlink()
arch = os.environ.get('RYNET_ARCH', 'arm64')
for f in c.rglob('*'):
    if f.is_file() and not f.is_symlink():
        info = subprocess.run(['lipo', '-archs', str(f)], capture_output=True, text=True)
        archs = info.stdout.split() if info.returncode == 0 else []
        if len(archs) > 1 and arch in archs:
            subprocess.run(['lipo', str(f), '-thin', arch, '-output', str(f)], check=True)
print('kept frameworks:', ' '.join(sorted(need)))
print('kept qml:', ' '.join(sorted(keep_dirs)))
