"""Real eframe/wgpu window acceptance in a private Xvfb. Never drives the user's desktop."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time
ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/'artifacts/p7-gui'
BIN=Path(os.environ.get('CAD_BIN',ROOT/'target/debug'))
class Smoke:
    def __init__(self):
        self.probe=subprocess.Popen([sys.executable,'-u',str(ROOT/'tests/gui_probe.py')],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
        self.info=json.loads(self.probe.stdout.readline());self.runtime=self.info['runtime'];self.receipts=[];self.mcp=None;self.last_view_epoch=-1
    def ui(self,op,**args):
        self.probe.stdin.write(json.dumps(dict(op=op,**args))+'\n');self.probe.stdin.flush()
        result=json.loads(self.probe.stdout.readline());assert result['ok'],result
        return result['result']
    def cli(self,command):
        result=json.loads(subprocess.check_output([str(BIN/'cad-cli'),'--runtime-dir',self.runtime,'--client-id','native-smoke-cli'],input=json.dumps(command)+'\n',text=True))
        self.receipts.append({'command':command,'response':result});return result
    def documents(self):return self.cli({'op':'document.list'})['result']
    def state(self):return self.documents()[0]
    def wait_revision(self,revision):
        end=time.monotonic()+8
        while time.monotonic()<end:
            state=self.state()
            if state['revision']==revision:return state
            time.sleep(.1)
        raise AssertionError(f'window did not commit revision {revision}: {state}')
    def wait_view(self,preview,revision=None):
        until=time.monotonic()+12
        while True:
            frames=self.cli({'op':'render.frames'})['result']
            latest=frames[-1] if frames else None
            if latest and latest.get('epoch',-1)>self.last_view_epoch and latest.get('preview')==preview and (revision is None or latest['revision']==revision):
                self.last_view_epoch=latest['epoch'];time.sleep(.3);return latest
            assert time.monotonic()<until,'viewport did not publish expected frame'
            time.sleep(.1)
    def shot(self,name):
        if name in ['02-box-preview','04-ready-to-confirm','06-fillet-preview','10-before-agent-edit']:self.wait_view(True)
        time.sleep(.65)
        result=Path(self.ui('shot',name=name))
        frames=self.cli({'op':'render.frames'})['result'] if name!='13-service-unavailable' else []
        if frames:self.last_view_epoch=max(self.last_view_epoch,frames[-1].get('epoch',-1))
        return result
    @staticmethod
    def pixels(path):return subprocess.check_output(['/usr/bin/ffmpeg','-hide_banner','-loglevel','error','-i',str(path),'-f','rawvideo','-pix_fmt','rgb24','-'])
    def blue_count(self,path):
        data=self.pixels(path);hits=0
        for y in range(160,740,3):
            for x in range(250,970,3):
                r,g,b=data[(y*1280+x)*3:(y*1280+x)*3+3]
                hits+=b>r+35 and g>r+15 and b>g
        return hits
    def rpc(self,method,params=None):
        self.mcp.stdin.write(json.dumps(dict(jsonrpc='2.0',id=1,method=method,params=params or {}))+'\n');self.mcp.stdin.flush();return json.loads(self.mcp.stdout.readline())
    def tool(self,command):return self.rpc('tools/call',dict(name='cad_command',arguments={'command':command}))['result']['structuredContent']
    def run(self):
        time.sleep(1.5)
        assert self.ui('state')['alive']
        self.shot('01-empty')
        self.ui('control',name='new-document')
        end=time.monotonic()+8
        while not self.documents():assert time.monotonic()<end;time.sleep(.1)
        doc=self.state()['document'];service=self.cli({'op':'service.info'})['result']['service_id']
        self.ui('control',name='preview')
        assert self.blue_count(self.shot('02-box-preview'))>1000
        assert self.state()['data']['empty'] and self.state()['revision']==0
        self.ui('control',name='cancel')
        assert self.blue_count(self.shot('03-cancelled'))<1000
        assert self.state()['revision']==0
        self.ui('control',name='preview');self.shot('04-ready-to-confirm')
        self.ui('control',name='confirm');assert self.wait_revision(1)['data']['volume']==1000
        assert 'confirm' not in self.ui('layout')['controls'];assert self.state()['revision']==1 # Closed command cannot commit twice.
        self.wait_view(False,1)
        v=self.ui('layout')['viewport'];self.ui('click',x=round(v[0]+v[2]*.5),y=round(v[1]+v[3]*.65));self.shot('05-selected-edge')
        self.mcp=subprocess.Popen([str(BIN/'cad-mcp'),'--runtime-dir',self.runtime,'--client-id','native-smoke-agent'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
        assert 'result' in self.rpc('initialize',dict(protocolVersion='2025-11-25',capabilities={},clientInfo={'name':'native-smoke','version':'1'}))
        self.mcp.stdin.write('{"jsonrpc":"2.0","method":"notifications/initialized"}\n');self.mcp.stdin.flush()
        assert self.tool(dict(op='document.state',document=doc))['result']['revision']==1
        missing=self.tool(dict(op='model.fillet',document=doc,revision=1,radius=1))
        assert not missing['ok'] and missing['error']['kind']=='invalid_params' # GUI selection is private.
        self.ui('control',name='preview');self.shot('06-fillet-preview');assert self.state()['revision']==1
        self.ui('control',name='confirm');fillet=self.wait_revision(2)
        assert fillet['data']['valid'] and abs(fillet['data']['volume']-997.8539818)<1e-4
        self.shot('07-fillet-committed')
        before=self.shot('08-before-orbit');cursor=self.cli({'op':'service.snapshot'})['result']['cursor']
        self.ui('drag',**{'from':[650,420],'to':[720,470]})
        after=self.shot('09-local-orbit')
        assert hashlib.sha256(self.pixels(before)).digest()!=hashlib.sha256(self.pixels(after)).digest()
        assert self.state()['revision']==2
        assert self.cli({'op':'service.events','after':cursor})['result']['events']==[]
        self.ui('control',name='undo');assert self.wait_revision(3)['data']['volume']==1000
        self.ui('control',name='redo');assert self.wait_revision(4)['data']['volume']<1000
        self.ui('control',name='box');self.ui('control',name='preview');self.shot('10-before-agent-edit')
        changed=self.tool(dict(op='model.box',document=doc,revision=4,size=[12,10,10]));assert changed['ok']
        self.shot('11-stale-preview');self.ui('control',name='confirm',allow_disabled=True);assert self.state()['revision']==5
        assert self.state()['data']['volume']==1200
        self.ui('stop-gui')
        assert self.state()['revision']==5 # Native window close leaves the shared service alive.
        self.ui('start-gui');time.sleep(1.2);self.shot('12-reopened')
        assert self.cli({'op':'service.info'})['result']['service_id']==service
        path=str(OUT/'native-workflow.smb');assert self.tool(dict(op='export.native',document=doc,path=path))['ok']
        self.cli({'op':'service.shutdown'});self.shot('13-service-unavailable')
        self.ui('stop-gui')
        (OUT/'acceptance.json').write_text(json.dumps({'passed':True,'window':self.info,'document':doc,'service_id':service,'fillet':fillet,'receipts':self.receipts},indent=2))
        print(json.dumps({'passed':True,'screenshots':str(OUT),'revision_after_agent_edit':5,'fillet_volume':fillet['data']['volume']}))
    def close(self):
        if self.mcp:self.mcp.stdin.close();self.mcp.wait(timeout=10);self.mcp.stdout.close()
        try:self.ui('quit')
        except (BrokenPipeError,ValueError):pass
        self.probe.stdin.close();self.probe.wait(timeout=15);self.probe.stdout.close()
if __name__=='__main__':
    smoke=Smoke()
    try:smoke.run()
    finally:smoke.close()
