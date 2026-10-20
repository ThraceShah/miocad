"""Non-destructive legacy owner + persistent current worker behind one relay/session.
Requires CAD_LEGACY_SERVER pointing at a pre-feature worker; never touches live services.
"""
import json,os,pathlib,signal,socket,subprocess,tempfile,time,sys
root=pathlib.Path(__file__).resolve().parents[1];legacy=os.environ.get('CAD_LEGACY_SERVER')
if not legacy:print('SKIP legacy binary not provided');sys.exit(77)
service=str(root/'target/debug/cad-service');cli=str(root/'target/debug/cad-cli')
with tempfile.TemporaryDirectory(prefix='miocad-hybrid-') as tmp:
 runtime=pathlib.Path(tmp);env={**os.environ,'CAD_CURRENT_SERVER':str(root/'build/cad-server')};owner=None;relay=None
 def until(fn):
  end=time.monotonic()+20
  while not fn():assert time.monotonic()<end;time.sleep(.05)
 def command(op,**kw):
  r=subprocess.run([cli,'--runtime-dir',tmp,'--client-id','hybrid-fixture'],input=json.dumps({'op':op,**kw})+'\n',text=True,capture_output=True,timeout=20);assert r.returncode==0,r.stderr
  v=json.loads(r.stdout);assert v['ok'],v;return v['result']
 def control(endpoint,request):
  host,port=endpoint['address'].rsplit(':',1)
  with socket.create_connection((host,int(port)),timeout=10) as s:
   s.sendall((json.dumps({**{k:endpoint[k] for k in ['protocol','service_id','token']},'request':request})+'\n').encode());r=json.loads(s.makefile('rb').readline());assert 'result' in r,r;return r['result']
 def launch_relay():
  before=json.loads((runtime/'endpoint.json').read_text())['address']
  proc=subprocess.Popen([service,'--relay-runtime-dir',tmp],env=env,stdout=subprocess.DEVNULL,stderr=open(runtime/'relay.log','a'))
  until(lambda:proc.poll() is not None or json.loads((runtime/'endpoint.json').read_text())['address']!=before);assert proc.poll() is None,(runtime/'relay.log').read_text();return proc
 try:
  owner=subprocess.Popen([service,'--runtime-dir',tmp,'--worker',legacy],env=env,stdout=subprocess.DEVNULL,stderr=open(runtime/'owner.log','w'));until(lambda:(runtime/'endpoint.json').exists())
  original=json.loads((runtime/'endpoint.json').read_text());session=control(original,{'method':'ping'})['default_session'];doc=command('document.create')['document'];command('model.box',document=doc,revision=0,size=[10,10,10]);command('model.box',document=doc,revision=1,size=[8,8,8]);command('document.undo',document=doc,revision=2)
  baseline=control(original,{'method':'snapshot','session':session});assert baseline['documents'][0]['revision']==3
  relay=launch_relay();docs=command('service.snapshot')['documents'];assert len(docs)==1 and docs[0]['document']==doc
  assert 'features' not in command('render.scene',document=doc)['data']
  new=command('document.create')['document'];command('model.box',document=new,revision=0,size=[12,10,10]);features=command('feature.list',document=new)['data'];command('feature.edit',document=new,revision=1,feature=features[0]['id'],patch={'size':[20,10,10]});assert command('document.state',document=new)['data']['volume']==2000
  assert control(original,{'method':'snapshot','session':session})==baseline,'legacy owner was modified'
  events=command('service.events',after=0);assert any(e['document']==new for e in events['events']);assert any(e['document']==doc for e in events['events'])
  relay.send_signal(signal.SIGTERM);assert relay.wait(timeout=15)==0;relay=None;assert owner.poll() is None
  relay=launch_relay();snap=command('service.snapshot');assert len(snap['documents'])==2;assert command('document.state',document=new)['revision']==2
  command('document.undo',document=new,revision=2);assert command('document.state',document=new)['data']['volume']==1200
  command('document.redo',document=new,revision=3);assert command('document.state',document=new)['data']['volume']==2000
  command('document.redo',document=doc,revision=3);assert abs(command('document.state',document=doc)['data']['volume']-512)<1e-6
  assert command('document.state',document=new)['data']['volume']==2000
  print('PASS: same session, legacy undo retained, new feature editing, merged documents/events, relay restart restores new history, no owner restart')
 finally:
  for p in [relay,owner]:
   if p and p.poll() is None:p.send_signal(signal.SIGINT);p.wait(timeout=15)
