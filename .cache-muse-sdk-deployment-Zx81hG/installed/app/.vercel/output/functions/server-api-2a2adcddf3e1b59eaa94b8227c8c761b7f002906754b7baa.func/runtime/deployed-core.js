import {workerData} from 'node:worker_threads';
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
