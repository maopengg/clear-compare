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
  window.__qa={calls:[],verifyResolve:null,verifyReject:null,emptyText:false};
  let session=0;
  window.__TAURI__={core:{invoke:async(command,args)=>{
   args=JSON.parse(JSON.stringify(args,(_,v)=>v instanceof Map?Object.fromEntries(v):v));
   window.__qa.calls.push({command,args});
   if(command==='load_session')return {request:{left:'/sample/project-left',right:'/sample/project-right',exclusions:'-node_modules/',content:false},english:false};
   if(command==='save_session')return null;
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
   if(command==='text_diff'&&window.__qa.emptyText)return {id:'text-empty',path:'src/main.rs',lines:[]};
   if(command==='text_diff')return {id:'text-1',path:'src/main.rs',lines:[{tag:'delete',old_line:1,new_line:null,text:'let version = 1;\n',right_text:null,changes:[{start:14,end:15}]},{tag:'insert',old_line:null,new_line:1,text:'let version = 2;\n',right_text:null,changes:[{start:14,end:15}]}]};
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
 await page.screenshot({path:'target/work/directory-paired-qa.png',fullPage:true});
 await page.getByRole('button',{name:'main.rs',exact:true}).first().dblclick();
 await page.locator('#text-comparison').waitFor();
 await page.locator('.diff-cell.delete').first().waitFor();
 assert((await page.locator('#text-comparison').innerText()).includes('version = 1;'));
 assert((await page.locator('#text-comparison').innerText()).includes('version = 2;'));
 assert(!(await page.locator('#text-comparison').innerText()).includes('rows.with'));
 assert.equal(await page.locator('#text-comparison').getByRole('button',{name:'下一页',exact:true}).isDisabled(),true);
 await page.getByRole('button',{name:'下一差异',exact:true}).click();
 assert.equal(await page.locator('#text-comparison .diff-focus').count(),2);
 await page.locator('#text-comparison').scrollIntoViewIfNeeded();
 await page.locator('#text-comparison').screenshot({path:'target/work/directory-text-qa.png'});
 await page.getByRole('button',{name:'关闭预览',exact:true}).click();
 // Empty previews and the legacy breadcrumb path previously escaped browser coverage.
 await page.evaluate(()=>{window.__qa.emptyText=true});
 await page.getByRole('button',{name:'main.rs',exact:true}).first().dblclick();
 await page.locator('#text-comparison').waitFor();
 await page.waitForFunction(()=>!document.querySelector('.progress'));
 assert.equal(await page.locator('#text-comparison .diff-cell').count(),0);
 await page.getByRole('button',{name:'关闭预览',exact:true}).click();
 await page.getByRole('checkbox',{name:'目录树',exact:true}).uncheck();
 await page.getByRole('button',{name:'src',exact:true}).first().dblclick();
 await page.getByRole('button',{name:'src/main.rs',exact:true}).first().waitFor();
 assert.equal(await page.locator('.outline-row').count(),1);
 await page.getByRole('button',{name:'根目录',exact:true}).click();
 await page.waitForFunction(()=>document.querySelectorAll('.outline-row').length===2);
 await page.getByRole('checkbox',{name:'目录树',exact:true}).check();
 await page.waitForFunction(()=>!document.querySelector('.progress'));

 await page.getByRole('button',{name:'全部折叠',exact:true}).click();
 await page.waitForFunction(()=>document.querySelectorAll('.outline-row').length===2);
 await page.getByRole('button',{name:'全部展开',exact:true}).click();
 await page.waitForFunction(()=>document.querySelectorAll('.outline-row').length===3);
 await page.getByRole('button',{name:'折叠目录',exact:true}).first().click();
 await page.waitForFunction(()=>document.querySelectorAll('.outline-row').length===2);
 await page.getByRole('button',{name:'全部展开',exact:true}).click();
 await page.getByRole('button',{name:'main.rs',exact:true}).first().click();
 await page.evaluate(()=>window.scrollTo(0,0));
 await page.screenshot({path:'target/work/directory-paired-qa.png',fullPage:true});
 // Exercise the earlier two-phase quick/full flow and cancellation using a delayed bridge reply.
 await page.getByRole('checkbox',{name:'列表显示后完整校验内容（SHA-256，较慢）',exact:true}).check();
 await page.getByRole('button',{name:'比较文件夹',exact:true}).click();
 await page.getByRole('button',{name:'停止校验，保留列表',exact:true}).waitFor();
 await page.waitForFunction(()=>window.__qa.verifyReject!==null);
 assert.equal(await page.locator('.outline-row').count(),2);
 await page.getByRole('button',{name:'停止校验，保留列表',exact:true}).click();
 await page.getByRole('button',{name:'比较文件夹',exact:true}).waitFor({state:'visible'});
 await page.waitForFunction(()=>!document.querySelector('.progress'));
 assert.equal(await page.locator('.outline-row').count(),2);
 assert((await page.locator('.comparison-basis').innerText()).includes('未证明内容一致'));
 assert.deepEqual(errors,[]);
 console.log(JSON.stringify({passed:true,checks:['collapsed root','paired row positions','missing-side rendering','timestamps','expand/collapse/all exception','file selection','double-click text diff','line highlighting','quick-list-before-full-verification','stop-and-retain-quick-list'],pageErrors:errors}));
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exit(1)});
