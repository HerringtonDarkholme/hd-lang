export function entrySource(names, policy) {
  return `import { readFile } from 'node:fs/promises';
import { createSupervisor } from './sdk/supervisor.js';
import { createFetchHandler } from './sdk/fetch.js';
import { createVercelNodeHandler } from './sdk/vercel-node.js';
const module=new WebAssembly.Module(await readFile(new URL('./app.wasm',import.meta.url)));
const names=${JSON.stringify(names)};
const environment=Object.freeze(Object.fromEntries(names.map(name=>[name,typeof process.env[name]==='string'?process.env[name]:null])));
const grants=JSON.parse(await readFile(new URL('./grants.json',import.meta.url),'utf8'));
const policy=${JSON.stringify(policy)};
const ready=new Set();let accept,refuse;
export const startup=new Promise((resolve,reject)=>{accept=resolve;refuse=reject;});
startup.catch(()=>{});
const supervisor=createSupervisor({program:{module,environment,grants,
 bindingsURL:new URL('./bindings.js',import.meta.url).href,
 runtimeURL:new URL('./runtime/deployed-core.js',import.meta.url).href,
 applicationURL:new URL('./sdk/application.js',import.meta.url).href,
},policy,event(record){
 if(record.kind==='ready'){ready.add(record.slot);if(ready.size===policy.workers)accept();}
 else if(record.kind==='terminated')refuse(new Error('application startup failed'));
},log(record){(record.stream==='err'?console.error:console.log)(JSON.stringify(record));}});
const fetcher=createFetchHandler(supervisor);
export default createVercelNodeHandler(fetcher,{isReady:supervisor.ready,retryAfter:policy.retryAfter});
export const close=()=>supervisor.close();
`;
}
export const deployedCoreSource = `import {workerData} from 'node:worker_threads';
import {createCoreHost as base} from './core.js';
export {createPackageRuntime} from './package-providers.js';
export function createCoreHost(sink,env,layer){
 const environment=workerData.program.environment;
 const result=base(sink,env,{...layer,providers(context){
  const platform=layer.providers(context);
  return {...platform,traits:{...platform.traits,'hd:Env':{
   get(name){return context.grant.coversName('Env',name)&&Object.hasOwn(environment,name)?environment[name]:null;},
   names(){return Object.keys(environment).filter(name=>environment[name]!==null&&context.grant.coversName('Env',name));}
  }}};
 }});
 delete result.imports['hd:Args'];
 for(const key of ['Clock','Random'])if(env.grants[key]===false)delete result.imports['hd:'+key];
 return result;
}
`;
