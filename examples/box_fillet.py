#!/usr/bin/env python3
"""Real C++ SMLib → shaded screenshot → CPU pixel pick → fillet → validate → SMB."""
from pathlib import Path
import json
import sys
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'scripts'))
from client import Client
out=ROOT/'artifacts'; out.mkdir(exist_ok=True)
client=Client()
def call(op,**args):
    result=client.command(dict(op=op,**args))
    if not result['ok']: raise RuntimeError(result)
    return result['result']
try:
    doc=call('document.create')['document']
    call('model.box',document=doc,revision=0,size=[10,10,10])
    before=call('render.snapshot',document=doc,directory=str(out))
    # Deterministic fixture: choose a visible CPU edge-ID pixel. A VLM can supply x/y instead.
    meta,pixels=client.frames[before['render_id']]
    index=next(i for i,value in enumerate(pixels) if value)
    x,y=index%meta['width'],index//meta['width']
    selected=call('render.pick',render_id=before['render_id'],x=x,y=y)
    state=call('model.fillet',document=doc,revision=1,edge=selected['reference'],radius=1)
    if not state['data']['valid']: raise RuntimeError('invalid model')
    after=call('render.snapshot',document=doc,directory=str(out))
    call('export.native',document=doc,path=str(out/'box-fillet.smb'))
    report={'state':state,'before':before,'selection':dict(x=x,y=y,**selected),'after':after,'step_export':False}
    (out/'demo.json').write_text(json.dumps(report,indent=2))
    print(json.dumps(report,indent=2))
finally: client.close()
