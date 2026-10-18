"""Integration contracts: failure atomicity, revisions, isolation and exact geometry."""
import json
import math
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SERVER = sys.argv.pop() if len(sys.argv)>1 else 'build/cad-server'
class ServiceTest(unittest.TestCase):
    def setUp(self):
        self.p=subprocess.Popen([SERVER],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
        self.doc=self.call('document.create')['document']
    def tearDown(self):
        self.p.stdin.close()
        self.p.wait(timeout=10)
        self.p.stdout.close()
        self.assertEqual(self.p.returncode,0)
    def raw(self, value):
        self.p.stdin.write(json.dumps(value)+'\n'); self.p.stdin.flush()
        return json.loads(self.p.stdout.readline())
    def call(self, op, **args):
        result=self.raw(dict(op=op,**args))
        self.assertTrue(result['ok'],result)
        return result['result']
    def box(self):
        return self.call('model.box',document=self.doc,revision=0,size=[10,10,10])
    def state(self):
        return self.call('document.state',document=self.doc)
    def test_box_fillet_history_and_isolation(self):
        self.box()
        other=self.call('document.create')['document']
        edge=self.call('topology.edges',document=self.doc)['data'][0]['reference']
        before=self.state()
        self.assertEqual(before['data']['volume'],1000)
        result=self.call('model.fillet',document=self.doc,revision=1,edge=edge,radius=1)
        self.assertAlmostEqual(result['data']['volume'],1000-10*(1-math.pi/4),places=4)
        self.assertTrue(result['data']['valid'])
        mesh=self.call('render.mesh',document=self.doc)['data']
        self.assertGreater(len(mesh['triangles']),12)
        for tri in mesh['triangles']:
            self.assertTrue(all(0<=i<len(mesh['points']) for i in tri['vertices']))
        undone=self.call('document.undo',document=self.doc,revision=2)
        self.assertEqual(undone['data']['volume'],1000)
        self.assertEqual(undone['revision'],3)
        stale=self.raw(dict(op='model.fillet',document=self.doc,revision=3,edge=edge,radius=1))
        self.assertFalse(stale['ok'])  # old token stays stale even after undo
        self.call('document.redo',document=self.doc,revision=3)
        self.assertTrue(self.call('document.state',document=other)['data']['empty'])
    def test_failures_preserve_state_and_redo(self):
        self.box()
        edge=self.call('topology.edges',document=self.doc)['data'][0]['reference']
        self.call('model.fillet',document=self.doc,revision=1,edge=edge,radius=1)
        self.call('document.undo',document=self.doc,revision=2)
        edge=self.call('topology.edges',document=self.doc)['data'][0]['reference']
        before=self.state()
        for args in [dict(op='model.fillet',edge=edge,radius=-1),
                     dict(op='model.fillet',edge=edge,radius=100),
                     dict(op='model.fillet',edge='foreign:edge:0',radius=1),
                     dict(op='model.box',size=[0,10,10]),
                     dict(op='model.box',size=[10,10])]:
            result=self.raw(dict(document=self.doc,revision=3,**args))
            self.assertFalse(result['ok'],result)
            self.assertEqual(before,self.state())
        self.call('document.redo',document=self.doc,revision=3)
        before=self.state()
        bad=self.raw(dict(op='document.undo',document=self.doc,revision=1))
        self.assertFalse(bad['ok'])
        self.assertEqual(before,self.state())
    def test_new_edit_truncates_redo_and_revision_aba(self):
        self.box()
        self.call('document.undo',document=self.doc,revision=1)
        self.call('model.box',document=self.doc,revision=2,size=[2,3,4])
        self.assertEqual(self.state()['data']['volume'],24)
        self.assertFalse(self.raw(dict(op='document.redo',document=self.doc,revision=3))['ok'])
    def test_discovery_and_transport_errors(self):
        before=self.state()
        response=self.raw(dict(op='commands.list',id='catalog-request'))
        self.assertTrue(response['ok'])
        self.assertEqual(response['id'],'catalog-request')
        catalog={item['id']:item for item in response['result']}
        self.assertEqual(catalog['model.fillet']['effect'],'document_write')
        self.assertIn('current_revision',catalog['model.fillet']['preconditions'])
        self.assertEqual(catalog['topology.edges']['effect'],'query')
        failed=self.raw(dict(op='missing-command',id=42))
        self.assertEqual(failed['id'],42)
        self.assertEqual(failed['error']['code'],'command_failed')
        self.assertEqual(failed['error']['kind'],'unknown_command')
        self.p.stdin.write('{bad json}\n'); self.p.stdin.flush()
        parsed=json.loads(self.p.stdout.readline())
        self.assertFalse(parsed['ok'])
        self.assertEqual(parsed['error']['kind'],'invalid_request')
        self.assertEqual(before,self.state())

    def test_protocol_and_exports(self):
        self.assertFalse(self.raw([])['ok'])
        self.assertFalse(self.raw(dict(op='document.state',document='absent'))['ok'])
        self.box()
        before=self.state()
        with tempfile.TemporaryDirectory() as tmp:
            path=str(Path(tmp)/'box.smb')
            self.call('export.native',document=self.doc,path=path)
            self.assertGreater(Path(path).stat().st_size,100)
            path=str(Path(tmp)/'box.txt')
            self.assertFalse(self.raw(dict(op='export.native',document=self.doc,path=path))['ok'])
        self.assertEqual(before,self.state())
        self.assertNotIn('step_export',self.call('capabilities'))

if __name__=='__main__': unittest.main()
