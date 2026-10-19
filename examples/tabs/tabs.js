// Each tab owns its location, navigation history, search, and selected file.
const folders = {
  Home: [
    ['Documents', 'Folder', '—'], ['Projects', 'Folder', '—'],
    ['Pictures', 'Folder', '—'], ['Downloads', 'Folder', '—'],
    ['Welcome.txt', 'Text document', '1 KB']
  ],
  Documents: [['Project brief.txt', 'Text document', '4 KB'], ['Meeting notes.txt', 'Text document', '2 KB'], ['Budget.csv', 'Spreadsheet', '8 KB']],
  Projects: [['Website plan.txt', 'Text document', '3 KB'], ['Release checklist.txt', 'Text document', '2 KB'], ['Design tokens.json', 'JSON document', '6 KB']],
  Pictures: [['Mountains.jpg', 'JPEG image', '2.4 MB'], ['Workspace.png', 'PNG image', '840 KB'], ['Summer.jpg', 'JPEG image', '1.8 MB']],
  Downloads: [['Getting started.pdf', 'PDF document', '420 KB'], ['Sample data.csv', 'Spreadsheet', '12 KB']]
};
let tabs = [];
let activeId = 0;
let nextId = 1;
const get = id => document.getElementById(id);
const current = () => tabs.find(tab => tab.id === activeId);
function button(text, action) {
  const element = document.createElement('button');
  element.textContent = text;
  element.addEventListener('click', action);
  return element;
}
function openTab(folder) {
  const tab = { id: nextId++, folder, history: [folder], index: 0, search: '', selected: '' };
  tabs.push(tab);
  activeId = tab.id;
  render();
}
function closeTab(id) {
  const index = tabs.findIndex(tab => tab.id === id);
  if (index < 0) return;
  tabs.splice(index, 1);
  if (!tabs.length) { openTab('Home'); return; }
  if (activeId === id) activeId = tabs[Math.min(index, tabs.length - 1)].id;
  render();
}
function navigate(folder) {
  const tab = current();
  if (tab.folder === folder) return;
  tab.history = tab.history.slice(0, tab.index + 1);
  tab.history.push(folder);
  tab.index++;
  tab.folder = folder;
  tab.search = '';
  tab.selected = '';
  render();
}
function moveHistory(delta) {
  const tab = current();
  const index = tab.index + delta;
  if (index < 0 || index >= tab.history.length) return;
  tab.index = index;
  tab.folder = tab.history[index];
  tab.search = '';
  tab.selected = '';
  render();
}
function renderFiles() {
  const tab = current();
  const files = folders[tab.folder].filter(file => file[0].toLowerCase().includes(tab.search.toLowerCase()));
  get('files').innerHTML = '';
  files.forEach(file => {
    const row = document.createElement('div');
    const open = button('', () => {
      if (file[1] === 'Folder') navigate(file[0]);
      else { tab.selected = file[0]; renderPreview(); }
    });
    row.className = 'file';
    open.textContent = (file[1] === 'Folder' ? '▣   ' : '▤   ') + file[0];
    open.className = 'file-open';
    row.appendChild(open);
    ['file-type', 'file-size'].forEach((className, i) => {
      const cell = document.createElement('span');
      cell.className = className;
      cell.textContent = file[i + 1];
      row.appendChild(cell);
    });
    get('files').appendChild(row);
  });
  get('status').textContent = files.length + ' items · ' + tabs.length + ' open tabs';
  renderPreview();
}
function renderPreview() {
  const tab = current();
  get('preview').textContent = tab.selected
    ? tab.selected + ' — Sample file selected. This example uses in-memory folders; it does not read or open files on your computer.'
    : 'Click a folder to explore, or select a file. Each tab remembers its folder, search, and selection.';
}
function render() {
  const tab = current();
  get('tabs').innerHTML = '';
  tabs.forEach(item => {
    const wrapper = document.createElement('div');
    wrapper.className = 'tab' + (item.id === activeId ? ' active' : '');
    const select = button('', () => { activeId = item.id; render(); });
    select.className = 'tab-select';
    select.id = 'tab-' + item.id;
    select.setAttribute('role', 'tab');
    select.setAttribute('aria-selected', item.id === activeId ? 'true' : 'false');
    select.setAttribute('aria-controls', 'panel');
    select.setAttribute('tabindex', item.id === activeId ? '0' : '-1');
    select.textContent = '▣   ' + item.folder;
    wrapper.appendChild(select);
    const close = button('×', () => closeTab(item.id));
    close.className = 'tab-close';
    close.setAttribute('aria-label', 'Close ' + item.folder);
    wrapper.appendChild(close);
    get('tabs').appendChild(wrapper);
  });
  get('panel').setAttribute('aria-labelledby', 'tab-' + tab.id);
  get('folders').innerHTML = '';
  Object.keys(folders).forEach(folder => {
    const entry = button('▣   ' + folder, () => navigate(folder));
    entry.className = folder === tab.folder ? 'selected' : '';
    get('folders').appendChild(entry);
  });
  get('address').textContent = tab.folder === 'Home' ? 'Home' : 'Home  /  ' + tab.folder;
  get('folder-name').textContent = tab.folder;
  get('search').value = tab.search;
  get('back').disabled = tab.index === 0;
  get('forward').disabled = tab.index === tab.history.length - 1;
  document.title = tab.folder + ' — Files';
  renderFiles();
}
get('new-tab').addEventListener('click', () => openTab('Home'));
get('open-tab').addEventListener('click', () => openTab(current().folder));
get('back').addEventListener('click', () => moveHistory(-1));
get('forward').addEventListener('click', () => moveHistory(1));
get('search').addEventListener('input', () => { current().search = get('search').value; renderFiles(); });
document.addEventListener('keydown', event => {
  const key = event.key.toLowerCase();
  const command = event.ctrlKey || event.metaKey;
  if (command && key === 't') { event.preventDefault(); openTab('Home'); }
  else if (command && key === 'w') { event.preventDefault(); closeTab(activeId); }
  else if (event.ctrlKey && key === 'tab') {
    event.preventDefault();
    const index = tabs.findIndex(tab => tab.id === activeId);
    activeId = tabs[(index + (event.shiftKey ? tabs.length - 1 : 1)) % tabs.length].id;
    render();
    get('tab-' + activeId).focus();
  }
  else if (event.target.getAttribute('role') === 'tab' && ['arrowleft', 'arrowright', 'home', 'end'].includes(key)) {
    event.preventDefault();
    const index = tabs.findIndex(tab => tab.id === activeId);
    const next = key === 'home' ? 0 : key === 'end' ? tabs.length - 1 : (index + (key === 'arrowleft' ? tabs.length - 1 : 1)) % tabs.length;
    activeId = tabs[next].id;
    render();
    get('tab-' + activeId).focus();
  }
});
openTab('Home');
openTab('Documents');
openTab('Projects');
activeId = tabs[1].id;
render();
