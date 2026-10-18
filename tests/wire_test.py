"""Versioned wire contracts against the real C++ worker (legacy tests stay separate)."""
import json
import subprocess
import sys
import unittest
SERVER=sys.argv.pop() if len(sys.argv)>1 else 'build/cad-server'
class WireTest(unittest.TestCase):
    def setUp(self):
        self.p=subprocess.Popen([SERVER],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
        self.addCleanup(self.close_worker)
        self.n=0
        self.hello=self.raw(dict(protocol='1.0',method='hello',request_id=0,client_id='wire-test'))
        self.assertTrue(self.hello['ok'],self.hello)
    def close_worker(self):
        self.p.stdin.close(); self.p.wait(timeout=10); self.p.stdout.close()
    def raw(self,r):
        self.p.stdin.write(json.dumps(r)+'\n');self.p.stdin.flush()
        return json.loads(self.p.stdout.readline())
    def request(self,command):
        self.n+=1
        return dict(protocol='1.0',method='execute',client_id='wire-test',request_id=self.n,
                    worker_id=self.hello['worker_id'],session_id=self.hello['session_id'],command=command)
    def call(self,command):
        response=self.raw(self.request(command)); self.assertTrue(response['ok'],response);return response
    def test_workflow_events_and_revision(self):
        created=self.call(dict(op='document.create'))
        doc=created['result']['document']
        self.assertEqual(created['events'][0]['kind'],'document.created')
        box=self.call(dict(op='model.box',document=doc,revision=0,size=[10,10,10]))
        self.assertEqual(box['events'][0]['revision'],1)
        edges=self.call(dict(op='topology.edges',document=doc))
        self.assertEqual(edges['events'],[])
        fillet=self.call(dict(op='model.fillet',document=doc,revision=1,edge=edges['result']['data'][0]['reference'],radius=1))
        self.assertTrue(fillet['result']['data']['valid'])
        self.assertAlmostEqual(fillet['result']['data']['volume'],997.8539816339745,places=4)
        state=self.call(dict(op='document.state',document=doc))
        self.assertEqual(state['result']['revision'],2)
    def test_dedup_conflict_status_and_eviction(self):
        request=self.request(dict(op='document.create'))
        first=self.raw(request); second=self.raw(request)
        self.assertTrue(second['replayed'])
        self.assertEqual(first['result'],second['result'])
        self.assertEqual(first['events'],second['events'])
        altered=dict(request,command=dict(op='capabilities'))
        self.assertEqual(self.raw(altered)['error']['kind'],'request_conflict')
        status=dict(self.request(dict(op='capabilities')),method='request.status',target_request_id=1)
        status.pop('command')
        result=self.raw(status)
        self.assertEqual(result['result']['status'],'completed')
        self.assertEqual(result['result']['response']['result'],first['result'])
        for _ in range(66): self.call(dict(op='capabilities'))
        self.assertEqual(self.raw(request)['error']['kind'],'outcome_unavailable')
        resumed=self.raw(dict(protocol='1.0',method='hello',request_id=0,client_id='wire-test'))
        self.assertGreater(resumed['result']['last_request_id'],1)
        self.assertEqual(self.raw(request)['error']['kind'],'outcome_unavailable')
    def test_invalid_context_and_version(self):
        req=self.request(dict(op='document.create'))
        for change,kind in [({'protocol':'999.0'},'version_mismatch'),({'worker_id':'other'},'wrong_instance'),
                            ({'session_id':'other'},'wrong_session'),({'client_id':'unregistered'},'client_not_initialized')]:
            self.assertEqual(self.raw(dict(req,**change))['error']['kind'],kind)
        good=self.raw(req);self.assertTrue(good['ok'],good)
        self.assertEqual(good['request_id'],req['request_id'])
    def test_invalid_inputs_do_not_change_document(self):
        doc=self.call(dict(op='document.create'))['result']['document']
        self.call(dict(op='model.box',document=doc,revision=0,size=[10,10,10]))
        before=self.call(dict(op='document.state',document=doc))['result']
        for command,kind in [(dict(op='model.box',document=doc,revision=True,size=[2,3,4]),'invalid_params'),
                             (dict(op='model.box',document=doc,revision=0,size=[2,3,4]),'revision_conflict'),
                             (dict(op='model.box',document=doc,revision=1,size=[1e309,2,3]),'invalid_request')]:
            result=self.raw(self.request(command));self.assertFalse(result['ok'])
            self.assertEqual(result['error']['kind'],kind)
        self.assertEqual(self.call(dict(op='document.state',document=doc))['result'],before)
if __name__=='__main__':unittest.main()
