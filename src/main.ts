import './style.css';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWebview } from '@tauri-apps/api/webview';

type Status = 'ready' | 'working' | 'success' | 'error' | 'cancelled';
interface Track { path: string; name: string; title: string; artist: string; format: string; duration: number; bitrate: number; error: string | null; status: Status; progress: number; message: string; output?: string }
interface Update { path: string; status: Status; progress: number; message: string; output: string | null }
interface Summary { succeeded: number; failed: number; cancelled: number; outputs: string[] }

const paths: Record<string, string> = {
  music: '<path d="M9 18V5l12-2v13"/><path d="m9 9 12-2"/><ellipse cx="6" cy="18" rx="3" ry="3"/><ellipse cx="18" cy="16" rx="3" ry="3"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
  folder: '<path d="M3 7a2 2 0 0 1 2-2h5l2 2h7a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z"/>',
  arrow: '<path d="M5 12h14m-6-6 6 6-6 6"/>',
  check: '<path d="m5 12 4 4L19 6"/>',
  close: '<path d="m6 6 12 12M6 18 18 6"/>',
  lock: '<rect x="5" y="10" width="14" height="11" rx="3"/><path d="M8 10V7a4 4 0 0 1 8 0v3m-4 5v2"/>',
  chevron: '<path d="m8 10 4 4 4-4"/>',
  alert: '<circle cx="12" cy="12" r="9"/><path d="M12 7v6m0 4h.01"/>',
  stop: '<rect x="6" y="6" width="12" height="12" rx="2"/>',
  retry: '<path d="M3 10a9 9 0 1 1 2 8M3 4v6h6"/>',
  disc: '<circle cx="12" cy="12" r="9"/><circle cx="12" cy="12" r="3"/><path d="m5 9 2-2m10 10 2-2"/>'
};
const icon = (name: string, cls = '') => `<svg class="icon ${cls}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${paths[name] || paths.music}</svg>`;
const escape = (value: string) => value.replace(/[&<>"']/g, c => ({ '&':'&amp;', '<':'&lt;', '>':'&gt;', '"':'&quot;', "'":'&#39;' }[c]!));
const stored = (key: string, fallback: string) => { try { return localStorage.getItem(key) || fallback; } catch { return fallback; } };
const persist = (key: string, value: string) => { try { localStorage.setItem(key, value); } catch { /* Preferences are optional. */ } };
let tracks: Track[] = [];
let running = false;
let importing = false;
let cancelling = false;
let outputDir = stored('musicdecode.output', '');
let bitrate = Number(stored('musicdecode.bitrate', '320'));
if (![192,256,320].includes(bitrate)) bitrate = 320;
let recursive = stored('musicdecode.recursive', 'false') === 'true';
let lastOutput = '';
let sessionPaths: string[] = [];
let noticeTimer: ReturnType<typeof setTimeout>;

document.querySelector<HTMLDivElement>('#app')!.innerHTML = `
  <main class="shell">
    <header class="app-header">
      <div class="brand"><span class="brand-mark">${icon('music')}</span><div><span class="brand-name">MusicDecode</span><span class="brand-caption">给音乐，一个轻松的出口</span></div></div>
      <span class="offline">${icon('lock')} 本地离线处理</span>
    </header>
    <section class="intro"><div><p class="eyebrow">NCM <span>→</span> MP3</p><h1>音乐解码，简单一点。</h1><p class="subtitle">拖入歌曲，轻松还原。保留喜欢的声音，也保留每一张封面。</p></div><span class="intro-decoration" aria-hidden="true"><i></i><i></i><i></i><i></i><i></i></span></section>
    <div class="workspace">
      <section class="queue-card soft-card" aria-label="待转换歌曲">
        <div class="section-heading"><h2>我的歌曲 <span id="count">0</span></h2><button class="text-button" id="clear" disabled>清空</button></div>
        <div class="queue-actions"><button class="soft-button" id="add-files">${icon('plus')} 添加文件</button><button class="soft-button" id="add-folder">${icon('folder')} 添加文件夹</button></div>
        <div class="drop-zone" id="drop-zone">
          <div class="empty-state" id="empty-state"><div class="record"><div class="record-label">${icon('music')}</div></div><h3>把 NCM 文件拖到这里</h3><p>也可以添加文件或文件夹，支持批量转换</p><span class="file-chip">.ncm</span></div>
          <div class="track-list" id="track-list" role="list"></div>
          <div class="drag-overlay">${icon('plus')} 松开，添加到歌曲列表</div>
        </div>
        <div class="queue-footnote">${icon('check')} 源文件始终保留 <span>·</span> 同名文件自动编号</div>
      </section>
      <aside class="settings-column">
        <section class="soft-card output-card"><div class="section-heading"><h2>保存到哪里</h2><span class="small-icon">${icon('folder')}</span></div><div class="path-well"><span class="path-icon">${icon('folder')}</span><div><span id="output-label">源文件旁的 Decoded 文件夹</span><span id="output-description">自动整理，不打扰原文件</span></div></div><button class="soft-button full-width" id="choose-output">选择输出文件夹 ${icon('chevron')}</button><button class="text-button reset-path" id="reset-output" hidden>恢复默认位置</button></section>
        <section class="soft-card quality-card"><div class="section-heading"><h2>声音，保持原样</h2>${icon('disc','small-icon')}</div><p class="quality-copy">原本是 MP3，直接还原原始音质。</p><label class="field-label" for="bitrate">其他格式转为 MP3 时</label><div class="select-wrap"><select id="bitrate" aria-label="其他格式转码音质"><option value="320">320 kbps · 高品质</option><option value="256">256 kbps · 均衡</option><option value="192">192 kbps · 小体积</option></select>${icon('chevron')}</div><p class="muted-hint">FLAC 等格式转为 MP3 会有损压缩。</p><label class="check-option"><input type="checkbox" id="recursive"/><span>添加文件夹时包含子文件夹</span></label></section>
        <button class="open-output" id="open-output" disabled>${icon('folder')} 打开输出文件夹 ${icon('arrow')}</button>
      </aside>
    </div>
    <footer class="conversion-bar soft-card"><div class="conversion-info"><span class="status-orb" id="status-orb">${icon('music')}</span><div><strong id="summary-title">等待添加歌曲</strong><p id="summary-description">准备好后，点击开始解码</p></div></div><div class="conversion-actions"><button class="soft-button cancel-button" id="cancel" hidden>${icon('stop')} 取消</button><button class="primary-button" id="start" disabled>开始解码 ${icon('arrow')}</button></div><div class="overall-track"><div id="overall-progress"></div></div></footer>
    <div class="bottom-line"><span>简单一点，听见更多。</span><span>MusicDecode <span class="version">1.1</span></span></div>
  </main><div id="notice" class="notice" role="status" aria-live="polite"></div>`;

const el = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
const button = (id: string) => el<HTMLButtonElement>(id);
const formatDuration = (ms: number) => { const seconds = Math.round(ms/1000); return seconds ? `${Math.floor(seconds/60)}:${String(seconds%60).padStart(2,'0')}` : ''; };

function notice(message: string, error = false) {
  clearTimeout(noticeTimer);
  el('notice').textContent = message;
  el('notice').className = `notice visible${error ? ' error' : ''}`;
  noticeTimer = setTimeout(() => el('notice').classList.remove('visible'), 5500);
}

function updateSettings() {
  el('output-label').textContent = outputDir || '源文件旁的 Decoded 文件夹';
  el('output-label').title = outputDir || '在每个源文件所在目录创建 Decoded 子文件夹';
  el('output-description').textContent = outputDir ? '已选择自定义输出位置' : '自动整理，不打扰原文件';
  button('reset-output').hidden = !outputDir;
  el<HTMLSelectElement>('bitrate').value = String(bitrate);
  el<HTMLInputElement>('recursive').checked = recursive;
}

function renderTracks() {
  el('count').textContent = String(tracks.length);
  el('empty-state').hidden = tracks.length > 0;
  el('drop-zone').classList.toggle('has-tracks', tracks.length > 0);
  el('track-list').innerHTML = tracks.map((track,index) => {
    const meta = [track.artist || track.name, formatDuration(track.duration)].filter(Boolean).join(' · ');
    const label = track.status === 'ready' ? (track.error || `${track.format==='MP3'?'直接还原':'转为 MP3'} · 等待解码`) : track.message;
    const stateIcon = track.status === 'success' ? 'check' : track.status === 'error' ? 'alert' : track.status === 'cancelled' ? 'stop' : 'music';
    return `<article class="track ${track.status}" role="listitem"><span class="track-art">${icon(stateIcon)}</span><div class="track-content"><strong title="${escape(track.path)}">${escape(track.title)}</strong><p title="${escape(meta)}">${escape(meta)}</p><div class="track-bottom"><span class="format-tag">${escape(track.format)}${track.bitrate ? ` · ${Math.round(track.bitrate/1000)}k` : ''}</span><span class="track-message" title="${escape(label)}">${escape(label)}</span></div>${track.status === 'working' ? `<div class="track-progress"><i style="width:${track.progress}%"></i></div>` : ''}</div><button class="remove-track" data-index="${index}" aria-label="移除 ${escape(track.title)}" ${running ? 'disabled' : ''}>${icon('close')}</button></article>`;
  }).join('');
  syncControls();
}

function syncControls() {
  const pending = tracks.filter(t => t.status !== 'success' && !t.error).length;
  for (const id of ['add-files','add-folder','choose-output','reset-output','clear']) button(id).disabled = running || importing || (id === 'clear' && !tracks.length);
  el<HTMLSelectElement>('bitrate').disabled = running;
  el<HTMLInputElement>('recursive').disabled = running || importing;
  button('start').disabled = running || importing || !pending;
  button('cancel').hidden = !running;
  button('cancel').disabled = cancelling;
  button('cancel').innerHTML = `${icon('stop')} ${cancelling ? '正在取消' : '取消'}`;
  button('start').innerHTML = running ? '<span class="spinner"></span> 正在解码' : `${tracks.some(t=>(t.status==='error'&&!t.error)||t.status==='cancelled') && pending ? '重试未完成' : '开始解码'} ${icon('arrow')}`;
  button('open-output').disabled = !lastOutput && !outputDir;
  el('status-orb').classList.toggle('active',running);
  if (running) {
    const current = tracks.filter(t=>sessionPaths.includes(t.path));
    const done = current.filter(t=>['success','error','cancelled'].includes(t.status)).length;
    const percent = current.length ? current.reduce((sum,t)=>sum+(['success','error','cancelled'].includes(t.status)?100:t.progress),0)/current.length : 0;
    el('overall-progress').style.width = `${percent}%`;
    el('summary-title').textContent = cancelling ? '正在取消任务…' : `正在处理 ${Math.min(done+1,current.length)} / ${current.length} 首歌曲`;
    el('summary-description').textContent = cancelling ? '正在清理未完成文件，请稍候' : '请稍等，正在为你还原音乐';
  } else if (importing) {
    el('summary-title').textContent = '正在读取歌曲…';
    el('summary-description').textContent = '正在识别音频格式与歌曲信息';
  } else {
    const success = tracks.filter(t=>t.status==='success').length;
    el('summary-title').textContent = success ? `已完成 ${success} 首${pending ? `，还有 ${pending} 首待处理` : ''}` : tracks.length ? `${tracks.length} 首歌曲已添加` : '等待添加歌曲';
    el('summary-description').textContent = tracks.length ? pending ? 'MP3 直接还原，其他格式按所选音质转换' : success ? '音乐已保存，可以打开输出文件夹' : '这些文件暂时无法处理，请查看歌曲列表中的原因' : '准备好后，点击开始解码';
    if (!tracks.length) el('overall-progress').style.width = '0%';
  }
}

function requireDesktop() {
  if (isTauri()) return true;
  notice('这是界面预览，请在 MusicDecode 桌面程序中添加和转换文件。');
  return false;
}

async function addPaths(paths: string[]) {
  if (!paths.length || running || importing) return;
  importing = true; syncControls();
  try {
    const result = await invoke<{items: Omit<Track,'status'|'progress'|'message'>[]; skipped: number}>('import_paths',{paths,recursive});
    const existing = new Set(tracks.map(t=>t.path));
    let added = 0;
    for (const item of result.items) {
      if (existing.has(item.path)) continue;
      tracks.push({...item,status:item.error?'error':'ready',progress:0,message:item.error || '等待解码'});
      existing.add(item.path); added++;
    }
    notice(added ? `已添加 ${added} 首歌曲${result.skipped ? '，已略过非 NCM 或重复文件' : ''}` : '没有新的 NCM 文件，文件可能已在列表中');
  } catch (e) { notice(String(e),true); }
  finally { importing=false; renderTracks(); }
}

button('add-files').onclick = async () => {
  if (!requireDesktop()) return;
  try { await addPaths(await invoke<string[]>('pick_files')); } catch(e) { notice(String(e),true); }
};
button('add-folder').onclick = async () => {
  if (!requireDesktop()) return;
  try { const path=await invoke<string|null>('pick_directory',{title:'选择包含 NCM 的文件夹'}); if(path) await addPaths([path]); } catch(e) { notice(String(e),true); }
};
button('choose-output').onclick = async () => {
  if (!requireDesktop()) return;
  try { const path=await invoke<string|null>('pick_directory',{title:'选择 MP3 输出文件夹'}); if(path){outputDir=path;persist('musicdecode.output',path);updateSettings();syncControls();} } catch(e) { notice(String(e),true); }
};
button('reset-output').onclick = () => {outputDir='';persist('musicdecode.output','');updateSettings();syncControls();};
el<HTMLSelectElement>('bitrate').onchange = event => {bitrate=Number((event.target as HTMLSelectElement).value);persist('musicdecode.bitrate',String(bitrate));};
el<HTMLInputElement>('recursive').onchange = event => {recursive=(event.target as HTMLInputElement).checked;persist('musicdecode.recursive',String(recursive));};
button('clear').onclick = () => { if(running||importing)return;tracks=[];renderTracks(); };
el('track-list').onclick = event => {
  const remove=(event.target as HTMLElement).closest<HTMLButtonElement>('[data-index]');
  if(remove&&!running&&!importing){tracks.splice(Number(remove.dataset.index),1);renderTracks();}
};
button('open-output').onclick = async () => {
  if (!requireDesktop()) return;
  try { await invoke('open_directory',{path:lastOutput||outputDir}); } catch(e) { notice(String(e),true); }
};
button('cancel').onclick = async () => {
  if(!running)return;
  cancelling=true;syncControls();
  try {await invoke('cancel_batch');} catch(e){cancelling=false;syncControls();notice(String(e),true);}
};
button('start').onclick = async () => {
  if(!requireDesktop()||running||importing)return;
  sessionPaths=tracks.filter(t=>t.status!=='success'&&!t.error).map(t=>t.path);
  if(!sessionPaths.length)return;
  for(const t of tracks){if(sessionPaths.includes(t.path)){t.status='ready';t.progress=0;t.message='等待解码';}}
  running=true;cancelling=false;renderTracks();
  try {
    const result=await invoke<Summary>('convert_batch',{paths:sessionPaths,outputDir:outputDir||null,bitrate});
    if(result.outputs.length)lastOutput=result.outputs.at(-1)!;
    notice(`完成 ${result.succeeded} 首${result.failed?`，失败 ${result.failed} 首`:''}${result.cancelled?`，取消 ${result.cancelled} 首`:''}`,result.failed>0);
  } catch(e) {
    for(const t of tracks){if(sessionPaths.includes(t.path)&&['ready','working'].includes(t.status)){t.status='error';t.message=String(e);}}
    notice(String(e),true);
  } finally { running=false;cancelling=false;renderTracks(); }
};

async function initialize() {
  updateSettings();renderTracks();
  if(!isTauri())return;
  try {
    await listen<Update>('decode-progress',({payload})=>{
      const track=tracks.find(t=>t.path===payload.path);if(!track)return;
      track.status=payload.status;track.progress=payload.progress;track.message=payload.message;
      if(payload.output){track.output=payload.output;lastOutput=payload.output;}
      renderTracks();
    });
    await listen('close-during-job',()=>notice('请先取消当前任务，等待清理完成后再关闭窗口。'));
    await getCurrentWebview().onDragDropEvent(event=>{
      const drop=el('drop-zone');
      if(event.payload.type==='enter'||event.payload.type==='over'){if(!running&&!importing)drop.classList.add('dragging');}
      else{drop.classList.remove('dragging');if(event.payload.type==='drop')void addPaths(event.payload.paths);}
    });
  }catch(e){notice(`无法初始化桌面功能：${String(e)}`,true);}
}
void initialize();
