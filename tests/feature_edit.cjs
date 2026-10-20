// Shared egui/WASM feature editor, genuine commands, no UI-specific feature engine.
const assert=require('assert/strict'),fs=require('fs'),d=require('./ui_driver.cjs');
module.exports=async(page,cli,doc,out)=>{
 const scene=()=>cli({op:'render.scene',document:doc}),state=()=>cli({op:'document.state',document:doc});
 const f=scene().data.features[0],id=f.id;
 await d.click(page,`edit-feature-${id}`,true);await d.field(page,'feature-size-0','20');await d.click(page,'feature-apply',true);
 await d.until(()=>state().revision===2,'edited Box');assert.equal(state().data.volume,2000);assert.equal(scene().data.features[0].id,id);
 await d.click(page,'undo',true);await d.until(()=>state().revision===3,'Undo Box edit');assert.equal(state().data.volume,1000);
 await d.click(page,'redo',true);await d.until(()=>state().revision===4,'Redo Box edit');assert.equal(state().data.volume,2000);
 const edge=scene().data.edges[0].reference;cli({op:'model.fillet',document:doc,revision:4,edge,radius:1});await d.until(async()=>!!(await d.ui(page)).controls['edit-feature-f2'],'Fillet feature');
 await d.click(page,'edit-feature-f2',true);await d.field(page,'feature-radius','2');await d.click(page,'feature-apply',true);await d.until(()=>state().revision===6,'edit Fillet radius');assert.equal(scene().data.features[1].parameters.radius,2);assert(state().data.valid);
 await d.click(page,'edit-feature-f2',true);await d.field(page,'feature-radius','1000');await d.click(page,'feature-apply',true);await d.until(async()=>!!(await d.ui(page)).controls['feature-apply']?.enabled,'failed edit completed');assert.equal(state().revision,6);assert.equal(scene().data.features[1].parameters.radius,2);
 await d.click(page,'feature-close',true);await d.click(page,'edit-feature-f2',true);await d.click(page,'feature-suppressed',true);await d.click(page,'feature-apply',true);await d.until(()=>state().revision===7,'suppress Fillet');assert.equal(state().data.volume,2000);
 await d.click(page,'undo',true);await d.until(()=>state().revision===8,'Undo suppression');assert.equal(scene().data.features[1].suppressed,false);
 await d.click(page,`feature-${id}`,true);await d.field(page,'chat-input','edit feature');await d.click(page,'chat-send',true);await d.until(async()=>!(await d.ui(page)).agent_thinking,'Agent feature edit');const task=(await d.ui(page)).agent_task;assert.equal(task.phase,'completed',JSON.stringify(task));assert(task.atomic);assert.equal(state().revision,9);assert.equal(scene().data.features[0].parameters.size[0],12);assert(state().data.valid);
 await d.click(page,'agent-undo',true);await d.until(async()=>!(await d.ui(page)).agent_thinking,'Undo Agent feature edit');assert.equal(state().revision,10);assert.equal(scene().data.features[0].parameters.size[0],20);
 await d.click(page,'redo',true);await d.until(()=>state().revision===11,'Redo complete Agent feature task');assert.equal(scene().data.features[0].parameters.size[0],12);
 await page.setViewportSize({width:768,height:1024});await d.click(page,`edit-feature-${id}`,true);await d.until(async()=>!!(await d.ui(page)).controls['feature-apply'],'portrait editor');await d.until(async()=>(await d.ui(page)).frame_current,'current portrait feature frame');assert.equal((await d.ui(page)).model_error,null);await page.screenshot({path:out+'/feature-editor-portrait.png'});
 fs.writeFileSync(out+'/feature-results.json',JSON.stringify({passed:true,box_edit:true,fillet_edit:true,suppression:true,undo_redo:true,failed_recompute_rollback:true,agent_feature_edit:true,atomic_task:true,portrait:true},null,2));console.log('Feature editor and Agent feature-edit task passed');
};
