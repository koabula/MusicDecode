import { readFileSync, readdirSync, writeFileSync, existsSync } from 'node:fs';
import { resolve, dirname, basename } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'..');
const cargo=process.env.CARGO || 'cargo';
const metadata=spawnSync(cargo,['metadata','--manifest-path','src-tauri/Cargo.toml','--format-version','1','--offline'],{cwd:root,encoding:'utf8',maxBuffer:20*1024*1024,windowsHide:true});
if(metadata.status!==0)throw new Error(metadata.stderr);
const packages=JSON.parse(metadata.stdout).packages;
let text='MusicDecode third-party notices\n\nAudio sidecar: FFmpeg 9.0.2 + LAME 3.100, compiled from source.\nFFmpeg: LGPL-2.1-or-later; LAME: LGPL-2.0-or-later.\nGPL, nonfree and networking features are disabled in this build.\nThe audio executable is a separate local process. Its libraries are linked statically.\nSource URLs and SHA-256 hashes: third-party/sources.json in the source repository.\nExact build recipe: scripts/build-ffmpeg.sh in the source repository.\nMatching complete source archives and build recipe accompany every GitHub Release.\nAudio licenses and build configuration: licenses/audio/ in installed distributions.\nProject: https://github.com/koabula/MusicDecode\nFFmpeg: https://ffmpeg.org/\nLAME: https://lame.sourceforge.io/\n\n';
for(const name of ['FFmpeg-LICENSE.txt','LAME-LICENSE.txt','BUILD.txt']){
 const file=resolve(root,'third-party/generated',name);
 if(existsSync(file))text+=`\n--- Audio: ${name} ---\n${readFileSync(file,'utf8')}\n`;
}
text+='\nRust dependencies (including target-specific and build dependencies):\n';
for(const pkg of packages.sort((a,b)=>a.name.localeCompare(b.name))) {
 if(pkg.name==='musicdecode')continue;
 text+=`\n${'='.repeat(70)}\n${pkg.name} ${pkg.version}\nLicense: ${pkg.license||'See package license'}\n${pkg.repository||''}\n`;
 const dir=dirname(pkg.manifest_path);
 for(const entry of readdirSync(dir,{withFileTypes:true})) {
  if(entry.isFile()&&/^(licen[cs]e|copying|notice)($|[._-])/i.test(entry.name)) {
   text+=`\n--- ${entry.name} ---\n${readFileSync(resolve(dir,entry.name),'utf8')}\n`;
  }
 }
}
const api=resolve(root,'node_modules/@tauri-apps/api');
if(existsSync(api))for(const file of readdirSync(api).filter(n=>/^licen[cs]e/i.test(n))){text+=`\n@tauri-apps/api / ${basename(file)}\n${readFileSync(resolve(api,file),'utf8')}\n`;}
writeFileSync(resolve(root,'THIRD-PARTY-NOTICES.txt'),text);
console.log(`Wrote notices for ${packages.length-1} Rust packages.`);
