// Replacement review against real WASM/CodeMirror with synthetic native plans.
// Native disk writes are verified separately in workbench_replace.rs.
const {chromium}=require('playwright');const fs=require('fs/promises');const path=require('path');const assert=require('assert');
(async()=>{const browser=await chromium.launch({executablePath:'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true});try{
 const page=await browser.newPage({viewport:{width:1440,height:1000}});page.setDefaultTimeout(10000);const errors=[];page.on('pageerror',e=>errors.push(String(e)));
 await page.route('https://clear-compare.test/**',async route=>{const name=decodeURIComponent(new URL(route.request().url()).pathname).replace(/^\//,'')||'index.html';const root=path.resolve('dist'),file=path.resolve(root,name);if(!file.startsWith(root+path.sep))return route.abort();try{await route.fulfill({body:await fs.readFile(file),contentType:name.endsWith('.wasm')?'application/wasm':name.endsWith('.js')?'text/javascript':name.endsWith('.css')?'text/css':'text/html'});}catch{await route.fulfill({status:404,body:''});}});
 await page.addInitScript(()=>{
  const session={request:{left:'/sample/left',right:'/sample/right',exclusions:'',content:false},english:false};const project={id:'p1',name:'Synthetic compare',revision:'r1',updated_ms:'1',session};
  const repos=['a','b'].map(n=>({id:'repo-'+n,name:'Project '+n,path:'/sample/project-'+n,kind:'repository',common_id:n,match_key:null,branch:'main',head:'a'.repeat(40),changed:0,conflicts:0,ahead:0,behind:0,favorite:false,last_opened:'',available:true,error:null}));
  const file=(id,name,path,kind='file')=>({id,name,path,kind,bytes:'42'});const trees={'repo-a/root':[file('src','src','src','directory'),file('readme','README.md','README.md')],'repo-a/src':[file('main','main.rs','src/main.rs')],'repo-b/root':[file('other','other.py','other.py')]};
  const disk={main:'fn main() {\n    println!("hello needle");\n}\n',readme:'# Readme\nneedle\n',other:'print("other")\n'};const docs={};const fileSettings={};window.__qa={calls:[],disk,docs,trees,fileSettings,failSave:false,failSearch:false,changed:false};
  const repositoryState=id=>({repository:repos.find(r=>r.id===id),revision:'native-revision',operation:null,changes:[],references:[],remotes:[]});
  window.__TAURI__={event:{listen:async(name,fn)=>{window.__qa.progress=fn;return ()=>{window.__qa.progress=null;};}},core:{invoke:async(command,args)=>{args=JSON.parse(JSON.stringify(args,(_,v)=>v instanceof Map?Object.fromEntries(v):v));window.__qa.calls.push({command,args});
   if(command==='load_active_project')return project;if(command==='active_operation')return null;if(['set_native_language','save_session'].includes(command))return null;
   if(command==='workspace_catalog')return {projects:[project],file_settings:fileSettings,settings:{roots:[],exclusions:[]},repositories:repos,warnings:[],scanned_directories:1,truncated:false};
   if(command==='workspace_open')return repositoryState(args.repositoryId);
   if(command==='workspace_file_tree'){const files=trees[args.repositoryId+'/'+(args.directory||'root')]||[];return {parent:args.directory?'root-native':null,path:args.directory==='src'?'src':'',files,offset:args.offset,total:files.length,warnings:[]};}
   if(command==='workspace_file_settings'){if(window.__qa.failSettings)throw Error('Synthetic settings save failure');fileSettings[args.repositoryId]=args.settings;return args.settings;}
   if(command==='workspace_source_open'){if(args.expected&&window.__qa.changed)throw Error('Synthetic stale search result');let slot=docs[args.fileId];if(!slot){const f=Object.values(trees).flat().find(f=>f.id===args.fileId);if(!f||f.kind!=='file')throw Error('Unknown file capability');slot=docs[args.fileId]={id:'doc-'+args.fileId,absolute_path:'/sample/'+args.repositoryId+'/'+f.path,path:f.path,side:'left',text:disk[args.fileId]||'',encoding:'utf8',bom:false,line_ending:'lf',digest:'digest-'+args.fileId,dirty:false,generation:0};}return {repository_id:args.repositoryId,file_id:args.fileId,document:slot};}
   if(command==='workspace_source_dirty'){const doc=Object.values(docs).find(d=>d.id===args.documentId);if(doc&&args.generation>=doc.generation){doc.dirty=args.dirty;doc.generation=args.generation;}return null;}
   if(command==='workspace_source_save'){if(window.__qa.failSave)throw Error('Synthetic source save failure');const entry=Object.entries(docs).find(([,d])=>d.id===args.documentId);if(!entry)throw Error('Unknown document');const [file,doc]=entry;doc.text=args.text;doc.encoding=args.options.encoding||doc.encoding;doc.line_ending=args.options.line_ending||doc.line_ending;doc.bom=args.options.bom??doc.bom;doc.dirty=false;doc.generation=args.generation;doc.digest='saved-'+args.generation;disk[file]=args.text;return {document:doc,backup:'/synthetic/backup'};}
   if(command==='workspace_source_close'){const entry=Object.entries(docs).find(([,d])=>d.id===args.documentId);if(entry){if(entry[1].dirty&&!args.discard)throw Error('Unsaved native buffer');delete docs[entry[0]];}return null;}
   if(command==='workspace_source_check')return Object.values(docs).map(d=>({id:d.id,changed:window.__qa.changed}));
   if(command==='workspace_source_create'){const key=args.repositoryId+'/'+(args.directory||'root');const f=file('new-'+args.name,args.name,(args.directory==='src'?'src/':'')+args.name,args.folder?'directory':'file');if(trees[key].some(old=>old.name===args.name))throw Error('File exists');trees[key].push(f);disk[f.id]='';if(args.folder)trees[args.repositoryId+'/'+f.id]=[];return f;}
   if(command==='workspace_source_search'){if(window.__qa.failSearch)throw Error('Synthetic search failure');const q=args.query;const hits=q.query==='missing'?[]:['main','readme'].map(id=>{const file=Object.values(trees).flat().find(f=>f.id===id);const offset=Math.max(0,disk[id].indexOf('needle'));return {file_id:id,repository_id:'repo-a',repository:'Project a',path:file.path,line:1,column:offset+1,offset_utf16:offset,length_utf16:6,preview:'needle in '+file.path,digest:docs[id]?.digest||'digest-'+id};});return {hits,searched_files:3,skipped_files:0,truncated:false,cancelled:false,warnings:[]};}
   if(command==='workspace_source_replace_preview'){
    if(window.__qa.failPreview)throw Error('Synthetic preview failure');
    const files=['main','readme','other'].map((id,i)=>({id:'plan-file-'+id,file_id:id,repository_id:i===2?'repo-b':'repo-a',repository:i===2?'Project b':'Project a',path:i===0?'src/main.rs':i===1?'README.md':'other.py',matches:450,before_bytes:42,after_bytes:56,encoding:'utf8',line_ending:'lf',issue:i===1?'unsaved_buffer':null}));
    if(window.__qa.many)for(let i=0;i<202;i++)files.push({...files[1],id:'extra-'+i,path:'extra-'+i+'.txt',issue:'unchanged'});
    return {id:'plan-current',request:args.request,files,searched_files:260,skipped_files:4,warnings:['Synthetic ignored file warning']};
   }
   if(command==='workspace_source_replace_patch'){
    if(window.__qa.failPatch)throw Error('Synthetic diff failure');
    return {path:'src/main.rs',text:args.offset?' continuation':'--- before\n+++ after\n@@ -1 +1 @@\n-needle\n+replacement',binary:false,offset:args.offset,total_lines:501,next_offset:args.offset?null:500,long_lines:false};
   }
   if(command==='workspace_source_replace_apply'){
    if(window.__qa.failApply)throw Error('Synthetic stale native plan');
    window.__qa.progress?.({payload:{plan_id:'different',processed:99,total:99,path:'wrong.txt'}});
    window.__qa.progress?.({payload:{plan_id:args.planId,processed:1,total:args.selected.length,path:'src/main.rs'}});
    if(window.__qa.delay)await new Promise(resolve=>window.__qa.finish=resolve);
    const updated=[];if(!window.__qa.cancelled){disk.main='fn main() { /* replacement */ }\n';if(docs.main){Object.assign(docs.main,{text:disk.main,digest:'batch-saved',dirty:false});updated.push({repository_id:'repo-a',file_id:'main',document:docs.main});}}
    return {status:window.__qa.cancelled?'cancelled':'completed',journal:'/synthetic/recovery/intent.json',outcomes:window.__qa.cancelled?[]:args.selected.map(id=>({id,status:'completed',backup_directory:'/synthetic/recovery/files',issue:null})),updated_documents:updated,issue:null};
   }
   if(command==='cancel'){window.__qa.cancelled=true;window.__qa.finish?.();return null;}
   throw Error('Unexpected command '+command);
  }}};
 });
 await page.goto('https://clear-compare.test/',{waitUntil:'domcontentloaded'});
 await page.locator('.workbench-repository-open').first().click();
 await page.getByRole('button',{name:'文件 / 全局搜索',exact:true}).click();
 await page.getByRole('button',{name:/src/}).filter({has:page.locator('.entry-icon')}).first().click();
 await page.getByRole('button',{name:'main.rs',exact:false}).click();
 const editor=()=>page.locator('.source-code-view[data-document="doc-main"] .cm-content');await editor().waitFor();
 assert.equal(await page.evaluate(()=>window.ClearCode.refresh('doc-main','different content','must not write')),false);
 await page.getByLabel('全局搜索内容',{exact:true}).fill('needle');
 await page.locator('.source-batch-controls>summary').click();
 const text=page.getByLabel('批量替换内容',{exact:true}),prepare=page.getByRole('button',{name:'预览批量替换',exact:true});
 const dialog=page.getByRole('dialog',{name:'批量替换预览',exact:true});
 const close=()=>dialog.getByRole('button',{name:'关闭',exact:true}).click();
 const lastPreview=()=>page.evaluate(()=>window.__qa.calls.filter(c=>c.command==='workspace_source_replace_preview').at(-1).args.request);
 await text.fill('replacement');await page.getByLabel('搜索范围',{exact:true}).selectOption('all');
 await page.getByLabel('批量替换换行格式',{exact:true}).selectOption('crlf');
 await page.evaluate(()=>window.__qa.failPreview=true);await prepare.click();
 await page.getByRole('alert').filter({hasText:'Synthetic preview failure'}).waitFor();assert.equal(await text.inputValue(),'replacement');
 await page.evaluate(()=>window.__qa.failPreview=false);await prepare.click();await dialog.waitFor();
 assert.deepEqual((await lastPreview()).query.repositories,['repo-a','repo-b']);assert.equal((await lastPreview()).line_ending,'cr_lf');
 assert.equal(await page.evaluate(()=>window.__qa.calls.some(c=>c.command==='workspace_source_replace_apply')),false);
 assert.equal(await dialog.locator('.source-replace-file').count(),3);assert.equal(await dialog.locator('input[type=checkbox]:disabled').count(),1);
 assert((await dialog.innerText()).includes('未保存草稿'));assert((await dialog.innerText()).includes('4 个非文本'));
 await dialog.locator('.source-replace-file button').first().click();await dialog.locator('.source-replace-diff').waitFor();assert((await dialog.locator('.source-replace-diff').innerText()).includes('+replacement'));
 await page.evaluate(()=>window.__qa.failPatch=true);await dialog.locator('.source-replace-file button').last().click();await dialog.getByRole('status').filter({hasText:'Synthetic diff failure'}).waitFor();assert((await dialog.locator('.source-replace-diff').innerText()).includes('+replacement'));await page.evaluate(()=>window.__qa.failPatch=false);
 await dialog.locator('main footer').getByRole('button',{name:'›',exact:true}).click();assert((await dialog.locator('.source-replace-diff').innerText()).includes('continuation'));
 await dialog.locator('main footer').getByRole('button',{name:'‹',exact:true}).click();
 await dialog.getByRole('button',{name:'清空选择',exact:true}).click();assert.equal(await dialog.getByRole('button',{name:'替换所选文件',exact:true}).isDisabled(),true);
 await dialog.getByRole('button',{name:'全选可替换文件',exact:true}).click();
 await dialog.locator('.source-replace-file input').last().uncheck();
 const saves=await page.evaluate(()=>window.__qa.calls.filter(c=>c.command==='workspace_source_save').length);await dialog.press('Meta+s');assert.equal(await page.evaluate(()=>window.__qa.calls.filter(c=>c.command==='workspace_source_save').length),saves);
 // Focus must cycle within the modal, including Shift+Tab from the first control.
 await dialog.getByRole('button',{name:'关闭',exact:true}).focus();await page.keyboard.press('Shift+Tab');assert.equal(await page.evaluate(()=>document.activeElement.textContent),'替换所选文件');await page.keyboard.press('Tab');assert.equal(await page.evaluate(()=>document.activeElement.textContent),'关闭');
 for(const size of [{width:1440,height:1000},{width:900,height:620}]){await page.setViewportSize(size);const box=await dialog.boundingBox();assert(box.x>=0&&box.y>=0&&box.x+box.width<=size.width&&box.y+box.height<=size.height);assert.equal(await dialog.evaluate(e=>e.scrollWidth>e.clientWidth),false);}
 await page.setViewportSize({width:1440,height:1000});await page.screenshot({path:'target/work/source-replacement.png'});
 await dialog.getByRole('button',{name:'替换所选文件',exact:true}).click();await dialog.locator('.source-replace-result').waitFor();assert((await dialog.locator('.source-replace-result').innerText()).includes('/synthetic/recovery/intent.json'));
 const apply=await page.evaluate(()=>window.__qa.calls.filter(c=>c.command==='workspace_source_replace_apply').at(-1).args);assert.deepEqual(apply,{planId:'plan-current',selected:['plan-file-main']});
 assert.equal(await dialog.getByRole('button',{name:'替换所选文件',exact:true}).isDisabled(),true);await close();assert((await editor().innerText()).includes('replacement'));await page.getByRole('button',{name:'撤销',exact:true}).click();assert((await editor().innerText()).includes('hello needle'));await page.getByRole('button',{name:'重做',exact:true}).click();await page.waitForFunction(()=>!window.__qa.docs.main.dirty);assert((await editor().innerText()).includes('replacement'));
 // All scopes use native IDs from the same request builder as search.
 await page.getByLabel('搜索范围',{exact:true}).selectOption('directory');await prepare.click();await dialog.waitFor();assert.equal((await lastPreview()).query.directory,'src');await close();
 await page.getByLabel('搜索范围',{exact:true}).selectOption('file');await prepare.click();await dialog.waitFor();assert.equal((await lastPreview()).query.file,'main');assert.deepEqual((await lastPreview()).query.repositories,['repo-a']);await close();
 await page.getByLabel('搜索范围',{exact:true}).selectOption('current');await prepare.click();await dialog.waitFor();assert.equal((await lastPreview()).query.directory,null);assert.equal((await lastPreview()).query.file,null);
 // A native rejection consumes the one-shot plan and retains replacement input.
 await page.evaluate(()=>window.__qa.failApply=true);await dialog.getByRole('button',{name:'替换所选文件',exact:true}).click();await dialog.getByRole('status').filter({hasText:'Synthetic stale native plan'}).waitFor();assert.equal(await dialog.getByRole('button',{name:'替换所选文件',exact:true}).isDisabled(),true);await close();assert.equal(await text.inputValue(),'replacement');
 await page.evaluate(()=>{window.__qa.failApply=false;window.__qa.delay=true;});await prepare.click();await dialog.waitFor();await dialog.getByRole('button',{name:'替换所选文件',exact:true}).click();await dialog.getByRole('button',{name:'取消当前任务',exact:true}).waitFor();await page.waitForFunction(()=>document.querySelector('.source-replace-progress').textContent.includes('1 / 2'));
 assert.equal(await dialog.getByRole('button',{name:'关闭',exact:true}).isDisabled(),true);assert.equal(await editor().getAttribute('contenteditable'),'false');assert(!(await dialog.locator('.source-replace-progress').innerText()).includes('wrong.txt'));
 await dialog.getByRole('button',{name:'取消当前任务',exact:true}).click();await dialog.locator('.source-replace-result').getByText('已取消；已完成项保留',{exact:true}).waitFor();assert((await dialog.locator('.source-replace-files').innerText()).includes('未执行'));await close();
 await page.evaluate(()=>{window.__qa.delay=false;window.__qa.cancelled=false;});await editor().fill('draft after preview');await page.waitForFunction(()=>window.__qa.docs.main.dirty);await prepare.click();await dialog.waitFor();const beforeDirty=await page.evaluate(()=>window.__qa.calls.filter(c=>c.command==='workspace_source_replace_apply').length);await dialog.getByRole('button',{name:'替换所选文件',exact:true}).click();await dialog.getByRole('status').filter({hasText:'所选文件中有未保存草稿'}).waitFor();assert.equal(await page.evaluate(()=>window.__qa.calls.filter(c=>c.command==='workspace_source_replace_apply').length),beforeDirty);await close();assert.equal(await editor().innerText(),'draft after preview');await editor().fill(await page.evaluate(()=>window.__qa.disk.main));await page.waitForFunction(()=>!window.__qa.docs.main.dirty);
 await page.evaluate(()=>window.__qa.many=true);await prepare.click();await dialog.waitFor();assert.equal(await dialog.locator('.source-replace-file').count(),200);await dialog.locator('aside footer').getByRole('button',{name:'›',exact:true}).click();assert.equal(await dialog.locator('.source-replace-file').count(),5);await dialog.press('Escape');await dialog.waitFor({state:'hidden'});
 await page.getByRole('button',{name:'English',exact:true}).click();await prepare.waitFor({state:'hidden'});await page.getByRole('button',{name:'Preview replacements',exact:true}).click();await page.getByRole('dialog',{name:'Replacement preview',exact:true}).waitFor();assert((await page.locator('.source-replace-files').innerText()).includes('Unsaved buffer'));assert.deepEqual(errors,[]);
 console.log(JSON.stringify({passed:true,checks:['shared all/repository/directory/file scopes','native one-shot IDs and subset selection','blocked files and warnings','bounded file/diff pagination','failure retains inputs and diff','real clean CodeMirror buffer update','modal focus and save suppression','running progress and cancellation','stale-plan refusal','Chinese/English and 900/1440 layout'],pageErrors:errors}));
 }finally{await browser.close();}})().catch(e=>{console.error(e);process.exit(1)});
