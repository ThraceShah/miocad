const base = new URL(document.querySelector('base').href);
window.cadBase = base.pathname;
window.cadView = crypto.randomUUID().replaceAll('-','');
const api = (name, body = {}) => fetch(new URL(`api/${name}`, base), {method:'POST', credentials:'same-origin', headers:{'Content-Type':'application/json','X-CAD-Web':'1','X-CAD-View':window.cadView},body:JSON.stringify(body)});
const error = document.querySelector('#error');
async function open() {
  document.querySelector('#loading').style.display='grid';
  try {
    const {default:init,start_web}=await import(new URL('pkg/cad_web.js',base));
    await init();
    document.querySelector('#login').style.display='none';
    document.querySelector('#workspace').style.display='block';
    await start_web();
  } catch(e) { error.textContent=`Workspace failed to start: ${e}`;document.querySelector('#login').style.display='grid';document.querySelector('#workspace').style.display='none'; }
  finally {document.querySelector('#loading').style.display='none';}
}
document.querySelector('#unlock').addEventListener('submit',async event=>{
  event.preventDefault();const button=document.querySelector('#connect');button.disabled=true;error.textContent='';
  try {const response=await api('login',{code:document.querySelector('#code').value});document.querySelector('#code').value='';if(!response.ok)throw new Error(await response.text());await open();}catch(e){error.textContent=e.message;}finally{button.disabled=false;}
});
document.querySelector('#lock').addEventListener('click',async()=>{await api('logout');location.reload();});
try { const response=await api('hello');if(response.ok)await open(); } catch { error.textContent='Service unavailable. Refresh when the connection returns.'; }
