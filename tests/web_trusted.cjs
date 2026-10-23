// Read-only acceptance for the explicitly credential-free Tailscale deployment.
// No deployment hostname is committed: set CAD_WEB_URL to the published base URL.
const {chromium}=require(process.env.CAD_PLAYWRIGHT||'playwright');
const assert=require('assert/strict'),fs=require('fs');
const url=process.env.CAD_WEB_URL;
if(!url){console.error('CAD_WEB_URL is required (for example https://<host>.<tailnet>.ts.net/apps/miocad/)');process.exit(2);}
const out=process.env.CAD_WEB_TEST_OUTPUT||'artifacts/trusted-web';fs.mkdirSync(out,{recursive:true});
(async()=>{
 const browser=await chromium.launch({headless:true,args:['--use-angle=swiftshader','--enable-unsafe-swiftshader']});
 try {
  const context=await browser.newContext({viewport:{width:1280,height:820},...(process.env.CAD_TEST_PROXY?{proxy:{server:process.env.CAD_TEST_PROXY,bypass:new URL(url).hostname}}:{})});
  const page=await context.newPage(),errors=[];page.on('pageerror',e=>errors.push(e.message));
  const frameResponse=page.waitForResponse(r=>r.url().endsWith('/api/view')&&r.ok());
  await page.goto(url);
  await page.locator('#workspace').waitFor({state:'visible'});
  assert.equal(await page.locator('#code').isVisible(),false);
  assert.equal(await page.locator('#lock').isVisible(),false);
  const frame=await(await frameResponse).json();assert.equal(frame.metadata.backend,'wgpu');
  const cookie=(await context.cookies()).find(c=>c.name==='miocad_web');
  assert(cookie.httpOnly&&cookie.secure);assert.equal(cookie.sameSite,'Strict');assert.equal(cookie.path,new URL(url).pathname);
  const policy=await page.evaluate(async()=>{
   const r=await fetch(new URL('api/access',document.baseURI),{method:'POST',headers:{'X-CAD-Web':'1'}});return r.json();
  });assert.equal(policy.requires_code,false);
  // A hostile origin cannot bootstrap a grant, even when no access code is required.
  const blocked=await context.request.post(url+'api/login',{headers:{Origin:'https://evil.example','X-CAD-Web':'1'},data:{code:''}});assert.equal(blocked.status(),403);
  const unauth=await browser.newContext({... (process.env.CAD_TEST_PROXY?{proxy:{server:process.env.CAD_TEST_PROXY,bypass:new URL(url).hostname}}:{})});
  const response=await unauth.request.post(url+'api/snapshot',{headers:{Origin:new URL(url).origin,'X-CAD-Web':'1','X-CAD-View':'0123456789abcdef0123456789abcdef'},data:{}});assert.equal(response.status(),401);await unauth.close();
  await page.screenshot({path:out+'/workspace.png'});
  await page.reload();await page.locator('#workspace').waitFor({state:'visible'});assert.equal(await page.locator('#login').isVisible(),false);
  const nested=await context.newPage();await nested.goto(url+'workspace/example');await nested.locator('#workspace').waitFor({state:'visible'});assert.equal(await nested.locator('#code').isVisible(),false);
  for(const resource of ['health','boot.js','pkg/cad_web_bg.wasm'])assert.equal((await context.request.get(url+resource)).status(),200,resource);
  assert.deepEqual(errors,[]);
  const result={passed:true,url,requires_code:false,reload:true,nested:true,backend:frame.metadata.backend,cookie:{httpOnly:cookie.httpOnly,secure:cookie.secure,sameSite:cookie.sameSite,path:cookie.path},errors};
  fs.writeFileSync(out+'/browser.json',JSON.stringify(result,null,2));console.log(JSON.stringify(result));
 } finally {await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
