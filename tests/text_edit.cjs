// Exercise the actual egui text buffer, including mobile beforeinput without keydown.
const assert=require('assert/strict'),d=require('./ui_driver.cjs');
module.exports=async function textEdit(page){
 const expect=async text=>d.until(async()=>(await d.ui(page)).agent_draft===text,`draft equals ${JSON.stringify(text)}`,3000);
 const mobile=async(type,composing=false)=>page.evaluate(({type,composing})=>{
  const input=document.activeElement;
  if(!(input instanceof HTMLInputElement)||input.id==='code')throw Error('egui text agent not focused');
  return !input.dispatchEvent(new InputEvent('beforeinput',{inputType:type,bubbles:true,cancelable:true,isComposing:composing}));
 },{type,composing});
 await d.click(page,'chat-input',true);await page.keyboard.type('abcd',{delay:50});await expect('abcd');
 await page.keyboard.press('Backspace');await expect('abc');
 assert(await mobile('deleteContentBackward'),'soft keyboard deletion must be handled');await expect('ab');
 await page.keyboard.press('Home');assert(await mobile('deleteContentForward'));await expect('b');
 await page.keyboard.press('ControlOrMeta+a');assert(await mobile('deleteContentBackward'));await expect('');
 // Android can send keyCode 229/Unidentified before a usable beforeinput.
 await page.keyboard.type('xy',{delay:50});await expect('xy');
 await page.evaluate(()=>document.activeElement.dispatchEvent(new KeyboardEvent('keydown',{key:'Unidentified',keyCode:229,bubbles:true})));
 assert(await mobile('deleteContentBackward'));await expect('x');
 // Physical Delete also has a browser beforeinput: it must not delete twice.
 await page.keyboard.press('Home');await page.keyboard.type('abcd',{delay:50});await expect('abcdx');
 await page.keyboard.press('Home');await page.keyboard.press('Delete');await expect('bcdx');
 await page.keyboard.press('ControlOrMeta+a');await page.keyboard.type('new',{delay:50});await expect('new');
 // Preedit deletion belongs to the IME, not to the committed egui buffer.
 assert.equal(await mobile('deleteContentBackward',true),false);await expect('new');
 await page.keyboard.press('ControlOrMeta+a');await page.keyboard.press('Backspace');await expect('');
 await page.evaluate(()=>{const i=document.activeElement;for(const [type,data] of [['compositionstart',''],['compositionupdate','中文'],['compositionend','中文']])i.dispatchEvent(new CompositionEvent(type,{data,bubbles:true}));});
 await expect('中文');assert(await mobile('deleteContentBackward'));await expect('中');
 assert(await mobile('deleteContentBackward'));await expect('');assert(await mobile('deleteContentBackward'));await expect('');
 await page.keyboard.insertText('🙂');await expect('🙂');assert(await mobile('deleteContentBackward'));await expect('');
 await page.keyboard.type('one two',{delay:50});await expect('one two');assert(await mobile('deleteWordBackward'));await expect('one ');
 await page.keyboard.press('ControlOrMeta+a');await page.keyboard.press('Backspace');await expect('');
 assert.equal((await d.ui(page)).agent_thinking,false);console.log('Text editing: hardware/mobile/selection/IME/word deletion passed');
};
