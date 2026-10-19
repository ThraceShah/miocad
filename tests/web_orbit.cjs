// Read-only touch orbit regression: rendering must advance even while /api/view is held.
const {chromium}=require(process.env.CAD_PLAYWRIGHT||'playwright');
const driver=require('./ui_driver.cjs');
const assert=require('assert/strict'),fs=require('fs'),{spawnSync}=require('child_process');
const url=process.env.CAD_WEB_URL||'https://example.ts.net/apps/miocad/';
const out=process.env.CAD_WEB_TEST_OUTPUT||'artifacts/orbit';fs.mkdirSync(out,{recursive:true});
(async()=>{
 const webgl=process.env.CAD_TEST_GPU==='webgl';
 const browser=await chromium.launch({headless:true,args:webgl?['--use-angle=swiftshader','--enable-unsafe-swiftshader','--disable-webgpu']:['--use-angle=vulkan','--enable-features=Vulkan','--disable-vulkan-surface','--enable-unsafe-webgpu']});
 try {
  const context=await browser.newContext({viewport:{width:1280,height:820},hasTouch:true,...(process.env.CAD_TEST_PROXY?{proxy:{server:process.env.CAD_TEST_PROXY,bypass:new URL(url).hostname}}:{})});
  const page=await context.newPage(),errors=[];await page.addInitScript(()=>{window.cadTestSubmits=0;if(window.GPUQueue){const submit=GPUQueue.prototype.submit;GPUQueue.prototype.submit=function(...args){window.cadTestSubmits++;return submit.apply(this,args);};}});page.on('pageerror',e=>errors.push(e.message));
  const initial=page.waitForResponse(r=>r.url().endsWith('/api/view')&&r.ok());
  await page.goto(url);await page.locator('#workspace').waitFor({state:'visible'});await initial;
  await page.waitForTimeout(500);
  const files=[];async function shot(name){const path=`${out}/${name}.png`;await page.screenshot({path});files.push(path);}
  const viewport=await driver.view(page);await shot('before');
  let release;const held=new Promise(r=>release=r);let requests=0;
  await page.route('**/api/view',async route=>{requests++;await held;await route.continue().catch(()=>{});});
  await page.evaluate(()=>{window.orbitTimings=[];window.cadTestSubmits=0;window.orbitStarted=performance.now();window.orbitRunning=true;let previous=performance.now();function tick(now){window.orbitTimings.push(now-previous);previous=now;if(window.orbitRunning)requestAnimationFrame(tick);}requestAnimationFrame(tick);});
  const cdp=await context.newCDPSession(page);
  await cdp.send('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{x:580,y:400}]});
  for(let i=1;i<=48;i++){
   await cdp.send('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:[{x:580+180*Math.sin(i/22),y:400+90*Math.sin(i/15)}]});
   await page.waitForTimeout(16);
   if(i%12===0)await shot(`touch-${i}`);
  }
  await cdp.send('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});
  await page.waitForTimeout(250);await shot('held-after');
  const telemetry=await page.evaluate(()=>{window.orbitRunning=false;return {timings:window.orbitTimings,submits:window.cadTestSubmits,duration:performance.now()-window.orbitStarted};});const timings=telemetry.timings;
  if(!webgl)assert(telemetry.submits>20,`only ${telemetry.submits} client GPU frame submissions`);
  // No authoritative viewport frame was available throughout the gesture.
  assert(requests<=1,`queued ${requests} remote frames during continuous gesture`);
  const analysis=spawnSync('python3',['-c',`
import sys,json,hashlib
from PIL import Image
results=[]
for file in sys.argv[5:]:
    image=Image.open(file).convert('RGB').crop((int(sys.argv[1]),int(sys.argv[2]),int(sys.argv[1])+int(sys.argv[3]),int(sys.argv[2])+int(sys.argv[4])))
    blue=sum(1 for r,g,b in image.getdata() if b>r*1.15 and g>r*1.1 and b>65 and r>25)
    results.append(dict(file=file,blue_pixels=blue,hash=hashlib.sha256(image.tobytes()).hexdigest()))
print(json.dumps(results))
`,String(Math.round(viewport.x)),String(Math.round(viewport.y)),String(Math.round(viewport.w)),String(Math.round(viewport.h)),...files],{encoding:'utf8'});assert.equal(analysis.status,0,analysis.stderr);
  const samples=JSON.parse(analysis.stdout);for(const sample of samples)assert(sample.blue_pixels>5000,`blank viewport: ${sample.file}`);
  assert(new Set(samples.map(s=>s.hash)).size>=5,'camera did not produce distinct local frames');
  release();await page.unroute('**/api/view');await page.waitForTimeout(600);
  assert.deepEqual(errors,[]);
  timings.sort((a,b)=>a-b);const result={passed:true,url,gpu_path:webgl?'webgl2':'webgpu',client_gpu_submissions:telemetry.submits,gesture_ms:telemetry.duration,touch_steps:48,remote_frames_during_gesture:requests,all_samples_have_geometry:true,distinct_images:new Set(samples.map(s=>s.hash)).size,raf_median_ms:timings[Math.floor(timings.length*.5)],raf_p95_ms:timings[Math.floor(timings.length*.95)],samples,errors};
  fs.writeFileSync(out+'/results.json',JSON.stringify(result,null,2));console.log(JSON.stringify(result));
 } finally {await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
