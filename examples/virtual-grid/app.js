// The data model is independent of the rendered DOM. Replace valueAt with your data source.
const ROW_HEIGHT=36, COLUMN_WIDTH=140, COLUMN_COUNT=100, ROW_BUFFER=3, COLUMN_BUFFER=2;
const $=id=>document.getElementById(id);
const viewport=$('viewport'), cells=$('cells'), spacer=$('spacer'), editor=$('editor');
let rowCount=100000, active={row:0,col:0}, windowKey='', pending=false, editing=null;
const edits=new Map();
const key=(r,c)=>r+':'+c;
const escapeText=s=>String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;');
function valueAt(row,col) {
  if(edits.has(key(row,col)))return edits.get(key(row,col));
  if(col===0)return String(row+1).padStart(7,'0');
  if(col===1)return ['North','South','East','West'][row%4];
  if(col===2)return ['Active','Pending','Review'][row%3];
  return ((row*17+col*137)%100000/100).toFixed(2);
}
function bounds() {
  return {r0:Math.max(0,Math.floor(viewport.scrollTop/ROW_HEIGHT)-ROW_BUFFER),r1:Math.min(rowCount,Math.ceil((viewport.scrollTop+viewport.clientHeight)/ROW_HEIGHT)+ROW_BUFFER),c0:Math.max(0,Math.floor(viewport.scrollLeft/COLUMN_WIDTH)-COLUMN_BUFFER),c1:Math.min(COLUMN_COUNT,Math.ceil((viewport.scrollLeft+viewport.clientWidth)/COLUMN_WIDTH)+COLUMN_BUFFER)};
}
function selection() {
  cells.querySelectorAll('[data-row]').forEach(cell=>{const on=Number(cell.dataset.row)===active.row&&Number(cell.dataset.col)===active.col;cell.classList.toggle('selected',on);cell.setAttribute('aria-selected',String(on));});
  $('selection').textContent='Row '+(active.row+1)+' · Column '+(active.col+1);
}
function render(force=false) {
  const {r0,r1,c0,c1}=bounds(), next=[r0,r1,c0,c1].join(':');
  // The header follows horizontal scrolling even while the mounted window stays unchanged.
  $('headers').style.left=(c0*COLUMN_WIDTH-viewport.scrollLeft)+'px';
  if(!force&&next===windowKey)return;
  windowKey=next;
  cells.style.top=r0*ROW_HEIGHT+'px';cells.style.left=c0*COLUMN_WIDTH+'px';
  cells.style.gridTemplateColumns='repeat('+(c1-c0)+', '+COLUMN_WIDTH+'px)';
  let html='';
  for(let r=r0;r<r1;r++)for(let c=c0;c<c1;c++)html+='<div class="cell'+(r%2?' alternate':'')+(c===0?' id':'')+'" role="gridcell" aria-rowindex="'+(r+1)+'" aria-colindex="'+(c+1)+'" data-row="'+r+'" data-col="'+c+'">'+escapeText(valueAt(r,c))+'</div>';
  cells.innerHTML=html;
  $('headers').style.gridTemplateColumns='repeat('+(c1-c0)+', '+COLUMN_WIDTH+'px)';
  $('headers').innerHTML=Array.from({length:c1-c0},(_,i)=>'<div class="column">'+(c0+i===0?'Record ID':c0+i===1?'Region':c0+i===2?'Status':'Field '+(c0+i+1))+'</div>').join('');
  $('mounted').textContent=(r1-r0)*(c1-c0);
  $('range').textContent='Rows '+(r0+1)+'–'+r1+'\nColumns '+(c0+1)+'–'+c1;
  selection();
}
function schedule() {if(pending)return;pending=true;requestAnimationFrame(()=>{pending=false;render();});}
function resizeData() {
  rowCount=Number($('size').value);spacer.style.height=rowCount*ROW_HEIGHT+'px';spacer.style.width=COLUMN_COUNT*COLUMN_WIDTH+'px';viewport.setAttribute('aria-rowcount',String(rowCount));$('logical').textContent=rowCount===100000?'10 million':'100 million';active.row=Math.min(active.row,rowCount-1);render(true);
}
function reveal() {
  const top=active.row*ROW_HEIGHT,left=active.col*COLUMN_WIDTH;
  if(top<viewport.scrollTop)viewport.scrollTop=top;else if(top+ROW_HEIGHT>viewport.scrollTop+viewport.clientHeight)viewport.scrollTop=top+ROW_HEIGHT-viewport.clientHeight;
  if(left<viewport.scrollLeft)viewport.scrollLeft=left;else if(left+COLUMN_WIDTH>viewport.scrollLeft+viewport.clientWidth)viewport.scrollLeft=left+COLUMN_WIDTH-viewport.clientWidth;
  render();selection();
}
function finish(save) {
  if(!editing)return;
  if(save){edits.set(key(editing.row,editing.col),editor.value);$('status').textContent='Saved cell '+(editing.row+1)+', '+(editing.col+1)+' · '+edits.size+' edited values';}
  editing=null;editor.style.display='none';render(true);viewport.focus();
}
function begin() {
  reveal();editing={...active};editor.style.top=active.row*ROW_HEIGHT+'px';editor.style.left=active.col*COLUMN_WIDTH+'px';editor.style.width=COLUMN_WIDTH+'px';editor.value=valueAt(active.row,active.col);editor.style.display='block';editor.focus();editor.select();
}
function jump(row) {finish(true);active.row=Math.max(0,Math.min(rowCount-1,row));reveal();viewport.focus();}
viewport.addEventListener('scroll',schedule);
new ResizeObserver(schedule).observe(viewport);
cells.addEventListener('click',e=>{const cell=e.target.closest('[data-row]');if(!cell)return;finish(true);active={row:Number(cell.dataset.row),col:Number(cell.dataset.col)};selection();viewport.focus();});
viewport.addEventListener('keydown',e=>{
  if(e.target===editor)return;
  if(e.key==='Enter'||e.key==='F2'){e.preventDefault();begin();return;}
  let {row,col}=active;
  if(e.key==='ArrowDown')row++;else if(e.key==='ArrowUp')row--;else if(e.key==='ArrowRight')col++;else if(e.key==='ArrowLeft')col--;else if(e.key==='PageDown')row+=Math.max(1,Math.floor(viewport.clientHeight/ROW_HEIGHT));else if(e.key==='PageUp')row-=Math.max(1,Math.floor(viewport.clientHeight/ROW_HEIGHT));else if(e.key==='Home'){col=0;if(e.ctrlKey||e.metaKey)row=0;}else if(e.key==='End'){col=COLUMN_COUNT-1;if(e.ctrlKey||e.metaKey)row=rowCount-1;}else return;
  e.preventDefault();active={row:Math.max(0,Math.min(rowCount-1,row)),col:Math.max(0,Math.min(COLUMN_COUNT-1,col))};reveal();
});
editor.addEventListener('keydown',e=>{if(e.key==='Enter'||e.key==='Escape'){e.preventDefault();e.stopPropagation();finish(e.key==='Enter');}});
editor.addEventListener('blur',()=>finish(true));
$('go').addEventListener('click',()=>{const n=Number($('jump').value);if(Number.isInteger(n)&&n>=1&&n<=rowCount)jump(n-1);else $('status').textContent='Enter a row from 1 to '+rowCount;});
$('jump').addEventListener('keydown',e=>{if(e.key==='Enter')$('go').click();});
$('first').addEventListener('click',()=>jump(0));$('last').addEventListener('click',()=>jump(rowCount-1));$('size').addEventListener('change',()=>{finish(true);resizeData();});
resizeData();
