"""Internal acceptance driver. Product processes run with PATH containing no Python."""
import base64
import json
import os
from pathlib import Path
import struct
import subprocess
import tempfile
import unittest
import zlib

ROOT = Path(__file__).resolve().parents[1]
BIN = Path(os.environ.get('CAD_BIN', ROOT / 'target/debug')).resolve()
WORKER = Path(os.environ.get('CAD_SERVER', ROOT / 'build/cad-server')).resolve()

class Products(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='cad-product-')
        self.runtime = Path(self.tmp.name) / 'runtime'
        self.env = dict(os.environ, PATH=str(BIN), CAD_SERVER=str(WORKER))
        self.flags = ['--runtime-dir', str(self.runtime)]
        self.mcp = None
    def tearDown(self):
        if self.mcp:
            self.mcp.stdin.close()
            self.mcp.wait(timeout=10)
            self.mcp.stdout.close()
            self.mcp.stderr.close()
        if (self.runtime / 'endpoint.json').exists():
            self.cli({'op': 'service.shutdown'})
        self.tmp.cleanup()
    def cli(self, command, extra=()):
        run = subprocess.run([str(BIN/'cad-cli'), *self.flags, *extra], input=json.dumps(command)+'\n', text=True, capture_output=True, env=self.env, timeout=20)
        self.assertEqual(run.returncode, 0, run.stderr)
        lines = run.stdout.splitlines()
        self.assertEqual(len(lines), 1, run.stdout)
        return json.loads(lines[0])
    def rpc(self, method, params=None, id=1):
        self.mcp.stdin.write(json.dumps(dict(jsonrpc='2.0', id=id, method=method, params=params or {}))+'\n')
        self.mcp.stdin.flush()
        return json.loads(self.mcp.stdout.readline())
    def tool(self, command):
        result = self.rpc('tools/call', dict(name='cad_command', arguments={'command':command}))['result']
        self.assertEqual(json.loads(result['content'][0]['text']), result['structuredContent'])
        return result
    def start_mcp(self):
        self.mcp = subprocess.Popen([str(BIN/'cad-mcp'), *self.flags], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=self.env)
        self.assertIn('error', self.rpc('tools/list'))
        init = self.rpc('initialize', dict(protocolVersion='2025-11-25', capabilities={}, clientInfo={'name':'acceptance', 'version':'1'}))
        self.assertEqual(init['result']['protocolVersion'], '2025-11-25')
        self.mcp.stdin.write('{"jsonrpc":"2.0","method":"notifications/initialized"}\n')
        self.mcp.stdin.flush()
        tool = self.rpc('tools/list')['result']['tools'][0]
        self.assertEqual(tool['name'], 'cad_command')
        self.assertIn('inputSchema', tool)
    @staticmethod
    def rgb(encoded):
        data=base64.b64decode(encoded)
        assert data[:8] == b'\x89PNG\r\n\x1a\n'
        offset=8; compressed=b''
        while offset < len(data):
            length=struct.unpack('>I',data[offset:offset+4])[0]
            kind=data[offset+4:offset+8]; payload=data[offset+8:offset+8+length]
            assert struct.unpack('>I',data[offset+8+length:offset+12+length])[0] == zlib.crc32(kind+payload)&0xffffffff
            if kind==b'IHDR': width,height,depth,color,*_=struct.unpack('>IIBBBBB',payload); assert (depth,color)==(8,2)
            if kind==b'IDAT': compressed+=payload
            offset+=12+length
        raw=zlib.decompress(compressed); rows=[]
        for y in range(height):
            row=raw[y*(width*3+1):(y+1)*(width*3+1)]
            assert row[0]==0
            rows.append(row[1:])
        return width,height,b''.join(rows)
    def test_cross_process_workflow_without_python(self):
        doc=self.cli({'op':'document.create'})['result']['document']
        boxed=self.cli(dict(op='model.box',document=doc,revision=0,size=[10,10,10]))
        self.assertTrue(boxed['ok'])
        self.start_mcp()
        state=self.tool(dict(op='document.state',document=doc))['structuredContent']
        self.assertEqual(state['result']['data']['volume'],1000)
        shot=self.tool(dict(op='render.snapshot',document=doc))
        self.assertFalse(shot['isError'])
        frame=shot['structuredContent']['result']
        self.assertEqual(base64.b64decode(shot['content'][1]['data']),base64.b64decode(frame['images']['shaded']))
        capture=self.tool(dict(op='render.capture',render_id=frame['render_id']))
        self.assertFalse(capture['isError'])
        self.assertEqual(capture['content'][1]['data'],frame['images']['shaded'])
        width,height,pixels=self.rgb(frame['images']['edge-id'])
        self.assertEqual((width,height),(384,384))
        index=next(i for i in range(width*height) if any(pixels[i*3:i*3+3]))
        pick=self.cli(dict(op='render.pick',render_id=frame['render_id'],x=index%width,y=index//width))
        self.assertTrue(pick['ok']); selected=pick['result']['reference']; self.assertTrue(selected)
        fillet=self.cli(dict(op='model.fillet',document=doc,revision=1,edge=selected,radius=1))
        self.assertTrue(fillet['ok']); self.assertTrue(fillet['result']['data']['valid'])
        self.assertAlmostEqual(fillet['result']['data']['volume'],997.853981819698,places=4)
        stale=self.tool(dict(op='render.pick',render_id=frame['render_id'],x=index%width,y=index//width))
        self.assertTrue(stale['isError']);self.assertEqual(stale['structuredContent']['error']['kind'],'stale_render')
        bad=self.tool(dict(op='model.fillet',document=doc,revision=2,edge=selected,radius=1))
        self.assertTrue(bad['isError']);self.assertEqual(bad['structuredContent']['error']['kind'],'invalid_reference')
        self.assertEqual(self.cli(dict(op='document.state',document=doc))['result']['revision'],2)
        self.assertTrue(self.tool(dict(op='document.undo',document=doc,revision=2))['structuredContent']['ok'])
        self.assertTrue(self.cli(dict(op='document.redo',document=doc,revision=3))['ok'])
        final=self.cli(dict(op='render.snapshot',document=doc))['result']
        self.assertEqual(final['revision'],4);self.rgb(final['images']['shaded'])
        path=str(Path(self.tmp.name)/'model.smb')
        self.assertTrue(self.tool(dict(op='export.native',document=doc,path=path))['structuredContent']['ok'])
        self.assertGreater(Path(path).stat().st_size,0)
        self.assertEqual(len(self.cli({'op':'document.list'})['result']),1)
    def test_protocol_errors_and_catalog(self):
        self.start_mcp()
        self.assertEqual(self.rpc('unknown')['error']['code'],-32601)
        self.assertEqual(self.rpc('tools/call',{'name':'missing','arguments':{}})['error']['code'],-32602)
        invalid=self.tool({'op':'model.box','document':'absent','revision':False,'size':[1,1,1]})
        self.assertTrue(invalid['isError']); self.assertEqual(invalid['structuredContent']['error']['kind'],'invalid_params')
        self.mcp.stdin.write('not-json\n');self.mcp.stdin.flush()
        self.assertEqual(json.loads(self.mcp.stdout.readline())['error']['code'],-32700)
        self.mcp.stdin.write('{"jsonrpc":"2.0","method":"tools/call","params":{"name":"cad_command","arguments":{"command":{"op":"document.create"}}}}\n'); self.mcp.stdin.flush()
        self.assertEqual(self.rpc('ping')['result'],{})
        self.assertEqual(self.cli({'op':'document.list'})['result'],[])
        catalog=self.cli({'op':'commands.list'})['result']
        self.assertTrue({'render.snapshot','render.pick','model.fillet','document.list'} <= {c['id'] for c in catalog})

    def test_render_cache_dimensions_isolation_and_legacy_migration(self):
        doc=self.cli({'op':'document.create'})['result']['document']
        empty=self.cli(dict(op='render.snapshot',document=doc))
        self.assertFalse(empty['ok']); self.assertEqual(empty['error']['kind'],'empty_document')
        self.assertTrue(self.cli(dict(op='model.box',document=doc,revision=0,size=[10,10,10]))['ok'])
        invalid=self.cli(dict(op='render.snapshot',document=doc,width=10000))
        self.assertFalse(invalid['ok']);self.assertEqual(invalid['error']['kind'],'invalid_params')
        frame=self.cli(dict(op='render.snapshot',document=doc,width=96,height=80,directory=self.tmp.name))['result']
        self.assertEqual(self.rgb(frame['images']['shaded'])[:2],(96,80))
        for mode,path in frame['files'].items(): self.assertEqual(Path(path).read_bytes(),base64.b64decode(frame['images'][mode]))
        self.assertEqual(self.cli(dict(op='render.pick',render_id=frame['render_id'],x=96,y=0))['error']['kind'],'invalid_params')
        isolated=self.cli({'op':'service.session.create','key':'isolated-test'})['result']['session_id']
        self.assertEqual(self.cli({'op':'document.list'},['--session',isolated])['result'],[])
        foreign=self.cli(dict(op='render.pick',render_id=frame['render_id'],x=0,y=0),['--session',isolated])
        self.assertEqual(foreign['error']['kind'],'render_not_found')
        for _ in range(4): self.assertTrue(self.cli(dict(op='render.snapshot',document=doc,width=64,height=64))['ok'])
        evicted=self.cli(dict(op='render.pick',render_id=frame['render_id'],x=0,y=0))
        self.assertEqual(evicted['error']['kind'],'render_not_found')
        self.assertEqual(self.cli(dict(op='document.state',document=doc))['result']['revision'],1)
        for path in ['modules/cad-cli/main.py','modules/cad-mcp/server.py','examples/box_fillet.py']:
            run=subprocess.run([__import__('sys').executable,str(ROOT/path)],capture_output=True,text=True)
            self.assertEqual(run.returncode,2);self.assertEqual(run.stdout,'');self.assertIn('Rust',run.stderr)
    def test_cli_jsonl_continues_after_errors_and_one_shot_exit_status(self):
        run=subprocess.run([str(BIN/'cad-cli'),*self.flags],input='bad-json\n{"op":"capabilities","id":"echo"}\n',capture_output=True,text=True,env=self.env,timeout=20)
        results=[json.loads(line) for line in run.stdout.splitlines()]
        self.assertEqual(len(results),2);self.assertFalse(results[0]['ok']);self.assertTrue(results[1]['ok']);self.assertEqual(results[1]['id'],'echo')
        bad=subprocess.run([str(BIN/'cad-cli'),*self.flags,'--command','{"op":"does.not.exist"}'],capture_output=True,text=True,env=self.env)
        self.assertEqual(bad.returncode,1);self.assertEqual(json.loads(bad.stdout)['error']['kind'],'unknown_command')
        help=subprocess.run([str(BIN/'cad-cli'),'--help'],capture_output=True,text=True,env=self.env)
        self.assertEqual(help.returncode,0);self.assertIn('--runtime-dir',help.stdout)

    def test_mcp_negotiation_and_readiness_are_validated(self):
        for version,expected in [('2025-06-18','2025-06-18'),('unknown-version','2025-11-25')]:
            self.mcp=subprocess.Popen([str(BIN/'cad-mcp'),*self.flags],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,env=self.env)
            self.assertEqual(self.rpc('initialize',{})['error']['code'],-32602)
            params=dict(protocolVersion=version,capabilities={},clientInfo={'name':'test','version':'1'})
            self.assertEqual(self.rpc('initialize',params)['result']['protocolVersion'],expected)
            self.mcp.stdin.write('{"jsonrpc":"2.0","method":"notifications/initialized","params":[]}\n');self.mcp.stdin.flush()
            self.assertEqual(self.rpc('tools/list')['error']['code'],-32002)
            self.mcp.stdin.write('{"jsonrpc":"2.0","method":"notifications/initialized"}\n');self.mcp.stdin.flush()
            self.assertIn('tools',self.rpc('tools/list')['result'])
            self.assertEqual(self.rpc('initialize',params)['error']['code'],-32600)
            self.assertIsNone(self.rpc('ping',id=1.5)['id'])
            self.mcp.stdin.close();self.mcp.wait(timeout=10);self.mcp.stdout.close();self.mcp.stderr.close();self.mcp=None
        self.assertFalse((self.runtime/'endpoint.json').exists()) # Negotiation never launches a CAD worker.

if __name__=='__main__': unittest.main()
