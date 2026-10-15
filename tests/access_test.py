import base64
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'scripts'))
from client import Client

class AccessTest(unittest.TestCase):
    def test_preview_pick_and_stale_frame(self):
        client=Client()
        try:
            doc=client.command({'op':'document.create'})['result']['document']
            client.command(dict(op='model.box',document=doc,revision=0,size=[10,10,10]))
            with tempfile.TemporaryDirectory() as tmp:
                frame=client.command(dict(op='render.snapshot',document=doc,directory=tmp))['result']
                for path in frame['images'].values(): self.assertEqual(Path(path).read_bytes()[:8],b'\x89PNG\r\n\x1a\n')
                meta,pixels=client.frames[frame['render_id']]
                idx=next(i for i,v in enumerate(pixels) if v)
                selected=client.command(dict(op='render.pick',render_id=frame['render_id'],x=idx%384,y=idx//384))['result']['reference']
                self.assertTrue(client.command(dict(op='model.fillet',document=doc,revision=1,edge=selected,radius=1))['ok'])
                with self.assertRaisesRegex(ValueError,'stale render'):
                    client.command(dict(op='render.pick',render_id=frame['render_id'],x=idx%384,y=idx//384))
        finally: client.close()
    def test_mcp_handshake_and_modeling(self):
        p=subprocess.Popen([sys.executable,str(ROOT/'modules/cad-mcp/server.py')],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
        def rpc(method,params=None):
            p.stdin.write(json.dumps(dict(jsonrpc='2.0',id=1,method=method,params=params or {}))+'\n'); p.stdin.flush()
            return json.loads(p.stdout.readline())
        def call(command):
            response=rpc('tools/call',{'name':'cad_command','arguments':{'command':command}})['result']
            return json.loads(response['content'][0]['text'])
        try:
            self.assertIn('error',rpc('tools/list'))
            init=rpc('initialize',{'protocolVersion':'2025-11-25','capabilities':{},'clientInfo':{'name':'test','version':'1'}})
            self.assertEqual(init['result']['protocolVersion'],'2025-11-25')
            p.stdin.write(json.dumps(dict(jsonrpc='2.0',method='notifications/initialized'))+'\n'); p.stdin.flush()
            self.assertEqual(rpc('tools/list')['result']['tools'][0]['name'],'cad_command')
            doc=call({'op':'document.create'})['result']['document']
            self.assertTrue(call(dict(op='model.box',document=doc,revision=0,size=[10,10,10]))['ok'])
            edge=call(dict(op='topology.edges',document=doc))['result']['data'][0]['reference']
            self.assertTrue(call(dict(op='model.fillet',document=doc,revision=1,edge=edge,radius=1))['ok'])
            self.assertFalse(call(dict(op='model.fillet',document=doc,revision=2,edge=edge,radius=1))['ok'])
            with tempfile.TemporaryDirectory() as tmp:
                response=rpc('tools/call',{'name':'cad_command','arguments':{'command':dict(op='render.snapshot',document=doc,directory=tmp)}})['result']
                self.assertFalse(response['isError'])
                self.assertEqual(base64.b64decode(response['content'][1]['data'])[:8],b'\x89PNG\r\n\x1a\n')
            if call({'op':'capabilities'})['result']['step_export']:
                with tempfile.TemporaryDirectory() as tmp:
                    path=str(Path(tmp)/'mcp-fillet.step')
                    self.assertTrue(call(dict(op='export.step',document=doc,path=path))['ok'])
                    source=call(dict(op='document.state',document=doc))['result']['data']
                    imported=call({'op':'document.create'})['result']['document']
                    restored=call(dict(op='import.step',document=imported,revision=0,path=path))
                    self.assertTrue(restored['ok'],restored)
                    self.assertAlmostEqual(source['volume'],restored['result']['data']['volume'],places=5)
                    self.assertTrue(restored['result']['data']['valid'])
                    corrupt=Path(tmp)/'bad.step'; corrupt.write_text('bad STEP')
                    failed=rpc('tools/call',{'name':'cad_command','arguments':{'command':dict(op='import.step',document=imported,revision=1,path=str(corrupt))}})['result']
                    self.assertTrue(failed['isError'])
                    self.assertEqual(restored['result'],call(dict(op='document.state',document=imported))['result'])
            self.assertEqual(rpc('missing')['error']['code'],-32601)
        finally:
            p.stdin.close(); p.wait(timeout=10); p.stdout.close()
        self.assertEqual(p.returncode,0)
    def test_complete_cpu_workflow_and_step_roundtrip(self):
        if os.environ.get('CAD_KERNEL','smlib')!='occt': self.skipTest('STEP workflow requires OCCT')
        with tempfile.TemporaryDirectory() as tmp:
            result=subprocess.run([sys.executable,str(ROOT/'examples/box_fillet.py'),'--kernel','occt','--output',tmp],capture_output=True,text=True,check=True)
            report=json.loads(result.stdout)
            self.assertTrue(report['step_export'])
            self.assertTrue(Path(report['step_path']).is_file())
            self.assertAlmostEqual(report['state']['data']['volume'],report['roundtrip']['data']['volume'],places=5)
            for name in ('before','after'):
                self.assertEqual(Path(report[name]['images']['shaded']).read_bytes()[:8],b'\x89PNG\r\n\x1a\n')
    def test_cli(self):
        p=subprocess.Popen([sys.executable,str(ROOT/'modules/cad-cli/main.py'),'--kernel',os.environ.get('CAD_KERNEL','smlib')],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
        def call(command):
            p.stdin.write(json.dumps(command)+'\n'); p.stdin.flush()
            response=json.loads(p.stdout.readline())
            self.assertTrue(response['ok'],response)
            return response['result']
        try:
            caps=call({'op':'capabilities'})
            self.assertEqual(caps['kernel'],os.environ.get('CAD_KERNEL','smlib'))
            doc=call({'op':'document.create'})['document']
            before=call(dict(op='model.box',document=doc,revision=0,size=[2,3,4]))
            if caps['step_export']:
                with tempfile.TemporaryDirectory() as tmp:
                    path=str(Path(tmp)/'cli.step')
                    call(dict(op='export.step',document=doc,path=path))
                    other=call({'op':'document.create'})['document']
                    restored=call(dict(op='import.step',document=other,revision=0,path=path))
                    self.assertTrue(restored['data']['valid'])
                    self.assertAlmostEqual(restored['data']['volume'],before['data']['volume'],places=6)
        finally:
            p.stdin.close(); p.wait(timeout=10); p.stdout.close()
        self.assertEqual(p.returncode,0)
if __name__=='__main__': unittest.main()
