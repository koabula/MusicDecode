import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, writeFileSync, renameSync, unlinkSync } from 'node:fs';
import { resolve } from 'node:path';
const sources=JSON.parse(readFileSync(new URL('../third-party/sources.json',import.meta.url),'utf8'));
const folder=resolve('work/sources');mkdirSync(folder,{recursive:true});
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
for(const [name,source] of Object.entries(sources)){
 const dest=resolve(folder,source.filename);
 if(existsSync(dest)&&hash(readFileSync(dest))===source.sha256){console.log(`${name}: cached source verified`);continue;}
 let last;
 for(let attempt=0;attempt<3;attempt++){
  try{
   const response=await fetch(source.url,{signal:AbortSignal.timeout(180000)});
   if(!response.ok)throw new Error(`HTTP ${response.status}`);
   const bytes=Buffer.from(await response.arrayBuffer());
   if(hash(bytes)!==source.sha256)throw new Error(`${name}: SHA-256 mismatch`);
   const partial=dest+'.download';writeFileSync(partial,bytes);if(existsSync(dest))unlinkSync(dest);renameSync(partial,dest);
   console.log(`${name} ${source.version}: downloaded and verified`);last=null;break;
  }catch(error){last=error;console.warn(`${name}: ${error.message}`);}
 }
 if(last)throw last;
}
