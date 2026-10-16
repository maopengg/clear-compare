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
  window.__qa={calls:[],verifyResolve:null,verifyReject:null,emptyText:false,endingDiff:false};
  let session=0;
  window.__TAURI__={core:{invoke:async(command,args)=>{
   args=JSON.parse(JSON.stringify(args,(_,v)=>v instanceof Map?Object.fromEntries(v):v));
   window.__qa.calls.push({command,args});
   if(command==='load_session')return {request:{left:'/sample/project-left',right:'/sample/project-right',exclusions:'-node_modules/',content:false},english:false};
   if(command==='save_session')return null;
   if(command==='active_operation')return null;
   if(command==='compare'){
    if(args.request.content)await new Promise((resolve,reject)=>{window.__qa.verifyResolve=resolve;window.__qa.verifyReject=reject});
    return {session:'s'+ ++session,left:'/sample/project-left',right:'/sample/project-right',summary,warnings:['Symbolic links are listed but not followed, compared or synchronized.'],page:{rows:[],total:3}};
   }
   if(command==='query_folder'){
    const flat=!args.expanded;
    const visible=flat?(args.query.parent_id==='d0'?[rows[1]]:[rows[0],rows[2]]):rows.filter(r=>r.id!=='f1'||args.expandAll!==args.expanded.includes('d0')||args.query.search);
    const outline=Object.fromEntries(visible.map(r=>[r.id,{name:r.path.split('/').at(-1),depth:r.id==='f1'?1:0,has_children:r.id==='d0',expanded:r.id==='d0'&&visible.some(r=>r.id==='f1'),context_only:false}]));
    return {page:{rows:visible,total:visible.length},breadcrumbs:flat&&args.query.parent_id==='d0'?[{id:'d0',label:'src'}]:[],outline:flat?{}:outline};
   }
   if(command==='text_diff'&&window.__qa.emptyText)return {id:'text-empty',path:'src/main.rs',lines:[]};
   if(command==='text_diff'&&window.__qa.endingDiff)return {id:'text-endings',path:'src/main.rs',lines:[{tag:'delete',old_line:1,new_line:null,text:'old\n',changes:[]},{tag:'insert',old_line:null,new_line:1,text:'new\n',changes:[]},{tag:'delete',old_line:2,new_line:null,text:'same\n',changes:[{start:4,end:5}]},{tag:'insert',old_line:null,new_line:2,text:'same\r\n',changes:[{start:4,end:6}]}]};
   if(command==='text_diff')return {id:'text-1',path:'src/main.rs',lines:[{tag:'delete',old_line:1,new_line:null,text:'let version = 1;\n',right_text:null,changes:[{start:14,end:15}]},{tag:'insert',old_line:null,new_line:1,text:'let version = 2;\n',right_text:null,changes:[{start:14,end:15}]}]};
   if(command.startsWith('preview_')){
    if(window.__qa.failPreview)throw new Error('Synthetic preview failed');
    return {id:'plan-1',session:args.session,source_side:args.sourceSide||'left',actions:[],backup_root:'/sample/backups',target:'/sample/destination',policy:command==='preview_delete'?{mode:'delete_selected',target_side:args.targetSide}:command==='preview_rename'?{mode:'rename_selected',target_side:args.targetSide}:{mode:'selected_copy'}};
   }
   if(command==='open_document')throw new Error('Synthetic editor unavailable');
   if(command==='cancel'){if(window.__qa.verifyReject)window.__qa.verifyReject('Cancelled');return null;}
   throw new Error('Unexpected mock command '+command);
  }}};
 });
 await page.goto('https://clear-compare.test/',{waitUntil:'domcontentloaded',timeout:15000});
 await page.waitForTimeout(1500);
 await page.getByLabel('左侧 · 来源',{exact:true}).waitFor();
 await page.getByRole('button',{name:'比较文件夹',exact:true}).click();
 await page.locator('.outline-row').first().waitFor();
 assert.equal(await page.locator('.outline-row').count(),2);
 assert.equal(await page.locator('.directory-time').first().innerText()==='',false);
 assert.equal(await page.locator('.directory-name.absent .entry-name:visible').count(),0);
 await page.getByRole('button',{name:'展开目录',exact:true}).first().click();
 await page.getByRole('button',{name:'main.rs',exact:true}).first().waitFor();
 assert.equal(await page.locator('.outline-row').count(),3);
 assert.equal(await page.getByRole('button',{name:'main.rs',exact:true}).count(),2);
 await page.getByRole('button',{name:'main.rs',exact:true}).first().click();
 assert.equal(await page.locator('.outline-row.focused').count(),1);
 await page.locator('.outline-row.focused input[type=checkbox]').check();
 assert.equal(await page.getByRole('button',{name:'预览复制 →',exact:true}).isEnabled(),true);
 await page.locator('.outline-row.focused input[type=checkbox]').uncheck();
 const y=await page.getByRole('button',{name:'main.rs',exact:true}).evaluateAll(nodes=>nodes.map(n=>n.getBoundingClientRect().top));assert.equal(y[0],y[1]);

 const menu=()=>page.getByRole('menu',{name:'文件操作',exact:true});
 const left=page.getByRole('button',{name:'main.rs',exact:true}).first();
 const right=page.getByRole('button',{name:'main.rs',exact:true}).last();
 const calls=command=>page.evaluate(c=>window.__qa.calls.filter(x=>x.command===c),command);
 const action=(name)=>menu().getByRole('menuitem',{name,exact:true});
 const dismiss=async()=>{await page.waitForFunction(()=>!!document.activeElement?.closest('.file-context-menu'));await page.keyboard.press('Escape');await menu().waitFor({state:'detached'});};
 assert((await page.locator('.warnings').innerText()).includes('符号链接仅列出'));
 await left.click({button:'right'});await menu().waitFor();await page.waitForFunction(()=>document.activeElement?.getAttribute('role')==='menuitem');
 assert.equal(await action('复制到右侧…').isEnabled(),true);
 assert(!(await menu().innerText()).includes('Reload'));
 assert.equal(await page.locator('.outline-row.focused').getAttribute('title'),'src/main.rs');
 await page.keyboard.press('End');assert.equal(await page.evaluate(()=>document.activeElement.textContent),'属性');
 await page.keyboard.press('Home');assert.equal(await page.evaluate(()=>document.activeElement.textContent),'比较内容');
 await dismiss();assert.equal(await left.evaluate(e=>e===document.activeElement),true);
 // A context mutation intentionally targets the clicked file, replacing stale checkbox selection.
 await page.locator('.outline-row').filter({has:page.getByRole('button',{name:'notes.txt',exact:true})}).getByRole('checkbox').check();
 await left.click({button:'right'});await action('复制到右侧…').click();
 await page.getByRole('button',{name:'执行已预览的复制',exact:true}).waitFor();
 assert.deepEqual((await calls('preview_sync')).at(-1).args,{session:'s1',rowIds:['f1'],sourceSide:'left'});
 await page.getByRole('button',{name:'返回',exact:true}).click();
 await right.click({button:'right'});await action('复制到左侧…').click();
 await page.getByRole('button',{name:'执行已预览的复制',exact:true}).waitFor();
 assert.equal((await calls('preview_sync')).at(-1).args.sourceSide,'right');
 await page.getByRole('button',{name:'返回',exact:true}).click();
 await right.click({button:'right'});await action('重命名…').click();
 await menu().getByRole('textbox',{name:'新文件名',exact:true}).fill('renamed.rs');
 await action('预览重命名…').click();await page.getByRole('button',{name:'执行已预览的重命名',exact:true}).waitFor();
 assert.deepEqual((await calls('preview_rename')).at(-1).args,{session:'s1',rowId:'f1',targetSide:'right',newName:'renamed.rs'});
 await page.getByRole('button',{name:'返回',exact:true}).click();
 await right.click({button:'right'});await action('删除右侧文件…').click();await page.getByRole('button',{name:'执行已预览的删除',exact:true}).waitFor();
 assert.deepEqual((await calls('preview_delete')).at(-1).args,{session:'s1',rowIds:['f1'],targetSide:'right'});
 await page.getByRole('button',{name:'返回',exact:true}).click();
 assert.equal(await page.evaluate(()=>window.__qa.calls.filter(c=>/^(apply|start_)/.test(c.command)).length),0);
 await left.focus();await page.keyboard.press('Shift+F10');await menu().waitFor();await action('属性').click();
 assert((await menu().getByRole('region',{name:'属性',exact:true}).innerText()).includes('210 B'));
 await page.screenshot({path:'target/work/directory-context-menu.png'});await dismiss();
 // Clipboard uses the real browser API, not a successful mock result.
 await page.context().grantPermissions(['clipboard-read','clipboard-write']);
 await left.click({button:'right'});await action('复制文件名').click();await menu().waitFor({state:'detached'});
 assert.equal(await page.evaluate(()=>navigator.clipboard.readText()),'main.rs');
 await right.click({button:'right'});await action('编辑右侧文件').click();
 await page.waitForFunction(()=>window.__qa.calls.some(c=>c.command==='open_document'));
 assert.equal((await calls('open_document')).at(-1).args.side,'right');
 // Missing side and folders do not offer unsupported file mutation paths.
 const note=page.locator('.outline-row').filter({has:page.getByRole('button',{name:'notes.txt',exact:true})});
 await note.locator('td').nth(5).click({button:'right'});assert.equal(await action('复制到左侧…').isDisabled(),true);assert.equal(await action('删除右侧文件…').isDisabled(),true);await dismiss();
 await page.getByRole('button',{name:'src',exact:true}).first().click({button:'right'});assert.equal(await action('重命名…').isDisabled(),true);assert.equal(await action('展开 / 折叠目录').isEnabled(),true);await dismiss();
 await left.click({button:'right'});await page.mouse.click(5,5);await menu().waitFor({state:'detached'});
 await left.click({button:'right'});await action('比较内容').click();await page.locator('#text-comparison').waitFor();
 assert.equal((await calls('text_diff')).at(-1).args.rowId,'f1');
 await page.getByRole('button',{name:'返回目录',exact:true}).click();
 await page.getByRole('button',{name:'English',exact:true}).click();await right.click({button:'right'});
 const enMenu=page.getByRole('menu',{name:'File actions',exact:true});await enMenu.waitFor();assert.equal(await enMenu.getByRole('menuitem',{name:'Copy to left…',exact:true}).isEnabled(),true);
 await page.keyboard.press('Escape');
 // Bottom-right viewport edge must keep the full menu on screen.
 await page.setViewportSize({width:900,height:620});await left.dispatchEvent('contextmenu',{clientX:895,clientY:615,bubbles:true});await enMenu.waitFor();await page.waitForTimeout(100);
 const box=await enMenu.boundingBox();assert(box.x>=0 && box.y>=0 && box.x+box.width<=900 && box.y+box.height<=620);
 await page.keyboard.press('Escape');
 // Preview failures report an error without executing anything.
 await page.evaluate(()=>window.__qa.failPreview=true);await left.click({button:'right'});await enMenu.getByRole('menuitem',{name:'Copy to right…',exact:true}).click();
 await page.getByText(/Synthetic preview failed/).waitFor();assert.equal(await page.getByRole('button',{name:'Compare directories',exact:true}).isEnabled(),true);
 assert.equal(await page.evaluate(()=>window.__qa.calls.filter(c=>/^(apply|start_)/.test(c.command)).length),0);
 assert.deepEqual(errors,[]);
 console.log(JSON.stringify({passed:true,checks:['Chinese and English menu','clicked row and side','preview only copy/delete/rename','stale selection isolation','properties','real clipboard','edit and compare IDs','missing side and directory restrictions','keyboard navigation and focus restoration','outside dismissal','viewport clamping','preview failure retention'],pageErrors:errors}));
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exit(1)});
