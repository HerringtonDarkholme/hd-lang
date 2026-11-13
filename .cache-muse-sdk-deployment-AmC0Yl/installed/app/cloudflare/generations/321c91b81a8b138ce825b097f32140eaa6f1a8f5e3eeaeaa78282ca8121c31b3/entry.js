import module from './app.wasm';
import * as bindings from './bindings.js';
import { createCoreHost } from './runtime/core.js';
import { createPackageRuntime } from './runtime/package-providers.js';
import { createApplication } from './sdk/application.js';
import { createFetchHandler } from './sdk/fetch.js';
import grants from './grants.json';
const names=[];
let handler,failed=false;
export default {fetch(request,bindingsEnv){
 if(failed)return new Response(null,{status:503});
 try {if(!handler){
  const environment=Object.freeze(Object.fromEntries(names.map(name=>[name,typeof bindingsEnv[name]==='string'?bindingsEnv[name]:null])));
  const portableCore=(sink,env,layer)=>{
   const result=createCoreHost(sink,env,{...layer,providers(context){
    const platform=layer.providers(context);
    return {...platform,traits:{...platform.traits,'hd:Env':{
     get(name){return context.grant.coversName('Env',name)&&Object.hasOwn(environment,name)?environment[name]:null;},
     names(){return names.filter(name=>environment[name]!==null&&context.grant.coversName('Env',name));}
    }}};
   }});
   delete result.imports['hd:Args'];
   for(const key of ['Clock','Random'])if(grants[key]===false)delete result.imports['hd:'+key];
   return result;
  };
  const sink={out:text=>console.log(text),err:text=>console.error(text)};
  const app=createApplication({module,bindings,createCoreHost:portableCore,createPackageRuntime,grants,sink,
   log:record=>(record.stream==='err'?console.error:console.log)(JSON.stringify(record))});
  app.start();handler=createFetchHandler(app,{sink});
 }
 return handler.fetch(request);
 }catch{failed=true;console.error(JSON.stringify({event:'http.startup-failed'}));return new Response(null,{status:500});}
}};
