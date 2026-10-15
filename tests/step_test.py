"""STEP acceptance: real CAD exchange, transaction rollback, unit normalization."""
import json
import math
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SERVER = sys.argv[1]
FIXTURES = sys.argv[2]
del sys.argv[1:]
class StepTest(unittest.TestCase):
    def setUp(self):
        self.p=subprocess.Popen([SERVER,'--kernel','occt'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
        self.temp=tempfile.TemporaryDirectory()
        self.root=Path(self.temp.name)
        self.doc=self.call('document.create')['document']
    def tearDown(self):
        self.p.stdin.close(); self.p.wait(timeout=10); self.p.stdout.close()
        self.temp.cleanup()
        self.assertEqual(self.p.returncode,0)
    def raw(self,op,**args):
        self.p.stdin.write(json.dumps(dict(op=op,**args))+'\n'); self.p.stdin.flush()
        return json.loads(self.p.stdout.readline())
    def call(self,op,**args):
        response=self.raw(op,**args)
        self.assertTrue(response['ok'],response)
        return response['result']
    def box(self,size=(10,10,10)):
        return self.call('model.box',document=self.doc,revision=0,size=size)
    def state(self): return self.call('document.state',document=self.doc)
    def compare(self,a,b):
        self.assertTrue(b['valid'])
        self.assertAlmostEqual(a['volume'],b['volume'],delta=max(1e-6,abs(a['volume'])*1e-7))
        for name in ('min','max'):
            for x,y in zip(a['bounds'][name],b['bounds'][name]): self.assertAlmostEqual(x,y,delta=1e-5)
        self.assertEqual(a['faces'],b['faces'])
        self.assertEqual(a['edges'],b['edges'])
    def test_box_and_fillet_roundtrip(self):
        caps=self.call('capabilities')
        self.assertEqual(caps['kernel'],'occt')
        self.assertTrue(caps['step_import'] and caps['step_export'])
        self.box()
        for fixture in ('box','fillet'):
            if fixture=='fillet':
                edge=self.call('topology.edges',document=self.doc)['data'][0]['reference']
                self.call('model.fillet',document=self.doc,revision=1,edge=edge,radius=1)
            before=self.state()
            path=str(self.root/(fixture+'.step'))
            self.call('export.step',document=self.doc,path=path)
            text=Path(path).read_text()
            self.assertTrue(text.startswith('ISO-10303-21;'))
            self.assertIn('MANIFOLD_SOLID_BREP',text)
            self.assertNotIn('TRIANGULATED_FACE_SET',text)
            self.assertEqual(before,self.state())
            new=self.call('document.create')['document']
            loaded=self.call('import.step',document=new,revision=0,path=path)
            self.compare(before['data'],loaded['data'])
            # A transferred STEP body must remain editable, with fresh refs.
            self.assertGreater(len(self.call('render.mesh',document=new)['data']['triangles']),0)
            self.call('document.undo',document=new,revision=1)
            self.assertTrue(self.call('document.state',document=new)['data']['empty'])
            self.call('document.redo',document=new,revision=2)
            self.compare(before['data'],self.call('document.state',document=new)['data'])
    def test_import_failure_and_export_failure_are_atomic(self):
        self.box()
        valid=str(self.root/'valid.step')
        self.call('export.step',document=self.doc,path=valid)
        self.call('model.box',document=self.doc,revision=1,size=[2,3,4])
        self.call('document.undo',document=self.doc,revision=2)
        before=self.state()
        corrupt=self.root/'bad.step'; corrupt.write_text('not STEP')
        empty=self.root/'empty.step'; empty.write_text('ISO-10303-21;\nHEADER;ENDSEC;\nDATA;ENDSEC;\nEND-ISO-10303-21;')
        for path,revision in [(str(corrupt),3),(str(empty),3),(str(self.root/'missing.step'),3),(valid,1)]:
            self.assertFalse(self.raw('import.step',document=self.doc,revision=revision,path=path)['ok'])
            self.assertEqual(before,self.state())
        dest=self.root/'keep.txt'; dest.write_text('keep')
        self.assertFalse(self.raw('export.step',document=self.doc,path=str(dest))['ok'])
        self.assertEqual(dest.read_text(),'keep')
        self.assertFalse(self.raw('export.step',document=self.doc,path=str(self.root/'missing'/'out.step'))['ok'])
        self.assertFalse(list(self.root.glob('*.tmp-*')))
        self.assertEqual(before,self.state())
        self.call('document.redo',document=self.doc,revision=3)
        self.assertAlmostEqual(self.state()['data']['volume'],24)
    def test_successful_import_truncates_redo_and_invalidates_references(self):
        self.box((2,3,4))
        path=str(self.root/'small.stp')
        self.call('export.step',document=self.doc,path=path)
        old=self.call('topology.edges',document=self.doc)['data'][0]['reference']
        self.call('model.box',document=self.doc,revision=1,size=[10,10,10])
        self.call('document.undo',document=self.doc,revision=2)
        self.call('import.step',document=self.doc,revision=3,path=path)
        self.assertFalse(self.raw('document.redo',document=self.doc,revision=4)['ok'])
        self.assertFalse(self.raw('model.fillet',document=self.doc,revision=4,edge=old,radius=.2)['ok'])
        edge=self.call('topology.edges',document=self.doc)['data'][0]['reference']
        result=self.call('model.fillet',document=self.doc,revision=4,edge=edge,radius=.2)
        self.assertTrue(result['data']['valid'])
        self.assertLess(result['data']['volume'],24)
    def test_periodic_and_nurbs_fixture_roundtrips(self):
        subprocess.run([FIXTURES,str(self.root)],check=True,stdout=subprocess.DEVNULL)
        expected={'cylinder':20*math.pi,'sphere':32*math.pi/3,'torus':10*math.pi**2,'nurbs':20*math.pi}
        for name,volume in expected.items():
            with self.subTest(fixture=name):
                doc=self.call('document.create')['document']
                loaded=self.call('import.step',document=doc,revision=0,path=str(self.root/(name+'.step')))
                self.assertAlmostEqual(loaded['data']['volume'],volume,delta=volume*1e-7)
                edges=self.call('topology.edges',document=doc)['data']
                self.assertGreater(len(edges),0)
                self.assertGreater(len(self.call('render.mesh',document=doc)['data']['triangles']),0)
                # Read-only tessellation must not mutate the immutable history body.
                self.assertEqual(loaded,self.call('document.state',document=doc))
                out=str(self.root/(name+'-out.step'))
                self.call('export.step',document=doc,path=out)
                other=self.call('document.create')['document']
                again=self.call('import.step',document=other,revision=0,path=out)
                self.compare(loaded['data'],again['data'])
    def test_unsupported_components_do_not_partially_import(self):
        subprocess.run([FIXTURES,str(self.root)],check=True,stdout=subprocess.DEVNULL)
        self.box()
        before=self.state()
        for name in ('sheet','multiple','mixed'):
            with self.subTest(fixture=name):
                result=self.raw('import.step',document=self.doc,revision=1,path=str(self.root/(name+'.step')))
                self.assertFalse(result['ok'],result)
                self.assertEqual(before,self.state())

    def test_step_length_units_are_normalized_to_mm(self):
        self.box((2,3,4))
        path=self.root/'unit.step'
        self.call('export.step',document=self.doc,path=str(path))
        text=path.read_text()
        self.assertIn('SI_UNIT(.MILLI.,.METRE.)',text)
        # Same coordinates, changed STEP unit: 2x3x4 metres => 2000x3000x4000 mm.
        path.write_text(text.replace('SI_UNIT(.MILLI.,.METRE.)','SI_UNIT($,.METRE.)'))
        doc=self.call('document.create')['document']
        state=self.call('import.step',document=doc,revision=0,path=str(path))['data']
        self.assertTrue(state['valid'])
        self.assertAlmostEqual(state['volume']/1e9,24,places=6)
        for x,y in zip(state['bounds']['max'],[2000,3000,4000]): self.assertAlmostEqual(x,y,places=4)

if __name__=='__main__': unittest.main()
