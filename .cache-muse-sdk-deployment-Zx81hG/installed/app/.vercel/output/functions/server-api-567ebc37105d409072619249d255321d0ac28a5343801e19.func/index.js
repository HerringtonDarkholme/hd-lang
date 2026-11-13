import { readFile } from 'node:fs/promises';
import { createSupervisor } from './sdk/supervisor.js';
import { createFetchHandler } from './sdk/fetch.js';
import { createVercelNodeHandler } from './sdk/vercel-node.js';
const module=new WebAssembly.Module(await readFile(new URL('./app.wasm',import.meta.url)));
const names=[];
const environment=Object.freeze(Object.fromEntries(names.map(name=>[name,typeof process.env[name]==='string'?process.env[name]:null])));
const grants=JSON.parse(await readFile(new URL('./grants.json',import.meta.url),'utf8'));
const policy={"workers":1,"startupMs":3000,"requestMs":20000,"drainMs":1000,"stopKillMs":300,"restartLimit":2,"restartWindowMs":60000,"retryAfter":"1"};
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
