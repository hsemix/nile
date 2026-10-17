import * as store from './store.js';
import { fs } from '@sys';
import { escape as esc, uid, plain, today, statuses, priorities } from './model.js';
import { cleanHtml, reportHtml, exportPdf } from './documents.js';

const $ = id => document.getElementById(id);
const btn = (label, action, value = '', cls = '') => `<button class="${cls}" data-action="${action}" data-value="${esc(value)}">${esc(label)}</button>`;
const empty = text => `<div class="empty">${esc(text)}</div>`;
const field = (label, id, value = '', type = 'text') => `<label class="field"><span>${esc(label)}</span><input id="${id}" type="${type}" value="${esc(value)}"></label>`;
const select = (label, id, values, value) => `<label class="field"><span>${esc(label)}</span><select id="${id}">${values.map(item => `<option${item === value ? ' selected' : ''}>${esc(item)}</option>`).join('')}</select></label>`;
const now = () => new Date().toISOString();
let state = { projects: [], tasks: [], notes: [], attachments: [], preferences: {} };
let tabs = [{ key: 'dashboard', kind: 'dashboard' }];
let active = 'dashboard';
let modal = null;
let dirty = null;
let draftTimer;
let pending = 0;
let busy = false;
let taskFilter = 'All';
let taskQuery = '';
let selectedTasks = new Set();
let searchTimer;
let searchGeneration = 0;
let searchQuery = '';
let searchResults = [];
let preview = '';
let recoveries = [];
let ready = false;
let closing = false;
const current = () => tabs.find(tab => tab.key === active) || tabs[0];
const projectFor = tab => state.projects.find(project => project.id === (tab || current()).projectId);
const noteFor = tab => state.notes.find(note => note.id === (tab || current()).noteId);
function notice(text, error = false) { $('notice').textContent = text; $('notice').className = error ? 'error' : ''; if (error) console.error('northstar:', text); }
function saveStatus() { $('save-status').textContent = dirty ? 'Unsaved changes · recovery draft on this device' : pending ? 'Saving…' : 'All changes saved'; }
async function perform(action) {
  pending++; saveStatus();
  try { return await action(); }
  catch (error) { notice(error.message || String(error), true); if ($('validation')) $('validation').textContent = error.message; throw error; }
  finally { pending--; saveStatus(); }
}
async function reload() { state = await store.load(); }
async function session() {
  await store.preference('session', { tabs, active });
}
function label(tab) {
  if (tab.kind === 'project') return projectFor(tab)?.title || 'Project';
  if (tab.kind === 'note') return noteFor(tab)?.title || 'Note';
  return ({ dashboard: 'Overview', search: 'Search', settings: 'Settings', archives: 'Archived projects' })[tab.kind] || 'Workspace';
}
async function flush() {
  clearTimeout(draftTimer);
  if (!dirty) return;
  const snapshot = { ...dirty };
  const saved = await perform(() => store.saveNote(snapshot));
  const index = state.notes.findIndex(note => note.id === saved.id);
  if (index >= 0) state.notes[index] = saved;
  if (dirty && dirty.html === snapshot.html && dirty.title === snapshot.title) {
    dirty = null;
    try { store.discardDraft(saved.id); } catch (error) { notice('Saved, but the recovery draft could not be removed: ' + error.message, true); }
  }
  saveStatus();
  renderTabs();
}
function captureNote() {
  const note = noteFor();
  if (!note || !$('editor')) return;
  dirty = { ...note, title: $('note-title').value, html: $('editor').innerHTML, updated: now() };
  try { store.writeDraft(dirty); }
  catch (error) { notice('Recovery draft could not be written: ' + error.message + '. Keep this window open and save again.', true); }
  clearTimeout(draftTimer);
  draftTimer = setTimeout(() => flush().catch(() => {}), 700);
  saveStatus();
  if ($('words')) $('words').textContent = plain(dirty.html).split(/\s+/).filter(Boolean).length + ' words';
}
async function open(tab) {
  await flush();
  const existing = tabs.find(item => item.key === tab.key);
  if (!existing) tabs.push(tab);
  else if (tab.kind === 'project' && tab.view) existing.view = tab.view;
  active = tab.key;
  selectedTasks.clear(); preview = '';
  render(); await session();
}
async function closeTab(key) {
  await flush();
  const index = tabs.findIndex(tab => tab.key === key);
  if (index < 0) return;
  tabs.splice(index, 1);
  if (!tabs.length) tabs.push({ key: 'dashboard', kind: 'dashboard' });
  if (active === key) active = tabs[Math.min(index, tabs.length - 1)].key;
  render(); await session();
}
function projectTab(id, view = 'tasks') { return { key: 'project:' + id, kind: 'project', projectId: id, view }; }
function noteTab(note) { return { key: 'note:' + note.id, kind: 'note', noteId: note.id, projectId: note.project_id }; }
function renderTabs() {
  $('tabs').innerHTML = tabs.map(tab => `<div class="tab${tab.key === active ? ' active' : ''}"><button class="tab-label" data-action="switch-tab" data-value="${esc(tab.key)}" id="tab-${esc(tab.key)}" role="tab" aria-selected="${tab.key === active}" aria-controls="content" tabindex="${tab.key === active ? 0 : -1}">${esc(label(tab))}</button>${btn('×', 'close-tab', tab.key, 'tab-close')}</div>`).join('');
  document.querySelectorAll('.tab-close').forEach(button => button.setAttribute('aria-label', 'Close tab'));
}
function renderSidebar() {
  $('projects').innerHTML = state.projects.filter(project => !project.archived).map(project => btn('▣   ' + project.title, 'project', project.id, 'project-nav' + (project.id === current().projectId ? ' active' : ''))).join('');
}
function heading(title, subtitle, actions = '') { return `<div class="page-heading"><div><h1>${esc(title)}</h1><div class="muted">${esc(subtitle)}</div></div><div class="row">${actions}</div></div>`; }
function render() {
  renderTabs(); renderSidebar();
  const tab = current();
  document.title = label(tab) + ' — Northstar';
  $('breadcrumb').textContent = 'Workspace / ' + label(tab);
  $('content').setAttribute('role', 'tabpanel');
  $('content').setAttribute('aria-labelledby', 'tab-' + tab.key);
  if (tab.kind === 'dashboard') $('content').innerHTML = dashboard();
  else if (tab.kind === 'project') $('content').innerHTML = projectView();
  else if (tab.kind === 'note') $('content').innerHTML = noteView();
  else if (tab.kind === 'settings') $('content').innerHTML = settingsView();
  else if (tab.kind === 'archives') $('content').innerHTML = archivedView();
  else if (tab.kind === 'search') $('content').innerHTML = searchView();
  if (tab.kind === 'dashboard') drawChart();
  if (tab.kind === 'note' && noteFor()) {
    $('editor').innerHTML = cleanHtml(noteFor().html);
    $('editor').addEventListener('input', captureNote);
    $('note-title').addEventListener('input', captureNote);
    $('editor').addEventListener('paste', () => setTimeout(() => {
      // Clipboard HTML is filtered by the engine; sanitize again before storage.
      captureNote();
    }, 0));
  }
  saveStatus();
}
function dashboard() {
  const projects = state.projects.filter(project => !project.archived);
  const projectIds = new Set(projects.map(project => project.id));
  const tasks = state.tasks.filter(task => projectIds.has(task.project_id));
  const done = tasks.filter(task => task.status === 'Done').length;
  const overdue = tasks.filter(task => task.due && task.due < today() && task.status !== 'Done');
  return heading('Room for your best work.', 'Projects, ideas, and the details that bring them together.', btn('+ New project', 'new-project', '', 'primary')) +
    (recoveries.length ? `<div class="recovery">${recoveries.length} recovery draft(s) found. ${btn('Review drafts', 'recovery')}</div>` : '') +
    `<div class="cards"><div class="card"><div class="card-caption">ACTIVE PROJECTS</div><div class="metric">${projects.length}</div><div class="card-caption">A home for every idea</div></div><div class="card"><div class="card-caption">TASKS COMPLETED</div><div class="metric">${done}<small> / ${tasks.length}</small></div><div class="card-caption">Progress, one step at a time</div></div><div class="card"><div class="card-caption">NEEDS ATTENTION</div><div class="metric${overdue.length ? ' overdue' : ''}">${overdue.length}</div><div class="card-caption">Tasks past their deadline</div></div></div><div class="two-columns"><section><div class="row between"><h2>Your projects</h2><span class="muted">${projects.length} active</span></div>` +
    (projects.length ? projects.map(project => {
      const work = tasks.filter(task => task.project_id === project.id);
      const completed = work.filter(task => task.status === 'Done').length;
      return `<div class="card project-card"><div class="row between"><h3>${esc(project.title)}</h3>${btn('Open ↗', 'project', project.id)}</div><p class="muted">${esc(project.description || 'No description yet.')}</p><div class="progress"><div style="width:${work.length ? Math.round(completed / work.length * 100) : 0}%"></div></div><div class="row between"><small>${completed} of ${work.length} tasks complete</small><small>${state.notes.filter(note => note.project_id === project.id).length} notes</small></div></div>`;
    }).join('') : `<div class="card">${empty('Start with a project. Add tasks, collect notes, and keep your files together.')}${btn('Create your first project', 'new-project', '', 'primary')}</div>`) +
    `</section><section><div class="card"><h2>Work in motion</h2><canvas id="chart" width="420" height="145" aria-label="Task completion by status" role="img"></canvas><div class="help">${statuses.map(status => status + ': ' + tasks.filter(task => task.status === status).length).join(' · ')}</div></div><div class="card" style="margin-top:20px"><h2>Coming up</h2>` +
    (tasks.filter(task => task.status !== 'Done' && task.due).sort((a, b) => a.due.localeCompare(b.due)).slice(0, 5).map(task => `<div class="row between" style="margin-bottom:12px">${btn(task.title, 'edit-task', task.id)}<small class="${task.due < today() ? 'overdue' : ''}">${esc(task.due)}</small></div>`).join('') || '<p class="muted">No upcoming deadlines. A little breathing room.</p>') + '</div></section></div>';
}
function drawChart() {
  const canvas = $('chart'); if (!canvas) return;
  const context = canvas.getContext('2d');
  const ids = new Set(state.projects.filter(project => !project.archived).map(project => project.id));
  const counts = statuses.map(status => state.tasks.filter(task => ids.has(task.project_id) && task.status === status).length);
  const max = Math.max(1, ...counts);
  context.clearRect(0, 0, 420, 145);
  ['#a6b7d6', '#6e94dc', '#71ac98'].forEach((color, index) => {
    context.fillStyle = color;
    const height = Math.max(3, counts[index] / max * 95);
    context.fillRect(35 + index * 130, 110 - height, 70, height);
    context.fillStyle = '#8390a5'; context.font = '11px sans-serif';
    context.fillText(statuses[index], 28 + index * 130, 135);
    context.fillText(String(counts[index]), 65 + index * 130, 105 - height);
  });
}
function projectView() {
  const project = projectFor(); if (!project) return empty('This project no longer exists.');
  const tab = current();
  const view = tab.view || 'tasks';
  const head = heading(project.title, project.description || 'Make something worth making.', btn('Edit', 'edit-project', project.id) + btn('Report PDF', 'report', project.id) + btn(project.archived ? 'Unarchive' : 'Archive', 'archive', project.id));
  const navigation = `<div class="view-tabs">${['tasks', 'notes', 'files'].map(key => btn(({ tasks: 'Tasks', notes: 'Notes & documents', files: 'Attachments' })[key], 'project-view', key, view === key ? 'active' : '')).join('')}</div>`;
  if (view === 'notes') return head + navigation + `<div class="toolbar">${btn('+ New note', 'new-note', project.id, 'primary')}</div>` + (state.notes.filter(note => note.project_id === project.id).map(note => `<div class="card note-card"><div class="note-summary">${btn(note.title, 'note', note.id, 'note-title')}<p class="muted">${esc(plain(note.html).slice(0, 150) || 'An empty page, ready for your ideas.')}</p><small>Updated ${esc(note.updated.slice(0, 10))}</small></div>${btn('Open window', 'note-window', note.id)}${btn('Delete', 'delete-note', note.id, 'danger')}</div>`).join('') || empty('Capture your first idea. Create a note to get started.'));
  if (view === 'files') return head + navigation + `<div class="toolbar">${btn('+ Attach file', 'attach', project.id, 'primary')}<span class="help">Files are copied into this workspace · up to 10 MB each</span></div>` + (state.attachments.filter(file => file.project_id === project.id).map(file => `<div class="card note-card"><div class="note-summary"><h3>${esc(file.title)}</h3><small>${Math.ceil(file.size / 1024)} KB · ${esc(file.updated.slice(0, 10))}</small></div>${btn('Preview', 'preview', file.id)}${btn('Save copy', 'save-file', file.id)}${btn('Remove', 'delete-file', file.id, 'danger')}</div>`).join('') || empty('Keep the files that belong to this project in one place.')) + (preview ? '<div class="attachment-preview" id="file-preview"></div>' : '');
  const tasks = state.tasks.filter(task => task.project_id === project.id && (taskFilter === 'All' || task.status === taskFilter) && task.title.toLowerCase().includes(taskQuery.toLowerCase()));
  return head + navigation + `<div class="toolbar"><input id="task-search" placeholder="Filter tasks…" aria-label="Filter tasks" value="${esc(taskQuery)}"><select id="task-filter" aria-label="Filter by status">${['All', ...statuses].map(value => `<option${value === taskFilter ? ' selected' : ''}>${value}</option>`).join('')}</select>${btn('+ Add task', 'new-task', project.id, 'primary')}</div><div class="toolbar">${btn('Mark selected done', 'bulk-done')}${btn('Clear selection', 'clear-selection')}<small id="selected-count">${selectedTasks.size} selected</small></div><div class="table-head"><span></span><span>Task</span><span>Status</span><span>Priority</span><span>Deadline</span><span></span></div><div id="task-list">` + taskRows(tasks) + '</div>';
}
function taskRows(tasks) {
  return tasks.map(task => `<div class="task-row" data-task="${esc(task.id)}"><input type="checkbox" data-check-task="${esc(task.id)}" aria-label="Select ${esc(task.title)}"${selectedTasks.has(task.id) ? ' checked' : ''}>${btn(task.title, 'edit-task', task.id, 'task-title')}<select data-task-status="${esc(task.id)}" aria-label="Status of ${esc(task.title)}">${statuses.map(status => `<option${status === task.status ? ' selected' : ''}>${status}</option>`).join('')}</select><span class="${task.priority === 'High' ? 'overdue' : 'muted'}">${esc(task.priority)}</span><span class="${task.due && task.due < today() && task.status !== 'Done' ? 'overdue' : 'muted'}">${esc(task.due || '—')}</span>${btn('×', 'delete-task', task.id, 'danger')}</div>`).join('') || empty('No tasks here. Add one, or change your filters.');
}
function noteView() {
  const note = noteFor(); if (!note) return empty('This note no longer exists.');
  return `<div class="row between" style="margin-bottom:18px"><span class="muted">${esc(projectFor()?.title || 'Project')} / Document</span><div class="row">${btn('Back to project', 'project', note.project_id)}${btn('Save now', 'save-note', '', 'primary')}${btn('Open window', 'note-window', note.id)}</div></div><input id="note-title" class="note-title-input" value="${esc(note.title)}" aria-label="Note title"><div class="editor-toolbar" role="toolbar" aria-label="Document formatting">` +
    [['Bold', 'bold'], ['Italic', 'italic'], ['Underline', 'underline'], ['H1', 'h1'], ['H2', 'h2'], ['Text', 'p'], ['• List', 'insertUnorderedList'], ['1. List', 'insertOrderedList'], ['Left', 'justifyLeft'], ['Center', 'justifyCenter'], ['Undo', 'undo'], ['Redo', 'redo']].map(([text, command]) => `<button data-format="${command}" aria-label="${text}">${text}</button>`).join('') +
    `${btn('Table', 'insert-table')}${btn('Image', 'insert-image')}${btn('Page view', 'page-view')}</div><div id="editor" class="editor" contenteditable="true" role="textbox" aria-multiline="true" aria-label="Document content"></div><div class="editor-meta"><span id="words">${plain(note.html).split(/\s+/).filter(Boolean).length} words</span><span>Autosaves after typing · Ctrl/Cmd+S to save</span></div>`;
}
function settingsView() {
  return heading('Workspace settings', 'Your tools, your space, your data.') + `<div class="card settings-section"><h2>Appearance</h2><div class="help">Choose a comfortable workspace.</div>${btn('Light', 'theme', 'light')}${btn('Dark', 'theme', 'dark')}</div><div class="card settings-section"><h2>Backup & restore</h2><p class="help">A portable JSON backup includes all projects, tasks, notes, and attachment bytes. Restoring replaces the current workspace; a safety backup is written first. Settings and open tabs are reset on restore.</p>${btn('Create backup…', 'backup', '', 'primary')}${btn('Restore backup…', 'restore')}<p class="help">Backups are unencrypted. Keep them somewhere you trust. Maximum restore file size: 150 MB.</p></div><div class="card settings-section"><h2>Local storage</h2><p class="path">${esc(store.dataDir)}</p><p class="help">SQLite holds your records. Attachments are managed copies; changing the original file does not change your attachment. Removed attachment bytes remain in storage so recovery never deletes a file unexpectedly.</p></div><div class="card settings-section"><h2>Keyboard shortcuts</h2><p class="help">Ctrl/Cmd+K: commands & search · Ctrl/Cmd+N: new project · Ctrl/Cmd+S: save note<br>Ctrl+Tab / Ctrl+Shift+Tab: switch tabs · Ctrl/Cmd+W: close tab<br>Left/Right/Home/End while a tab is focused: navigate tabs · Escape: close a dialog</p></div>`;
}
function archivedView() {
  return heading('Archived projects', 'Finished work, kept close at hand.') + (state.projects.filter(project => project.archived).map(project => `<div class="card note-card"><div><h3>${esc(project.title)}</h3><p class="muted">${esc(project.description)}</p></div>${btn('Open', 'project', project.id)}${btn('Unarchive', 'archive', project.id)}${btn('Delete permanently', 'delete-project', project.id, 'danger')}</div>`).join('') || empty('No archived projects.'));
}
function searchView() {
  return heading('Find your next thought.', 'Search project descriptions, tasks, document text, and attachment names.') + `<input id="global-search" class="wide-search" placeholder="Search your workspace…" aria-label="Search your workspace" value="${esc(searchQuery)}"><div id="search-results">${searchRows()}</div>`;
}
function searchRows() {
  return searchResults.map((result, index) => `<button class="search-result" data-action="search-result" data-value="${index}">${esc(result.title)}<small>${esc(result.kind)} · ${esc(state.projects.find(project => project.id === result.project_id)?.title || '')}</small></button>`).join('') || empty(searchQuery ? 'No matches found.' : 'Type to search.');
}
function showModal(title, body, action, value = '', confirmLabel = 'Save') {
  modal = { action, value, focus: document.activeElement };
  $('overlay').innerHTML = `<div class="scrim"><div class="dialog" role="dialog" aria-modal="true" aria-label="${esc(title)}"><div class="row between"><h2>${esc(title)}</h2>${btn('×', 'dismiss')}</div>${body}<div id="validation" class="validation" role="alert"></div><div class="row dialog-actions">${btn('Cancel', 'dismiss')}${action ? btn(confirmLabel, action, value, 'primary') : ''}</div></div></div>`;
  const input = $('overlay').querySelector('input,textarea') || $('overlay').querySelector('button'); if (input) input.focus();
}
function dismiss() {
  const focus = modal?.focus;
  modal = null; $('overlay').innerHTML = '';
  if (focus && document.body.contains(focus)) focus.focus();
}
function editProject(id) {
  const project = state.projects.find(row => row.id === id);
  showModal(project ? 'Edit project' : 'A new beginning', field('Project name', 'project-title', project?.title || '') + `<label class="field"><span>Description</span><textarea id="project-description">${esc(project?.description || '')}</textarea></label>`, 'save-project', id);
}
function editTask(id, projectId) {
  const task = state.tasks.find(row => row.id === id);
  showModal(task ? 'Edit task' : 'One step forward', field('Task', 'task-title', task?.title || '') + select('Status', 'task-status', statuses, task?.status || 'To do') + select('Priority', 'task-priority', priorities, task?.priority || 'Normal') + field('Deadline (optional)', 'task-due', task?.due || '', 'date'), 'save-task', id || projectId);
  modal.task = task; modal.projectId = task?.project_id || projectId;
}
let confirmAction;
function confirm(title, message, action, label = 'Confirm') { confirmAction = action; showModal(title, `<p class="muted">${esc(message)}</p>`, 'confirm', '', label); }
async function mutate(action, message) { await perform(action); await reload(); render(); if (message) notice(message); }
async function chooseBackup() {
  await flush();
  const destination = await Window.this.selectFile({ mode: 'save', caption: 'Save Northstar backup', extension: 'json', filter: 'Northstar backup (*.json)|*.json' });
  if (!destination) return;
  await perform(() => store.backup(nativePath(destination)));
  notice('Backup saved with all attachments.');
}
function nativePath(url) {
  if (!url.startsWith('file:')) return url;
  const parsed = new URL(url); let result = decodeURIComponent(parsed.pathname);
  if (/^\/[A-Za-z]:/.test(result)) result = result.slice(1);
  if (parsed.hostname && parsed.hostname !== 'localhost') result = '//' + parsed.hostname + result;
  return result;
}
async function previewAttachment(id) {
  const file = state.attachments.find(row => row.id === id); if (!file) return;
  preview = id; render();
  const target = $('file-preview'); if (!target) return;
  if (/\.(png|jpe?g|webp|gif)$/i.test(file.title)) {
    const bytes = new Uint8Array(await fs.readFile(file.path));
    let binary = ''; for (let i = 0; i < bytes.length; i += 8192) binary += String.fromCharCode(...bytes.slice(i, i + 8192));
    const image = document.createElement('img');
    image.setAttribute('alt', file.title);
    const type = /\.jpe?g$/i.test(file.title) ? 'jpeg' : file.title.split('.').pop().toLowerCase();
    image.setAttribute('src', 'data:image/' + type + ';base64,' + btoa(binary)); target.appendChild(image);
  } else if (/\.(txt|md|csv|json|js|css|html|log)$/i.test(file.title) && file.size <= 65536) {
    const text = document.createElement('pre'); text.textContent = new TextDecoder().decode(await fs.readFile(file.path)); target.appendChild(text);
  } else target.textContent = 'Preview is available for images and text files up to 64 KB. Use Save copy to export this file.';
}
async function noteWindow(id) {
  await flush();
  const note = state.notes.find(row => row.id === id); if (!note) return;
  // A read-only snapshot avoids competing writers and shares no live DOM.
  new Window({ url: 'document.html', caption: note.title, width: 780, height: 700, parameters: { title: note.title, html: cleanHtml(note.html), theme: state.preferences.theme || 'light' } });
}
function palette() {
  showModal('Go somewhere. Make something.', `<div class="palette">${btn('Search everything', 'search')}${btn('Overview', 'dashboard')}${btn('Create project', 'new-project')}${btn('Workspace settings', 'settings')}${btn('Create backup', 'backup')}</div>`, null);
}
async function action(name, value, target) {
  if (name === 'dismiss') { dismiss(); return; }
  if (name === 'palette') { palette(); return; }
  if (name === 'confirm') { const work = confirmAction; dismiss(); await work(); return; }
  if (['dashboard', 'search', 'settings', 'archives'].includes(name)) {
    dismiss(); await open({ key: name, kind: name }); if (name === 'search') $('global-search').focus(); return;
  }
  if (name === 'new-project' || name === 'edit-project') { await flush(); editProject(value); return; }
  if (name === 'switch-tab') { const tab = tabs.find(row => row.key === value); if (tab) await open(tab); return; }
  if (name === 'close-tab') { await closeTab(value); return; }
  if (name === 'project') { dismiss(); await open(projectTab(value)); return; }
  if (name === 'project-view') { await flush(); current().view = value; selectedTasks.clear(); preview = ''; render(); await session(); return; }
  if (name === 'save-project') {
    const old = state.projects.find(row => row.id === value);
    const project = await perform(() => store.saveProject({ ...old, title: $('project-title').value, description: $('project-description').value }));
    dismiss(); await reload(); await open(projectTab(project.id)); notice('Project saved.'); return;
  }
  if (name === 'archive') {
    const project = state.projects.find(row => row.id === value);
    await mutate(() => store.saveProject({ ...project, archived: !project.archived }), project.archived ? 'Project restored.' : 'Project archived.'); return;
  }
  if (name === 'delete-project') {
    confirm('Delete this project?', 'This permanently removes the project and all its tasks, notes, and attachment records. Create a backup first if you may need them.', async () => {
      await flush(); await mutate(() => store.remove('projects', value), 'Project deleted.');
      tabs = tabs.filter(tab => tab.projectId !== value); if (!tabs.length) tabs.push({ key: 'dashboard', kind: 'dashboard' }); if (!tabs.some(tab => tab.key === active)) active = tabs[0].key; render(); await session();
    }, 'Delete project'); return;
  }
  if (name === 'new-task') { editTask('', value); return; }
  if (name === 'edit-task') { editTask(value); return; }
  if (name === 'save-task') {
    const task = { ...modal.task, project_id: modal.projectId, title: $('task-title').value, status: $('task-status').value, priority: $('task-priority').value, due: $('task-due').value };
    await perform(() => store.saveTask(task)); dismiss(); await reload(); render(); notice('Task saved.'); return;
  }
  if (name === 'delete-task') { confirm('Delete task?', 'This task will be permanently removed.', () => mutate(async () => { await store.remove('tasks', value); selectedTasks.delete(value); }, 'Task deleted.'), 'Delete'); return; }
  if (name === 'bulk-done') { const ids = [...selectedTasks]; await mutate(() => store.bulkTasks(ids, 'Done'), ids.length + ' task(s) completed.'); selectedTasks.clear(); render(); return; }
  if (name === 'clear-selection') { selectedTasks.clear(); render(); return; }
  if (name === 'new-note') {
    await flush(); const note = await perform(() => store.saveNote({ project_id: value, title: 'Untitled note', html: '<p>Start with a thought.</p>' }));
    await reload(); await open(noteTab(note)); $('note-title').focus(); $('note-title').select(); return;
  }
  if (name === 'note') { const note = state.notes.find(row => row.id === value); if (note) await open(noteTab(note)); return; }
  if (name === 'save-note') { await flush(); notice('Document saved.'); return; }
  if (name === 'delete-note') { confirm('Delete note?', 'This note will be permanently removed.', async () => {
    await flush(); await perform(() => store.remove('notes', value)); store.discardDraft(value); await reload();
    tabs = tabs.filter(tab => tab.noteId !== value); if (!tabs.some(tab => tab.key === active)) active = tabs[0].key; render(); await session(); notice('Note deleted.');
  }, 'Delete'); return; }
  if (name === 'note-window') { await noteWindow(value); return; }
  if (name === 'insert-table') { document.execCommand('insertTable', false, '3x3'); captureNote(); return; }
  if (name === 'insert-image') {
    const source = await Window.this.selectFile({ mode: 'open', filter: 'Images (*.png;*.jpg;*.webp)|*.png;*.jpg;*.jpeg;*.webp;*.gif' }); if (!source) return;
    const file = nativePath(source); const stat = await fs.stat(file);
    if (stat.st_size > 160 * 1024) throw new Error('Choose an inline image under 160 KB. Larger images can be added as project attachments.');
    const type = /\.jpe?g$/i.test(file) ? 'jpeg' : file.split('.').pop().toLowerCase();
    if (!['png', 'jpeg', 'webp', 'gif'].includes(type)) throw new Error('Choose a PNG, JPEG, WebP or GIF image.');
    const bytes = new Uint8Array(await fs.readFile(file)); let binary = '';
    for (let i = 0; i < bytes.length; i += 8192) binary += String.fromCharCode(...bytes.slice(i, i + 8192));
    document.execCommand('insertImage', false, 'data:image/' + type + ';base64,' + btoa(binary)); captureNote(); return;
  }
  if (name === 'page-view') { $('editor').classList.toggle('paged'); return; }
  if (name === 'attach') {
    const source = await Window.this.selectFile({ mode: 'open', caption: 'Attach a file to this project' }); if (!source) return;
    await mutate(() => store.attach(value, nativePath(source)), 'Attachment copied into workspace.'); return;
  }
  if (name === 'preview') { await previewAttachment(value); return; }
  if (name === 'save-file') {
    const file = state.attachments.find(row => row.id === value); if (!file) return;
    const destination = await Window.this.selectFile({ mode: 'save', caption: 'Save a copy of ' + file.title, extension: file.title.split('.').pop() }); if (!destination) return;
    await fs.copyfile(file.path, nativePath(destination)); notice('File copy saved.'); return;
  }
  if (name === 'delete-file') { confirm('Remove attachment?', 'The attachment record will be removed. The original file is untouched.', () => mutate(() => store.remove('attachments', value), 'Attachment removed.'), 'Remove'); return; }
  if (name === 'theme') { await perform(() => store.preference('theme', value)); state.preferences.theme = value; document.documentElement.setAttribute('data-theme', value); notice('Appearance saved.'); return; }
  if (name === 'backup') { dismiss(); await chooseBackup(); return; }
  if (name === 'restore') {
    await flush(); const source = await Window.this.selectFile({ mode: 'open', filter: 'Northstar backup (*.json)|*.json' }); if (!source) return;
    const backup = await store.readBackup(nativePath(source));
    confirm('Replace this workspace?', `Restore ${backup.projects.length} projects, ${backup.tasks.length} tasks, ${backup.notes.length} notes, and ${backup.attachments.length} files? A safety backup of the current workspace will be saved first.`, async () => {
      const safety = store.dataDir + '/before-restore-' + Date.now() + '.json';
      await perform(async () => { await store.backup(safety); await store.restore(backup); });
      for (const draft of store.drafts()) store.discardDraft(draft.id);
      dirty = null; recoveries = []; await reload(); tabs = [{ key: 'dashboard', kind: 'dashboard' }]; active = 'dashboard'; searchQuery = ''; searchResults = []; document.documentElement.setAttribute('data-theme', 'light'); render(); await session(); notice('Workspace restored. Safety backup: ' + safety);
    }, 'Restore workspace'); return;
  }
  if (name === 'report') {
    await flush(); const project = state.projects.find(row => row.id === value); if (!project) return;
    const destination = await Window.this.selectFile({ mode: 'save', caption: 'Export project report', extension: 'pdf', filter: 'PDF (*.pdf)|*.pdf' }); if (!destination) return;
    await perform(() => exportPdf(nativePath(destination), reportHtml(project, state))); notice('Project report exported to PDF.'); return;
  }
  if (name === 'search-result') {
    const result = searchResults[Number(value)]; if (!result) return;
    if (result.kind === 'note') await open(noteTab(state.notes.find(note => note.id === result.id)));
    else { await open(projectTab(result.project_id, result.kind === 'attachment' ? 'files' : 'tasks')); if (result.kind === 'task') editTask(result.id); if (result.kind === 'attachment') await previewAttachment(result.id); }
    return;
  }
  if (name === 'recovery') {
    showModal('Recovered drafts', '<p class="help">Recover a draft to replace its saved note, or discard it. Drafts from deleted projects cannot be recovered.</p>' + recoveries.map(draft => `<div class="card note-card"><span>${esc(draft.title || 'Untitled')}</span>${btn('Recover', 'recover-draft', draft.id)}${btn('Discard', 'discard-draft', draft.id)}</div>`).join(''), null); return;
  }
  if (name === 'recover-draft') {
    const draft = recoveries.find(row => row.id === value); if (!draft) return;
    if (!state.projects.some(project => project.id === draft.project_id)) throw new Error('The draft’s project no longer exists.');
    const recoveredTitle = String(draft.title || '').trim().slice(0, 160) || 'Recovered note';
    await perform(() => store.saveNote({ ...draft, title: recoveredTitle, html: cleanHtml(draft.html) })); store.discardDraft(value); recoveries = recoveries.filter(row => row.id !== value); dismiss(); await reload(); await open(noteTab(state.notes.find(note => note.id === value))); notice('Draft recovered.'); return;
  }
  if (name === 'discard-draft') { store.discardDraft(value); recoveries = recoveries.filter(row => row.id !== value); dismiss(); render(); if (recoveries.length) await action('recovery'); return; }
}
// Actions are serialized at the UI boundary to avoid double submits and dialog races.
document.addEventListener('click', event => {
  const format = event.target.closest('[data-format]');
  if (format && $('editor') && ready && !busy) {
    const command = format.dataset.format;
    document.execCommand(['h1', 'h2', 'p'].includes(command) ? 'formatBlock' : command, false, ['h1', 'h2', 'p'].includes(command) ? command : null);
    captureNote();
    return;
  }
  const target = event.target.closest('[data-action]'); if (!target || target.disabled || !ready || busy) return;
  $('menu').innerHTML = '';
  busy = true;
  action(target.dataset.action, target.dataset.value || '', target).catch(error => notice(error.message || String(error), true)).finally(() => { busy = false; });
});
document.addEventListener('mousedown', event => {
  if (event.target.closest('[data-format], [data-action="insert-table"], [data-action="insert-image"], [data-action="page-view"]')) event.preventDefault();
});
document.addEventListener('input', event => {
  if (event.target.id === 'task-search') {
    taskQuery = event.target.value;
    $('task-list').innerHTML = taskRows(state.tasks.filter(task => task.project_id === current().projectId && (taskFilter === 'All' || task.status === taskFilter) && task.title.toLowerCase().includes(taskQuery.toLowerCase())));
  }
  if (event.target.id === 'global-search') {
    searchQuery = event.target.value; const generation = ++searchGeneration;
    clearTimeout(searchTimer);
    searchTimer = setTimeout(async () => {
      try {
        const results = searchQuery.trim() ? await store.search(searchQuery.trim()) : [];
        if (generation !== searchGeneration) return;
        searchResults = results; if ($('search-results')) $('search-results').innerHTML = searchRows();
      } catch (error) { notice(error.message, true); }
    }, 180);
  }
});
document.addEventListener('change', event => {
  const target = event.target;
  if (target.id === 'task-filter') { taskFilter = target.value; selectedTasks.clear(); render(); }
  if (target.dataset.checkTask) { if (target.checked) selectedTasks.add(target.dataset.checkTask); else selectedTasks.delete(target.dataset.checkTask); $('selected-count').textContent = selectedTasks.size + ' selected'; }
  if (target.dataset.taskStatus) {
    const task = state.tasks.find(row => row.id === target.dataset.taskStatus);
    perform(() => store.saveTask({ ...task, status: target.value })).then(reload).then(render).catch(() => { render(); });
  }
});
document.addEventListener('contextmenu', event => {
  const task = event.target.closest('[data-task]'); const project = event.target.closest('.project-nav');
  if (!task && !project) return; event.preventDefault();
  const id = task ? task.dataset.task : project.dataset.value;
  $('menu').innerHTML = `<div class="context-menu" role="menu" style="left:${Math.max(0, Math.min(event.clientX, innerWidth - 200))}px;top:${Math.max(0, Math.min(event.clientY, innerHeight - 130))}px">${task ? btn('Edit task', 'edit-task', id) + btn('Delete task', 'delete-task', id, 'danger') : btn('Open project', 'project', id) + btn('Edit project', 'edit-project', id) + btn('Archive / unarchive', 'archive', id)}</div>`;
});
document.addEventListener('keydown', event => {
  if (!ready) return;
  const key = event.key.toLowerCase(); const command = event.ctrlKey || event.metaKey;
  if (key === 'escape') { dismiss(); $('menu').innerHTML = ''; return; }
  // Keep keyboard focus within an open dialog.
  if (modal && key === 'tab') {
    const controls = Array.from($('overlay').querySelectorAll('button,input,textarea,select')).filter(item => !item.disabled);
    if (controls.length) {
      const index = controls.indexOf(document.activeElement);
      if (index < 0 || (!event.shiftKey && index === controls.length - 1) || (event.shiftKey && index === 0)) { event.preventDefault(); controls[event.shiftKey ? controls.length - 1 : 0].focus(); }
    }
    return;
  }
  if (modal || busy) return;
  let work;
  if (command && key === 'k') work = () => palette();
  else if (command && key === 'n') work = () => action('new-project', '');
  else if (command && key === 's') work = () => flush();
  else if (command && key === 'w') work = () => closeTab(active);
  else if (event.ctrlKey && key === 'tab') {
    const index = tabs.findIndex(tab => tab.key === active); work = () => open(tabs[(index + (event.shiftKey ? tabs.length - 1 : 1)) % tabs.length]);
  } else if (event.target.getAttribute('role') === 'tab' && ['arrowleft', 'arrowright', 'home', 'end'].includes(key)) {
    const index = tabs.findIndex(tab => tab.key === active);
    const next = key === 'home' ? 0 : key === 'end' ? tabs.length - 1 : (index + (key === 'arrowleft' ? tabs.length - 1 : 1)) % tabs.length;
    work = async () => { await open(tabs[next]); $('tab-' + active).focus(); };
  }
  if (work) { event.preventDefault(); busy = true; Promise.resolve().then(work).catch(error => notice(error.message, true)).finally(() => { busy = false; }); }
});
let resizing = false;
$('resize').addEventListener('mousedown', event => { resizing = true; event.preventDefault(); });
document.addEventListener('mousemove', event => { if (resizing) $('sidebar').style.width = Math.max(180, Math.min(340, event.clientX)) + 'px'; });
document.addEventListener('mouseup', () => { if (resizing) { resizing = false; store.preference('sidebarWidth', parseInt($('sidebar').style.width)).catch(error => notice(error.message, true)); } });
$('resize').addEventListener('keydown', event => {
  if (!['ArrowLeft', 'ArrowRight'].includes(event.key)) return;
  event.preventDefault(); const width = Math.max(180, Math.min(340, $('sidebar').getBoundingClientRect().width + (event.key === 'ArrowRight' ? 10 : -10)));
  $('sidebar').style.width = width + 'px'; store.preference('sidebarWidth', width).catch(error => notice(error.message, true));
});
Window.this.on('closerequest', event => {
  if (closing || (!dirty && !pending && !busy)) return;
  event.preventDefault();
  if (pending || busy) { notice('Finish the current operation before closing.'); return; }
  flush().then(() => { closing = true; window.close(); }).catch(() => notice('Your document has not been saved. Fix the error and try Save now before closing.', true));
});
async function start() {
  state = await store.init();
  document.documentElement.setAttribute('data-theme', state.preferences.theme === 'dark' ? 'dark' : 'light');
  if (state.preferences.sidebarWidth) $('sidebar').style.width = Math.max(180, Math.min(340, state.preferences.sidebarWidth)) + 'px';
  const saved = state.preferences.session;
  if (saved && Array.isArray(saved.tabs)) {
    const keys = new Set();
    tabs = saved.tabs.filter(tab => {
      if (!tab || typeof tab.key !== 'string' || keys.has(tab.key)) return false;
      keys.add(tab.key);
      return ['dashboard', 'search', 'settings', 'archives'].includes(tab.kind) || (tab.kind === 'project' && state.projects.some(project => project.id === tab.projectId)) || (tab.kind === 'note' && state.notes.some(note => note.id === tab.noteId));
    }).slice(0, 30);
    if (!tabs.length) tabs = [{ key: 'dashboard', kind: 'dashboard' }];
    active = tabs.some(tab => tab.key === saved.active) ? saved.active : tabs[0].key;
  }
  recoveries = store.drafts().filter(draft => typeof draft.id === 'string' && typeof draft.html === 'string');
  render(); ready = true;
  notice(recoveries.length ? recoveries.length + ' recovery draft(s) found. Open Overview to review them.' : 'Your workspace is ready.');
  console.log('northstar ready');
}
start().catch(error => { $('content').innerHTML = heading('Could not open your workspace', error.message) + '<p class="help">Check the storage folder permissions and SQLite extension installation, then restart the app. Your data has not been replaced.</p>'; notice(error.message, true); });
