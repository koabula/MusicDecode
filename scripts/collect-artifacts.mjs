import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve, join, basename } from 'node:path';
import { createHash } from 'node:crypto';
const target=process.env.MUSICDECODE_TARGET;
if(!target)throw Error('MUSICDECODE_TARGET is required');
const version=JSON.parse(readFileSync('package.json','utf8')).version;
const folder=resolve('artifacts');mkdirSync(folder,{recursive:true});
const bundle=resolve('src-tauri/target',target,'release/bundle');
function walk(dir){return readdirSync(dir,{withFileTypes:true}).flatMap(e=>e.isDirectory()?walk(join(dir,e.name)):[join(dir,e.name)]);}
const files=walk(bundle).filter(p=>/\.(dmg|deb|AppImage)$/.test(p)||/-setup\.exe$/.test(p));
if(!files.length)throw Error('No installable bundles were produced');
for(const file of files)copyFileSync(file,join(folder,basename(file)));
copyFileSync('THIRD-PARTY-NOTICES.txt',join(folder,`THIRD-PARTY-NOTICES-${target}.txt`));
if(target==='x86_64-unknown-linux-gnu'){
 copyFileSync('work/sources/ffmpeg.tar.gz',join(folder,'ffmpeg-9.0.2-source.tar.gz'));
 copyFileSync('work/sources/lame.tar.gz',join(folder,'lame-3.100-source.tar.gz'));
 copyFileSync('scripts/build-ffmpeg.sh',join(folder,'build-ffmpeg.sh'));
 copyFileSync('third-party/sources.json',join(folder,'audio-sources.json'));
}
let hashes='';
for(const name of readdirSync(folder).sort()){
 const file=join(folder,name);hashes+=`${createHash('sha256').update(readFileSync(file)).digest('hex')}  ${name}\n`;
}
writeFileSync(join(folder,`SHA256SUMS-${target}.txt`),hashes);
console.log(`Collected MusicDecode ${version} ${target}: ${files.map(p=>basename(p)).join(', ')}`);
