import json
import os
from pathlib import Path
import subprocess
import sys
ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(Path(__file__).resolve().parent))
from preview import render
class Client:
    def __init__(self,server=None):
        self.process=subprocess.Popen([str(server or os.environ.get('CAD_SERVER') or ROOT/'build/cad-server')],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
        self.frames={}
    def raw(self,request):
        self.process.stdin.write(json.dumps(request,allow_nan=False)+'\n'); self.process.stdin.flush()
        line=self.process.stdout.readline()
        if not line: raise RuntimeError('CAD worker exited')
        return json.loads(line)
    def command(self,request):
        op=request.get('op')
        if op=='render.snapshot':
            doc=request['document']
            mesh=self.raw({'op':'render.mesh','document':doc})
            if not mesh['ok']: return mesh
            edges=self.raw({'op':'topology.edges','document':doc})
            if not edges['ok']: return edges
            revision=mesh['result']['revision']
            meta,pixels=render(mesh['result']['data'],edges['result']['data'],doc,revision,request.get('directory',ROOT/'artifacts'))
            self.frames[meta['render_id']]=(meta,pixels)
            return {'ok':True,'result':meta}
        if op=='render.pick':
            meta,pixels=self.frames[request['render_id']]
            state=self.raw({'op':'document.state','document':meta['document']})
            if not state['ok']: return state
            if state['result']['revision']!=meta['revision']: raise ValueError('stale render')
            x,y=request['x'],request['y']
            if type(x) is not int or type(y) is not int or not 0<=x<meta['width'] or not 0<=y<meta['height']: raise ValueError('pixel outside viewport')
            return {'ok':True,'result':{'reference':pixels[y*meta['width']+x],'document':meta['document'],'revision':meta['revision']}}
        return self.raw(request)
    def close(self):
        self.process.stdin.close()
        try: self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.process.kill(); self.process.wait()
        self.process.stdout.close()
