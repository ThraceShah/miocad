import base64
import json
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
            catalog=call({'op':'commands.list'})
            self.assertTrue(catalog['ok'])
            self.assertIn('model.fillet',{item['id'] for item in catalog['result']})
            doc=call({'op':'document.create'})['result']['document']
            self.assertTrue(call(dict(op='model.box',document=doc,revision=0,size=[10,10,10]))['ok'])
            edge=call(dict(op='topology.edges',document=doc))['result']['data'][0]['reference']
            self.assertTrue(call(dict(op='model.fillet',document=doc,revision=1,edge=edge,radius=1))['ok'])
            self.assertFalse(call(dict(op='model.fillet',document=doc,revision=2,edge=edge,radius=1))['ok'])
            with tempfile.TemporaryDirectory() as tmp:
                response=rpc('tools/call',{'name':'cad_command','arguments':{'command':dict(op='render.snapshot',document=doc,directory=tmp)}})['result']
                self.assertFalse(response['isError'])
                self.assertEqual(base64.b64decode(response['content'][1]['data'])[:8],b'\x89PNG\r\n\x1a\n')
            self.assertEqual(rpc('missing')['error']['code'],-32601)
        finally:
            p.stdin.close(); p.wait(timeout=10); p.stdout.close()
        self.assertEqual(p.returncode,0)
    def test_cli(self):
        result=subprocess.run([sys.executable,str(ROOT/'modules/cad-cli/main.py')],input='{"op":"capabilities"}\n',capture_output=True,text=True,check=True)
        self.assertEqual(json.loads(result.stdout)['result']['kernel'],'smlib')
if __name__=='__main__': unittest.main()
