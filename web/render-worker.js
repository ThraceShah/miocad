let pending=null,running=false;
self.onmessage=event=>{pending=event.data;if(!running)run();};
async function run(){running=true;while(pending){const request=pending;pending=null;let job;try{
 job=JSON.parse(request.job);const response=await fetch(new URL('api/view',self.location.href),{method:'POST',credentials:'same-origin',headers:{'Content-Type':'application/json','X-CAD-Web':'1','X-CAD-View':request.view},body:JSON.stringify({service:job.service,session:job.session,view:{document:job.bound.document,revision:job.bound.revision,width:job.width,height:job.height,camera:job.camera,epoch:job.epoch,view_id:request.view,preview:job.preview_operation,backend:'auto'}}),signal:AbortSignal.timeout(20000)});
 if(!response.ok)throw new Error(`Viewport HTTP ${response.status}: ${await response.text()}`);const frame=await response.json();self.postMessage({ok:true,epoch:job.epoch,result:JSON.stringify({epoch:job.epoch,document:job.bound.document,revision:job.bound.revision,preview:job.preview,width:job.width,height:job.height,selected:job.selected,frame})});
 }catch(error){self.postMessage({ok:false,epoch:job?.epoch,error:String(error)});}}running=false;}
