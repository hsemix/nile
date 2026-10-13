// Menus use Nile's popup API: positioning, lifting, animation and outside-click dismissal.
const editor = document.getElementById('editor');
const triggers = document.querySelectorAll('[data-menu]');
const initialHtml = editor.innerHTML;
const briefHtml = '<p class="eyebrow">PROJECT / NEW DIRECTION</p><h1>Make something thoughtful.</h1><p>Define the opportunity, listen to the people involved, and explore a direction together.</p><h2>Next steps</h2><p>Start with research. Build a small prototype. Refine the important details.</p>';
let rootMenu = null, subMenu = null, savedRange = null;
let draftName = 'Studio notes', showSidebar = true, showInspector = true, compact = false;
const keyName = '@menubar-draft-v1';
const shortcut = (key) => (/mac|osx|darwin/i.test(navigator.platform) ? '⌘' : navigator.platform ? 'Ctrl+' : 'Ctrl/Cmd+') + key;
const escapeText = (text) => String(text).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
const report = (text) => { document.getElementById('status').textContent = text; console.log('menu:', text); };
function refresh() {
  const text = editor.textContent.trim();
  document.getElementById('word-count').textContent = (text ? text.split(/\s+/).length : 0) + ' words';
  document.getElementById('draft-name').textContent = draftName;
  document.title = draftName + ' — Nile menus';
}
function rememberSelection() {
  const sel = window.getSelection();
  if (sel.rangeCount && editor.contains(sel.anchorNode) && editor.contains(sel.focusNode)) savedRange = sel.getRangeAt(0).cloneRange();
}
function restoreSelection() {
  editor.focus();
  const sel = window.getSelection();
  if (savedRange && editor.contains(savedRange.startContainer) && editor.contains(savedRange.endContainer)) { sel.removeAllRanges(); sel.addRange(savedRange); }
  else { const range = document.createRange(); range.selectNodeContents(editor); range.collapse(false); sel.removeAllRanges(); sel.addRange(range); }
}
function menuRows(kind) {
  if (kind === 'file') return [
    { label: 'DOCUMENT' }, { command: 'new', text: 'New draft', icon: '+', key: shortcut('N') },
    { command: 'save', text: 'Save draft', icon: '▣', key: shortcut('S') },
    { submenu: true, text: 'From a template', icon: '▤', key: '›' },
    { separator: true }, { label: 'EXPORT' }, { text: 'Export as PDF', icon: '▧', key: 'Soon', disabled: true },
    { separator: true }, { command: 'close', text: 'Close window', icon: '×' }
  ];
  if (kind === 'templates') return [{ label: 'START WITH AN IDEA' }, { command: 'studio', text: 'Studio notes', icon: '▤' }, { command: 'brief', text: 'Project brief', icon: '▧' }, { command: 'new', text: 'Blank draft', icon: '+' }];
  if (kind === 'edit') return [{ command: 'undo', text: 'Undo', icon: '↶', key: shortcut('Z') }, { command: 'redo', text: 'Redo', icon: '↷', key: shortcut('⇧Z') }, { separator: true }, { command: 'cut', text: 'Cut', icon: '✂', key: shortcut('X') }, { command: 'copy', text: 'Copy', icon: '▤', key: shortcut('C') }, { command: 'paste', text: 'Paste', icon: '▧', key: shortcut('V') }, { separator: true }, { command: 'selectAll', text: 'Select all', icon: '↖', key: shortcut('A') }];
  if (kind === 'view') return [{ label: 'WORKSPACE' }, { command: 'sidebar', text: 'Document sidebar', icon: showSidebar ? '✓' : '', checked: showSidebar }, { command: 'inspector', text: 'Details panel', icon: showInspector ? '✓' : '', checked: showInspector }, { separator: true }, { command: 'compact', text: 'Compact reading', icon: compact ? '✓' : '', checked: compact }];
  return [{ label: 'A LITTLE GUIDANCE' }, { command: 'shortcuts', text: 'Keyboard shortcuts', icon: '⌨' }, { command: 'about', text: 'About Nile menus', icon: 'i' }];
}
function makeMenu(kind) {
  const root = document.createElement('popup'); root.className = 'command-menu' + (kind === 'templates' ? ' submenu' : ''); root.setAttribute('role', 'menu');
  root.innerHTML = menuRows(kind).map((row) => {
    if (row.label) return '<div class="menu-label">' + row.label + '</div>';
    if (row.separator) return '<div class="menu-separator"></div>';
    return '<div class="menu-item" role="' + (row.checked !== undefined ? 'menuitemcheckbox' : 'menuitem') + '" tabindex="' + (row.disabled ? '-1' : '0') + '"' + (row.disabled ? ' disabled aria-disabled="true"' : '') + (row.command ? ' data-command="' + row.command + '"' : '') + (row.submenu ? ' data-submenu="templates" aria-haspopup="menu" aria-expanded="false"' : '') + (row.checked !== undefined ? ' aria-checked="' + row.checked + '"' : '') + '><span class="menu-icon symbol">' + escapeText(row.icon || '') + '</span><span>' + escapeText(row.text) + '</span><span class="key-label symbol">' + escapeText(row.key || '') + '</span></div>';
  }).join('') + (kind === 'file' ? '<div class="menu-footnote">Saved locally on this device</div>' : '');
  return root;
}
function items(menu) { return menu ? menu.root.querySelectorAll('.menu-item').filter((item) => !item.hasAttribute('disabled')) : []; }
function closeSub() { if (subMenu) subMenu.handle.close(); }
function closeMenus(restoreFocus = false) {
  const trigger = rootMenu?.trigger; if (rootMenu) rootMenu.handle.close();
  if (restoreFocus && trigger) trigger.focus();
}
function openSub(anchor, focus = false) {
  if (subMenu?.anchor === anchor && subMenu.handle.isValid) { if (focus) items(subMenu)[0]?.focus(); return; }
  closeSub(); const root = makeMenu('templates');
  const handle = anchor.popup(root, { anchorAt: 9, popupAt: 7, animationType: 'blend', animationDuration: 100 });
  if (!handle) { root.remove(); return; }
  const record = { root, handle, anchor }; subMenu = record; anchor.setAttribute('aria-expanded', 'true');
  handle.on('close', () => { anchor.setAttribute('aria-expanded', 'false'); if (subMenu === record) subMenu = null; root.remove(); });
  if (focus) items(record)[0]?.focus();
}
function openMenu(trigger, focus = false, point = null) {
  if (rootMenu?.trigger === trigger && !point) { if (focus) items(rootMenu)[0]?.focus(); else closeMenus(); return; }
  rememberSelection(); closeMenus();
  const kind = point ? 'edit' : trigger.dataset.menu, root = makeMenu(kind);
  const params = { animationType: 'blend', animationDuration: 100 };
  if (point) { params.x = point.x; params.y = point.y; params.popupAt = 7; }
  const handle = trigger.popup(root, params);
  if (!handle) { root.remove(); return; }
  const record = { trigger, root, handle, kind }; rootMenu = record;
  if (trigger.hasAttribute('data-menu')) { trigger.classList.add('open'); trigger.setAttribute('aria-expanded', 'true'); }
  handle.on('close', () => {
    trigger.classList.remove('open'); if (trigger.hasAttribute('data-menu')) trigger.setAttribute('aria-expanded', 'false');
    if (rootMenu === record) rootMenu = null; root.remove();
  });
  root.addEventListener('mouseover', (event) => { const item = event.target.closest('.menu-item'); if (item?.hasAttribute('data-submenu')) openSub(item); else if (item) closeSub(); });
  if (focus) items(record)[0]?.focus();
}
function loadTemplate(name) {
  savedRange = null; draftName = name === 'studio' ? 'Studio notes' : name === 'brief' ? 'Project brief' : 'Untitled draft';
  editor.innerHTML = name === 'studio' ? initialHtml : name === 'brief' ? briefHtml : '<h1>Untitled draft</h1><p>Start writing here.</p>';
  document.getElementById('save-state').textContent = 'Unsaved changes';
  document.querySelectorAll('.side-item').forEach((b) => b.classList.toggle('selected', b.dataset.command === name));
  restoreSelection(); refresh(); report(draftName + ' opened');
}
function command(name) {
  closeMenus();
  if (['undo', 'redo', 'cut', 'copy', 'paste', 'selectAll', 'bold', 'italic'].includes(name)) {
    restoreSelection(); const ok = document.execCommand(name); rememberSelection(); refresh(); report(ok ? name : 'Nothing to ' + name); return;
  }
  if (['new', 'studio', 'brief'].includes(name)) loadTemplate(name);
  if (name === 'save') { try { localStorage.setItem(keyName, JSON.stringify({ name: draftName, html: editor.innerHTML })); document.getElementById('save-state').textContent = 'Saved on this device'; report('Draft saved locally'); } catch (error) { report('Save failed: ' + error.message); } }
  if (name === 'sidebar') { showSidebar = !showSidebar; document.getElementById('sidebar').classList.toggle('hidden', !showSidebar); report('Sidebar ' + (showSidebar ? 'shown' : 'hidden')); }
  if (name === 'inspector') { showInspector = !showInspector; document.getElementById('inspector').classList.toggle('hidden', !showInspector); report('Details ' + (showInspector ? 'shown' : 'hidden')); }
  if (name === 'compact') { compact = !compact; document.body.classList.toggle('compact', compact); report('Compact reading ' + (compact ? 'on' : 'off')); }
  if (name === 'shortcuts') { showInspector = true; document.getElementById('inspector').classList.remove('hidden'); report('Arrow keys navigate · Enter activates · Escape closes a menu'); }
  if (name === 'about') report('Nile menus · anchored popups, nested menus, editable notes and local drafts');
  if (name === 'close') window.close();
}
document.addEventListener('selectionchange', rememberSelection);
editor.addEventListener('input', () => { document.getElementById('save-state').textContent = 'Unsaved changes'; refresh(); });
editor.addEventListener('contextmenu', (event) => { event.preventDefault(); openMenu(editor, false, { x: event.clientX, y: event.clientY }); });
document.addEventListener('mousedown', (event) => { if (event.target.closest('[data-menu], .toolbar button')) event.preventDefault(); });
document.addEventListener('click', (event) => {
  const trigger = event.target.closest('[data-menu]'); if (trigger) { openMenu(trigger); return; }
  const sub = event.target.closest('[data-submenu]'); if (sub) { openSub(sub, true); return; }
  const item = event.target.closest('[data-command]'); if (item && !item.hasAttribute('disabled')) command(item.dataset.command);
});
for (const trigger of triggers) trigger.addEventListener('mouseover', () => { if (rootMenu && rootMenu.trigger !== trigger) openMenu(trigger); });
document.addEventListener('keydown', (event) => {
  if ((event.ctrlKey || event.metaKey) && ['n', 's'].includes(event.key.toLowerCase())) { command(event.key.toLowerCase() === 'n' ? 'new' : 'save'); event.preventDefault(); return; }
  const trigger = event.target.closest('[data-menu]');
  if (trigger && ['ArrowDown', 'Enter', ' '].includes(event.key)) { openMenu(trigger, true); event.preventDefault(); return; }
  if (!rootMenu) return;
  const active = subMenu || rootMenu, choices = items(active), index = choices.indexOf(document.activeElement);
  if (event.key === 'Escape') { if (subMenu) { const anchor = subMenu.anchor; closeSub(); anchor.focus(); } else closeMenus(true); }
  else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') choices[(index < 0 ? (event.key === 'ArrowDown' ? 0 : choices.length - 1) : (index + (event.key === 'ArrowDown' ? 1 : -1) + choices.length) % choices.length)]?.focus();
  else if (event.key === 'Home' || event.key === 'End') choices[event.key === 'Home' ? 0 : choices.length - 1]?.focus();
  else if (event.key === 'ArrowRight' && document.activeElement.hasAttribute('data-submenu')) openSub(document.activeElement, true);
  else if (event.key === 'ArrowLeft' && subMenu) { const anchor = subMenu.anchor; closeSub(); anchor.focus(); }
  else if (event.key === 'ArrowRight' || event.key === 'ArrowLeft') { const index = triggers.indexOf(rootMenu.trigger); openMenu(triggers[(index + (event.key === 'ArrowRight' ? 1 : -1) + triggers.length) % triggers.length], true); }
  else if (event.key === 'Enter' || event.key === ' ') { const target = document.activeElement; if (target.hasAttribute('data-submenu')) openSub(target, true); else if (target.dataset.command) command(target.dataset.command); }
  else if (event.key === 'Tab') { closeMenus(); return; }
  else return;
  event.preventDefault(); event.stopImmediatePropagation();
}, true);
try { const data = JSON.parse(localStorage.getItem(keyName) || 'null'); if (data && typeof data.html === 'string' && typeof data.name === 'string') { editor.innerHTML = data.html; draftName = data.name; document.getElementById('save-state').textContent = 'Saved on this device'; } } catch (error) { report('Saved draft could not be loaded'); }
refresh(); console.log('menubar ready: anchored popups, templates and keyboard navigation');
