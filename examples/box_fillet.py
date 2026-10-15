#!/usr/bin/env python3
"""Exact B-Rep → shaded PNG → CPU pixel pick → fillet → validate → STEP roundtrip.

Default backend is SMLib (native SMB only); select --kernel occt for STEP.
A deterministic visible-pixel fixture stands in for a VLM's coordinate input.
"""
import argparse
import json
import math
from pathlib import Path
import sys
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'scripts'))
from client import Client

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--kernel',choices=['smlib','occt'],help='Backend (or CAD_KERNEL)')
parser.add_argument('--server',help='CAD worker executable (or CAD_SERVER)')
parser.add_argument('--output',type=Path,help='Artifact directory (default artifacts/<kernel>)')
args=parser.parse_args()
client=Client(args.server,args.kernel)
def call(op,**args):
    result=client.command(dict(op=op,**args))
    if not result['ok']: raise RuntimeError(result)
    return result['result']
try:
    caps=call('capabilities')
    out=(args.output or ROOT/'artifacts'/caps['kernel']).resolve(); out.mkdir(parents=True,exist_ok=True)
    doc=call('document.create')['document']
    call('model.box',document=doc,revision=0,size=[10,10,10])
    before=call('render.snapshot',document=doc,directory=str(out))
    # A VLM can supply x/y here. This fixture chooses an actual visible ID pixel.
    meta,pixels=client.frames[before['render_id']]
    index=next(i for i,value in enumerate(pixels) if value)
    x,y=index%meta['width'],index//meta['width']
    selected=call('render.pick',render_id=before['render_id'],x=x,y=y)
    state=call('model.fillet',document=doc,revision=selected['revision'],edge=selected['reference'],radius=1)
    expected=1000-10*(1-math.pi/4)
    if not state['data']['valid'] or not math.isclose(state['data']['volume'],expected,rel_tol=1e-7):
        raise RuntimeError('fillet model failed analytic validation')
    after=call('render.snapshot',document=doc,directory=str(out))
    native=out/('box-fillet.'+caps['native_format'])
    call('export.native',document=doc,path=str(native))
    report={'kernel':caps['kernel'],'state':state,'before':before,'selection':dict(x=x,y=y,**selected),'after':after,'native_path':str(native),'step_export':caps['step_export']}
    if caps['step_export']:
        step=out/'box-fillet.step'
        call('export.step',document=doc,path=str(step))
        restored=call('document.create')['document']
        roundtrip=call('import.step',document=restored,revision=0,path=str(step))
        if not roundtrip['data']['valid'] or not math.isclose(roundtrip['data']['volume'],state['data']['volume'],rel_tol=1e-7):
            raise RuntimeError('STEP roundtrip volume or validity failed')
        for side in ('min','max'):
            if any(abs(a-b)>1e-5 for a,b in zip(state['data']['bounds'][side],roundtrip['data']['bounds'][side])):
                raise RuntimeError('STEP roundtrip bounds failed')
        report.update(step_path=str(step),roundtrip=roundtrip)
    (out/'demo.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2))
finally: client.close()
