// Isolated UI acceptance for home and repository inspection; no real repository writes.
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
 await page.getByRole('region',{name:'项目工作台',exact:true}).waitFor().catch(()=>page.locator('.workbench').waitFor());
 await page.locator('.workbench-project').filter({hasText:'代码同步'}).waitFor();assert.equal(await page.locator('.workbench-repository').count(),3);
 assert.equal(await page.locator('.path-section').isVisible(),false);
 await page.locator('.app-menu > summary').click();await page.getByRole('button',{name:'Git',exact:true}).click();await page.getByText('Git 版本与工作区比较',{exact:true}).waitFor();assert.equal(await page.locator('.workbench').isVisible(),false);await page.locator('.archive-panel').filter({hasText:'Git 版本与工作区比较'}).getByRole('button',{name:'收起',exact:true}).click();await page.locator('.workbench').waitFor();

 await page.getByLabel('搜索项目与仓库',{exact:true}).fill('app-copy');assert.equal(await page.locator('.workbench-repository').count(),1);
 await page.getByLabel('搜索项目与仓库',{exact:true}).fill('');
 await page.locator('.workbench-repository').first().getByRole('button',{name:'收藏仓库',exact:true}).click();
 await page.getByRole('button',{name:'收藏',exact:true}).click();assert.equal(await page.locator('.workbench-repository').count(),1);
 await page.locator('.workbench-repository-open').first().click();await page.getByText('src/main.rs',{exact:true}).waitFor();
 assert((await page.locator('.workbench-detail').innerText()).includes('已修改'));
 assert.equal(await page.evaluate(()=>window.__qa.calls.find(c=>c.command==='workspace_open').args.repositoryId),'opaque-1');

 await page.getByRole('button',{name:'暂存',exact:true}).click();
 await page.getByRole('button',{name:'取消暂存',exact:true}).waitFor({state:'visible'});
 await page.waitForFunction(()=>window.__qa.index==='M');
 assert.deepEqual(await page.evaluate(()=>window.__qa.pending.action),{kind:'stage',files:['file-id']});
 await page.getByRole('button',{name:'取消暂存',exact:true}).click();
 await page.waitForFunction(()=>window.__qa.index==='.');
 await page.getByRole('button',{name:'放弃修改',exact:true}).click();
 await page.getByRole('region',{name:'确认 Git 操作',exact:true}).waitFor();
 const appliedBefore=await page.evaluate(()=>window.__qa.calls.filter(c=>c.command==='workspace_apply').length);
 await page.getByRole('button',{name:'取消操作',exact:true}).click();
 assert.equal(await page.evaluate(()=>window.__qa.calls.filter(c=>c.command==='workspace_apply').length),appliedBefore);
 await page.getByRole('button',{name:'放弃修改',exact:true}).click();
 await page.getByRole('button',{name:'确认执行',exact:true}).click();
 await page.waitForFunction(n=>window.__qa.calls.filter(c=>c.command==='workspace_apply').length===n+1,appliedBefore);
 await page.evaluate(()=>window.__qa.failApply=true);
 await page.getByRole('button',{name:'暂存',exact:true}).click();
 await page.getByText('Synthetic hook failure',{exact:false}).waitFor();
 assert.equal(await page.getByText('src/main.rs',{exact:true}).isVisible(),true);
 await page.evaluate(()=>window.__qa.failApply=false);
 await page.getByRole('button',{name:'刷新仓库',exact:true}).click();await page.waitForFunction(()=>!document.querySelector('[role=alert]'));
 await page.screenshot({path:'target/work/workbench-repository.png'});
 await page.getByRole('button',{name:'最近',exact:true}).click();assert.equal(await page.locator('.workbench-repository').count(),1);
 await page.getByRole('button',{name:'扫描设置',exact:true}).click();await page.getByLabel('扫描目录路径',{exact:true}).fill('/sample/other');await page.getByRole('button',{name:'添加',exact:true}).click();
 await page.getByRole('button',{name:'保存并扫描',exact:true}).click();await page.waitForFunction(()=>window.__qa.scanArgs!==null);
 assert.deepEqual(await page.evaluate(()=>window.__qa.scanArgs.settings),{roots:['/sample/code','/sample/other'],exclusions:['node_modules']});
 await page.getByRole('button',{name:'全部',exact:true}).click();
 for(const size of [{width:1440,height:1000},{width:900,height:620}]){await page.setViewportSize(size);assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth||document.documentElement.scrollHeight>innerHeight),false);}
 await fs.mkdir('target/work',{recursive:true});await page.screenshot({path:'target/work/workbench-home.png'});
 await page.locator('.workbench-project').filter({hasText:'代码同步'}).click();await page.getByLabel('左侧 · 来源',{exact:true}).waitFor();assert.equal(await page.getByLabel('左侧 · 来源',{exact:true}).inputValue(),'/sample/left');
 assert.deepEqual(errors,[]);console.log(JSON.stringify({passed:true,checks:['home comparison and repository entries','legacy Git navigation','search','favorites','recent repositories','native opaque IDs','repository state and references','scan roots and exclusions','comparison navigation','compact desktop sizes','stage and unstage opaque file selection','destructive review cancel and apply','failed action retains repository view'],pageErrors:errors}));
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exit(1)});
