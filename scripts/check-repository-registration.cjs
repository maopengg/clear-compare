// Registration UI with real WASM and synthetic IPC; native disk/persistence tests are separate.
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

   if(command==='pick_directory')return window.__qa.pick||null;
   if(command==='workspace_registration_prepare'){
    if(window.__qa.failInspect)throw Error('Repository is already registered');
    const previous=args.previous?catalog.repositories.find(r=>r.id===args.previous):null;
    const existing=previous&&window.__qa.existing?catalog.repositories.find(r=>r.id==='new-native'):null;const registration={id:'registration-native',repository:{...repo(existing?existing.id:previous?'relocated-native':'new-native',previous?'moved-copy':'added-copy'),path:args.path,match_key:previous?'example.com/different/project':'example.com/team/app'},previous,existing};window.__qa.registration=registration;return registration;
   }
   if(command==='workspace_registration_apply'){
    if(window.__qa.failRegistration)throw Error('Repository location changed after review');
    const plan=window.__qa.registration;catalog.repositories=catalog.repositories.filter(r=>r.id!==plan.previous?.id&&r.id!==plan.existing?.id);catalog.repositories.push(plan.repository);return catalog;
   }
   if(command==='workspace_remove'){
    if(window.__qa.failRemove)throw Error("Close this repository's source tabs before removing it");
    catalog.repositories=catalog.repositories.filter(r=>r.id!==args.repositoryId);return catalog;
   }
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
 await page.goto('https://clear-compare.test/',{waitUntil:'domcontentloaded'});
 await page.locator('.workbench-repository').first().waitFor();
 const calls=async name=>page.evaluate(n=>window.__qa.calls.filter(c=>c.command===n),name);
 const add=page.getByRole('button',{name:'添加仓库',exact:true});const panel=page.getByRole('region',{name:'仓库收录设置',exact:true});
 await add.click();await page.getByLabel('仓库根目录',{exact:true}).waitFor();assert(await page.getByLabel('仓库根目录',{exact:true}).evaluate(e=>e===document.activeElement));
 await page.evaluate(()=>window.__qa.pick='/sample/chosen repo');await panel.getByRole('button',{name:'选择目录…',exact:true}).click();await page.waitForFunction(()=>document.querySelector('.repository-registration input').value==='/sample/chosen repo');
 await panel.getByRole('button',{name:'检查仓库',exact:true}).click();await page.getByRole('region',{name:'确认仓库收录',exact:true}).waitFor();assert.equal((await calls('workspace_registration_apply')).length,0);assert.equal((await calls('workspace_registration_prepare')).at(-1).args.path,'/sample/chosen repo');assert.equal((await calls('workspace_registration_prepare')).at(-1).args.previous??null,null);
 await panel.press('Escape');await panel.waitFor({state:'hidden'});assert(await add.evaluate(e=>e===document.activeElement));
 await add.click();await page.getByLabel('仓库根目录',{exact:true}).fill('/sample/new');await panel.getByRole('button',{name:'检查仓库',exact:true}).click();await page.getByRole('region',{name:'确认仓库收录',exact:true}).waitFor();
 await page.getByLabel('仓库根目录',{exact:true}).fill('/sample/newer');assert.equal(await page.getByRole('region',{name:'确认仓库收录',exact:true}).count(),0);
 await page.evaluate(()=>window.__qa.failInspect=true);await panel.getByRole('button',{name:'检查仓库',exact:true}).click();await page.getByText('这个仓库已收录，请直接在列表中打开。',{exact:true}).waitFor();assert.equal(await page.getByLabel('仓库根目录',{exact:true}).inputValue(),'/sample/newer');
 await page.evaluate(()=>window.__qa.failInspect=false);await panel.getByRole('button',{name:'检查仓库',exact:true}).click();await page.getByRole('region',{name:'确认仓库收录',exact:true}).waitFor();await page.evaluate(()=>window.__qa.failRegistration=true);await panel.getByRole('button',{name:'确认收录',exact:true}).click();await page.getByText('仓库位置在检查后发生变化，请重新检查。',{exact:true}).waitFor();assert.equal(await page.locator('.workbench-repository').count(),3);assert.equal(await page.getByRole('region',{name:'确认仓库收录',exact:true}).count(),0);
 await page.evaluate(()=>window.__qa.failRegistration=false);await panel.getByRole('button',{name:'检查仓库',exact:true}).click();await panel.getByRole('button',{name:'确认收录',exact:true}).click();await panel.waitFor({state:'hidden'});assert.deepEqual((await calls('workspace_registration_apply')).at(-1).args,{planId:'registration-native'});assert.equal(await page.locator('.workbench-repository').count(),4);
 await page.evaluate(()=>window.__qa.existing=true);const old=page.locator('.workbench-repository').filter({has:page.locator('.workbench-repository-open strong',{hasText:'app-copy'})});await old.getByRole('button',{name:'管理仓库收录',exact:true}).click();await page.getByLabel('仓库根目录',{exact:true}).fill('/sample/moved');await panel.getByRole('button',{name:'检查仓库',exact:true}).click();await page.getByText('远程地址与原记录不同，请核对后再确认。',{exact:true}).waitFor();assert.equal((await calls('workspace_registration_prepare')).at(-1).args.previous,'opaque-2');await page.getByText('新位置已收录：合并为一条记录；原记录的文件规则和分支偏好优先。',{exact:true}).waitFor();
 for(const size of [{width:1440,height:1000},{width:900,height:620}]){await page.setViewportSize(size);assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth||document.documentElement.scrollHeight>innerHeight),false);assert(await page.locator('.workbench-body').evaluate(e=>e.clientHeight>200));}
 await fs.mkdir('target/work',{recursive:true});await page.screenshot({path:'target/work/repository-registration.png'});
 await panel.getByRole('button',{name:'确认重新定位',exact:true}).click();await panel.waitFor({state:'hidden'});assert.equal(await old.count(),0);
 await page.locator('.workbench-repository').first().getByRole('button',{name:'管理仓库收录',exact:true}).click();await page.evaluate(()=>window.__qa.failRemove=true);await panel.getByRole('button',{name:'移除收录（保留磁盘文件）',exact:true}).click();await page.getByText('请先关闭这个仓库的源文件标签，再移除收录。',{exact:true}).waitFor();assert(await panel.isVisible());
 await page.evaluate(()=>window.__qa.failRemove=false);await panel.getByRole('button',{name:'移除收录（保留磁盘文件）',exact:true}).click();await panel.waitFor({state:'hidden'});assert.equal(await page.locator('.workbench-repository').count(),2);
 await page.getByRole('button',{name:'English',exact:true}).click();await page.getByRole('button',{name:'Add repository',exact:true}).click();await page.getByRole('region',{name:'Repository registration',exact:true}).waitFor();assert(await page.getByLabel('Repository root',{exact:true}).isVisible());
 assert.equal((await calls('workspace_apply')).length,0);assert.deepEqual(errors,[]);console.log(JSON.stringify({passed:true,checks:['inspect before register','opaque one-use confirmation','picker and manual root','changing input clears review','cancel and focus return','failed inspect and save preserve list/input','relocation identity, remote and existing-preference warnings','remove without filesystem action','source-tab protection message','Chinese/English and compact layout'],pageErrors:errors}));
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exit(1)});
