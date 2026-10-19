// Isolated, repeatable touch/edge acceptance. All geometry lives in a disposable daemon.
// CAD_PLAYWRIGHT=<playwright package> CAD_BIN=target/release node tests/web_interaction.cjs
// CAD_TEST_GPU=webgpu selects actual browser WebGPU; default exercises WebGL2 fallback.
const fs=require('fs'),os=require('os'),path=require('path'),net=require('net'),zlib=require('zlib');
const {spawn,spawnSync}=require('child_process'),assert=require('assert/strict');
const {chromium}=require(process.env.CAD_PLAYWRIGHT||'playwright');
const root=path.resolve(__dirname,'..'),bin=path.resolve(process.env.CAD_BIN||path.join(root,'target/debug'));
const gpu=process.env.CAD_TEST_GPU==='webgpu',dpr=Number(process.env.CAD_TEST_DPR||2);
const out=path.join(root,`artifacts/interaction-${gpu?'webgpu':'webgl'}-${dpr}`);fs.mkdirSync(out,{recursive:true});fs.rmSync(path.join(out,'failure.png'),{force:true});
const runtime=fs.mkdtempSync(path.join(os.tmpdir(),'cad-interaction-'));
const pause=ms=>new Promise(r=>setTimeout(r,ms));
function cli(command){const p=spawnSync(path.join(bin,'cad-cli'),['--runtime-dir',runtime,'--worker',path.join(root,'build/cad-server'),'--client-id','interaction-test'],{input:JSON.stringify(command)+'\n',encoding:'utf8',maxBuffer:16*1024*1024});assert.equal(p.status,0,p.stderr);const r=JSON.parse(p.stdout);assert(r.ok,JSON.stringify(r));return r.result;}
async function freePort(){const s=net.createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
const rect={x:218,y:110,w:780,h:672};
function samples(frame){const bytes=zlib.inflateSync(Buffer.from(frame.edges,'base64')),groups=new Map();for(let y=0;y<frame.height;y++)for(let x=0;x<frame.width;x++){const id=bytes.readUInt32LE((y*frame.width+x)*4);if(!id||id===0xffffffff)continue;if(!groups.has(id))groups.set(id,[]);groups.get(id).push([x+.5,y+.5]);}
 return [...groups].filter(([,p])=>p.length>30).map(([id,points])=>{const center=points.reduce((a,p)=>[a[0]+p[0]/points.length,a[1]+p[1]/points.length],[0,0]);let xx=0,xy=0,yy=0;for(const p of points){let x=(p[0]-center[0])*rect.w/frame.width,y=(p[1]-center[1])*rect.h/frame.height;xx+=x*x;xy+=x*y;yy+=y*y;}const angle=.5*Math.atan2(2*xy,xx-yy);const p=points.reduce((a,p)=>(p[0]-center[0])**2+(p[1]-center[1])**2<(a[0]-center[0])**2+(a[1]-center[1])**2?p:a);return {id,reference:frame.edge_refs[id],x:rect.x+p[0]/frame.width*rect.w,y:rect.y+p[1]/frame.height*rect.h,nx:-Math.sin(angle),ny:Math.cos(angle)};});}
(async()=>{let gateway,browser,releaseHeld=()=>{};try{
 const document=cli({op:'document.create'}).document;cli({op:'model.box',document,revision:0,size:[10,10,10]});const before=cli({op:'service.snapshot'});
 const port=await freePort(),origin=`http://127.0.0.1:${port}`,url=origin+'/apps/miocad/';
 const log=fs.openSync(path.join(out,'gateway.log'),'w');gateway=spawn(path.join(bin,'cad-web'),['--runtime-dir',runtime],{env:{...process.env,HOST:'127.0.0.1',PORT:String(port),BASE_PATH:'/apps/miocad/',CAD_WEB_ORIGIN:origin,CAD_WEB_ATTACH_ONLY:'1',CAD_WEB_TRUSTED_ACCESS:'1',CAD_WEB_STRIP_PREFIX:'0',CAD_WEB_ASSETS:path.join(root,'artifacts/web')},stdio:['ignore',log,log]});
 for(let i=0;;i++){try{if((await fetch(url+'health')).ok)break;}catch{}assert(i<100,'gateway startup timeout');await pause(50);}
 browser=await chromium.launch({headless:true,args:gpu?['--force-device-scale-factor='+dpr,'--use-angle=vulkan','--enable-features=Vulkan','--disable-vulkan-surface','--enable-unsafe-webgpu']:['--force-device-scale-factor='+dpr,'--use-angle=swiftshader','--enable-unsafe-swiftshader','--disable-webgpu']});
 const context=await browser.newContext({viewport:{width:1280,height:820},deviceScaleFactor:dpr,hasTouch:true});const page=await context.newPage(),errors=[],frames=[];
 page.on('pageerror',e=>errors.push(e.message));page.on('response',async r=>{if(r.url().endsWith('/api/view')&&r.ok()){try{frames.push(await r.json());}catch{}}});
 async function until(fn,label){const end=Date.now()+12000;while(!fn()){assert(Date.now()<end,`${label}; ${errors.join(';')}`);await pause(25);}}
 async function settled(after=-1){await until(()=>frames.some(f=>f.metadata.epoch>after&&!f.metadata.preview),'matching frame');await pause(90);return frames.filter(f=>!f.metadata.preview).at(-1);}
 async function tap(x,y){await page.touchscreen.tap(x,y);await pause(110);}
 async function pixels(name){const file=path.join(out,name+'.png');await page.screenshot({path:file});const p=spawnSync('python3',['-c',`from PIL import Image
import json,sys
im=Image.open(sys.argv[1]).convert('RGB');d=float(sys.argv[2]);im=im.crop((int(218*d),int(150*d),int(998*d),int(750*d))).resize((780,600));pixels=list(im.getdata());print(json.dumps(dict(amber=sum(r>210 and 110<g<220 and b<110 for r,g,b in pixels),cyan=sum(r<100 and g>175 and b>160 for r,g,b in pixels))))`,file,String(dpr)],{encoding:'utf8'});assert.equal(p.status,0,p.stderr);return JSON.parse(p.stdout);}
 async function preview(reference){const waiting=page.waitForResponse(r=>{try{return r.url().endsWith('/api/rpc')&&JSON.parse(r.request().postData()).command?.op==='model.preview';}catch{return false;}});await tap(1120,424);const r=await waiting;const command=JSON.parse(r.request().postData()).command;assert.equal(command.operation.edge,reference);assert((await r.json()).ok);await until(()=>frames.at(-1)?.metadata.preview,'candidate viewport');const e=frames.at(-1).metadata.epoch;await tap(1194,469);return settled(e);}
 await page.goto(url);await page.locator('#workspace').waitFor({state:'visible'});let frame=await settled();const initial=samples(frame);assert.equal(initial.length,9,'all nine visible cube edges must have usable IDs');fs.writeFileSync(path.join(out,'samples.json'),JSON.stringify(initial,null,2));
 // Mouse hover is thick cyan; touch selection is thick amber, and touch hit slop is 10 CSS px.
 await page.mouse.move(initial[0].x,initial[0].y);await pause(120);const hover=await pixels('hover');assert(hover.cyan>100,JSON.stringify(hover));await page.mouse.move(15,15);
 const selected=[];
 for(const [i,s] of initial.entries()){
  const e=frame.metadata.epoch;await tap(s.x+s.nx*10,s.y+s.ny*10);frame=await settled(e);
  const highlight=await pixels(`edge-${i}`);assert(highlight.amber>200,`touch selection not clearly visible for ${s.reference}: ${JSON.stringify(highlight)}`);
  frame=await preview(s.reference);selected.push({reference:s.reference,highlight});
 }
 // Direction contract is checked against actual outgoing UI cameras, not copied formulas.
 async function drag(dx,dy){const cdp=await context.newCDPSession(page);await cdp.send('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{x:620,y:420}]});for(let i=1;i<=8;i++){await cdp.send('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:[{x:620+dx*i/8,y:420+dy*i/8}]});await pause(20);}await cdp.send('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});await cdp.detach();}
 let e=frame.metadata.epoch,old=frame.metadata.camera;await drag(50,0);frame=await settled(e);assert(frame.metadata.camera.yaw>old.yaw,'right drag must increase yaw');e=frame.metadata.epoch;old=frame.metadata.camera;await drag(0,40);frame=await settled(e);assert(frame.metadata.camera.pitch>old.pitch,'down drag must increase pitch');
 const rotated=[];
 for(const [i,s] of samples(frame).slice(0,3).entries()) {const epoch=frame.metadata.epoch;await tap(s.x,s.y);frame=await settled(epoch);assert((await pixels(`rotated-edge-${i}`)).amber>200);frame=await preview(s.reference);rotated.push(s.reference);}
 // A tap made before the new ID frame arrives must be queued and resolved after arrival.
 await tap(250,160);frame=await settled(frame.metadata.epoch);assert.equal((await pixels('cleared')).amber,0);
 let heldFrame,release;const gate=new Promise(r=>{release=r;releaseHeld=r;});let first=true;
 await page.route('**/api/view',async route=>{if(!first){await route.continue();return;}first=false;const response=await route.fetch();heldFrame=await response.json();await gate;await route.fulfill({response});});
 await drag(-35,0);await until(()=>heldFrame,'held picking frame');let s=samples(heldFrame)[0];await tap(s.x,s.y);assert.equal((await pixels('queued-before-frame')).amber,0);const previous=frame.metadata.epoch;release();await until(()=>frames.some(f=>f.metadata.epoch>heldFrame.metadata.epoch),'queued selection resolved');await page.unroute('**/api/view');frame=await settled(previous);assert((await pixels('queued-resolved')).amber>200);await preview(s.reference);
 assert.deepEqual(cli({op:'service.snapshot'}),before,'touch, orbit and previews must not commit');assert.deepEqual(errors,[]);
 const result={passed:true,backend:gpu?'webgpu':'webgl2',dpr,visible_edges:initial.length,selected,rotated,touch_offset_css_px:10,queued_tap:true,direction:true,model_unchanged:true,hover,errors};fs.writeFileSync(path.join(out,'results.json'),JSON.stringify(result,null,2));console.log(JSON.stringify(result));
}catch(e){if(browser){const page=browser.contexts()[0]?.pages()[0];if(page)await page.screenshot({path:path.join(out,"failure.png")}).catch(()=>{});}throw e;}finally{releaseHeld();if(browser)await browser.close();if(gateway){gateway.kill('SIGINT');await pause(200);}if(fs.existsSync(path.join(runtime,'endpoint.json'))){try{cli({op:'service.shutdown'});}catch{}}fs.rmSync(runtime,{recursive:true,force:true});}})().catch(e=>{console.error(e);process.exitCode=1;});
