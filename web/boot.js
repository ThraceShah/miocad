const base = new URL(document.querySelector('base').href);
window.cadBase = base.pathname;
window.cadView = crypto.randomUUID().replaceAll('-','');
const api = (name, body = {}) => fetch(new URL(`api/${name}`, base), {method:'POST', credentials:'same-origin', headers:{'Content-Type':'application/json','X-CAD-Web':'1','X-CAD-View':window.cadView},body:JSON.stringify(body)});
const error = document.querySelector('#error');
// eframe 0.33 clears its hidden text agent after insertion. Mobile keyboards
// delete via beforeinput (often with keyCode 229), which eframe doesn't forward.
// Keep editing in egui, where the actual text, selection and undo state live.
const isTextAgent = target => target instanceof HTMLInputElement
  && target.type === 'text' && target.getAttribute('autocapitalize') === 'off'
  && target === document.activeElement;
document.addEventListener('keydown', event => {
  if (isTextAgent(event.target) && !event.isComposing && event.keyCode !== 229
      && (event.key === 'Delete' || event.key === 'Backspace')) {
    // egui already consumes this key. Cancel native editing so a physical
    // Delete cannot also produce beforeinput and delete a second character.
    event.preventDefault();
  }
}, true);
document.addEventListener('beforeinput', event => {
  if (!isTextAgent(event.target) || event.isComposing || !event.cancelable) return;
  const backward = event.inputType === 'deleteContentBackward' || event.inputType === 'deleteWordBackward';
  const forward = event.inputType === 'deleteContentForward' || event.inputType === 'deleteWordForward';
  if (!backward && !forward) return;
  event.preventDefault();
  const word = event.inputType.startsWith('deleteWord');
  const mac = /Mac|iPhone|iPad|iPod/.test(navigator.platform);
  const init = {key: backward ? 'Backspace' : 'Delete', bubbles:true, cancelable:true,
    ctrlKey:word && !mac, altKey:word && mac};
  event.target.dispatchEvent(new KeyboardEvent('keydown', init));
  event.target.dispatchEvent(new KeyboardEvent('keyup', init));
}, true);
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

// User-selected files only; bytes stay in page memory until a turn is sent.
window.cadChooseFiles=()=>{
  const picker=document.createElement('input');picker.type='file';picker.multiple=true;picker.accept='.txt,.md,.json,.csv,.rs,.cpp,.h,.png,.jpg,.jpeg';picker.id='cad-file-picker';picker.hidden=true;document.body.appendChild(picker);
  picker.addEventListener('change',async()=>{
    try {
      const files=[...picker.files];if(files.length>8)throw new Error('Maximum 8 attachments');
      if(files.some(f=>f.size>2250000)||files.reduce((n,f)=>n+f.size,0)>4000000)throw new Error('Attachments exceed size limit (4 MB total)');
      const result=await Promise.all(files.map(async f=>{
        const data=await new Promise((resolve,reject)=>{const r=new FileReader();r.onload=()=>resolve(String(r.result).split(',')[1]);r.onerror=reject;r.readAsDataURL(f);});
        return {name:f.name,mime:f.type==='image/png'||f.type==='image/jpeg'?f.type:'text/plain',data};
      }));window.cadPendingAttachments=JSON.stringify(result);
    }catch(e){window.cadAttachmentError=String(e.message||e);}finally{picker.remove();}
  },{once:true});picker.addEventListener('cancel',()=>picker.remove(),{once:true});picker.click();
};
