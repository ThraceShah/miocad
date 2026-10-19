// Regression for a 0.001mm fillet whose two tangent edges occupy the same pixels.
// Disposable daemon only. Never attach to or mutate production documents.
const fs=require('fs'),os=require('os'),path=require('path'),net=require('net');
const {spawn,spawnSync}=require('child_process'),assert=require('assert/strict');
const {chromium}=require(process.env.CAD_PLAYWRIGHT||'playwright');
const root=path.resolve(__dirname,'..'),bin=path.resolve(process.env.CAD_BIN||'target/debug');
const gpu=process.env.CAD_TEST_GPU==='webgpu',dpr=Number(process.env.CAD_TEST_DPR||1);
const out=path.join(root,`artifacts/micro-fillet-${gpu?'webgpu':'webgl'}-${dpr}`);fs.mkdirSync(out,{recursive:true});fs.rmSync(path.join(out,'failure.png'),{force:true});
const runtime=fs.mkdtempSync(path.join(os.tmpdir(),'cad-micro-pick-')),pause=ms=>new Promise(r=>setTimeout(r,ms));
function cli(command){const p=spawnSync(path.join(bin,'cad-cli'),['--runtime-dir',runtime,'--worker',path.join(root,'build/cad-server'),'--client-id','micro-regression'],{input:JSON.stringify(command)+'\n',encoding:'utf8',maxBuffer:16*1024*1024});assert.equal(p.status,0,p.stderr);const r=JSON.parse(p.stdout);assert(r.ok,JSON.stringify(r));return r.result;}
async function port(){const s=net.createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p;}
(async()=>{let gateway,browser,page;try{
 const document=cli({op:'document.create'}).document;cli({op:'model.box',document,revision:0,size:[10,10,10]});
 for(const [i,[x,y,radius]] of [[10,10,1],[0,10,2],[0,0,2],[10,0,.001]].entries()){
  const edges=cli({op:'topology.edges',document}).data;
  const edge=edges.find(e=>Math.abs(e.length-10)<1e-6&&e.points.every(p=>Math.abs(p[0]-x)<1e-6&&Math.abs(p[1]-y)<1e-6));assert(edge);
  cli({op:'model.fillet',document,revision:i+1,edge:edge.reference,radius});
 }
 const edges=cli({op:'topology.edges',document}).data;
 const targets=edges.filter(e=>Math.abs(e.length-10)<1e-6&&e.points.every(p=>p[0]>9.998&&p[1]<.002)).map(e=>e.reference);assert.equal(targets.length,2);
 const before=cli({op:'service.snapshot'}),p=await port(),origin=`http://127.0.0.1:${p}`,url=origin+'/apps/miocad/';
 const log=fs.openSync(path.join(out,'gateway.log'),'w');gateway=spawn(path.join(bin,'cad-web'),['--runtime-dir',runtime],{env:{...process.env,HOST:'127.0.0.1',PORT:String(p),BASE_PATH:'/apps/miocad/',CAD_WEB_ORIGIN:origin,CAD_WEB_ATTACH_ONLY:'1',CAD_WEB_TRUSTED_ACCESS:'1',CAD_WEB_STRIP_PREFIX:'0',CAD_WEB_ASSETS:path.join(root,'artifacts/web')},stdio:['ignore',log,log]});
 for(let i=0;;i++){try{if((await fetch(url+'health')).ok)break;}catch{}assert(i<100);await pause(50);}
 browser=await chromium.launch({headless:true,args:gpu?['--force-device-scale-factor='+dpr,'--use-angle=vulkan','--enable-features=Vulkan','--disable-vulkan-surface','--enable-unsafe-webgpu']:['--force-device-scale-factor='+dpr,'--use-angle=swiftshader','--enable-unsafe-swiftshader','--disable-webgpu']});
 const context=await browser.newContext({viewport:{width:1280,height:820},deviceScaleFactor:dpr,hasTouch:true});page=await context.newPage();const errors=[],frames=[];
 await page.addInitScript(()=>{window.cadPickJobs=[];const send=Worker.prototype.postMessage;Worker.prototype.postMessage=function(message,...args){try{const job=JSON.parse(message.job);window.cadPickJobs.push({selected:job.selected,epoch:job.epoch});}catch{}return send.call(this,message,...args);};});
 page.on('pageerror',e=>errors.push(e.message));page.on('response',async r=>{if(r.url().endsWith('/api/view')&&r.ok()){try{frames.push(await r.json());}catch{}}});
 async function until(fn,label){const end=Date.now()+12000;while(!(await fn())){assert(Date.now()<end,`${label}; ${errors}`);await pause(30);}}
 await page.goto(url);await page.locator('#workspace').waitFor({state:'visible'});await until(()=>frames.length,'initial frame');await pause(150);
 async function drag(dx,start=620,y=420){const cdp=await context.newCDPSession(page);await cdp.send('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{x:start,y}]});await pause(25);for(let i=1;i<=8;i++){await cdp.send('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:[{x:start+dx*i/8,y}]});await pause(25);}await cdp.send('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});await cdp.detach();}
 const initialEpoch=frames.at(-1).metadata.epoch;await drag(-232);await until(()=>frames.at(-1).metadata.epoch>initialEpoch,'front-facing micro-fillet camera');await pause(150);
 const f=frames.at(-1),ids=targets.map(ref=>Object.keys(f.edge_refs).find(id=>f.edge_refs[id]===ref));assert(ids.every(Boolean));
 // Choose the middle of the actual ambiguous ID strip; both references must be present.
 const indices=Object.entries(f.candidates).filter(([,c])=>ids.every(id=>c.includes(Number(id)))).map(([index])=>Number(index));assert(indices.length>100,'two complete long edges must remain present as candidates');
 const meanY=indices.reduce((sum,i)=>sum+Math.floor(i/f.width)/indices.length,0);const mid=indices.reduce((a,i)=>Math.abs(Math.floor(i/f.width)-meanY)<Math.abs(Math.floor(a/f.width)-meanY)?i:a);
 const x=218+(mid%f.width+.5)/f.width*780,y=110+(Math.floor(mid/f.width)+.5)/f.height*672;
 const chosen=[];
 for(let i=0;i<2;i++){
  const prior=(await page.evaluate(()=>window.cadPickJobs.length));await page.touchscreen.tap(x,y);await pause(200);await page.screenshot({path:path.join(out,`choices-${i}.png`)});
  // Large touch target for the first choice; keyboard shortcut exercises the second.
  if(i===0)await page.touchscreen.tap(640,400);else await page.keyboard.press('2');
  await until(async()=>{const jobs=await page.evaluate(()=>window.cadPickJobs);return jobs.slice(prior).some(j=>targets.includes(j.selected));},'explicit choice must select one of the micro edges');
  const selected=await page.evaluate(()=>window.cadPickJobs.at(-1).selected);chosen.push(selected);const image=path.join(out,`selected-${i}.png`);await page.screenshot({path:image});
  const count=spawnSync('python3',['-c',`from PIL import Image
import sys
im=Image.open(sys.argv[1]).convert('RGB');d=float(sys.argv[2]);im=im.crop((int(218*d),int(150*d),int(998*d),int(750*d))).resize((780,600));print(sum(r>210 and 110<g<220 and b<110 for r,g,b in im.getdata()))`,image,String(dpr)],{encoding:'utf8'});assert.equal(count.status,0,count.stderr);assert(Number(count.stdout)>200,'chosen edge must have visible amber highlight');
 }
 assert.equal(new Set(chosen).size,2,'both overlapping edges must be individually reachable');
 // Opening choices and moving the camera must dismiss the old scope, so a stale shortcut cannot select.
 await page.touchscreen.tap(250,160);await pause(400);await page.touchscreen.tap(x,y);await pause(200);
 await drag(-50,940,580);await pause(400);await page.keyboard.press('1');await pause(250);
 assert.equal(await page.evaluate(()=>window.cadPickJobs.at(-1).selected),null,'stale chooser must not survive camera motion');
 assert.deepEqual(cli({op:'service.snapshot'}),before);assert.deepEqual(errors,[]);
 const result={passed:true,backend:gpu?'webgpu':'webgl2',dpr,radii:[1,2,2,.001],targets,chosen,ambiguous_pixels:indices.length,model_unchanged:true,stale_choice_rejected:true,errors};fs.writeFileSync(path.join(out,'results.json'),JSON.stringify(result,null,2));console.log(JSON.stringify(result));
}catch(e){if(page)await page.screenshot({path:path.join(out,'failure.png')}).catch(()=>{});throw e;}finally{if(browser)await browser.close();if(gateway){gateway.kill('SIGINT');await pause(150);}if(fs.existsSync(path.join(runtime,'endpoint.json'))){try{cli({op:'service.shutdown'});}catch{}}fs.rmSync(runtime,{recursive:true,force:true});}})().catch(e=>{console.error(e);process.exitCode=1;});
