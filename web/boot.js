const base = new URL(document.querySelector('base').href);
window.cadBase = base.pathname;
window.cadView = crypto.randomUUID().replaceAll('-','');
const api = (name, body = {}) => fetch(new URL(`api/${name}`, base), {method:'POST', credentials:'same-origin', headers:{'Content-Type':'application/json','X-CAD-Web':'1','X-CAD-View':window.cadView},body:JSON.stringify(body)});
const error = document.querySelector('#error');
async function open() {
  document.querySelector('#loading').style.display='grid';
  try {
    const {default:init,start_web,ui_geometry}=await import(new URL('pkg/cad_web.js',base));
    await init();
    window.cadUI=()=>JSON.parse(ui_geometry());
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
// Discover the explicit server policy before displaying any credential form.
try {
  const policy=await api('access');
  if(!policy.ok)throw new Error(await policy.text());
  const {requires_code}=await policy.json();
  document.querySelector('#lock').hidden=!requires_code;
  const response=await api('hello');
  if(response.ok)await open();
  else if(!requires_code && response.status===401){
    const connected=await api('login',{code:''});
    if(!connected.ok)throw new Error(await connected.text());
    await open();
  } else if(requires_code && response.status===401)document.querySelector('#login').style.display='grid';
  else throw new Error(await response.text());
} catch(e) {
  error.textContent=`Connection failed: ${e.message}. Refresh to retry.`;
  document.querySelector('#login').style.display='grid';
} finally {document.querySelector('#loading').style.display='none';}
