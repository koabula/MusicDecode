import { readFileSync } from 'node:fs';
const packageVersion=JSON.parse(readFileSync('package.json','utf8')).version;
const config=JSON.parse(readFileSync('src-tauri/tauri.conf.json','utf8'));
const rust=readFileSync('src-tauri/Cargo.toml','utf8').match(/^version\s*=\s*"([^"]+)"/m)?.[1];
const lock=readFileSync('src-tauri/Cargo.lock','utf8').match(/name = "musicdecode"\r?\nversion = "([^"]+)"/)?.[1];
if([config.version,rust,lock].some(v=>v!==packageVersion))throw Error(`Version mismatch: package=${packageVersion}, tauri=${config.version}, cargo=${rust}, lock=${lock}`);
const tag=process.env.RELEASE_TAG;
if(tag&&tag!==`v${packageVersion}`)throw Error(`Tag ${tag} does not match v${packageVersion}`);
console.log(`Version verified: ${packageVersion}`);
