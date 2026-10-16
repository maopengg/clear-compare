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
  window.__qa={rows,summary,calls:[],verifyResolve:null,verifyReject:null,emptyText:false,endingDiff:false};
  let session=0;
  window.__TAURI__={core:{invoke:async(command,args)=>{
   args=JSON.parse(JSON.stringify(args,(_,v)=>v instanceof Map?Object.fromEntries(v):v));
   window.__qa.calls.push({command,args});
   if(command==='load_active_project')return null;
   if(command==='load_session')return {request:{left:'/sample/project-left',right:'/sample/project-right',exclusions:'-node_modules/',content:false},english:false};
   if(command==='save_session'||command==='set_native_language')return null;
   if(command==='active_operation')return null;
   if(command==='compare'){
    if(window.__qa.delayCompare)await new Promise(resolve=>{window.__qa.compareResolve=resolve});
    if(args.request.content)await new Promise((resolve,reject)=>{window.__qa.verifyResolve=resolve;window.__qa.verifyReject=reject});
    return {session:'s'+ ++session,left:'/sample/project-left',right:'/sample/project-right',summary,warnings:['Symbolic links are listed but not followed, compared or synchronized.'],page:{rows:[],total:3}};
   }
   if(command==='query_folder'){
    const flat=!args.expanded;
    let visible=flat?(args.query.parent_id==='d0'?[rows[1]]:[rows[0],rows[2]]):rows.filter(r=>r.id!=='f1'||args.expandAll!==args.expanded.includes('d0')||args.query.search);
    if(args.query.differences_only)visible=visible.filter(r=>r.status!=='same');
    const outline=Object.fromEntries(visible.map(r=>[r.id,{name:r.path.split('/').at(-1),depth:r.id==='f1'?1:0,has_children:r.id==='d0',expanded:r.id==='d0'&&visible.some(r=>r.id==='f1'),context_only:false}]));
    return {page:{rows:visible,total:visible.length},breadcrumbs:flat&&args.query.parent_id==='d0'?[{id:'d0',label:'src'}]:[],outline:flat?{}:outline};
   }
   if(command==='text_diff'&&window.__qa.emptyText)return {id:'text-empty',path:'src/main.rs',lines:[]};
   if(command==='text_diff'&&window.__qa.endingDiff)return {id:'text-endings',path:'src/main.rs',lines:[{tag:'delete',old_line:1,new_line:null,text:'old\n',changes:[]},{tag:'insert',old_line:null,new_line:1,text:'new\n',changes:[]},{tag:'delete',old_line:2,new_line:null,text:'same\n',changes:[{start:4,end:5}]},{tag:'insert',old_line:null,new_line:2,text:'same\r\n',changes:[{start:4,end:6}]}]};
   if(command==='text_diff')return {id:'text-1',path:'src/main.rs',lines:[{tag:'delete',old_line:1,new_line:null,text:'let version = 1;\n',right_text:null,changes:[{start:14,end:15}]},{tag:'insert',old_line:null,new_line:1,text:'let version = 2;\n',right_text:null,changes:[{start:14,end:15}]}]};
   if(command.startsWith('preview_')){
    if(window.__qa.delayPreview)await new Promise(resolve=>{window.__qa.previewResolve=resolve});
    if(window.__qa.failPreview)throw new Error('Synthetic preview failed');
    const action={operation:'copy',row_id:args.rowIds?.[0]||args.rowId||'f1',path:'src/main.rs',target_existed:true,bytes:'200',source_digest:'digest',target_digest:'old'};
    const plan={id:'plan-1',session:args.session,source_side:args.sourceSide||'left',actions:[action],backup_root:'/sample/backups',target:'/sample/destination',policy:command==='preview_delete'?{mode:'delete_selected',target_side:args.targetSide}:command==='preview_rename'?{mode:'rename_selected',target_side:args.targetSide}:{mode:'selected_copy'}};window.__qa.plan=plan;return plan;
   }
   if(command==='apply_sync'){if(window.__qa.failApply)throw new Error('Synthetic copy failed');return {id:'copy-task',kind:'file'};}
   if(command==='operation_status')return {handle:{id:'copy-task',kind:'file'},phase:'completed',stage:'copy',current_path:null,completed_actions:1,total_actions:1,bytes_read:'200',bytes_written:'200',elapsed_ms:'1',error:null};
   if(command==='take_operation_result')return {kind:'file',value:{result:{plan:window.__qa.plan,actions:[{path:'src/main.rs',state:'completed',error:null}],journal:'/sample/journal'},locations:[],stale_documents:[],warnings:[]}};
   if(command==='refresh_compared_file'){
    if(window.__qa.failRefresh)throw new Error('Synthetic refresh failed');
    const row=rows.find(r=>r.id===args.rowId);if(window.__qa.plan.source_side==='left')row.right={...row.left};else row.left={...row.right};row.status='same';rows[0].status='same';summary.changed=0;summary.same=1;
    return {session:args.session,left:'/sample/project-left',right:'/sample/project-right',summary,warnings:[],page:{rows:[],total:3}};
   }
   if(command==='open_document')throw new Error('Synthetic editor unavailable');
   if(command==='cancel'){if(window.__qa.verifyReject)window.__qa.verifyReject('Cancelled');return null;}
   throw new Error('Unexpected mock command '+command);
  }}};
 });
 await page.goto('https://clear-compare.test/',{waitUntil:'domcontentloaded',timeout:15000});
 await page.waitForTimeout(1500);
 await page.getByLabel('左侧 · 来源',{exact:true}).waitFor();
 await page.evaluate(()=>window.__qa.delayCompare=true);
 await page.getByRole('button',{name:'比较文件夹',exact:true}).click();
 await page.waitForFunction(()=>typeof window.__qa.compareResolve==='function');
 await page.locator('.workspace-progress:not(.bottom-progress) .progress').waitFor();
 assert.equal(await page.locator('.workspace-progress.bottom-progress').count(),0);
 await page.evaluate(()=>{window.__qa.delayCompare=false;window.__qa.compareResolve();});
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


 const count=command=>page.evaluate(c=>window.__qa.calls.filter(x=>x.command===c),command);
 const row=()=>page.locator('.outline-row').filter({has:page.getByRole('button',{name:'main.rs',exact:true})});
 const arrow=right=>row().getByRole('button',{name:right?'往右合并文件':'往左合并文件',exact:true});
 async function fresh(all=true){await page.reload();await page.getByRole('button',{name:'比较文件夹',exact:true}).click();await page.locator('.outline-row').first().waitFor();if(all)await page.getByRole('checkbox',{name:'只看差异',exact:true}).uncheck();await page.getByRole('button',{name:'展开目录',exact:true}).first().click();await page.getByRole('button',{name:'main.rs',exact:true}).first().waitFor();}
 // Familiar icon silhouettes and local labels; no ambiguous DIR/LINK glyphs.
 assert.equal(await page.locator('svg.folder-icon').count(),2);assert.equal(await page.locator('svg.folder-icon[data-expanded=true]').count(),2);
 assert.equal(await page.locator('.directory-size').first().innerText(),'—');assert.equal(await page.locator('.directory-size').first().getAttribute('title'),'文件夹');
 assert.equal(await page.locator('.entry-icon').evaluateAll(nodes=>nodes.every(n=>n.tagName.toLowerCase()==='svg')),true);
 const note=page.locator('.outline-row').filter({has:page.getByRole('button',{name:'notes.txt',exact:true})});assert.equal(await note.getByRole('button',{name:'往左合并文件',exact:true}).isDisabled(),true);
 assert.equal(await page.locator('.directory-size').nth(2).getAttribute('title'),'200 字节；KB = 1000 字节');
 assert.equal(await page.getByRole('button',{name:'十六进制',exact:true}).count(),1);
 await page.getByRole('checkbox',{name:'只看差异',exact:true}).uncheck();
 await page.evaluate(()=>window.__qa.delayPreview=true);await arrow(true).click();await page.waitForFunction(()=>typeof window.__qa.previewResolve==='function');
 assert.equal(await page.locator('.warnings').count(),0);
 const loading=page.locator('.workspace-progress.bottom-progress .progress').last();await loading.waitFor();
 const loadingBounds=await loading.boundingBox(), contentBounds=await page.locator('.directory-scroll').boundingBox();
 assert(loadingBounds.y>=contentBounds.y+contentBounds.height,'Loading belongs below existing directory contents');
 assert(loadingBounds.height<=35,'Loading remains compact');
 assert.equal(await arrow(false).isDisabled(),true);assert.equal(await arrow(true).isDisabled(),true);assert.equal(await page.getByRole('button',{name:'执行已预览的复制',exact:true}).count(),0);
 await page.evaluate(()=>{window.__qa.delayPreview=false;window.__qa.previewResolve();});await page.getByText(/文件已合并，来源文件保留/).waitFor();
 assert.deepEqual((await count('preview_sync')).at(-1).args,{session:'s1',rowIds:['f1'],sourceSide:'left'});assert.equal((await count('apply_sync')).length,1);assert.equal((await count('compare')).length,1);
 assert.equal((await count('refresh_compared_file')).at(-1).args.rowId,'f1');assert.equal(await arrow(true).isDisabled(),true);assert.equal(await page.locator('.outline-row').count(),3);
 assert.equal(await page.locator('svg.folder-icon[data-expanded=true]').count(),2);
 await fresh();await arrow(false).focus();await page.keyboard.press('Enter');await page.getByText(/文件已合并，来源文件保留/).waitFor();assert.equal((await count('preview_sync')).at(-1).args.sourceSide,'right');assert.equal((await count('apply_sync')).length,1);
 await fresh(false);await arrow(true).click();await page.getByText(/文件已合并，来源文件保留/).waitFor();assert.equal(await row().count(),0);
 await fresh();await page.evaluate(()=>window.__qa.failPreview=true);await arrow(true).click();await page.getByText(/Synthetic preview failed/).waitFor();assert.equal((await count('apply_sync')).length,0);assert.equal(await arrow(true).isEnabled(),true);
 await fresh();await page.evaluate(()=>window.__qa.failApply=true);await arrow(true).click();await page.getByText(/Synthetic copy failed/).waitFor();assert.equal((await count('refresh_compared_file')).length,0);assert.equal(await row().count(),0);assert.equal(await page.getByRole('button',{name:'比较文件夹',exact:true}).isEnabled(),true);
 await fresh();await page.evaluate(()=>window.__qa.failRefresh=true);await arrow(true).click();await page.getByText(/Synthetic refresh failed/).waitFor();assert.equal((await count('apply_sync')).length,1);
 // Dense visual sample and a file size that previously used an ellipsis.
 await fresh();await page.evaluate(()=>{const row=window.__qa.rows[1];row.left.bytes='155300';row.right.bytes='155300';});await page.getByRole('checkbox',{name:'只看差异',exact:true}).check();await page.getByRole('checkbox',{name:'只看差异',exact:true}).uncheck();
 const sizes=await row().locator('.directory-size').allTextContents();assert(sizes.every(v=>v==='155.3 KB'));
 assert.equal(await row().locator('.directory-size').first().evaluate(n=>n.scrollWidth<=n.clientWidth),true);
 await page.screenshot({path:'target/work/familiar-directory-icons.png'});
 await page.getByRole('button',{name:'English',exact:true}).click();await page.waitForFunction(()=>window.__qa.calls.some(c=>c.command==='set_native_language'&&c.args.english===true));
 assert.deepEqual(errors,[]);console.log(JSON.stringify({passed:true,checks:['familiar folder/file SVGs','localized type/size hints','untruncated decimal sizes','direct copy both directions without modal','single native ID and internal plan','busy guard','single-pair refresh and retained expansion','differences filter refresh','preview/apply/refresh failures','native language bridge'],pageErrors:errors}));
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exit(1)});
