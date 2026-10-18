// Internal browser acceptance. npm install --prefix artifacts/browser-tools playwright@1.56.1
// CAD_PLAYWRIGHT=<package directory> node tests/web_smoke.cjs
const fs=require('fs');const os=require('os');const path=require('path');const net=require('net');const {spawn,spawnSync}=require('child_process');const assert=require('assert/strict');
const {chromium}=require(process.env.CAD_PLAYWRIGHT||'playwright');
const root=path.resolve(__dirname,'..'),out=path.join(root,'artifacts/p6-web');fs.mkdirSync(out,{recursive:true});
const bin=process.env.CAD_BIN||path.join(root,'target/debug'),runtime=fs.mkdtempSync(path.join(os.tmpdir(),'miocad-web-smoke-'));
function cli(command){const result=spawnSync(path.join(bin,'cad-cli'),['--runtime-dir',runtime,'--worker',path.join(root,'build/cad-server'),'--client-id','browser-smoke-agent'],{input:JSON.stringify(command)+'\n',encoding:'utf8'});assert.equal(result.status,0,result.stderr);const value=JSON.parse(result.stdout);assert(value.ok,JSON.stringify(value));return value.result;}
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function port(){const server=net.createServer();await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));const port=server.address().port;await new Promise(resolve=>server.close(resolve));return port;}
(async()=>{let gateway,browser;const errors=[],receipts=[];let document;
try{
 const origin=`http://127.0.0.1:${await port()}`,url=`${origin}/apps/miocad/`;
 const log=fs.openSync(path.join(out,'gateway.log'),'w');gateway=spawn(path.join(bin,'cad-web'),['--runtime-dir',runtime,'--worker',path.join(root,'build/cad-server')],{env:{...process.env,HOST:'127.0.0.1',PORT:new URL(origin).port,BASE_PATH:'/apps/miocad/',CAD_WEB_ORIGIN:origin,CAD_WEB_ASSETS:path.join(root,'artifacts/web')},stdio:['ignore',log,log]});
 for(let i=0;i<100;i++){try{if((await fetch(url+'health')).ok)break;}catch{}assert(i<99,'gateway startup timeout');await sleep(100);}
 browser=await chromium.launch({headless:true,args:['--use-angle=swiftshader','--enable-unsafe-swiftshader']});const context=await browser.newContext({viewport:{width:1280,height:820},deviceScaleFactor:1,acceptDownloads:true});const page=await context.newPage();page.on('pageerror',e=>errors.push(e.message));
 page.on('response',async response=>{if(response.url().endsWith('/api/rpc')){try{receipts.push({request:JSON.parse(response.request().postData()),response:await response.json()});}catch{}}});
 async function shot(name){await sleep(850);await page.screenshot({path:path.join(out,name+'.png')});}
 async function clickRpc(x,y,op){const reply=page.waitForResponse(response=>{if(!response.url().endsWith('/api/rpc'))return false;try{return JSON.parse(response.request().postData()).command?.op===op;}catch{return false;}},{timeout:10000});await page.mouse.move(x,y);await sleep(80);await page.mouse.click(x,y,{delay:60});const response=await (await reply).json();assert(response.ok,JSON.stringify(response));return response;}
 function state(){return cli({op:'document.state',document});}
 await page.goto(url);await shot('01-login');await page.locator('#code').fill(fs.readFileSync(path.join(runtime,'web-access-code'),'utf8'));await page.locator('#connect').click();await page.locator('#workspace').waitFor({state:'visible'});await sleep(1500);
 const created=await clickRpc(80,78,'document.create');document=created.result.document;await shot('02-document');
 await clickRpc(1120,480,'model.preview');await shot('03-box-preview');assert.equal(state().revision,0);assert(state().data.empty);
 await page.mouse.click(1194,524);await shot('04-preview-cancelled');assert.equal(state().revision,0);
 await clickRpc(1120,480,'model.preview');await sleep(800);await clickRpc(1070,524,'model.box');await shot('05-box');assert.equal(state().revision,1);assert.equal(state().data.volume,1000);
 await page.mouse.click(1070,524);await sleep(150);assert.equal(state().revision,1);
 await page.mouse.move(608,555);await sleep(100);await page.mouse.click(608,555,{delay:60});await shot('06-selected-edge');await clickRpc(1120,424,'model.preview');await shot('07-fillet-preview');assert.equal(state().revision,1);
 await clickRpc(1070,469,'model.fillet');await shot('08-fillet');assert.equal(state().revision,2);assert(state().data.valid);assert(Math.abs(state().data.volume-997.8539818)<1e-4);
 const cursor=cli({op:'service.snapshot'}).cursor;await page.mouse.move(650,420);await page.mouse.down();await page.mouse.move(720,470,{steps:8});await page.mouse.up();await shot('09-local-orbit');assert.equal(state().revision,2);assert.deepEqual(cli({op:'service.events',after:cursor}).events,[]);
 await clickRpc(195,78,'document.undo');await shot('10-undo');assert.equal(state().revision,3);await clickRpc(265,78,'document.redo');await shot('11-redo');assert.equal(state().revision,4);
 // Browser-native export: no server path entry is exposed.
 await page.mouse.move(1180,680);await page.mouse.wheel(0,700);await sleep(300);await shot('12-download-panel');
 await page.mouse.click(1080,680,{delay:60});await sleep(300);const downloaded=page.waitForEvent('download');await page.mouse.click(1080,716,{delay:60});const file=await downloaded;assert.equal(file.suggestedFilename(),'model.smb');await file.saveAs(path.join(out,'browser-workflow.smb'));assert(fs.statSync(path.join(out,'browser-workflow.smb')).size>100);await shot('13-downloaded');
 // Agent edit invalidates the browser candidate instead of silently applying it.
 await page.mouse.click(1080,680,{delay:60});await sleep(200);await page.mouse.move(1190,400);await page.mouse.wheel(0,-2000);await sleep(500);await page.mouse.click(1040,228,{delay:60});await sleep(200);await page.mouse.move(1190,400);await page.mouse.wheel(0,-2000);await sleep(700);await shot('14-box-mode');await clickRpc(1120,480,'model.preview');await shot('14-before-agent-edit');cli({op:'model.box',document,revision:4,size:[12,10,10]});await shot('15-agent-edit-invalidates-preview');await page.mouse.click(1070,524,{delay:60});assert.equal(state().revision,5);assert.equal(state().data.volume,1200);
 // Disconnect and a missed event require a fresh authoritative snapshot.
 await context.setOffline(true);await shot('16-offline');cli({op:'model.box',document,revision:5,size:[11,10,10]});await context.setOffline(false);await page.mouse.click(435,78,{delay:60});await shot('17-reconnected');assert.equal(state().revision,6);
 // Drop a real modeling HTTP acknowledgement after the kernel has committed.
 let drop=true;await page.route('**/api/rpc',async route=>{const command=JSON.parse(route.request().postData()).command;if(drop&&command?.op==='model.box'){drop=false;await route.fetch();await route.abort('failed');}else await route.continue();});
 await clickRpc(1120,480,'model.preview');await sleep(800);await page.mouse.click(1070,524,{delay:60});await shot('18-unknown-outcome');assert.equal(state().revision,7);assert.equal(drop,false);
 await page.mouse.click(435,78,{delay:60});await shot('19-reconciled-without-replay');assert.equal(state().revision,7);await page.unroute('**/api/rpc');
 await page.reload();await page.locator('#workspace').waitFor({state:'visible'});await shot('20-reloaded-shared-document');assert.equal(state().revision,7);
 await page.locator('#lock').click();await page.locator('#login').waitFor({state:'visible'});await shot('21-locked');
 await fs.promises.writeFile(path.join(out,'acceptance.json'),JSON.stringify({passed:true,document,final_state:state(),browser:browser.version(),receipts,errors},null,2));
 assert.deepEqual(errors,[]);
 console.log(JSON.stringify({passed:true,document,screenshots:out}));
}finally{if(browser)await browser.close();if(fs.existsSync(path.join(runtime,'endpoint.json'))){try{cli({op:'service.shutdown'});}catch{}}if(gateway){gateway.kill('SIGINT');await Promise.race([new Promise(resolve=>gateway.once('exit',resolve)),sleep(3000)]);if(gateway.exitCode===null)gateway.kill('SIGKILL');}fs.rmSync(runtime,{recursive:true,force:true});}})().catch(error=>{console.error(error);process.exitCode=1;});
