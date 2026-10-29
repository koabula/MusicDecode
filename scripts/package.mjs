import { copyFileSync, cpSync, existsSync, mkdirSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
const root=resolve(dirname(fileURLToPath(import.meta.url)),'..');
if(process.platform!=='win32')throw Error('Portable staging is Windows-only. Use npm run package for native installers.');
if(!process.env.npm_execpath)throw Error('Run npm run package:portable.');
const target='x86_64-pc-windows-msvc';
if(!process.argv.includes('--skip-build')){
 const build=spawnSync(process.execPath,[process.env.npm_execpath,'run','tauri','--','build','--target',target,'--bundles','nsis'],{cwd:root,stdio:'inherit',windowsHide:true});
 if(build.status!==0)process.exit(build.status||1);
}
const output=resolve(root,'release');mkdirSync(output,{recursive:true});
const files=[
 [`src-tauri/target/${target}/release/MusicDecode.exe`,'MusicDecode.exe'],
 [`src-tauri/binaries/musicdecode-ffmpeg-${target}.exe`,'musicdecode-ffmpeg.exe'],
 ['README.zh-CN.md','使用说明.md'],['README.md','README.md'],['LICENSE','LICENSE'],['THIRD-PARTY-NOTICES.txt','THIRD-PARTY-NOTICES.txt']
];
for(const [source,dest]of files){
 if(!existsSync(resolve(root,source)))throw Error(`Missing build input: ${source}`);
 try{copyFileSync(resolve(root,source),resolve(output,dest));}catch(error){if(error.code==='EBUSY')throw Error('Close MusicDecode before packaging it again.');throw error;}
}
mkdirSync(resolve(output,'licenses'),{recursive:true});
cpSync(resolve(root,'third-party/generated'),resolve(output,'licenses/audio'),{recursive:true});
console.log(`Portable files saved to ${output}`);
