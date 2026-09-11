import fs from 'node:fs';
import path from 'node:path';
import vm from 'node:vm';
import {createRequire} from 'node:module';
import {fileURLToPath} from 'node:url';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../app');
const require=createRequire(path.join(root,'package.json'));
const ts=require('typescript');
const result={};
function flatten(value,prefix='',out={}){for(const [key,item] of Object.entries(value)){const name=prefix?prefix+'.'+key:key;if(typeof item==='string')out[name]=item;else if(item&&typeof item==='object')flatten(item,name,out);}return out;}
const modules=new Map();
function load(locale) {
    if(modules.has(locale))return modules.get(locale);
    const source=fs.readFileSync(path.join(root,'src/i18n',locale+'.ts'),'utf8');
    const output=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
    const exports={};modules.set(locale,exports);
    vm.runInNewContext(output,{exports,require:specifier=>{if(!/^\.\/[a-zA-Z-]+$/.test(specifier))throw new Error('Unexpected locale import');return load(specifier.slice(2));}},{timeout:5000});
    return exports;
}
for(const locale of ['zh-CN','zh-TW','en','ja','ko','es','fr','de'])result[locale]=flatten(Object.values(load(locale)).find(value=>value&&typeof value==='object'));
fs.mkdirSync(path.join(root,'linux-egui/assets'),{recursive:true});
fs.writeFileSync(path.join(root,'linux-egui/assets/ui-locales.json'),JSON.stringify(result)+'\n','utf8');
