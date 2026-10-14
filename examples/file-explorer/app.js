import { fs } from '@sys';
const $ = (id) => document.getElementById(id);
const esc = (s) => String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;');
const join = (a,b) => a.replace(/[\\/]+$/,'') + '/' + b;
const parent = (p) => fs.realpathSync(join(p,'..'));
const basename = (p) => p.replace(/[\\/]+$/,'').split(/[\\/]/).pop() || p;
const size = (n) => n < 1024 ? n+' B' : n < 1048576 ? (n/1024).toFixed(1)+' KB' : (n/1048576).toFixed(1)+' MB';
const date = (n) => n ? new Date(n*1000).toISOString().slice(0,10) : '—';
const report = (s) => { $('status').textContent=s; };
const workspace = fs.realpathSync(join(__DIR__,'..'));
let current='', entries=[], visible=[], selected=null, history=[], cursor=-1, sort='name', direction=1, hidden=false, expanded=new Set(), menu=null;
let lastClick = null;
function load(path, record=true) {
  try {
    const resolved=fs.realpathSync(path), data=fs.readdirSync(resolved);
    if (!data) throw new Error('This path is not a folder');
    const next=data.map((e) => { const p=join(resolved,e.name); let stat=null; try { stat=fs.statSync(p); } catch (_) {} return {name:e.name,path:p,folder:e.type===fs.UV_DIRENT_DIR,size:stat?.st_size||0,modified:stat?.st_mtime||0,type:e.type===fs.UV_DIRENT_DIR?'Folder':(e.name.includes('.')?e.name.split('.').pop().toUpperCase():'File')}; });
    current=resolved; entries=next; selected=null; lastClick=null; $('search').value='';
    if(record) { history=history.slice(0,cursor+1); history.push(current); cursor=history.length-1; }
    $('path').value=current; $('folder-name').textContent=basename(current); render(); tree(); details(); report('Opened '+current); return true;
  } catch(e) { $('path').value=current; report('Could not open folder: '+e.message); return false; }
}
function render() {
  const q=$('search').value.toLowerCase();
  visible=entries.filter(e=>(hidden||!e.name.startsWith('.'))&&e.name.toLowerCase().includes(q)).sort((a,b)=>Number(b.folder)-Number(a.folder)||direction*(typeof a[sort]==='number'?a[sort]-b[sort]:a[sort].toLowerCase().localeCompare(b[sort].toLowerCase())));
  if (selected && !visible.some(e=>e.path===selected)) { selected=null; details(); }
  $('list').innerHTML=visible.length?visible.map(e=>'<div class="row'+(selected===e.path?' selected':'')+'" role="option" aria-selected="'+(selected===e.path)+'" data-path="'+esc(e.path)+'"><div class="filename"><span class="icon '+(e.folder?'folder':'')+'">'+''+'</span><span>'+esc(e.name)+'</span></div><span class="cell">'+esc(e.type)+'</span><span class="cell">'+(e.folder?'—':size(e.size))+'</span><span class="cell">'+date(e.modified)+'</span></div>').join(''):'<div class="empty">No files to show.</div>';
  $('folder-count').textContent=visible.length+' items'; $('back').disabled=cursor<=0; $('forward').disabled=cursor>=history.length-1; $('hidden').classList.toggle('primary',hidden);
}
function tree() {
  let html='';
  function branch(path,depth) {
    html+='<button data-folder="'+esc(path)+'" class="'+(path===current?'active':'')+'" style="padding-left:'+(10+depth*14)+'px"><span data-toggle="'+esc(path)+'">'+(expanded.has(path)?'−':'+')+'</span>  '+esc(basename(path))+'</button>';
    if(!expanded.has(path)||depth>=5) return;
    try { (fs.readdirSync(path)||[]).filter(e=>e.type===fs.UV_DIRENT_DIR&&(hidden||!e.name.startsWith('.'))).forEach(e=>branch(join(path,e.name),depth+1)); } catch(_) {}
  }
  branch(workspace,0); if(current!==workspace&&!current.startsWith(workspace+'/')) branch(current,0);
  $('tree').innerHTML=html;
}
function details() {
  const e=entries.find(e=>e.path===selected); $('preview').style.display='none'; $('preview').textContent='';
  $('item-name').textContent=e?e.name:'Select a file'; $('preview-icon').textContent=e&&e.folder?'FOLDER':'FILE';
  if(!e) { $('details').textContent='Browse your workspace and preview text files.'; return; }
  $('details').innerHTML='<div class="detail">Kind<b>'+esc(e.type)+'</b></div><div class="detail">Size<b>'+(e.folder?'Folder':size(e.size))+'</b></div><div class="detail">Modified<b>'+date(e.modified)+'</b></div><div class="detail">Location<b>'+esc(e.path)+'</b></div>';
  if(!e.folder&&/\.(txt|md|js|ts|json|html|css|rs|toml|csv|log|xml|svg)$/i.test(e.name)) {
    if(e.size>65536) { report('Text preview limited to files under 64 KB'); return; }
    try { $('preview').textContent=new TextDecoder().decode(fs.readFileSync(e.path)); $('preview').style.display='block'; } catch(error) { report('Could not preview: '+error.message); }
  }
}
function select(path) {
  selected=path;
  $('list').querySelectorAll('[data-path]').forEach(row => {
    const on=row.dataset.path===path;
    row.classList.toggle('selected',on);
    row.setAttribute('aria-selected',String(on));
  });
  details();
}
function openSelected() { const e=entries.find(e=>e.path===selected); if(e?.folder) load(e.path); else if(e) { details(); report('Selected '+e.name); } }
function up() { try { load(parent(current)); } catch(e) { report(e.message); } }
$('search').addEventListener('input',()=>{render(); if(!visible.some(e=>e.path===selected)) {selected=null; details();} });
$('path').addEventListener('keydown',e=>{if(e.key==='Enter')load($('path').value);});
$('back').addEventListener('click',()=>{if(cursor>0){cursor--;if(!load(history[cursor],false))cursor++;}});
$('forward').addEventListener('click',()=>{if(cursor<history.length-1){cursor++;if(!load(history[cursor],false))cursor--;}});
$('up').addEventListener('click',up); $('refresh').addEventListener('click',()=>load(current,false));
$('hidden').addEventListener('click',()=>{hidden=!hidden;render();tree();});
$('choose').addEventListener('click',()=>{const p=Window.this.selectFolder({caption:'Open a folder',path:current}); if(p)load(p);});
$('tree').addEventListener('click',e=>{const toggle=e.target.closest('[data-toggle]'); if(toggle){const p=toggle.dataset.toggle; expanded.has(p)?expanded.delete(p):expanded.add(p);tree();return;} const b=e.target.closest('[data-folder]'); if(b)load(b.dataset.folder);});
document.querySelectorAll('[data-sort]').forEach(b=>b.addEventListener('click',()=>{direction=sort===b.dataset.sort?-direction:1;sort=b.dataset.sort;render();}));
$('list').addEventListener('click',e=>{
  const row=e.target.closest('[data-path]');
  if(!row) { lastClick=null; return; }
  const path=row.dataset.path, now=Date.now();
  // Nile emits click events; recognize a double-click without requiring dblclick.
  const twice=lastClick && lastClick.path===path && now-lastClick.time<=400
    && Math.abs(e.clientX-lastClick.x)<=6 && Math.abs(e.clientY-lastClick.y)<=6;
  lastClick=twice?null:{path,time:now,x:e.clientX,y:e.clientY};
  select(path);
  $('list').focus();
  if(twice) openSelected();
});
$('list').addEventListener('keydown',e=>{
  if(e.key==='Enter'){e.preventDefault();openSelected();} else if(e.key==='Backspace'){e.preventDefault();up();}
  else if(['ArrowDown','ArrowUp','Home','End'].includes(e.key)&&visible.length){e.preventDefault();let i=visible.findIndex(e=>e.path===selected);i=e.key==='Home'?0:e.key==='End'?visible.length-1:Math.max(0,Math.min(visible.length-1,i+(e.key==='ArrowDown'?1:-1)));select(visible[i].path);$('list').querySelector('.selected')?.scrollIntoView({block:'nearest'});}
});
$('list').addEventListener('contextmenu',e=>{
  e.preventDefault();const row=e.target.closest('[data-path]'); if(row)select(row.dataset.path); if(menu)menu.close();
  const root=document.createElement('popup'); root.innerHTML='<button data-command="open">Open / Preview</button><button data-command="refresh">Refresh folder</button><button data-command="up">Go to parent folder</button>';
  root.addEventListener('click',e=>{const b=e.target.closest('[data-command]');if(!b)return;menu.close();if(b.dataset.command==='open')openSelected();else if(b.dataset.command==='up')up();else load(current,false);});
  menu=$('list').popup(root,{x:e.clientX,y:e.clientY,popupAt:7}); if(menu)menu.on('close',()=>root.remove()); else root.remove();
});
expanded.add(workspace); load(workspace);
