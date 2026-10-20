"""Real SMLib feature graph, immutable history, private transaction and recovery acceptance."""
import json, subprocess, sys, tempfile, pathlib, os, math
binary=str(pathlib.Path(sys.argv[1]).resolve())
with tempfile.TemporaryDirectory(prefix='miocad-features-') as tmp:
 store=pathlib.Path(tmp)/'store';env={**os.environ,'CAD_DOCUMENT_STORE':str(store)}
 def start():return subprocess.Popen([binary],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True,env=env)
 p=start()
 def raw(**cmd):
  p.stdin.write(json.dumps(cmd)+'\n');p.stdin.flush();line=p.stdout.readline();assert line,'worker exited';return json.loads(line)
 def call(op,**kw):
  r=raw(op=op,**kw);assert r['ok'],r;return r['result']
 doc=call('document.create')['document']
 def scene(d=doc):return call('render.scene',document=d)
 def write(op,d=doc,**kw):return call(op,document=d,revision=scene(d)['revision'],**kw)
 write('model.box',size=[10,10,10]);s=scene();edge=s['data']['edges'][0];a,b=edge['points'][0],edge['points'][-1];axis=max(range(3),key=lambda i:abs(a[i]-b[i]))
 write('model.fillet',edge=edge['reference'],radius=1)
 # Pick a distinct parallel, straight edge for the second Fillet.
 s=scene();candidates=[]
 for e in s['data']['edges']:
  pts=e['points']
  if len(pts)>=2 and abs(pts[0][axis]-pts[-1][axis])>5 and all(abs(q[k]-pts[0][k])<1e-7 for q in pts for k in range(3) if k!=axis):candidates.append(e)
 assert candidates
 write('model.fillet',edge=candidates[0]['reference'],radius=1)
 old=scene();ids=[f['id'] for f in old['data']['features']];assert len(ids)==3
 write('feature.edit',feature=ids[0],patch={'size':[20,10,10]})
 resized=scene();assert resized['data']['state']['valid'];assert resized['data']['state']['volume']>old['data']['state']['volume']+900
 assert [f['id'] for f in resized['data']['features']]==ids
 write('feature.edit',feature=ids[1],patch={'radius':2});edited=scene();assert edited['data']['state']['volume']<resized['data']['state']['volume']
 before=scene();failed=raw(op='feature.edit',document=doc,revision=before['revision'],feature=ids[1],patch={'radius':10000});assert not failed['ok'];assert failed['error']['kind']=='recompute_failed',failed;assert scene()==before
 write('document.undo');undone=scene();assert undone['data']['features'][1]['parameters']['radius']==1;assert undone['data']['state']['volume']==resized['data']['state']['volume']
 # A failed edit must not destroy the redo branch.
 r=raw(op='feature.edit',document=doc,revision=undone['revision'],feature='missing',patch={'radius':2});assert not r['ok'];assert scene()==undone
 write('document.redo');assert scene()['data']['state']['volume']==edited['data']['state']['volume']
 write('feature.edit',feature=ids[1],patch={'name':'Main radius'});assert scene()['data']['features'][1]['name']=='Main radius'
 write('feature.edit',feature=ids[1],patch={'suppressed':True});assert scene()['data']['features'][1]['status']=='suppressed'
 write('feature.edit',feature=ids[1],patch={'suppressed':False});assert scene()['data']['state']['valid']
 # Real private branch: two model operations, no parent mutations until one commit.
 parent=scene();tx=write('transaction.begin')['data']['transaction'];ts=scene(tx)
 write('model.box',d=tx,size=[8,8,8]);te=scene(tx)['data']['edges'][0]['reference'];write('model.fillet',d=tx,edge=te,radius=1)
 assert scene()==parent;assert len(call('document.list'))==1
 write('transaction.commit',transaction=tx,label='Two-step Agent task');committed=scene();assert committed['revision']==parent['revision']+1
 assert committed['data']['timeline']['entries'][-1]['label']=='Two-step Agent task'
 write('document.undo');assert scene()['data']['features']==parent['data']['features'];assert scene()['data']['state']['volume']==parent['data']['state']['volume']
 write('document.redo');assert scene()['data']['state']['volume']==committed['data']['state']['volume']
 tx=write('transaction.begin')['data']['transaction'];write('feature.edit',feature=scene()['data']['features'][0]['id'],patch={'name':'Concurrent name'})
 before=scene();r=raw(op='transaction.commit',document=doc,revision=before['revision'],transaction=tx,label='must conflict');assert not r['ok'] and r['error']['kind']=='transaction_conflict';assert scene()==before
 call('transaction.abort',document=doc,transaction=tx)
 # Persist the cursor in the middle of history and restore native shapes without recompute.
 write('document.undo');before=scene();project=str(pathlib.Path(tmp)/'part.miocad');write('document.save',path=project)
 p.stdin.close();assert p.wait()==0;p=start()
 def equivalent(a,b):
  if isinstance(a,(int,float)) and not isinstance(a,bool) and isinstance(b,(int,float)):return math.isclose(a,b,rel_tol=1e-10,abs_tol=1e-10)
  if type(a)!=type(b):return False
  if isinstance(a,dict):return a.keys()==b.keys() and all(equivalent(v,b[k]) for k,v in a.items())
  if isinstance(a,list):return len(a)==len(b) and all(equivalent(x,y) for x,y in zip(a,b))
  return a==b
 assert equivalent(scene(),before)
 write('document.redo');assert scene()['data']['state']['valid']
 opened=call('document.open',path=project)['document'];assert opened!=doc;assert equivalent(json.loads(json.dumps(scene(opened)['data']).replace(opened,doc)),before['data'])
 # Failed persistence does not publish an in-memory modification or lose redo.
 # Rename the store directory away and replace it with a file to inject I/O failure.
 backup=pathlib.Path(tmp)/'backup';store.rename(backup);store.write_text('not a directory');before=scene();r=raw(op='feature.edit',document=doc,revision=before['revision'],feature=scene()['data']['features'][0]['id'],patch={'name':'must not publish'});assert not r['ok'];assert scene()==before
 store.unlink();backup.rename(store)
 p.stdin.close();assert p.wait()==0
 # Recovery refuses missing geometry rather than starting with empty documents.
 manifest=(store/'workspace.json').read_bytes();shape=next(store.glob('*.smb'));missing=shape.with_suffix('.hidden');shape.rename(missing)
 broken=start();assert broken.wait(timeout=5)!=0;assert (store/'workspace.json').read_bytes()==manifest;missing.rename(shape)
 p=start();assert len(call('document.list'))==2;p.stdin.close();assert p.wait()==0
 print('PASS: Box + two Fillets, upstream resize, radius edit, suppression, rename, failure rollback, undo/redo, atomic tasks, conflicts, native restart recovery, project open, I/O rollback')
