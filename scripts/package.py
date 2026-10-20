#!/usr/bin/env python3
"""Internal build/packaging tool. The resulting product contains no Python runtime."""
import argparse
import json
from pathlib import Path
import shutil
import subprocess
ROOT=Path(__file__).resolve().parents[1]

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--output',type=Path,default=ROOT/'artifacts/dist')
    parser.add_argument('--web-only',action='store_true',help='Headless server + WASM bundle, omit desktop/CLI/MCP binaries')
    args=parser.parse_args()
    output=args.output.resolve()
    cargo=shutil.which('cargo') or str(Path.home()/'.cargo/bin/cargo')
    def run(*command): subprocess.run(command,cwd=ROOT,check=True)
    if not (ROOT/'build/CMakeCache.txt').exists():
        run('cmake','-S',str(ROOT),'-B',str(ROOT/'build'),'-G','Ninja','-DCMAKE_BUILD_TYPE=Release')
    run('cmake','--build',str(ROOT/'build'),'--target','cad-server','-j','6')
    # Product Web assets and gateway are Rust; this Python file is only a build tool.
    import os
    env=dict(os.environ);env['PATH']=str(Path(cargo).parent)+os.pathsep+env.get('PATH','')
    subprocess.run(['bash','scripts/build-web.sh'],cwd=ROOT,env=env,check=True)
    if not args.web_only: run(cargo,'build','--release','--workspace','--locked')
    (output/'bin').mkdir(parents=True,exist_ok=True)
    (output/'launch.sh').unlink(missing_ok=True)
    for name in (['cad-service','cad-web'] if args.web_only else ['cad-service','cad-cli','cad-mcp','cad-gui','cad-web']):
        shutil.copy2(ROOT/'target/release'/name,output/'bin'/name)
    shutil.copy2(ROOT/'build/cad-server',output/'bin/cad-server')
    shutil.copytree(ROOT/'artifacts/web',output/'web',dirs_exist_ok=True)
    share=output/'share/ai-cad';share.mkdir(parents=True,exist_ok=True)
    for source in ['Cargo.lock','THIRD_PARTY.md','README.md','docs/migration-p4.md','docs/desktop-p5.md','docs/web-p6.md','docs/viewport-p7.md','docs/tablet-ui.md','docs/agent-workspace.md','docs/parametric-history.md','modules/cad-skills/GUIDE.md']:
        shutil.copy2(ROOT/source,share/Path(source).name)
    notices=share/'licenses';notices.mkdir(exist_ok=True)
    for source in [ROOT/'third_party/usd-brep',ROOT/'third_party/json']:
        dest=notices/source.name;dest.mkdir(exist_ok=True)
        for file in source.iterdir():
            if file.is_file() and file.name.upper().startswith(('LICENSE','COPYING','NOTICE','THIRD_PARTY')):
                shutil.copy2(file,dest/file.name)
    rustc=shutil.which('rustc') or str(Path(cargo).with_name('rustc'))
    host=next(line.split(': ',1)[1] for line in subprocess.check_output([rustc,'-vV'],text=True).splitlines() if line.startswith('host: '))
    # All native sources were just built. Do not fetch unrelated Apple/Android/Windows crates for notices.
    packages={}
    for platform in [host,'wasm32-unknown-unknown']:
        metadata=json.loads(subprocess.check_output([cargo,'metadata','--locked','--offline','--filter-platform',platform,'--format-version','1'],cwd=ROOT))
        for package in metadata['packages']: packages[package['id']]=package
    metadata={'packages':list(packages.values())}
    inventory=[]
    for package in metadata['packages']:
        if not package['source']: continue
        root=Path(package['manifest_path']).parent
        dest=notices/f"{package['name']}-{package['version']}";dest.mkdir(exist_ok=True)
        for file in root.rglob('*'):
            notice=file.name.upper().startswith(('LICENSE','COPYING','NOTICE','COPYRIGHT')) or file.name.upper() in ('OFL.TXT','UFL.TXT') or 'license' in file.name.lower()
            font_notice='fonts' in file.relative_to(root).parts and file.suffix=='.txt'
            if file.is_file() and (notice or font_notice):
                target=dest/file.relative_to(root);target.parent.mkdir(parents=True,exist_ok=True)
                shutil.copy2(file,target)
        inventory.append({key:package[key] for key in ['name','version','license','source']})
    shutil.copytree(ROOT/'third_party/notices',notices/'supplemental',dirs_exist_ok=True)
    (share/'dependencies.json').write_text(json.dumps(inventory,indent=2)+'\n')
    archive=shutil.make_archive(str(output),'gztar',root_dir=output)
    print(output)
    print(archive)
if __name__=='__main__': main()
