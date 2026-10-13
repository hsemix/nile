// A working word processor around the ribbon: the page is a real contenteditable document and
// every command is document.execCommand on the live selection.
const paper = document.getElementById('paper');
const status = document.getElementById('status');
const report = (text) => { status.textContent = text; console.log('ribbon:', text); };
let zoom = 100, marks = false;
let savedRange = null;
function rememberSelection() {
  const sel = document.getSelection();
  if (sel.rangeCount && paper.contains(sel.anchorNode) && paper.contains(sel.focusNode)) savedRange = sel.getRangeAt(0).cloneRange();
}
function restoreSelection() {
  rememberSelection();
  const range = savedRange && paper.contains(savedRange.startContainer) && paper.contains(savedRange.endContainer)
    ? savedRange.cloneRange() : document.createRange();
  if (!savedRange || !paper.contains(savedRange.startContainer) || !paper.contains(savedRange.endContainer)) {
    // A command before the first editor click inserts at the end of the document.
    range.selectNodeContents(paper);
    range.collapse(false);
  }
  paper.focus();
  const sel = document.getSelection();
  sel.removeAllRanges();
  sel.addRange(range);
}
const cmd = (name, value) => {
  restoreSelection();
  const ok = document.execCommand(name, false, value);
  rememberSelection();
  refresh();
  return ok;
};

// Which ribbon buttons look pressed follows the caret.
const FORMAT_STATE = { bold: 'bold', italic: 'italic', underline: 'underline', strike: 'strikeThrough' };
function blockOf(node) {
  for (let n = node && (node.nodeType === 3 ? node.parentNode : node); n && n !== paper; n = n.parentNode) {
    if (/^(P|H[1-6]|LI|DIV|BLOCKQUOTE)$/.test(n.tagName) && n.parentNode) return n;
  }
  return null;
}
function refresh() {
  if (!document.getSelection().rangeCount) return;
  document.querySelectorAll('[data-format]').forEach((b) => {
    const f = b.dataset.format;
    let on = false;
    if (FORMAT_STATE[f]) on = document.queryCommandState(FORMAT_STATE[f]);
    else if (f === 'highlight') on = !!document.queryCommandValue('hiliteColor');
    else if (f === 'color') on = !!document.queryCommandValue('foreColor');
    b.classList.toggle('selected', on);
  });
  const align = ['Left', 'Center', 'Right'].find((a) => document.queryCommandState('justify' + a)) || 'Left';
  document.querySelectorAll('[data-align]').forEach((b) => b.classList.toggle('selected', b.dataset.align === align.toLowerCase()));
  const block = document.queryCommandValue('formatBlock');
  const style = block === 'h1' ? 'title' : block === 'h2' ? 'heading' : 'normal';
  document.querySelectorAll('[data-style]').forEach((b) => b.classList.toggle('selected', b.dataset.style === style));
}
const words = () => { const t = paper.textContent.trim(); return t ? t.split(/\s+/).length : 0; };
const countWords = () => { document.getElementById('word-count').textContent = words() + ' words'; };
countWords();
paper.addEventListener('input', countWords);
document.addEventListener('selectionchange', () => { rememberSelection(); refresh(); });
// Ribbon buttons act on the document without taking the caret from it.
document.addEventListener('mousedown', (e) => {
  if (!e.target.closest('.ribbon-wrap button, .quick button, .statusbar button, [data-tab]')) return;
  rememberSelection();
  e.preventDefault();
}, true);


document.querySelectorAll('[data-format]').forEach((button) => button.addEventListener('click', () => {
  const key = button.dataset.format;
  if (key === 'highlight') cmd('hiliteColor', document.queryCommandValue('hiliteColor') ? '' : '#fff0aa');
  else if (key === 'color') cmd('foreColor', document.queryCommandValue('foreColor') ? '' : '#b64449');
  else cmd(FORMAT_STATE[key]);
  report(key + ' · ' + (button.classList.contains('selected') ? 'on' : 'off'));
}));
document.querySelectorAll('[data-align]').forEach((button) => button.addEventListener('click', () => {
  cmd('justify' + button.dataset.align[0].toUpperCase() + button.dataset.align.slice(1)); report('Align ' + button.dataset.align);
}));
document.querySelectorAll('[data-style]').forEach((button) => button.addEventListener('click', () => {
  cmd('formatBlock', { normal: 'p', heading: 'h2', title: 'h1' }[button.dataset.style]); report(button.dataset.style + ' style');
}));
document.getElementById('font-family').addEventListener('change', (e) => { cmd('fontName', e.target.value); report('Font ' + e.target.value); });
document.getElementById('font-size').addEventListener('change', (e) => { cmd('fontSizePx', e.target.value + 'px'); report('Font size ' + e.target.value); });
document.getElementById('autosave').addEventListener('click', (e) => { const off = e.currentTarget.classList.toggle('off'); report('AutoSave ' + (off ? 'off' : 'on')); });
document.querySelectorAll('[data-section]').forEach((b) => b.addEventListener('click', () => {
  document.getElementById(b.dataset.section).scrollIntoView();
  document.querySelectorAll('[data-section]').forEach((n) => n.classList.toggle('current', n === b));
  report(b.textContent);
}));

// Find: select the next match in the document.
let findFrom = 0;
function findNext(query, fromStart) {
  if (!query) return report('Ready');
  const q = query.toLowerCase(), texts = [];
  const walk = (n) => { for (const c of n.childNodes) c.nodeType === 3 ? texts.push(c) : walk(c); };
  walk(paper);
  const total = texts.reduce((n, t) => n + t.data.toLowerCase().split(q).length - 1, 0);
  let skip = fromStart ? 0 : findFrom;
  for (let pass = 0; pass < 2; pass++) {
    let pos = 0;
    for (const t of texts) {
      const lower = t.data.toLowerCase();
      for (let i = lower.indexOf(q); i >= 0; i = lower.indexOf(q, i + 1)) {
        if (pos++ < skip) continue;
        document.getSelection().setBaseAndExtent(t, i, t, i + q.length);
        findFrom = pos;
        return report(total + ' matches for “' + query + '”');
      }
    }
    skip = 0; findFrom = 0;
  }
  report('No matches for “' + query + '”');
}
document.getElementById('find-input').addEventListener('input', (e) => { findFrom = 0; findNext(e.target.value, true); });
document.getElementById('find-input').addEventListener('keydown', (e) => { if (e.key === 'Enter') findNext(e.target.value, false); });

function currentSize() {
  const sel = document.getSelection();
  const node = sel.focusNode && (sel.focusNode.nodeType === 3 ? sel.focusNode.parentNode : sel.focusNode);
  return node ? parseFloat(getComputedStyle(node).fontSize) || 15 : 15;
}
// The ribbon tabs: Home is built into the page, the others are filled in when chosen.
const menus = {
  File: [['Document', [['▤', 'New copy', 'new'], ['▣', 'Save', 'save'], ['▧', 'Print preview', 'preview']]], ['About', [['W', 'Ribbon demo', 'about']]]],
  Insert: [['Pages', [['▤', 'Cover page', 'cover'], ['+', 'Page break', 'page-break']]], ['Illustrations', [['▦', 'Table', 'table'], ['▧', 'Picture', 'picture'], ['○', 'Shapes', 'shapes']]], ['Table tools', [['↑+', 'Row above', 'tableInsertRowAbove'], ['↓+', 'Row below', 'tableInsertRowBelow'], ['←+', 'Column left', 'tableInsertColumnLeft'], ['→+', 'Column right', 'tableInsertColumnRight'], ['↕×', 'Delete row', 'tableDeleteRow'], ['↔×', 'Delete column', 'tableDeleteColumn'], ['▦', 'Merge cells', 'tableMergeCells'], ['▥', 'Split cell', 'tableSplitCell'], ['×', 'Delete table', 'tableDeleteTable']]], ['Text', [['Aa', 'Text box', 'text-box'], ['Ω', 'Symbol', 'symbol']]]],
  Layout: [['Page setup', [['▤', 'Margins', 'margins'], ['▯', 'Orientation', 'orientation'], ['↔', 'Page width', 'page-width']]], ['Paragraph', [['≡›', 'Indent', 'indent'], ['↕', 'Spacing', 'spacing']]]],
  References: [['Table of contents', [['▤', 'Contents', 'contents']]], ['Footnotes', [['¹', 'Footnote', 'footnote']]], ['Citations', [['“ ”', 'Citation', 'citation']]]],
  Review: [['Proofing', [['Aa', 'Word count', 'word-count']]], ['Comments', [['▤', 'Comment', 'comments']]], ['Tracking', [['▧', 'Track changes', 'track']]]],
  View: [['Show', [['▤', 'Navigation', 'navigation'], ['↔', 'Ruler', 'ruler']]], ['Zoom', [['−', 'Zoom out', 'zoom-out'], ['+', 'Zoom in', 'zoom-in'], ['▯', '100%', 'zoom-reset']]]]
};
document.querySelectorAll('[data-tab]').forEach((button) => button.addEventListener('click', () => {
  const tab = button.dataset.tab;
  document.querySelectorAll('[data-tab]').forEach((b) => b.classList.toggle('active', b === button));
  document.getElementById('home-ribbon').classList.toggle('hidden', tab !== 'Home');
  const other = document.getElementById('other-ribbon');
  other.classList.toggle('hidden', tab === 'Home');
  if (tab !== 'Home') other.innerHTML = menus[tab].map(([name, commands]) => '<div class="group"><div class="group-body">' + commands.map(([icon, label, action]) => '<button class="large" data-action="' + action + '"><span class="large-icon">' + icon + '</span><span>' + label + '</span></button>').join('') + '</div><div class="group-label">' + name + '</div></div>').join('');
  report(tab + ' tab');
}));
document.addEventListener('click', (e) => {
  const button = e.target.closest('[data-action]'); if (!button) return;
  const action = button.dataset.action;
  const block = blockOf(document.getSelection().focusNode);
  if (action === 'undo' || action === 'redo') report(cmd(action) ? action : 'Nothing to ' + action);
  else if (action === 'grow' || action === 'shrink') { const size = Math.max(8, Math.min(72, Math.round(currentSize()) + (action === 'grow' ? 1 : -1))); cmd('fontSizePx', size + 'px'); report('Font size ' + size); }
  else if (action === 'clear') { cmd('removeFormat'); report('Formatting cleared'); }
  else if (action === 'bullets') { cmd('insertUnorderedList'); report('Bulleted list'); }
  else if (action === 'numbering') { cmd('insertOrderedList'); report('Numbered list'); }
  else if (action === 'indent' || action === 'outdent') { cmd(action); report(action); }
  else if (action === 'spacing') { const wide = block && block.style.lineHeight === '2.2'; cmd('blockStyle', 'line-height: ' + (wide ? '1.7' : '2.2')); report('Line spacing ' + (wide ? '1.7' : '2.2')); }
  else if (action === 'borders') { const on = block && block.style.borderLeftWidth !== '' || block && /solid/.test(block.style.border || ''); cmd('blockStyle', on ? 'border: 0; padding: 0' : 'border: 1px solid #91a2c6; padding: 6px 10px'); report('Paragraph border ' + (on ? 'off' : 'on')); }
  else if (action === 'cut' || action === 'copy' || action === 'paste') { cmd(action); report(action[0].toUpperCase() + action.slice(1)); }
  else if (action === 'select') { cmd('selectAll'); report('Selected all'); }
  else if (action === 'marks') { marks = !marks; paper.style.outline = marks ? '1px dashed #9aa7c7' : 'none'; report('Paragraph marks ' + (marks ? 'on' : 'off')); }
  else if (action.indexOf('zoom-') === 0) {
    zoom = action === 'zoom-reset' ? 100 : Math.max(70, Math.min(130, zoom + (action === 'zoom-in' ? 10 : -10)));
    paper.style.transform = 'scale(' + zoom / 100 + ')';
    paper.style.transformOrigin = 'top center';
    document.getElementById('zoom-label').textContent = zoom + '%'; report('Zoom ' + zoom + '%');
  } else if (action === 'navigation' || action === 'find' || action === 'contents') {
    const nav = document.getElementById('navigation'); nav.classList.toggle('hidden', action === 'navigation' ? !nav.classList.contains('hidden') : false);
    if (action === 'find') document.getElementById('find-input').focus(); report('Navigation');
  } else if (action === 'ruler') {
    document.querySelector('.ruler').classList.toggle('hidden'); report('Ruler toggled');
  } else if (action === 'table') {
    report(cmd('insertTable', '3x3') ? 'Inserted a 3 x 3 table (Tab moves between cells)' : 'Could not insert a table');
  } else if (/^table[A-Z]/.test(action)) {
    report(cmd(action) ? button.textContent.trim() : 'Put the cursor in a table first');
  } else if (action === 'picture') {
    cmd('insertImage', 'images/photo.png'); report('Picture inserted: click it to resize (drag a handle), Delete removes it');
  } else if (action === 'page-break') {
    cmd('insertHorizontalRule'); report('Page break');
  } else if (action === 'word-count') report(words() + ' words in document');
  else report(button.textContent.trim() + ' · not in this demo');
});
console.log('word processor ready: the page is editable');
