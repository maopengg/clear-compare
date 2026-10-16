// Optional browser regression: run from repository root after a release Trunk build.
// Uses synthetic bridge replies and an isolated browser, not native filesystem IPC.
// Install Playwright outside the source tree; see docs/directory-workspace.md.
const {chromium}=require('playwright');
const fs=require('fs/promises');const path=require('path');const assert=require('assert');
(async()=>{
 await fs.mkdir('target/work',{recursive:true});
 const executablePath=process.env.CLEAR_COMPARE_QA_BROWSER || (process.platform==='darwin'?'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome':undefined);
 const browser=await chromium.launch({executablePath,headless:true});
 try{
 const page=await browser.newPage({viewport:{width:1500,height:1100},deviceScaleFactor:1});
 page.setDefaultTimeout(10000);
 const errors=[];page.on('pageerror',e=>{errors.push(String(e));console.log('PAGE ERROR',String(e))});
 page.on('console',m=>{if(m.type()==='error')console.log('CONSOLE',m.text())});
 const dist=path.resolve('dist');
 await page.route('https://clear-compare.test/**',async route=>{
  const name=decodeURIComponent(new URL(route.request().url()).pathname).replace(/^\//,'')||'index.html';
  const file=path.resolve(dist,name); if(!file.startsWith(dist+path.sep))return route.abort();
  const mime=name.endsWith('.wasm')?'application/wasm':name.endsWith('.js')?'text/javascript':name.endsWith('.css')?'text/css':name.endsWith('.html')?'text/html':'application/octet-stream';
  try{await route.fulfill({status:200,body:await fs.readFile(file),contentType:mime});}catch{await route.fulfill({status:404,body:''});}
 });
 await page.addInitScript(()=>{
  const entry=(kind,bytes)=>({kind,bytes:String(bytes),modified_ms:'1792108800000',digest:null,error:null});
  const rows=[{id:'d0',path:'src',status:'changed',left:entry('directory',200),right:entry('directory',210)}, {id:'f1',path:'src/main.rs',status:'changed',left:entry('file',200),right:entry('file',210)},{id:'f2',path:'notes.txt',status:'left_only',left:entry('file',64),right:null}];
  const summary={same:0,changed:1,left_only:1,right_only:0,unverified:0,errors:0};
  window.__qa={calls:[],verifyResolve:null,verifyReject:null,emptyText:false,endingDiff:false,files:{left:'head\nleftA\nmiddle\nleftB\nend\n',right:'head\nrightA\nmiddle\nrightB\nend\n'},documents:{},saves:[],externalEdit:false,guards:[]};
  let session=0;
  window.__TAURI__={core:{invoke:async(command,args)=>{
   args=JSON.parse(JSON.stringify(args,(_,v)=>v instanceof Map?Object.fromEntries(v):v));
   window.__qa.calls.push({command,args});
   if(command==='load_active_project')return null;
   if(command==='load_session')return {request:{left:'/sample/project-left',right:'/sample/project-right',exclusions:'-node_modules/',content:false},english:false};
   if(command==='save_session'||command==='set_native_language')return null;
   if(command==='active_operation')return null;
   if(command==='compare'){
    if(args.request.content)await new Promise((resolve,reject)=>{window.__qa.verifyResolve=resolve;window.__qa.verifyReject=reject});
    return {session:'s'+ ++session,left:'/sample/project-left',right:'/sample/project-right',summary,warnings:[],page:{rows:[],total:3}};
   }
   if(command==='query_folder'){
    const flat=!args.expanded;
    const visible=flat?(args.query.parent_id==='d0'?[rows[1]]:[rows[0],rows[2]]):rows.filter(r=>r.id!=='f1'||args.expandAll!==args.expanded.includes('d0')||args.query.search);
    const outline=Object.fromEntries(visible.map(r=>[r.id,{name:r.path.split('/').at(-1),depth:r.id==='f1'?1:0,has_children:r.id==='d0',expanded:r.id==='d0'&&visible.some(r=>r.id==='f1'),context_only:false}]));
    return {page:{rows:visible,total:visible.length},breadcrumbs:flat&&args.query.parent_id==='d0'?[{id:'d0',label:'src'}]:[],outline:flat?{}:outline};
   }
   if(command==='text_diff'){
    const left=window.__qa.files.left.match(/[^\n]*\n|[^\n]+$/g)||[],right=window.__qa.files.right.match(/[^\n]*\n|[^\n]+$/g)||[];const lines=[];
    for(let i=0;i<Math.max(left.length,right.length);i++){
     if(left[i]===right[i])lines.push({tag:'equal',old_line:i+1,new_line:i+1,text:left[i],right_text:right[i]});
     else{if(left[i]!==undefined)lines.push({tag:'delete',old_line:i+1,new_line:null,text:left[i]});if(right[i]!==undefined)lines.push({tag:'insert',old_line:null,new_line:i+1,text:right[i]});}
    }
    return {id:'text-'+window.__qa.calls.length,path:'src/main.rs',lines};
   }
   if(command==='open_document'){
    const d={id:'doc-'+window.__qa.calls.length,path:'src/main.rs',absolute_path:'/sample/'+args.side+'/src/main.rs',side:args.side,text:window.__qa.files[args.side],encoding:'utf8',bom:false,line_ending:'lf',digest:'before'};
    window.__qa.documents[d.id]=d;return d;
   }
   if(command==='close_document'){delete window.__qa.documents[args.documentId];return null;}
   if(command==='set_text_merge_dirty'){window.__qa.guards.push(args.dirty);return null;}
   if(command==='save_document'){
    const d=window.__qa.documents[args.documentId];
    if(window.__qa.failSide===d.side)throw new Error('Synthetic side save failed');
    if(window.__qa.externalEdit||window.__qa.files[d.side]!==d.text)throw new Error('File changed externally; save blocked');
    window.__qa.saves.push({side:d.side,text:args.text});window.__qa.files[d.side]=args.text;delete window.__qa.documents[d.id];
    const saved={...d,id:d.id+'-saved',text:args.text,digest:'after'};window.__qa.documents[saved.id]=saved;
    return {document:saved,backup:'/sample/backups/original.bin'};
   }
   if(command==='cancel'){if(window.__qa.verifyReject)window.__qa.verifyReject('Cancelled');return null;}
   throw new Error('Unexpected mock command '+command);
  }}};
 });
 async function open(configure){
  await page.goto('https://clear-compare.test/',{waitUntil:'domcontentloaded'});
  if(configure)await page.evaluate(configure);
  await page.getByRole('button',{name:'比较文件夹',exact:true}).click();
  await page.locator('.outline-row').first().waitFor();
  await page.getByRole('button',{name:'展开目录',exact:true}).first().click();
  await page.getByRole('button',{name:'main.rs',exact:true}).first().dblclick();
  await page.locator('.merge-arrow').first().waitFor();
 }
 const rightArrows=()=>page.getByRole('button',{name:'将此差异合并到右侧',exact:true});
 const leftArrows=()=>page.getByRole('button',{name:'将此差异合并到左侧',exact:true});
 const rightText=()=>page.locator('.shared-diff').nth(1).innerText();
 await open();assert.equal(await rightArrows().count(),2);
 await rightArrows().first().click();
 await page.getByRole('button',{name:'保存两侧',exact:true}).waitFor();
 await page.waitForFunction(()=>!document.querySelector('.progress'));
 assert((await rightText()).includes('leftA'));assert((await rightText()).includes('rightB'));
 assert.equal(await page.evaluate(()=>window.__qa.saves.length),0);
 assert.equal(await leftArrows().first().isEnabled(),true);
 assert.equal(await page.getByRole('button',{name:'返回目录',exact:true}).isDisabled(),true);
 assert.equal(await page.locator('.top-actions').evaluate(n=>n.inert),true);
 await page.getByRole('button',{name:'撤销合并',exact:true}).click();
 await page.waitForFunction(()=>!document.querySelector('.progress'));
 assert((await rightText()).includes('rightA'));
 assert.equal(await page.evaluate(()=>window.__qa.guards.at(-1)),false);
 await page.getByRole('button',{name:'重做合并',exact:true}).click();
 await page.waitForFunction(()=>!document.querySelector('.progress'));
 assert((await rightText()).includes('leftA'));
 await page.screenshot({path:'target/work/inline-merge-staged.png'});
 await rightArrows().first().click();
 await page.waitForFunction(()=>!document.querySelector('.progress'));
 assert((await rightText()).includes('leftB'));
 await page.getByRole('button',{name:'保存两侧',exact:true}).click();
 await page.waitForFunction(()=>window.__qa.saves.length===1&&!document.querySelector('.progress'));
 assert.deepEqual(await page.evaluate(()=>window.__qa.saves),[{side:'right',text:'head\nleftA\nmiddle\nleftB\nend\n'}]);
 assert.equal(await page.evaluate(()=>window.__qa.guards.at(-1)),false);
 assert.equal(await page.evaluate(()=>Object.keys(window.__qa.documents).length),0);
 assert.equal(await page.getByRole('button',{name:'返回目录',exact:true}).isEnabled(),true);
 // Reverse direction and a failed save must retain the buffer, without touching either file.
 await page.reload();await open();
 await leftArrows().first().click();await page.waitForFunction(()=>!document.querySelector('.progress'));
 assert((await page.locator('.shared-diff').first().innerText()).includes('rightA'));
 await page.evaluate(()=>{window.__qa.externalEdit=true});
 await page.getByRole('button',{name:'保存两侧',exact:true}).click();
 await page.getByRole('alert').waitFor();
 assert((await page.getByRole('alert').innerText()).includes('File changed externally'));
 assert.equal(await page.evaluate(()=>window.__qa.saves.length),0);
 assert.equal(await page.evaluate(()=>window.__qa.guards.at(-1)),true);
 assert.equal(await page.getByRole('button',{name:'返回目录',exact:true}).isDisabled(),true);
 await page.getByRole('button',{name:'放弃合并',exact:true}).click();await page.waitForFunction(()=>!document.querySelector('.progress'));
 assert.equal(await page.evaluate(()=>window.__qa.guards.at(-1)),false);
 assert.equal(await page.evaluate(()=>window.__qa.files.left),'head\nleftA\nmiddle\nleftB\nend\n');
 // An external source edit must also block saving the staged target.
 await leftArrows().first().click();await page.waitForFunction(()=>!document.querySelector('.progress'));
 await page.evaluate(()=>{window.__qa.files.right='outside source change\n';window.__qa.externalEdit=false});
 await page.getByRole('button',{name:'保存两侧',exact:true}).click();await page.waitForFunction(()=>!document.querySelector('.progress'));
 assert((await page.getByRole('alert').innerText()).includes('来源或目标在合并期间发生变化'));
 assert.equal(await page.evaluate(()=>window.__qa.saves.length),0);
 await page.getByRole('button',{name:'放弃合并',exact:true}).click();await page.waitForFunction(()=>!document.querySelector('.progress'));
 // Refuse stale comparison evidence before the first staged change.
 await page.evaluate(()=>{window.__qa.files.left='outside change\n';window.__qa.externalEdit=false});
 await leftArrows().first().click();await page.waitForFunction(()=>!document.querySelector('.progress'));
 assert((await page.getByRole('alert').innerText()).includes('文件在比较后发生变化'));
 assert.equal(await page.getByRole('button',{name:'返回目录',exact:true}).isEnabled(),true);
 assert.equal(await page.evaluate(()=>Object.keys(window.__qa.documents).length),0);
 // Both targets may be dirty simultaneously; shortcuts save only the focused side.
 await open();
 await rightArrows().first().click();await page.waitForFunction(()=>!document.querySelector('.progress'));
 await page.getByLabel('左侧文本',{exact:true}).focus();await page.keyboard.press('Control+s');
 await page.waitForFunction(()=>!document.querySelector('.progress'));
 assert.equal(await page.evaluate(()=>window.__qa.saves.length),0);
 await leftArrows().first().click();await page.waitForFunction(()=>!document.querySelector('.progress'));
 assert((await page.locator('.merge-state').innerText()).includes('左侧未保存'));
 assert((await page.locator('.merge-state').innerText()).includes('右侧未保存'));
 assert.equal(await page.evaluate(()=>window.__qa.saves.length),0);
 await page.getByLabel('左侧文本',{exact:true}).focus();await page.keyboard.press('Meta+s');
 await page.waitForFunction(()=>window.__qa.saves.length===1&&!document.querySelector('.progress'));
 assert.deepEqual(await page.evaluate(()=>window.__qa.saves),[{side:'left',text:'head\nleftA\nmiddle\nrightB\nend\n'}]);
 assert((await page.locator('.merge-state').innerText()).includes('右侧未保存'));
 assert(!(await page.locator('.merge-state').innerText()).includes('左侧未保存'));
 assert.equal(await page.evaluate(()=>window.__qa.guards.at(-1)),true);
 await page.getByLabel('右侧文本',{exact:true}).focus();await page.keyboard.press('Control+s');
 await page.waitForFunction(()=>window.__qa.saves.length===2&&!document.querySelector('.progress'));
 assert.equal(await page.evaluate(()=>window.__qa.saves[1].side),'right');
 assert.equal(await page.evaluate(()=>window.__qa.guards.at(-1)),false);
 // The button saves both. A second-side failure must retain that side and the first committed save.
 await open();
 await rightArrows().first().click();await page.waitForFunction(()=>!document.querySelector('.progress'));
 await leftArrows().first().click();await page.waitForFunction(()=>!document.querySelector('.progress'));
 await page.getByRole('button',{name:'保存两侧',exact:true}).click();
 await page.waitForFunction(()=>window.__qa.saves.length===2&&!document.querySelector('.progress'));
 assert.deepEqual(await page.evaluate(()=>window.__qa.saves.map(s=>s.side)),['left','right']);
 assert.equal(await page.evaluate(()=>window.__qa.guards.at(-1)),false);
 await open();
 await rightArrows().first().click();await page.waitForFunction(()=>!document.querySelector('.progress'));
 await leftArrows().first().click();await page.waitForFunction(()=>!document.querySelector('.progress'));
 await page.evaluate(()=>window.__qa.failSide='right');
 await page.getByRole('button',{name:'保存两侧',exact:true}).click();await page.waitForFunction(()=>!document.querySelector('.progress'));
 assert.deepEqual(await page.evaluate(()=>window.__qa.saves.map(s=>s.side)),['left']);
 assert((await page.locator('.merge-state').innerText()).includes('右侧未保存'));
 assert(!(await page.locator('.merge-state').innerText()).includes('左侧未保存'));
 assert.equal(await page.evaluate(()=>window.__qa.guards.at(-1)),true);
 await page.evaluate(()=>window.__qa.failSide=null);
 await page.getByLabel('右侧文本',{exact:true}).focus();await page.keyboard.press('Control+s');
 await page.waitForFunction(()=>window.__qa.saves.length===2&&!document.querySelector('.progress'));
 assert.equal(await page.evaluate(()=>window.__qa.guards.at(-1)),false);
 // A hunk continued on page 2 can shrink the file below the old page offset.
 await open(()=>{window.__qa.files.left=Array.from({length:10},(_,i)=>'L'+i+'\n').join('')+'end\n';window.__qa.files.right=Array.from({length:600},(_,i)=>'R'+i+'\n').join('')+'end\n';});
 await page.locator('.text-pagination').getByRole('button',{name:'下一页',exact:true}).click();
 assert((await page.locator('.text-pagination').innerText()).includes('501–601'));
 await rightArrows().first().click();await page.waitForFunction(()=>!document.querySelector('.progress'));
 assert((await page.locator('.text-pagination').innerText()).includes('1–11 / 11'));
 assert.equal(await page.locator('.shared-diff').first().locator('.diff-cell').count(),11);
 assert.equal(await page.getByRole('button',{name:'下一差异',exact:true}).isDisabled(),true);
 await page.getByRole('button',{name:'放弃合并',exact:true}).click();await page.waitForFunction(()=>!document.querySelector('.progress'));
 assert.deepEqual(errors,[]);
 console.log(JSON.stringify({passed:true,checks:['independent simultaneous left/right staging','focused Meta+S and Control+S','clean focused pane does not save opposite side','save both','partial-save failure and retry','per-hunk left-to-right','unrelated hunk retained','undo and redo','dirty target and navigation guard','explicit save only','right-to-left','failed-save buffer retained','discard','external source change rejected','stale target rejected','continued hunk and shrinking-page recovery'],pageErrors:errors}));
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exit(1)});
