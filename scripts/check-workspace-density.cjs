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
  const names=['app.rs','compare.rs','config.rs','directory.rs','document.rs','encoding.rs','engine.rs','events.rs','exclusions.rs','file_ops.rs','filter.rs','format.rs','history.rs','index.rs','journal.rs','lib.rs','merge.rs','metadata.rs','native_path.rs','options.rs','picture.rs','plan.rs','preview.rs','query.rs','recovery.rs','report.rs','rules.rs','scan.rs','session.rs','snapshot.rs','sort.rs','status.rs','sync.rs','table.rs','task.rs','text.rs','tree.rs','types.rs','validation.rs','watch.rs'];
  const rows=[{id:'d0',path:'src',status:'changed',left:entry('directory',0),right:entry('directory',0)},...names.map((name,i)=>({id:'f'+i,path:'src/'+name,status:i%7===0?'left_only':i%5===0?'metadata_match':'changed',left:entry('file',1800+i*217),right:i%7===0?null:entry('file',1840+i*217)})),...['tests','docs','scripts'].flatMap((name,i)=>[{id:'d'+(i+1),path:name,status:'changed',left:entry('directory',0),right:entry('directory',0)},...names.slice(0,15).map((n,j)=>({id:name+j,path:name+'/'+n,status:'changed',left:entry('file',500+j*61),right:entry('file',520+j*61)}))]),...['.gitignore','Cargo.toml','Cargo.lock','LICENSE','README.md'].map((name,i)=>({id:'root'+i,path:name,status:'changed',left:entry('file',100+i*611),right:entry('file',140+i*611)}))];
  const files=rows.filter(r=>r.left.kind==='file');
  const summary={same:0,changed:files.filter(r=>r.status==='changed').length,left_only:files.filter(r=>r.status==='left_only').length,right_only:0,unverified:files.filter(r=>r.status==='metadata_match').length,errors:0};
  window.__qa={calls:[],verifyResolve:null,verifyReject:null,emptyText:false};
  let session=0;
  window.__TAURI__={core:{invoke:async(command,args)=>{
   args=JSON.parse(JSON.stringify(args,(_,v)=>v instanceof Map?Object.fromEntries(v):v));
   window.__qa.calls.push({command,args});
   if(command==='workspace_catalog')return {settings:{roots:[],exclusions:[]},projects:[],repositories:[],warnings:[],scanned_directories:0,truncated:false};
   if(command==='load_active_project')return null;
   if(command==='load_session')return {request:{left:'/sample/project-left',right:'/sample/project-right',exclusions:'-node_modules/',content:false},english:false};
   if(command==='save_session'||command==='set_native_language')return null;
   if(command==='active_operation')return null;
   if(command==='compare'){
    if(args.request.content)await new Promise((resolve,reject)=>{window.__qa.verifyResolve=resolve;window.__qa.verifyReject=reject});
    return {session:'s'+ ++session,left:'/sample/project-left',right:'/sample/project-right',summary,warnings:[],page:{rows:[],total:rows.length}};
   }
   if(command==='query_folder'){
    const visible=rows.filter(r=>!r.path.includes('/')||args.expandAll!==args.expanded.includes(rows.find(d=>d.path===r.path.split('/')[0]).id));
    const outline=Object.fromEntries(visible.map(r=>[r.id,{name:r.path.split('/').at(-1),depth:r.path.includes('/')?1:0,has_children:r.left.kind==='directory',expanded:args.expandAll,context_only:false}]));
    return {page:{rows:visible,total:visible.length},breadcrumbs:[],outline};
   }
   if(command==='text_diff'&&window.__qa.emptyText)return {id:'text-empty',path:'src/main.rs',lines:[]};
   if(command==='text_diff'){
    const lines=[];
    for(let i=1;i<=160;i++){
     const text=i%6===1?`pub fn ${names[Math.floor(i/6)%names.length].replace('.rs','')}_entry() {\n`:i%6===0?'}\n':`    let entry_${i} = session.read_metadata(relative_path)?;\n`;
     if(i===14||i===63||i===121){lines.push({tag:'delete',old_line:i,new_line:null,text:'    let policy = ComparePolicy::FullContent;\n',right_text:null,changes:[{start:32,end:43}]},{tag:'insert',old_line:null,new_line:i,text:'    let policy = ComparePolicy::MetadataFirst;\n',right_text:null,changes:[{start:32,end:45}]});}
     else lines.push({tag:'equal',old_line:i,new_line:i,text,right_text:null,changes:[]});
    }
    return {id:'text-long',path:'src/compare.rs',lines};
   }
   if(command==='cancel'){if(window.__qa.verifyReject)window.__qa.verifyReject('Cancelled');return null;}
   throw new Error('Unexpected mock command '+command);
  }}};
 });
 await page.goto('https://clear-compare.test/',{waitUntil:'domcontentloaded'});
 await page.getByRole('button',{name:'文件夹比较',exact:true}).click();
 await page.getByRole('button',{name:'比较文件夹',exact:true}).click();
 await page.locator('.outline-row').first().waitFor().catch(async e=>{console.log(await page.locator('body').innerText());console.log(await page.evaluate(()=>window.__qa.calls));throw e;});
 await page.locator('.view-settings>summary').click();
 await page.getByRole('button',{name:'全部展开',exact:true}).click();
 await page.locator('.view-settings>summary').click();
 await page.waitForFunction(()=>document.querySelectorAll('.outline-row').length>80);
 const metrics=[];
 for(const viewport of [{width:1500,height:1100},{width:1260,height:840},{width:900,height:620}]){
  await page.setViewportSize(viewport);
  const result=await page.evaluate(()=>{
   const scroller=document.querySelector('.directory-scroll');const r=scroller.getBoundingClientRect();
   return {width:innerWidth,height:innerHeight,contentHeight:r.height,ratio:r.height/innerHeight,visibleRows:[...document.querySelectorAll('.outline-row')].filter(n=>{const a=n.getBoundingClientRect();return a.top>=r.top&&a.bottom<=r.bottom}).length,pageOverflow:document.documentElement.scrollHeight>innerHeight||document.documentElement.scrollWidth>innerWidth};
  });
  assert(!result.pageOverflow,JSON.stringify(result));assert(result.ratio>=.70,JSON.stringify(result));assert(result.visibleRows>=17,JSON.stringify(result));metrics.push(result);
  assert(await page.getByRole('button',{name:'预览复制 →',exact:true}).evaluate(n=>n.getBoundingClientRect().right<=innerWidth), 'Copy action clipped');
 }
 await page.setViewportSize({width:1260,height:840});
 await page.getByRole('button',{name:'compare.rs',exact:true}).first().click();
 await page.screenshot({path:'target/work/compact-directory.png'});
 // All secondary controls remain accessible without consuming list space.
 const before=await page.locator('.directory-scroll').boundingBox();
 await page.locator('.comparison-tools>summary').click();
 await page.getByRole('checkbox',{name:'忽略大小写',exact:true}).check();
 await page.keyboard.press('Escape');
 assert.equal(await page.locator('.directory-scroll').evaluate(n=>n.getBoundingClientRect().height),before.height);
 await page.locator('.comparison-settings>summary').click();
 await page.getByRole('button',{name:'管理项目排除规则…',exact:true}).click();await page.getByRole('dialog',{name:'项目排除规则',exact:true}).waitFor();await page.getByRole('button',{name:'取消',exact:true}).click();
 await page.keyboard.press('Escape');
 await page.locator('.directory-scroll').evaluate(n=>{n.scrollTop=20});
 const retainedScroll=await page.locator('.directory-scroll').evaluate(n=>n.scrollTop);
 const textOpenStarted=performance.now();
 await page.getByRole('button',{name:'compare.rs',exact:true}).first().dblclick();
 await page.locator('#text-comparison .diff-cell').first().waitFor();
 metrics.push({view:'text-open',automationAndRenderMs:performance.now()-textOpenStarted});
 assert.equal(await page.locator('.directory-host').isVisible(),false);
 for(const viewport of [{width:1260,height:840},{width:900,height:620}]){
  await page.setViewportSize(viewport);
  const result=await page.locator('.text-panes').evaluate(n=>({height:innerHeight,contentHeight:n.getBoundingClientRect().height,ratio:n.getBoundingClientRect().height/innerHeight,pageOverflow:document.documentElement.scrollHeight>innerHeight||document.documentElement.scrollWidth>innerWidth}));
  assert(!result.pageOverflow,JSON.stringify(result));assert(result.ratio>.70,JSON.stringify(result));metrics.push({view:'text',...result});
 }
 await page.setViewportSize({width:1260,height:840});
 // The two merge directions share a fixed center gutter, even when text scrolls.
 const assertCenter=async()=>{
  const result=await page.evaluate(()=>{
   const panes=[...document.querySelectorAll('.shared-diff')].map(n=>n.getBoundingClientRect());
   const gutter=document.querySelector('.merge-gutter').getBoundingClientRect();
   const rows=[...document.querySelectorAll('.merge-gutter-row')];
   return {width:gutter.width,between:gutter.left>=panes[0].right-.5&&gutter.right<=panes[1].left+.5,
    buttons:rows.every(row=>{const [left,right]=row.querySelectorAll('button');const a=left.getBoundingClientRect(),b=right.getBoundingClientRect();return a.left>=gutter.left&&b.right<=gutter.right&&Math.abs(a.top-b.top)<.5;}),
    aligned:rows.every(row=>{const i=parseFloat(row.style.top)/22;const text=document.querySelectorAll('.shared-diff')[0].querySelectorAll('.diff-render-row')[i];return Math.abs(row.getBoundingClientRect().top-text.getBoundingClientRect().top)<1;})};
  });
  assert.equal(result.width,48);assert(result.between&&result.buttons&&result.aligned,JSON.stringify(result));
 };
 await assertCenter();
 await page.locator('.shared-diff').first().evaluate(n=>{n.scrollLeft=100;n.scrollTop=200;});await page.waitForTimeout(100);await assertCenter();
 const top=await page.locator('.shared-diff').first().evaluate(n=>n.scrollTop);
 const gutterBox=await page.locator('.merge-gutter').boundingBox();await page.mouse.move(gutterBox.x+24,gutterBox.y+100);await page.mouse.wheel(0,100);await page.waitForTimeout(150);
 assert((await page.locator('.shared-diff').first().evaluate(n=>n.scrollTop))>top);await assertCenter();
 // Return keyboard focus from the wheel target to the comparison for F7 navigation.
 await page.locator('#text-comparison').focus();
 await page.keyboard.press('F7');
 assert.equal(await page.locator('.diff-focus').count(),2);
 assert.equal(await page.locator('#text-comparison').evaluate(n=>n.contains(document.activeElement)),true);
 await page.screenshot({path:'target/work/compact-text.png'});
 await page.getByRole('button',{name:'返回目录',exact:true}).click();
 assert.equal(await page.locator('.directory-scroll').evaluate(n=>n.scrollTop),retainedScroll);
 assert.equal(await page.locator('.outline-row').count(),94);
 assert.equal(await page.locator('.outline-row.focused').count(),1);
 await page.getByRole('button',{name:'compare.rs',exact:true}).first().focus();
 await page.getByRole('button',{name:'English',exact:true}).click();
 assert.equal(await page.getByRole('button',{name:'Compare directories',exact:true}).isVisible(),true);
 await page.setViewportSize({width:900,height:620});
 assert(await page.getByRole('button',{name:'Preview copy →',exact:true}).evaluate(n=>n.getBoundingClientRect().right<=innerWidth), 'English copy action clipped');
 await page.screenshot({path:'target/work/compact-small-english.png'});
 assert.deepEqual(errors,[]);
 console.log(JSON.stringify({passed:true,metrics,pageErrors:errors}));
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exit(1)});
