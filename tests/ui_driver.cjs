// Read actual egui control bounds. No fixed coordinates, model writes, or private service token.
const assert=require('assert/strict');
const pause=ms=>new Promise(r=>setTimeout(r,ms));
async function ui(page){return page.evaluate(()=>window.cadUI?.());}
async function until(fn,label,timeout=15000){const end=Date.now()+timeout;while(!(await fn())){assert(Date.now()<end,label);await pause(25);}}
async function control(page,name){let item;await until(async()=>{const g=await ui(page);item=g?.controls[name];return item?.enabled;},`enabled control ${name}`);const g=await ui(page),r=item.rect,s=g.scale||1;return {x:(r[0]+r[2]/2)*s,y:(r[1]+r[3]/2)*s};}
async function click(page,name,touch=false){const p=await control(page,name);if(touch)await page.touchscreen.tap(p.x,p.y);else await page.mouse.click(p.x,p.y,{delay:50});await pause(70);}
async function field(page,name,value){const touch=await page.evaluate(()=>navigator.maxTouchPoints>0);await click(page,name,touch);await page.keyboard.press('ControlOrMeta+a');await pause(50);await page.keyboard.type(String(value),{delay:35});await pause(100);await page.keyboard.press('Tab');await pause(70);}
async function view(page){let g;await until(async()=>{g=await ui(page);return g?.viewport?.[2]>64;},'viewport geometry');const r=g.viewport,s=g.scale||1;return {x:r[0]*s,y:r[1]*s,w:r[2]*s,h:r[3]*s};}
async function frame(page,frames,preview=false){let result;await until(async()=>{const g=await ui(page);result=frames.findLast(f=>f.metadata.epoch===g?.view_epoch&&f.metadata.document===g.document&&!!f.metadata.preview===preview);return g?.frame_current&&result;},`current ${preview?'preview':'committed'} frame`);return result;}
async function pixel(page,f,x,y){const r=await view(page);return {x:r.x+(x+.5)/f.width*r.w,y:r.y+(y+.5)/f.height*r.h};}
module.exports={ui,until,click,field,view,frame,pixel,pause,control};
