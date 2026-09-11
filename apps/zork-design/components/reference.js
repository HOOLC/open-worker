/* Select a component in the original HTML prototype. No component markup or styles are copied. */
window.prepareZorkReference=async function(story){
 const one=s=>{const e=document.querySelector(s);if(!e)throw Error('设计稿缺少 '+s);return e};
 const click=s=>one(s).click();
 const settings=section=>{click('[data-action="settings"]');click(`[data-action="device-settings-page"][data-device="mini1"][data-page="${section}"]`)};
 const agent=()=>{settings('agents');click('[data-action="edit-agent"][data-agent="product"]')};
 const connection=()=>{settings('models');click('[data-action="add-profile"]');click('#profile-provider-menu summary');click('[data-action="profile-provider"][data-provider="openai"]')};
 const {family,state}=story;let selector;
 if(family==='brand'){const asset=document.createElement('img');asset.src=new URL(story.design.source).pathname;asset.width=story.design.bounds.width;asset.height=story.design.bounds.height;document.body.replaceChildren(asset);selector='body > img'}
 else if(['avatar','icons','providers'].includes(family)){
  const names=family==='avatar'?['cat','bunny','bear','fox','panda','chick','dog','owl','koala','penguin','deer','octopus']:family==='providers'?['openai','anthropic','githubcopilot','kimi','xai','openrouter','opencode','compatible']:story.reference_icons;
  const sheet=document.createElement('div');sheet.id='asset-reference';sheet.style.cssText='display:flex;flex-wrap:wrap;gap:16px;width:512px;background:white';
  for(const name of names){const img=document.createElement('img');img.src=family==='icons'?'/design/assets/native/current/icons/'+name:`/design/assets/${family==='avatar'?'avatars':'providers'}/${name}.svg`;img.width=img.height=family==='avatar'?Number(state):20;img.alt=name;sheet.append(img)}
  document.body.replaceChildren(sheet);selector='#asset-reference';
 }else{
  profiles.splice(0,profiles.length,{id:'story-profile',profile_id:'fixture',name:'fixture',node:'mini1',provider:'openai',billing:'subscription',models:[{id:'fixture-model'}],verified:false});
  people.product.modelConnectionId='story-profile';people.product.modelId='fixture-model';
  if(['button','field'].includes(family)){
   agent();
   if(family==='button'){
    selector='#agent-settings-form .modal-actions '+(state==='secondary'?'button[data-action="close-dialog"]':'button[type="submit"]');if(state==='disabled')one(selector).disabled=true;if(state==='focus')one(selector).focus();
   }else{selector='#agent-settings-form input[name="name"]';const input=one(selector);input.value=['value','focus'].includes(state)?'产品模型连接':state==='secret'?'fixture-secret':'';if(state==='secret')input.type='password';if(state==='focus')input.focus();else input.blur()}
  }else if(['choice','dropdown'].includes(family)){
   connection();
   if(family==='choice'){selector='.profile-billing';if(state==='disabled')one(selector).querySelectorAll('button').forEach(e=>e.disabled=true)}
   else if(['empty','disabled'].includes(state)){click('#chat-dialog [data-action="close-dialog"]');click('[data-action="device-settings-page"][data-device="mini1"][data-page="agents"]');click('[data-action="edit-agent"][data-agent="product"]');selector='#agent-model';one(selector).disabled=true;if(state==='empty')one(selector).innerHTML='<option>此连接尚未添加模型</option>'}
   else{if(state==='open')click('#profile-provider-menu summary');selector='#profile-provider-menu'}
  }else if(family==='navigation'){
   if(state==='selected'){settings('agents');selector='[data-action="device-settings-page"][data-device="mini1"][data-page="agents"]'}else selector='[data-action="collapse-device"][data-device="mini1"]';
  }else if(family==='markdown'){selector='.message-text';one(selector).textContent='这是一段强调文字，包含链接和 inline code。中文、英文与数字 123 保持清晰可读。'}
  else if(family==='feedback'){settings('models');click('[data-action="view-profile"]');click('[data-action="fetch-profile-models"]');selector='#profile-model-feedback'}
  else if(family==='connection'){if(state.startsWith('list')){settings('models');selector='.settings-content'}else{connection();if(state.startsWith('provider'))click('#profile-provider-menu summary');selector='#chat-dialog'}}
  else if(family==='model'){settings('models');click('[data-action="view-profile"]');if(state.startsWith('create')){click('[data-action="add-profile-model"]');click('.profile-capacity summary')}selector='#chat-dialog'}
  else if(family==='agent'){settings('agents');if(state.startsWith('list'))selector='.settings-content';else{click(state.startsWith('create')?'[data-action="add-agent"]':'[data-action="edit-agent"][data-agent="product"]');selector='#chat-dialog'}}
  else if(family==='conversation')selector=state.startsWith('composer')?'.composer':'.chat-app';
 }
 if(!selector)throw Error('此状态尚无 HTML 设计');
 const isolation=document.createElement('style');isolation.textContent='body *{visibility:hidden!important}[data-reference-target],[data-reference-target] *{visibility:visible!important}dialog::backdrop{visibility:hidden!important}';document.head.append(isolation);
 if(state==='hover'){
  const forced=document.createElement('style');
  const rules=Array.from(document.styleSheets).flatMap(sheet=>{try{return Array.from(sheet.cssRules)}catch{return[]}});
  forced.textContent=rules.filter(rule=>rule.selectorText?.includes(':hover')).map(rule=>rule.selectorText.replaceAll(':hover','[data-reference-hover]')+'{'+rule.style.cssText+'}').join('\n');document.head.append(forced);one(selector).setAttribute('data-reference-hover','');
 }
 const measure=()=>{const target=document.querySelector(selector);if(!target)return;target.setAttribute('data-reference-target','');const b=target.getBoundingClientRect();parent.postMessage({type:'zork-reference-bounds',id:story.id,bounds:{x:b.x,y:b.y,width:b.width,height:b.height}},location.origin)};
 await document.fonts.ready;await Promise.all([...document.images].map(i=>i.decode().catch(()=>{})));await new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)));measure();
 new MutationObserver(()=>requestAnimationFrame(measure)).observe(document.body,{subtree:true,childList:true});new ResizeObserver(measure).observe(one(selector));
 document.documentElement.dataset.referenceReady=story.id;
};
