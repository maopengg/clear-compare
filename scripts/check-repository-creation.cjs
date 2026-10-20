// Creation UI against real WASM with synthetic IPC; native tests cover real Git and disk effects.
const {chromium}=require('playwright');const fs=require('fs/promises');const path=require('path');const assert=require('assert');
(async()=>{
 const browser=await chromium.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true});
 try{
 const page=await browser.newPage({viewport:{width:1440,height:1000}});const errors=[];page.on('pageerror',e=>errors.push(String(e)));page.setDefaultTimeout(10000);
 await page.route('https://clear-compare.test/**',async route=>{const n=decodeURIComponent(new URL(route.request().url()).pathname).replace(/^\//,'')||'index.html';const root=path.resolve('dist'),file=path.resolve(root,n);if(!file.startsWith(root+path.sep))return route.abort();try{await route.fulfill({body:await fs.readFile(file),contentType:n.endsWith('.wasm')?'application/wasm':n.endsWith('.js')?'text/javascript':n.endsWith('.css')?'text/css':'text/html'});}catch{await route.fulfill({status:404,body:''});}});
 await page.addInitScript(()=>{
  const session={request:{left:'/sample/left',right:'/sample/right',exclusions:'-node_modules/',content:false},english:false};
  const project={id:'p1',name:'代码同步',revision:'r1',updated_ms:'1',session};
  const repo=(id,name,kind='repository')=>({id,name,path:'/sample/code/'+name,kind,common_id:kind==='worktree'?'common-a':id,match_key:'example.com/team/app',branch:'main',head:'a'.repeat(40),changed:2,conflicts:0,ahead:1,behind:2,favorite:false,last_opened:'',available:true,error:null});
  const catalog={settings:{roots:['/sample/code'],exclusions:['node_modules']},projects:[project],repositories:[repo('opaque-1','app'),repo('opaque-2','app-copy'),repo('opaque-3','parallel','worktree')],warnings:[],scanned_directories:123,truncated:false};
  window.__qa={catalog,calls:[],scanArgs:null,index:'.',worktree:'M',pending:null,failApply:false};
  window.__TAURI__={core:{invoke:async(command,args)=>{
   args=JSON.parse(JSON.stringify(args,(_,v)=>v instanceof Map?Object.fromEntries(v):v));window.__qa.calls.push({command,args});
   if(command==='pick_directory')return '/sample/parent';
   if(command==='workspace_creation_prepare'){
    if(window.__qa.failPrepare)throw Error('Repository destination already exists');
    const plan={id:'creation-native',destination:args.parent+'/'+args.name,operation:args.operation};window.__qa.creation=plan;return plan;
   }
   if(command==='workspace_creation_apply'){
    if(window.__qa.failCreation)throw Error('Repository parent changed after review');
    const result={status:'registered',path:window.__qa.creation.destination,journal:'/sample/log/creation-native',output:'Created <literal> repository',issue:null,catalog};
    if(window.__qa.unregistered){result.status='created_unregistered';result.catalog=null;result.issue='Synthetic settings save failure';return result;}
    if(window.__qa.cancelled){result.status='cancelled';result.path='/sample/parent/.clear-compare-create-native';result.catalog=null;result.issue='Operation cancelled';return result;}
    if(window.__qa.hold)return new Promise(resolve=>window.__qa.resolveCreation=()=>{result.status='cancelled';result.catalog=null;result.path='/sample/parent/.clear-compare-create-native';result.issue='Operation cancelled';resolve(result);});
    catalog.repositories.push({...repo('created-native','created'),path:result.path});return result;
   }
   if(command==='cancel'){window.__qa.resolveCreation?.();return null;}
   if(command==='workspace_registration_prepare')return {id:'registration-native',repository:repo('recovered','recovered'),previous:null,existing:null};
   if(command==='load_active_project')return project;
   if(command==='workspace_catalog')return catalog;
   if(['set_native_language','save_session'].includes(command))return null;
   if(command==='active_operation')return null;
   if(command==='workspace_scan'){window.__qa.scanArgs=args;catalog.settings=args.settings;return catalog;}
   if(command==='workspace_favorite'){catalog.repositories.find(r=>r.id===args.repositoryId).favorite=args.favorite;return catalog;}
   if(command==='workspace_prepare'){window.__qa.pending=args;return {id:'plan-token',repository_id:args.repositoryId,revision:args.revision,arguments:[args.action.kind,'selected-file'],dangerous:args.action.kind==='discard',backup_reference:null};}
   if(command==='workspace_apply'){if(window.__qa.failApply)throw new Error('Synthetic hook failure');const action=window.__qa.pending.action;if(action.kind==='stage'){window.__qa.index='M';window.__qa.worktree='.';}if(action.kind==='unstage'){window.__qa.index='.';window.__qa.worktree='M';}return {output:'Git action complete',backup_reference:null,state:await window.__TAURI__.core.invoke('workspace_open',{repositoryId:window.__qa.pending.repositoryId})};}
   if(command==='workspace_open'){const r=catalog.repositories.find(r=>r.id===args.repositoryId);if(!r)throw new Error('Unknown repository');r.last_opened='123';return {repository:r,revision:'opaque-revision',operation:null,changes:[{id:'file-id',path:'src/main.rs',original_path:null,index:window.__qa.index,worktree:window.__qa.worktree,untracked:false,conflicted:false}],references:[{id:'branch-id',name:'main',full_name:'refs/heads/main',target:'a'.repeat(40),kind:'local',upstream:'origin/main',tracking:'[ahead 1, behind 2]',current:true}],remotes:[{name:'origin',fetch:'https://example.com/team/app.git',push:'git@example.com:team/app.git',match_key:'example.com/team/app'}]};}
   if(command==='list_sessions')return [project];
   if(command==='activate_project')return project;
   throw new Error('Unexpected command '+command);
  }}};
 });
 await page.goto('https://clear-compare.test/',{waitUntil:'domcontentloaded'});await page.locator('.workbench-repository').first().waitFor();
 const calls=async name=>page.evaluate(n=>window.__qa.calls.filter(c=>c.command===n),name);
 const entry=page.getByRole('button',{name:'克隆 / 初始化',exact:true});const panel=page.getByRole('region',{name:'克隆或初始化仓库',exact:true});const review=page.getByRole('region',{name:'确认创建仓库',exact:true});
 await entry.click();assert(await page.getByLabel('创建方式',{exact:true}).evaluate(e=>e===document.activeElement));await panel.getByRole('button',{name:'选择父目录…',exact:true}).click();await page.waitForFunction(()=>document.querySelector('[aria-label="新仓库父目录"]').value==='/sample/parent');await page.getByLabel('新仓库目录名',{exact:true}).fill('new-copy');await page.getByLabel('克隆来源',{exact:true}).fill('https://example.com/team/repo.git');await page.getByLabel('创建分支或标签',{exact:true}).fill('release/v1');await page.getByLabel('浅克隆深度',{exact:true}).fill('3');await page.getByLabel('裸仓库（无工作区）',{exact:true}).check();await panel.getByRole('button',{name:'检查创建计划',exact:true}).click();await review.waitFor();
 assert.equal((await calls('workspace_creation_apply')).length,0);assert.deepEqual((await calls('workspace_creation_prepare')).at(-1).args,{parent:'/sample/parent',name:'new-copy',operation:{kind:'clone',url:'https://example.com/team/repo.git',branch:'release/v1',bare:true,depth:3}});
 await page.getByLabel('新仓库目录名',{exact:true}).fill('changed');assert.equal(await review.count(),0);await panel.press('Escape');await panel.waitFor({state:'hidden'});assert(await entry.evaluate(e=>e===document.activeElement));
 await entry.click();await page.getByLabel('创建方式',{exact:true}).selectOption('init');assert.equal(await page.getByLabel('克隆来源',{exact:true}).count(),0);assert.equal(await page.getByLabel('创建分支或标签',{exact:true}).inputValue(),'main');await page.getByLabel('裸仓库（无工作区）',{exact:true}).uncheck();await panel.getByRole('button',{name:'检查创建计划',exact:true}).click();await review.waitFor();assert.deepEqual((await calls('workspace_creation_prepare')).at(-1).args.operation,{kind:'init',branch:'main',bare:false});
 await page.evaluate(()=>window.__qa.failCreation=true);await panel.getByRole('button',{name:'确认执行创建',exact:true}).click();await page.getByText('父目录在检查后发生变化，请重新检查。',{exact:true}).waitFor();assert.equal(await review.count(),0);assert.equal(await page.getByLabel('新仓库目录名',{exact:true}).inputValue(),'changed');assert.equal(await page.locator('.workbench-repository').count(),3);
 await page.evaluate(()=>{window.__qa.failCreation=false;window.__qa.hold=true;});await panel.getByRole('button',{name:'检查创建计划',exact:true}).click();await panel.getByRole('button',{name:'确认执行创建',exact:true}).click();await panel.getByRole('button',{name:'取消创建',exact:true}).waitFor();assert(await page.locator('.workbench-body').isVisible());assert(await panel.getByRole('button',{name:'关闭',exact:true}).isDisabled());await panel.getByRole('button',{name:'取消创建',exact:true}).click();await panel.getByText('已取消，保留临时文件',{exact:true}).waitFor();assert(await panel.getByText('/sample/parent/.clear-compare-create-native',{exact:true}).isVisible());assert.equal(await page.locator('.workbench-repository').count(),3);assert.equal((await calls('cancel')).length,1);
 await panel.getByRole('button',{name:'再次创建',exact:true}).click();await page.evaluate(()=>{window.__qa.hold=false;window.__qa.unregistered=true;});await panel.getByRole('button',{name:'检查创建计划',exact:true}).click();await panel.getByRole('button',{name:'确认执行创建',exact:true}).click();await panel.getByText('仓库已创建，收录未完成',{exact:true}).waitFor();await panel.getByRole('button',{name:'收录已创建的仓库',exact:true}).click();await panel.waitFor({state:'hidden'});await page.getByRole('region',{name:'仓库收录设置',exact:true}).waitFor();assert.equal(await page.getByLabel('仓库根目录',{exact:true}).inputValue(),'/sample/parent/changed');
 await entry.click();assert.equal(await page.getByRole('region',{name:'仓库收录设置',exact:true}).count(),0);await page.evaluate(()=>{window.__qa.unregistered=false;window.__qa.failPrepare=true;});await panel.getByRole('button',{name:'检查创建计划',exact:true}).click();await page.getByText('目标位置已存在，请更换目录名；不会覆盖已有目录。',{exact:true}).waitFor();assert.equal(await page.getByLabel('新仓库目录名',{exact:true}).inputValue(),'changed');
 await page.evaluate(()=>window.__qa.failPrepare=false);await panel.getByRole('button',{name:'检查创建计划',exact:true}).click();await panel.getByRole('button',{name:'确认执行创建',exact:true}).click();await panel.getByText('仓库已创建并收录',{exact:true}).waitFor();assert.equal(await page.locator('.workbench-repository').count(),4);assert.deepEqual((await calls('workspace_creation_apply')).at(-1).args,{planId:'creation-native'});await panel.getByText('执行日志',{exact:true}).click();assert(await panel.getByText('Created <literal> repository',{exact:true}).isVisible());assert.equal(await panel.locator('literal').count(),0);
 for(const size of [{width:1440,height:1000},{width:900,height:620}]){await page.setViewportSize(size);assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth||document.documentElement.scrollHeight>innerHeight),false);assert(await page.locator('.workbench-body').evaluate(e=>e.clientHeight>180));}await fs.mkdir('target/work',{recursive:true});await page.screenshot({path:'target/work/repository-creation.png'});
 await page.getByRole('button',{name:'English',exact:true}).click();await page.getByRole('region',{name:'Clone or initialize repository',exact:true}).waitFor();await page.getByText('Creation options',{exact:true}).click();assert(await page.getByRole('button',{name:'Inspect creation plan',exact:true}).isVisible());await page.getByRole('button',{name:'Close',exact:true}).click();
 assert.equal((await calls('workspace_apply')).length,0);assert.deepEqual(errors,[]);console.log(JSON.stringify({passed:true,checks:['clone and init typed options','native review then one-use execute','changing input invalidates review','picker and keyboard focus','failure retains input and catalogue','live cancel retains displayed path','completed-but-unregistered recovery entry','auto registration and escaped log','existing content remains visible','Chinese/English and compact 900/1440 layouts'],pageErrors:errors}));
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exit(1)});
