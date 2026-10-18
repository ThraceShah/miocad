#!/usr/bin/env python3
"""Internal representative Box/Fillet headless render baseline; no performance promises."""
import json,os,statistics,subprocess,tempfile,time
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];BIN=Path(os.environ.get('CAD_BIN',ROOT/'target/release'));OUT=ROOT/'artifacts/p7';OUT.mkdir(parents=True,exist_ok=True)
with tempfile.TemporaryDirectory(prefix='p7-benchmark-') as runtime:
    service=subprocess.Popen([str(BIN/'cad-service'),'--runtime-dir',runtime,'--worker',str(ROOT/'build/cad-server')],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    def cli(command):
        start=time.perf_counter();result=json.loads(subprocess.check_output([str(BIN/'cad-cli'),'--runtime-dir',runtime,'--client-id','benchmark','--command',json.dumps(command)],text=True));assert result['ok'],result;return result['result'],(time.perf_counter()-start)*1000
    try:
        for _ in range(100):
            if (Path(runtime)/'endpoint.json').exists():break
            time.sleep(.02)
        doc=cli({'op':'document.create'})[0]['document'];cli({'op':'model.box','document':doc,'revision':0,'size':[10,10,10]});rows=[]
        for name,revision in [('box',1),('fillet',2)]:
            if name=='fillet':
                edge=cli({'op':'topology.edges','document':doc})[0]['data'][0]['reference'];cli({'op':'model.fillet','document':doc,'revision':1,'edge':edge,'radius':1})
            mesh=cli({'op':'render.mesh','document':doc})[0]['data']
            for size in [384,640,768]:
                for backend in ['gpu','cpu']:
                    samples=[];renders=[]
                    for _ in range(3):
                        result,total=cli({'op':'render.snapshot','document':doc,'revision':revision,'width':size,'height':size,'camera':{'yaw':.7853981633974483,'pitch':.6154797086703875,'zoom':1.,'pan':[0.,0.]},'backend':backend});samples.append(total);renders.append(result['timing_ms'])
                    rows.append({'model':name,'triangles':len(mesh['triangles']),'size':size,'backend':result['backend'],'adapter':result.get('adapter'),'roundtrip_ms':samples,'renderer_ms':renders,'median_roundtrip_ms':statistics.median(samples),'scene_cache_hit':result['scene_cache_hit'],'geometry_cache_hit':result.get('geometry_cache_hit'),'png_base64_bytes':sum(len(v) for v in result['images'].values())})
        (OUT/'performance.json').write_text(json.dumps({'profile':'release','samples_per_case':3,'rows':rows},indent=2));print(json.dumps([{'model':r['model'],'size':r['size'],'backend':r['backend'],'median_ms':round(r['median_roundtrip_ms'],2),'triangles':r['triangles']} for r in rows]))
    finally:
        try:cli({'op':'service.shutdown'})
        except Exception:pass
        try:service.wait(timeout=3)
        except subprocess.TimeoutExpired:service.kill();service.wait()
