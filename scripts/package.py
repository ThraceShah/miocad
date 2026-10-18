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
    args=parser.parse_args()
    output=args.output.resolve()
    cargo=shutil.which('cargo') or str(Path.home()/'.cargo/bin/cargo')
    def run(*command): subprocess.run(command,cwd=ROOT,check=True)
    if not (ROOT/'build/CMakeCache.txt').exists():
        run('cmake','-S',str(ROOT),'-B',str(ROOT/'build'),'-G','Ninja','-DCMAKE_BUILD_TYPE=Release')
    run('cmake','--build',str(ROOT/'build'),'--target','cad-server','-j','6')
    run(cargo,'build','--release','--workspace','--locked')
    (output/'bin').mkdir(parents=True,exist_ok=True)
    for name in ['cad-service','cad-cli','cad-mcp']:
        shutil.copy2(ROOT/'target/release'/name,output/'bin'/name)
    shutil.copy2(ROOT/'build/cad-server',output/'bin/cad-server')
    share=output/'share/ai-cad';share.mkdir(parents=True,exist_ok=True)
    for source in ['Cargo.lock','THIRD_PARTY.md','README.md','docs/migration-p4.md','modules/cad-skills/GUIDE.md']:
        shutil.copy2(ROOT/source,share/Path(source).name)
    notices=share/'licenses';notices.mkdir(exist_ok=True)
    for source in [ROOT/'third_party/usd-brep',ROOT/'third_party/json']:
        dest=notices/source.name;dest.mkdir(exist_ok=True)
        for file in source.iterdir():
            if file.is_file() and file.name.upper().startswith(('LICENSE','COPYING','NOTICE','THIRD_PARTY')):
                shutil.copy2(file,dest/file.name)
    metadata=json.loads(subprocess.check_output([cargo,'metadata','--locked','--format-version','1'],cwd=ROOT))
    inventory=[]
    for package in metadata['packages']:
        if not package['source']: continue
        root=Path(package['manifest_path']).parent
        dest=notices/f"{package['name']}-{package['version']}";dest.mkdir(exist_ok=True)
        for file in root.iterdir():
            if file.is_file() and file.name.upper().startswith(('LICENSE','COPYING','NOTICE','COPYRIGHT')):
                shutil.copy2(file,dest/file.name)
        inventory.append({key:package[key] for key in ['name','version','license','source']})
    (share/'dependencies.json').write_text(json.dumps(inventory,indent=2)+'\n')
    archive=shutil.make_archive(str(output),'gztar',root_dir=output)
    print(output)
    print(archive)
if __name__=='__main__': main()
