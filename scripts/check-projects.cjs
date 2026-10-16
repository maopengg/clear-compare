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
  const saved=(left,right,exclusions)=>({request:{left,right,exclusions,content:false},english:false,view:{search:'',differences_only:true,recursive:true,sort:'path',descending:false,reverse_sync:false,text_options:{ignore_case:false,ignore_whitespace:false,ignore_line_endings:false}}});
  let profiles=JSON.parse(localStorage.getItem('qa-projects')||'null')||{active:'a0',next:0,entries:[{id:'a0',name:'项目 A',revision:'a1',updated_ms:'1',session:saved('/sample/project-left','/sample/project-right','-node_modules/')},{id:'b0',name:'项目 B',revision:'b1',updated_ms:'2',session:saved('/sample/b-left','/sample/b-right','-*.tmp;-./cache/')} ]};
  const persist=()=>localStorage.setItem('qa-projects',JSON.stringify(profiles));persist();
  window.__qa={calls:[],verifyResolve:null,verifyReject:null,emptyText:false,endingDiff:false};
  let session=0;
  window.__TAURI__={core:{invoke:async(command,args)=>{
   args=JSON.parse(JSON.stringify(args,(_,v)=>v instanceof Map?Object.fromEntries(v):v));
   window.__qa.calls.push({command,args});
   if(command==='workspace_catalog')return {settings:{roots:[],exclusions:[]},projects:[],repositories:[],warnings:[],scanned_directories:0,truncated:false};
   if(command==='load_active_project')return profiles.entries.find(e=>e.id===profiles.active)||null;
   if(command==='load_session')return {request:{left:'/sample/project-left',right:'/sample/project-right',exclusions:'-node_modules/',content:false},english:false};
   if(command==='save_session'||command==='set_native_language')return null;
   if(command==='active_operation')return null;
   if(command==='list_sessions')return profiles.entries;
   if(command==='activate_project'){const entry=profiles.entries.find(e=>e.id===args.id);if(!entry)throw new Error('Unknown project');profiles.active=entry.id;persist();return entry;}
   if(command==='save_library_session'){
    if(args.id&&window.__qa.externalConflict)throw new Error('Project changed externally');
    let entry=args.id?profiles.entries.find(e=>e.id===args.id):null;if(args.id&&(!entry||entry.revision!==args.revision))throw new Error('Project changed externally');
    entry={id:entry?.id||'c'+profiles.next++,name:args.name,revision:'r'+Date.now()+Math.random(),updated_ms:String(Date.now()),session:args.session};profiles.entries=profiles.entries.filter(e=>e.id!==entry.id);profiles.entries.unshift(entry);profiles.active=entry.id;persist();return entry;
   }
   if(command==='archive_session'){profiles.entries=profiles.entries.filter(e=>e.id!==args.id);if(profiles.active===args.id)profiles.active=null;persist();return null;}
   if(command==='exclusion_for_entry')return args.current+';\n-./'+({'f2':'notes.txt','d0':'src/','f1':'src/main.rs'}[args.rowId]);
   if(command==='validate_exclusions'){if(args.rules.includes('-['))throw new Error('Invalid custom rule');return null;}

   if(command==='compare'){window.__qa.requestRules=args.request.exclusions;
    if(args.request.content)await new Promise((resolve,reject)=>{window.__qa.verifyResolve=resolve;window.__qa.verifyReject=reject});
    return {session:'s'+ ++session,left:args.request.left,right:args.request.right,summary,warnings:[],page:{rows:[],total:3}};
   }
   if(command==='query_folder'){
    const flat=!args.expanded;
    let visible=flat?(args.query.parent_id==='d0'?[rows[1]]:[rows[0],rows[2]]):rows.filter(r=>r.id!=='f1'||args.expandAll!==args.expanded.includes('d0')||args.query.search);
    if(window.__qa.requestRules?.includes('-./notes.txt'))visible=visible.filter(r=>r.id!=='f2');
    if(window.__qa.requestRules?.includes('-./src/'))visible=visible.filter(r=>!['f1','d0'].includes(r.id));
    const outline=Object.fromEntries(visible.map(r=>[r.id,{name:r.path.split('/').at(-1),depth:r.id==='f1'?1:0,has_children:r.id==='d0',expanded:r.id==='d0'&&visible.some(r=>r.id==='f1'),context_only:false}]));
    return {page:{rows:visible,total:visible.length},breadcrumbs:flat&&args.query.parent_id==='d0'?[{id:'d0',label:'src'}]:[],outline:flat?{}:outline};
   }
   if(command==='text_diff'&&window.__qa.emptyText)return {id:'text-empty',path:'src/main.rs',lines:[]};
   if(command==='text_diff'&&window.__qa.endingDiff)return {id:'text-endings',path:'src/main.rs',lines:[{tag:'delete',old_line:1,new_line:null,text:'old\n',changes:[]},{tag:'insert',old_line:null,new_line:1,text:'new\n',changes:[]},{tag:'delete',old_line:2,new_line:null,text:'same\n',changes:[{start:4,end:5}]},{tag:'insert',old_line:null,new_line:2,text:'same\r\n',changes:[{start:4,end:6}]}]};
   if(command==='text_diff')return {id:'text-1',path:'src/main.rs',lines:[{tag:'delete',old_line:1,new_line:null,text:'let version = 1;\n',right_text:null,changes:[{start:14,end:15}]},{tag:'insert',old_line:null,new_line:1,text:'let version = 2;\n',right_text:null,changes:[{start:14,end:15}]}]};
   if(command==='cancel'){if(window.__qa.verifyReject)window.__qa.verifyReject('Cancelled');return null;}
   throw new Error('Unexpected mock command '+command);
  }}};
 });
 await page.goto('https://clear-compare.test/',{waitUntil:'domcontentloaded',timeout:15000});
 await page.getByRole('button',{name:'文件夹比较',exact:true}).click();
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

 const store=()=>page.evaluate(()=>JSON.parse(localStorage.getItem('qa-projects')));
 const manager=()=>page.getByRole('dialog',{name:'项目管理',exact:true});
 const rulesDialog=()=>page.getByRole('dialog',{name:'项目排除规则',exact:true});
 const openRules=async()=>{await page.locator('.identity').getByRole('button',{name:/^排除 /}).click();await rulesDialog().waitFor();};
 const addRule=async value=>{await rulesDialog().getByRole('textbox',{name:'新增排除规则',exact:true}).fill(value);await rulesDialog().getByRole('button',{name:'添加',exact:true}).click();};
 const applyRules=async()=>{await rulesDialog().getByRole('button',{name:'应用并保存',exact:true}).click();await rulesDialog().waitFor({state:'hidden'});await page.waitForFunction(()=>!document.querySelector('.progress'));};
 const openManager=async()=>{await page.locator('.project-picker').click();await manager().waitFor();};
 const switchTo=async name=>{await manager().locator('.project-entry').filter({has:page.getByText(name,{exact:true})}).locator('.project-open').click();await manager().waitFor({state:'hidden'});};
 assert((await page.locator('.project-picker').innerText()).includes('项目 A'));
 const originalB=(await store()).entries.find(e=>e.id==='b0').session;
 await page.getByRole('button',{name:'notes.txt',exact:true}).click({button:'right'});await page.getByRole('menuitem',{name:'排除此项',exact:true}).click();
 await page.getByRole('button',{name:'notes.txt',exact:true}).waitFor({state:'detached'});
 let data=await store();assert(data.entries.find(e=>e.id==='a0').session.request.exclusions.includes('-./notes.txt'));assert.deepEqual(data.entries.find(e=>e.id==='b0').session,originalB);
 await openRules();await addRule('*.cache');await rulesDialog().getByRole('button',{name:'移除排除规则 -node_modules/',exact:true}).click();await applyRules();
 data=await store();assert(data.entries.find(e=>e.id==='a0').session.request.exclusions.includes('-*.cache'));assert(!data.entries.find(e=>e.id==='a0').session.request.exclusions.includes('node_modules'));
 const validRules=data.entries.find(e=>e.id==='a0').session.request.exclusions;
 await openRules();await rulesDialog().getByText('批量编辑规则',{exact:true}).click();await rulesDialog().getByRole('textbox',{name:'Exclusion rule draft',exact:true}).fill('-[');await rulesDialog().getByRole('button',{name:'应用并保存',exact:true}).click();await page.getByText(/Invalid custom rule/).waitFor();assert.equal((await store()).entries.find(e=>e.id==='a0').session.request.exclusions,validRules);await rulesDialog().getByRole('button',{name:'取消',exact:true}).click();
 await openManager();assert.equal(await manager().locator('.project-entry').count(),2);assert((await manager().locator('.project-rule-summary').innerText()).includes('*.cache'));await page.screenshot({path:'target/work/project-manager.png'});
 await switchTo('项目 B');assert.equal(await page.getByLabel('左侧 · 来源',{exact:true}).inputValue(),'/sample/b-left');
 await openRules();await addRule('*.bak');await applyRules();
 await openManager();await manager().getByRole('textbox',{name:'项目名称',exact:true}).fill('项目 B 改名');await switchTo('项目 A');
 data=await store();assert.equal(data.entries.find(e=>e.id==='b0').name,'项目 B 改名');assert(data.entries.find(e=>e.id==='b0').session.request.exclusions.includes('*.bak'));assert.equal(await page.getByLabel('左侧 · 来源',{exact:true}).inputValue(),'/sample/project-left');
 // Right-clicking a folder excludes its whole captured subtree in this project.
 await page.getByRole('button',{name:'比较文件夹',exact:true}).click();await page.getByRole('button',{name:'src',exact:true}).first().waitFor();await page.getByRole('button',{name:'src',exact:true}).first().click({button:'right'});await page.getByRole('menuitem',{name:'排除此项',exact:true}).click();await page.getByRole('button',{name:'src',exact:true}).first().waitFor({state:'detached'});assert((await store()).entries.find(e=>e.id==='a0').session.request.exclusions.includes('-./src/'));
 await openManager();await manager().getByRole('button',{name:'新建项目',exact:true}).click();await manager().getByRole('textbox',{name:'项目名称',exact:true}).fill('项目 C');await manager().getByRole('textbox',{name:'项目左侧文件夹',exact:true}).fill('/sample/c-left');await manager().getByRole('textbox',{name:'项目右侧文件夹',exact:true}).fill('/sample/c-right');await manager().getByRole('button',{name:'保存项目',exact:true}).click();await page.waitForFunction(()=>JSON.parse(localStorage.getItem('qa-projects')).entries.some(e=>e.name==='项目 C'));await manager().getByRole('button',{name:'关闭',exact:true}).click();await manager().waitFor({state:'hidden'});
 await page.reload();await page.getByRole('button',{name:'文件夹比较',exact:true}).click();await page.waitForFunction(()=>document.querySelector('.project-picker')?.textContent.includes('项目 C'));assert.equal(await page.getByLabel('右侧 · 目标',{exact:true}).inputValue(),'/sample/c-right');
 await openManager();await switchTo('项目 A');await openManager();await manager().getByRole('textbox',{name:'项目名称',exact:true}).fill('项目 A 本地修改');
 await page.evaluate(()=>{const p=JSON.parse(localStorage.getItem('qa-projects'));p.entries.find(e=>e.id==='a0').revision='external';localStorage.setItem('qa-projects',JSON.stringify(p));});
 // The mock backend reads the shared persisted revision, like a second client.
 await page.evaluate(()=>window.__qa.externalConflict=true);
 await manager().locator('.project-entry').filter({has:page.getByText('项目 C',{exact:true})}).locator('.project-open').click();await page.getByText(/Project changed externally/).waitFor();assert.equal(await manager().getByRole('textbox',{name:'项目名称',exact:true}).inputValue(),'项目 A 本地修改');assert((await page.locator('.project-picker').innerText()).includes('项目 A 本地修改'));
 await manager().getByRole('button',{name:'另存为新项目',exact:true}).click();await page.waitForFunction(()=>JSON.parse(localStorage.getItem('qa-projects')).entries.some(e=>e.name==='项目 A 本地修改'));await manager().getByRole('button',{name:'关闭',exact:true}).click();await manager().waitFor({state:'hidden'});
 await openRules();await page.screenshot({path:'target/work/project-exclusions.png'});await rulesDialog().getByRole('button',{name:'取消',exact:true}).click();
 assert.deepEqual(errors,[]);console.log(JSON.stringify({passed:true,checks:['right-click file and folder exclusion','custom add/remove and invalid rule retention','independent project rules','switch saves edited profile','new project and rename','restart restores active project','conflict retains draft and save-as recovery'],pageErrors:errors}));
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exit(1)});
