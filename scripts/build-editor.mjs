import {build} from 'esbuild';
import {mkdir} from 'node:fs/promises';
await mkdir('target/editor',{recursive:true});
await build({entryPoints:['crates/ui/editor/index.js'],bundle:true,minify:true,format:'iife',globalName:'ClearCode',outfile:'target/editor/editor.js',target:['safari16','chrome110'],legalComments:'eof'});
