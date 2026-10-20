import {basicSetup} from 'codemirror';
import {EditorState,Compartment} from '@codemirror/state';
import {EditorView,keymap} from '@codemirror/view';
import {indentWithTab,undo,redo,indentMore,indentLess,isolateHistory} from '@codemirror/commands';
import {openSearchPanel,closeSearchPanel,gotoLine} from '@codemirror/search';
import {javascript} from '@codemirror/lang-javascript';
import {python} from '@codemirror/lang-python';
import {rust} from '@codemirror/lang-rust';
import {html} from '@codemirror/lang-html';
import {css} from '@codemirror/lang-css';
import {json} from '@codemirror/lang-json';
import {markdown} from '@codemirror/lang-markdown';
import {sql} from '@codemirror/lang-sql';
import {xml} from '@codemirror/lang-xml';
import {yaml} from '@codemirror/lang-yaml';
const views=new Map();
const phrases={"Find":"查找","Replace":"替换","next":"下一个","previous":"上一个","all":"全部","match case":"区分大小写","by word":"全词匹配","regexp":"正则表达式","replace":"替换","replace all":"全部替换","close":"关闭","current match":"当前匹配","replaced $ matches":"已替换 $ 处","replaced match on line $":"已替换第 $ 行的匹配","on line":"位于行","Go to line":"跳转行","go":"跳转","Folded lines":"已折叠行","Unfolded lines":"已展开行","to":"至"};
function language(path){const ext=path.split('.').pop().toLowerCase();return ({js:()=>javascript(),jsx:()=>javascript({jsx:true}),ts:()=>javascript({typescript:true}),tsx:()=>javascript({typescript:true,jsx:true}),py:python,rs:rust,html,htm:html,css,json,md:markdown,sql,xml,svg:xml,yml:yaml,yaml}[ext]||(()=>[]))();}
export function mount(host,id,text,path,english,change,save){if(views.has(id)){activate(host,id);return;}
 const locale=new Compartment(),readonly=new Compartment();const box=document.createElement('div');box.className='source-code-view';box.dataset.document=id;host.append(box);
 const view=new EditorView({parent:box,state:EditorState.create({doc:text,extensions:[keymap.of([{key:'Mod-s',run:()=>{save();return true;}},{key:'Ctrl-s',run:()=>{save();return true;}},indentWithTab]),basicSetup,language(path),locale.of(EditorState.phrases.of(english?{}:phrases)),readonly.of([EditorState.readOnly.of(false),EditorView.editable.of(true)]),EditorView.contentAttributes.of({'aria-label':english?'Source editor':'代码编辑器',spellcheck:'false'}),EditorView.updateListener.of(update=>{if(update.docChanged)change(update.state.doc.toString());}),EditorView.theme({'&':{height:'100%',color:'var(--ink)',backgroundColor:'var(--bg)'},'.cm-scroller':{fontFamily:'ui-monospace,SFMono-Regular,Consolas,monospace',fontSize:'12px',lineHeight:'1.6'},'.cm-gutters':{backgroundColor:'var(--surface)',color:'var(--muted)',borderRight:'1px solid var(--line)'},'.cm-activeLine,.cm-activeLineGutter':{backgroundColor:'var(--selected)'},'.cm-panels':{backgroundColor:'var(--surface)',color:'var(--ink)'},'.cm-selectionBackground':{backgroundColor:'var(--selected) !important'},'.cm-cursor':{borderLeftColor:'var(--ink)'},'.cm-searchMatch':{backgroundColor:'var(--amber-bg)',outline:'1px solid var(--amber)'},'.cm-textfield,.cm-button':{fontSize:'11px',color:'var(--ink)',background:'var(--bg)',border:'1px solid var(--line)'},'.cm-tooltip':{backgroundColor:'var(--bg)',color:'var(--ink)',border:'1px solid var(--line)'}})]})});
 views.set(id,{view,box,locale,readonly,english});activate(host,id);
}
export function activate(host,id){for(const [key,{view,box}]of views){if(box.parentElement!==host)host.append(box);box.style.display=key===id?'flex':'none';if(key===id){view.requestMeasure();}}}
export function setReadonly(busy){for(const {view,readonly}of views.values())view.dispatch({effects:readonly.reconfigure([EditorState.readOnly.of(busy),EditorView.editable.of(!busy)])});}
export function setLocale(english){for(const item of views.values()){if(item.english===english)continue;item.english=english;closeSearchPanel(item.view);item.view.dispatch({effects:item.locale.reconfigure(EditorState.phrases.of(english?{}:phrases))});item.view.contentDOM.setAttribute('aria-label',english?'Source editor':'代码编辑器');}}
export function navigate(id,offset,length){const item=views.get(id);if(!item)return;const from=Math.min(Math.max(offset,0),item.view.state.doc.length);const to=Math.min(from+Math.max(length,0),item.view.state.doc.length);item.view.dispatch({selection:{anchor:from,head:to},effects:EditorView.scrollIntoView(from,{y:'center'})});item.view.focus();}
export function command(id,name){const view=views.get(id)?.view;if(!view)return;({find:openSearchPanel,goto:gotoLine,undo,redo,indent:indentMore,outdent:indentLess}[name]||(()=>false))(view);if(!['find','goto'].includes(name))view.focus();}
export function destroy(id){const item=views.get(id);if(item){item.view.destroy();item.box.remove();views.delete(id);}}
export function focusSearch(){requestAnimationFrame(()=>document.querySelector('.source-search-query')?.focus());}

export function replace(id,text){const item=views.get(id);if(!item||item.view.state.readOnly)return false;item.view.dispatch({changes:{from:0,to:item.view.state.doc.length,insert:text},selection:{anchor:Math.min(item.view.state.selection.main.head,text.length)},userEvent:'input.merge',annotations:isolateHistory.of('full')});item.view.focus();return true;}
