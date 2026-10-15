#!/usr/bin/env python3
"""Extract a local OCCT 7.9 Debian SDK without installing system packages.

Verified on Ubuntu 26.04 amd64, OCCT 7.9.2+dfsg-4. Requires matching apt sources.
Optional visualization runtime libraries are packaging dependencies of TKXCAF;
no viewer, display server, Java or VTK development package is used by AI-CAD.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--destination',type=Path,default=Path('build/occt-sdk'))
parser.add_argument('--packages',type=Path,default=Path('build/occt-packages'))
parser.add_argument('--version',default='7.9.2+dfsg-4')
args=parser.parse_args()
packages=args.packages.resolve(); destination=args.destination.resolve()
packages.mkdir(parents=True,exist_ok=True); destination.mkdir(parents=True,exist_ok=True)
occt=['libocct-'+name for name in ['data-exchange-dev','foundation-dev','modeling-algorithms-dev','modeling-data-dev','data-exchange-7.9','foundation-7.9','modeling-algorithms-7.9','modeling-data-7.9','ocaf-7.9','visualization-7.9']]
wanted=set(occt+['libtbb12','libtbbmalloc2'])
subprocess.run(['apt-get','download',*[p+'='+args.version for p in occt],'libtbb12','libtbbmalloc2'],cwd=packages,check=True)
manifest=[]
for path in sorted(packages.glob('*.deb')):
    fields=subprocess.check_output(['dpkg-deb','-f',str(path),'Package','Version'],text=True).splitlines()
    name,version=[line.split(': ',1)[1] for line in fields]
    if name not in wanted or (name in occt and version!=args.version): continue
    subprocess.run(['dpkg-deb','-x',str(path),str(destination)],check=True)
    manifest.append(dict(package=name,version=version,sha256=hashlib.sha256(path.read_bytes()).hexdigest()))
if {item['package'] for item in manifest}!=wanted: raise RuntimeError('incomplete local OCCT SDK')
(destination/'packages.json').write_text(json.dumps(manifest,indent=2)+'\n')
print('OCCT_ROOT='+str(destination/'usr'))
