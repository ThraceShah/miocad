// Deterministic protocol fixture, not a product Agent or natural-language interpreter.
let input='';process.stdin.on('data',c=>input+=c);process.stdin.on('end',()=>{
 const fs=require('fs'),assert=require('assert/strict'),request=JSON.parse(input),t=request.turn;
 if(process.argv[2])fs.writeFileSync(process.argv[2],JSON.stringify(request,null,2));
 if(t.text==='box')return console.log(JSON.stringify({text:'Preview replacement Box',proposal:{kind:'box',size:[6,7,8]}}));
 const edge=t.references.find(r=>r.kind==='edge');
 if(t.text.includes('unsupported'))return console.log(JSON.stringify({text:'Variable radius is unsupported; no operation was proposed.',proposal:null}));
 if(t.text.includes('invalid'))return console.log(JSON.stringify({text:'This must be rejected',proposal:{kind:'fillet',reference:'invented-edge',radius:1}}));
 assert(edge,'fixture requires an explicit edge');
 const reply={text:'将引用边做 1 mm 定半径倒圆。请预览并确认。',proposal:{kind:'fillet',reference:edge.target,radius:1}};
 setTimeout(()=>console.log(JSON.stringify(reply)),t.text.includes('delayed')?1500:10);
});
