// Deterministic protocol fixture, not a product Agent or natural-language interpreter.
let input='';process.stdin.on('data',c=>input+=c);process.stdin.on('end',()=>{
 const fs=require('fs'),assert=require('assert/strict'),request=JSON.parse(input),t=request.turn;
 if(process.argv[2])fs.writeFileSync(process.argv[2],JSON.stringify(request,null,2));
 if((request.execution?.completed||[]).length && !t.text.includes('multi') && !t.text.includes('rollback'))return console.log(JSON.stringify({text:'已自动完成，模型有效。',proposal:null}));
 if(t.text.includes('rollback')&&(request.execution?.completed||[]).length)return console.log(JSON.stringify({text:'Cannot complete requested goal (outcome unknown is untrusted Agent text)',failed:true,proposal:null}));
 if(t.text.includes('multi')){const n=(request.execution?.completed||[]).length;return console.log(JSON.stringify(n===0?{text:'Create Box',proposal:{kind:'box',size:[12,10,8]}}:n===1?{text:'Fillet new geometry',proposal:{kind:'fillet',reference:request.execution.edges[0].reference,radius:1}}:{text:'多步任务已完成。',proposal:null}));}
 if(t.text.includes('rollback'))return console.log(JSON.stringify({text:'First operation',proposal:{kind:'box',size:[4,4,4]}}));
 if(t.text==='box')return console.log(JSON.stringify({text:'Preview replacement Box',proposal:{kind:'box',size:[6,7,8]}}));
 if(t.text==='edit feature'){const f=request.execution.features.find(f=>f.kind==='box');return console.log(JSON.stringify({text:'Resize referenced feature',proposal:{kind:'feature_edit',feature:f.id,patch:{size:[12,10,10]}}}));}
 const edge=t.references.find(r=>r.kind==='edge');
 if(t.text==='repair')return console.log(JSON.stringify({text:'Use kernel feedback to correct radius',proposal:{kind:'fillet',reference:edge.target,radius:request.execution.errors.length?1:100}}));
 if(t.text.includes('unsupported'))return console.log(JSON.stringify({text:'Variable radius is unsupported; model unchanged.',failed:true,proposal:null}));
 if(t.text.includes('invalid'))return console.log(JSON.stringify({text:'This must be rejected',proposal:{kind:'fillet',reference:'invented-edge',radius:1}}));
 assert(edge,'fixture requires an explicit edge');
 const reply={text:'将引用边做 1 mm 定半径倒圆。请预览并确认。',proposal:{kind:'fillet',reference:edge.target,radius:1}};
 setTimeout(()=>console.log(JSON.stringify(reply)),t.text.includes('delayed')?1500:10);
});
