export function environmentNames(grants, names) {
  if (names === undefined) {
    if (
      grants.Env === true ||
      (Array.isArray(grants.Env) && grants.Env.some((x) => x.endsWith("*")))
    )
      throw new Error("Env=true or prefix scopes require explicit envBindings names");
    names = Array.isArray(grants.Env) ? grants.Env : [];
  }
  if (
    !Array.isArray(names) ||
    !names.every((x) => typeof x === "string" && /^[A-Za-z_][A-Za-z0-9_]*$/.test(x)) ||
    new Set(names).size !== names.length
  )
    throw new Error("envBindings needs distinct binding names");
  const covered = (name) =>
    grants.Env === true ||
    (Array.isArray(grants.Env) &&
      grants.Env.some((x) => (x.endsWith("*") ? name.startsWith(x.slice(0, -1)) : name === x)));
  if (!names.every(covered)) throw new Error("an Env binding name is outside production grants");
  return [...names].sort();
}
export function entrySource(names) {
  return `import module from './app.wasm';
import * as bindings from './bindings.mjs';
import { createCoreHost } from './runtime/core.mjs';
import { createApplication } from './sdk/application.mjs';
import { createFetchHandler } from './sdk/fetch.mjs';
import grants from './grants.json';
const names=${JSON.stringify(names)};
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
  const app=createApplication({module,bindings,createCoreHost:portableCore,grants,sink,
   log:record=>(record.stream==='err'?console.error:console.log)(JSON.stringify(record))});
  app.start();handler=createFetchHandler(app,{sink});
 }
 return handler.fetch(request);
 }catch{failed=true;console.error(JSON.stringify({event:'http.startup-failed'}));return new Response(null,{status:500});}
}};
`;
}
