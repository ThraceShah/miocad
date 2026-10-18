import init, {render_job} from './pkg/cad_web.js';
const ready=init();let pending=null,timer=null;
self.onmessage=event=>{pending=event.data;if(timer===null){timer=setTimeout(async()=>{await ready;const job=pending;pending=null;timer=null;try{self.postMessage({ok:true,result:render_job(job)});}catch(error){self.postMessage({ok:false,error:String(error)});}},0);}};
