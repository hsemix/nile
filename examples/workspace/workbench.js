import { escape as esc, plain, statuses, priorities } from './model.js';

const $ = id => document.getElementById(id);
const defaults = () => ({ explorer: true, inspector: true, activity: true, toolbar: true, explorerWidth: 210, inspectorWidth: 230, activityHeight: 145, expanded: [] });
const clamp = (value, min, max, fallback) => Number.isFinite(value) ? Math.max(min, Math.min(max, value)) : fallback;

// Application-level chrome: no native shell or engine changes are required.
export function createWorkbench({ context, dispatch, persist, failed, treeChanged }) {
  let layout = defaults();
  let selection = null;
  let tabKey = '';
  let messages = [];
  let errorsOnly = false;
  let menu = null;
  let menuMode = false;
  let menuFocus = null;
  let barIndex = 0;
  let menuPointerDown = false;
  let dismissedPointer = false;
  let ignoreClick = false;
  let ignoreClickTimer;
  let altPending = false;
  let range = null;
  let drag = null;
  let layoutWrites = 0;
  const menus = ['File', 'Edit', 'View', 'Window', 'Help'];
  const button = (label, action, value = '', cls = '') => `<button class="${cls}" data-action="${action}" data-value="${esc(value)}">${esc(label)}</button>`;
  function selected() {
    const current = context();
    if (selection?.kind === 'task') {
      const task = current.state.tasks.find(row => row.id === selection.id && row.project_id === current.project?.id);
      if (task) return { kind: 'task', record: task };
    }
    if (selection?.kind === 'attachment') {
      const file = current.state.attachments.find(row => row.id === selection.id && row.project_id === current.project?.id);
      if (file) return { kind: 'attachment', record: file };
    }
    if (current.note) return { kind: 'note', record: current.note };
    if (current.project) return { kind: 'project', record: current.project };
    return { kind: 'workspace', record: null };
  }
  function commands() {
    const current = context();
    const project = current.project;
    const note = current.note;
    const entry = (label, action, value = '', enabled = true, shortcut = '', checked) => ({ label, action, value, enabled, shortcut, checked });
    const format = (label, command, shortcut) => entry(label, 'format-command', command, !!note, shortcut);
    return {
      'new-project': entry('New project…', 'new-project', '', true, 'Ctrl/Cmd+N'),
      'new-task': entry('New task…', 'new-task', project?.id || '', !!project),
      'new-note': entry('New document', 'new-note', project?.id || '', !!project),
      attach: entry('Attach file…', 'attach', project?.id || '', !!project),
      save: entry('Save document', 'save-note', '', !!note, 'Ctrl/Cmd+S'),
      report: entry('Export project PDF…', 'report', project?.id || '', !!project),
      backup: entry('Back up workspace…', 'backup'),
      restore: entry('Restore workspace…', 'restore'),
      close: entry('Close tab', 'close-tab', current.tab.key, true, 'Ctrl/Cmd+W'),
      undo: format('Undo', 'undo', 'Ctrl/Cmd+Z'), redo: format('Redo', 'redo', 'Ctrl/Cmd+Shift+Z'),
      cut: format('Cut', 'cut', 'Ctrl/Cmd+X'), copy: format('Copy', 'copy', 'Ctrl/Cmd+C'),
      paste: format('Paste', 'paste', 'Ctrl/Cmd+V'), select: format('Select all', 'selectAll', 'Ctrl/Cmd+A'),
      search: entry('Search everything', 'search'),
      'toggle-explorer': entry('Project explorer', 'workbench-pane', 'explorer', true, 'Ctrl/Cmd+Shift+E', layout.explorer),
      'toggle-inspector': entry('Properties', 'workbench-pane', 'inspector', true, '', layout.inspector),
      'toggle-activity': entry('Activity panel', 'workbench-pane', 'activity', true, 'Ctrl/Cmd+J', layout.activity),
      'toggle-toolbar': entry('Command toolbar', 'workbench-pane', 'toolbar', true, '', layout.toolbar),
      'reset-layout': entry('Reset workspace layout', 'workbench-reset'),
      next: entry('Next tab', 'workbench-tab', 'next', current.tabs.length > 1, 'Ctrl+Tab'),
      previous: entry('Previous tab', 'workbench-tab', 'previous', current.tabs.length > 1, 'Ctrl+Shift+Tab'),
      'close-others': entry('Close other tabs', 'workbench-close-others', '', current.tabs.length > 1),
      snapshot: entry('Open document snapshot', 'note-window', note?.id || '', !!note),
      commands: entry('Command palette', 'palette', '', true, 'Ctrl/Cmd+K'),
      settings: entry('Workspace settings', 'settings'),
      shortcuts: entry('Keyboard shortcuts', 'workbench-help'),
      about: entry('About Northstar', 'workbench-about'),
      'activity-all': entry('Activity', 'workbench-activity', 'all'),
      'activity-errors': entry('Errors', 'workbench-activity', 'errors'),
      'clear-activity': entry('Clear activity', 'workbench-clear'),
    };
  }
  const menuCommands = {
    File: ['new-project', 'new-task', 'new-note', 'attach', '-', 'save', 'report', '-', 'backup', 'restore', '-', 'close'],
    Edit: ['undo', 'redo', '-', 'cut', 'copy', 'paste', 'select', '-', 'search'],
    View: ['toggle-explorer', 'toggle-inspector', 'toggle-activity', 'toggle-toolbar', '-', 'reset-layout'],
    Window: ['next', 'previous', 'close-others', '-', 'snapshot'],
    Help: ['commands', 'settings', 'shortcuts', '-', 'about'],
  };
  function rememberSelection() {
    const editor = $('editor'); const current = window.getSelection();
    if (editor && current.rangeCount && editor.contains(current.anchorNode) && editor.contains(current.focusNode)) range = current.getRangeAt(0).cloneRange();
  }
  function restoreSelection() {
    const editor = $('editor'); if (!editor) return;
    editor.focus();
    if (range && editor.contains(range.startContainer) && editor.contains(range.endContainer)) {
      const current = window.getSelection(); current.removeAllRanges(); current.addRange(range);
    }
  }
  function commandButton(id, label) {
    const command = commands()[id];
    return `<button data-wb-command="${id}"${command.enabled ? '' : ' disabled'} title="${esc(command.label + (command.shortcut ? ' (' + command.shortcut + ')' : ''))}">${esc(label || command.label)}</button>`;
  }
  function refreshCommands() {
    const registry = commands();
    document.querySelectorAll('[data-wb-command]').forEach(element => {
      const command = registry[element.dataset.wbCommand]; if (!command) return;
      element.disabled = !command.enabled;
      if (command.checked !== undefined) element.setAttribute('aria-pressed', String(command.checked));
    });
  }
  function refreshToolbar() {
    $('workbench-toolbar').innerHTML = '<div class="command-group">' + commandButton('new-project', '+ Project') + commandButton('new-task', '+ Task') + commandButton('new-note', '▤ Document') + '</div><div class="command-group">' + commandButton('save', '▣ Save') + commandButton('undo', '↶') + commandButton('redo', '↷') + '</div><div class="command-group">' + commandButton('attach', 'Attach') + commandButton('report', 'PDF report') + '</div><div class="command-spacer"></div>' + commandButton('commands', 'Commands…') + commandButton('toggle-explorer', 'Explorer') + commandButton('toggle-inspector', 'Properties') + commandButton('toggle-activity', 'Activity');
  }
  function barButtons() { return Array.from($('menubar').querySelectorAll('[data-wb-menu]')); }
  function focusBar(index) {
    barIndex = (index + menus.length) % menus.length;
    barButtons().forEach((element, i) => { element.setAttribute('tabindex', i === barIndex ? '0' : '-1'); });
    barButtons()[barIndex]?.focus();
  }
  function enterMenuMode() {
    if (menuMode) return;
    menuFocus = document.activeElement; rememberSelection(); menuMode = true;
    $('menubar').classList.add('menu-mode');
  }
  function closeDropDown(focus = false) {
    const previous = menu; menu = null; $('workbench-menu').innerHTML = '';
    document.querySelectorAll('[data-wb-menu]').forEach(element => { element.setAttribute('aria-expanded', 'false'); element.classList.remove('open'); });
    if (focus && previous) focusBar(menus.indexOf(previous));
  }
  function closeMenu(focus = false) {
    closeDropDown(); menuMode = false; menuPointerDown = false;
    $('menubar').classList.remove('menu-mode');
    const previous = menuFocus; menuFocus = null;
    if (focus && previous && document.body.contains(previous)) previous.focus();
  }
  function menuItems() { return Array.from($('workbench-menu').querySelectorAll('button')).filter(element => !element.disabled); }
  function highlightItem(index, focus = true) {
    const items = menuItems();
    $('workbench-menu').querySelectorAll('.workbench-menu-row').forEach(element => element.classList.remove('active'));
    const item = items[index];
    if (item) { item.closest('.workbench-menu-row').classList.add('active'); if (focus) item.focus(); }
    else if (focus && menu) barButtons()[barIndex]?.focus();
  }
  function openMenu(name, focus = false) {
    enterMenuMode(); closeDropDown(); menu = name; barIndex = menus.indexOf(name);
    $('menu').innerHTML = '';
    const trigger = document.querySelector(`[data-wb-menu="${name}"]`);
    trigger.setAttribute('aria-expanded', 'true'); trigger.classList.add('open');
    const registry = commands(); const rectangle = trigger.getBoundingClientRect();
    $('workbench-menu').innerHTML = `<div class="workbench-menu" role="menu" aria-label="${name}" style="left:${Math.max(4, Math.min(rectangle.x, innerWidth - 300))}px;top:${rectangle.bottom}px">` + menuCommands[name].map(id => {
      if (id === '-') return '<div class="menu-rule" role="separator"></div>';
      const command = registry[id];
      return `<div class="workbench-menu-row${command.enabled ? '' : ' disabled'}" data-wb-command="${id}"><span class="menu-check">${command.checked ? '✓' : ''}</span><button data-wb-command="${id}" role="${command.checked !== undefined ? 'menuitemcheckbox' : 'menuitem'}"${command.checked !== undefined ? ' aria-checked="' + command.checked + '"' : ''}${command.enabled ? '' : ' disabled aria-disabled="true"'}>${esc(command.label)}</button><small>${esc(command.shortcut)}</small></div>`;
    }).join('') + '</div>';
    focusBar(barIndex);
    if (focus) highlightItem(focus === 'last' ? menuItems().length - 1 : 0);
  }
  async function execute(id) {
    const command = commands()[id]; if (!command || !command.enabled || context().blocked) return;
    closeMenu(true);
    if (command.action === 'format-command') restoreSelection();
    await dispatch(command.action, command.value);
  }
  function layoutChanged() {
    const snapshot = { ...layout, expanded: [...layout.expanded] };
    layoutWrites++;
    return Promise.resolve().then(() => persist(snapshot)).finally(() => { layoutWrites--; });
  }
  function applyLayout() {
    $('sidebar').style.display = layout.explorer ? 'flex' : 'none';
    $('resize').style.display = layout.explorer ? 'block' : 'none';
    $('sidebar').style.width = layout.explorerWidth + 'px';
    $('inspector').style.display = layout.inspector ? 'flex' : 'none';
    $('inspector-resize').style.display = layout.inspector ? 'block' : 'none';
    $('inspector').style.width = layout.inspectorWidth + 'px';
    $('activity').style.display = layout.activity ? 'flex' : 'none';
    $('activity-resize').style.display = layout.activity ? 'block' : 'none';
    $('activity').style.height = Math.min(layout.activityHeight, Math.max(90, innerHeight * .35)) + 'px';
    $('workbench-toolbar').style.display = layout.toolbar ? 'flex' : 'none';
    document.body.classList.toggle('with-inspector', layout.inspector);
    for (const [id, value, min, max, orientation] of [['resize', layout.explorerWidth, 180, 340, 'vertical'], ['inspector-resize', layout.inspectorWidth, 210, 340, 'vertical'], ['activity-resize', layout.activityHeight, 90, 300, 'horizontal']]) {
      const element = $(id);
      element.setAttribute('aria-orientation', orientation); element.setAttribute('aria-valuenow', String(value)); element.setAttribute('aria-valuemin', String(min)); element.setAttribute('aria-valuemax', String(max));
    }
    refreshCommands();
  }
  function restore(saved = {}) {
    const clean = defaults();
    for (const key of ['explorer', 'inspector', 'activity', 'toolbar']) if (typeof saved[key] === 'boolean') clean[key] = saved[key];
    clean.explorerWidth = clamp(saved.explorerWidth, 180, 340, clean.explorerWidth);
    clean.inspectorWidth = clamp(saved.inspectorWidth, 210, 340, clean.inspectorWidth);
    clean.activityHeight = clamp(saved.activityHeight, 90, 300, clean.activityHeight);
    clean.expanded = Array.isArray(saved.expanded) ? saved.expanded.filter(id => typeof id === 'string').slice(0, 300) : [];
    layout = clean; applyLayout();
  }
  async function togglePane(name) { if (!['explorer', 'inspector', 'activity', 'toolbar'].includes(name)) return; layout[name] = !layout[name]; applyLayout(); await layoutChanged(); }
  async function toggleTree(id) {
    layout.expanded = layout.expanded.includes(id) ? layout.expanded.filter(value => value !== id) : [...layout.expanded, id];
    treeChanged(); await layoutChanged();
  }
  function renderActivity() {
    $('activity-all').classList.toggle('active', !errorsOnly); $('activity-errors').classList.toggle('active', errorsOnly);
    $('activity-count').textContent = messages.filter(message => message.error).length + ' errors';
    const visible = messages.filter(message => !errorsOnly || message.error);
    $('activity-log').innerHTML = visible.length ? visible.slice().reverse().map(message => `<div class="activity-row${message.error ? ' activity-error' : ''}"><time>${esc(message.time)}</time><span>${message.error ? 'ERROR' : 'INFO'}</span><div>${esc(message.text)}</div></div>`).join('') : '<div class="activity-empty">' + (errorsOnly ? 'No errors in this session.' : 'Workspace activity will appear here.') + '</div>';
  }
  function log(text, error = false) {
    messages.push({ text, error, time: new Date().toLocaleTimeString() }); messages = messages.slice(-80); renderActivity();
  }
  function property(label, value, id = '') { return `<div class="property"><span>${esc(label)}</span><div${id ? ' id="' + id + '"' : ''}>${esc(value)}</div></div>`; }
  function input(label, id, value, type = 'text') { return `<label class="property-field"><span>${esc(label)}</span><input id="${id}" type="${type}" value="${esc(value)}"></label>`; }
  function choices(label, id, values, value) { return `<label class="property-field"><span>${esc(label)}</span><select id="${id}">${values.map(option => `<option${option === value ? ' selected' : ''}>${option}</option>`).join('')}</select></label>`; }
  function renderInspector() {
    const { state, project } = context(); const { kind, record } = selected();
    document.querySelectorAll('[data-task]').forEach(element => element.classList.toggle('inspected', kind === 'task' && element.dataset.task === record.id));
    let html = `<div class="inspector-type">${kind === 'workspace' ? 'WORKSPACE' : kind.toUpperCase()}</div><h2>${esc(record?.title || 'Northstar')}</h2>`;
    if (kind === 'task') {
      html += input('Task title', 'property-title', record.title) + choices('Status', 'property-status', statuses, record.status) + choices('Priority', 'property-priority', priorities, record.priority) + input('Deadline', 'property-due', record.due, 'date') + button('Apply changes', 'apply-properties', '', 'primary') + '<div class="inspector-rule"></div>' + button('Open task dialog', 'edit-task', record.id);
    } else if (kind === 'project') {
      html += input('Project name', 'property-title', record.title) + `<label class="property-field"><span>Description</span><textarea id="property-description">${esc(record.description)}</textarea></label>` + button('Apply changes', 'apply-properties', '', 'primary') + '<div class="inspector-rule"></div>' + property('Status', record.archived ? 'Archived' : 'Active') + property('Tasks', state.tasks.filter(task => task.project_id === record.id).length) + property('Documents', state.notes.filter(note => note.project_id === record.id).length) + property('Attachments', state.attachments.filter(file => file.project_id === record.id).length);
    } else if (kind === 'note') {
      html += property('Project', project?.title || '') + property('Words', plain(record.html).split(/\s+/).filter(Boolean).length, 'property-words') + property('Last saved (UTC)', record.updated.slice(0, 16).replace('T', ' '), 'property-saved') + '<div class="inspector-rule"></div>' + button('Open snapshot window', 'note-window', record.id) + '<p class="help">Documents autosave as you type. Use the command toolbar to save or undo.</p>';
    } else if (kind === 'attachment') {
      html += property('Project', project?.title || '') + property('Size', Math.ceil(record.size / 1024) + ' KB') + property('Added', record.updated.slice(0, 10)) + button('Preview', 'preview', record.id) + button('Save a copy…', 'save-file', record.id);
    } else {
      html += property('Active projects', state.projects.filter(row => !row.archived).length) + property('Open tabs', context().tabs.length) + property('Storage', 'Local SQLite') + '<div class="inspector-rule"></div><p class="help">Open a project or select a task to inspect its properties. Changes apply to the real workspace.</p>' + button('New project…', 'new-project') + button('Workspace settings', 'settings');
    }
    $('inspector-content').innerHTML = html;
  }
  function refresh() {
    const current = context(); if (current.tab.key !== tabKey) { closeMenu(); selection = null; range = null; tabKey = current.tab.key; }
    $('window-title').textContent = 'Northstar Workspace';
    refreshToolbar(); renderInspector(); refreshCommands();
  }
  function inspect(kind, id) { selection = { kind, id }; renderInspector(); }
  function finishDrag() {
    if (!drag) return;
    drag = null; document.body.classList.remove('resizing-pane');
    layoutChanged().catch(failed);
  }
  for (const [id, key, direction, min, max] of [['resize', 'explorerWidth', 1, 180, 340], ['inspector-resize', 'inspectorWidth', -1, 210, 340], ['activity-resize', 'activityHeight', -1, 90, 300]]) {
    const element = $(id);
    element.addEventListener('mousedown', event => { if (context().blocked) return; event.preventDefault(); drag = { key, direction, min, max, start: key === 'activityHeight' ? event.clientY : event.clientX, value: layout[key] }; document.body.classList.add('resizing-pane'); });
    element.addEventListener('keydown', event => {
      if (context().blocked) return;
      const horizontal = key === 'activityHeight';
      const keys = horizontal ? ['ArrowUp', 'ArrowDown'] : ['ArrowLeft', 'ArrowRight'];
      if (!keys.includes(event.key)) return;
      event.preventDefault(); layout[key] = clamp(layout[key] + (event.key === keys[0] ? -10 : 10) * direction, min, max, layout[key]);
      applyLayout(); layoutChanged().catch(failed);
    });
  }
  document.addEventListener('mousemove', event => { if (!drag) return; layout[drag.key] = clamp(drag.value + ((drag.key === 'activityHeight' ? event.clientY : event.clientX) - drag.start) * drag.direction, drag.min, drag.max, drag.value); applyLayout(); });
  document.addEventListener('mouseup', finishDrag);
  window.addEventListener('blur', () => { finishDrag(); altPending = false; dismissedPointer = false; closeMenu(true); });
  window.addEventListener('resize', () => { closeMenu(true); applyLayout(); });
  // A menu owns the pointer until release, including a press on the menu bar
  // followed by dragging into a command. Suppress the matching synthesized
  // click so one release can never execute a command twice.
  function suppressClick() {
    ignoreClick = true; clearTimeout(ignoreClickTimer);
    ignoreClickTimer = setTimeout(() => { ignoreClick = false; }, 0);
  }
  document.addEventListener('mousedown', event => {
    const trigger = event.target.closest?.('[data-wb-menu]');
    const row = event.target.closest?.('.workbench-menu-row');
    if ((menuMode || trigger) && event.button !== 0) { closeMenu(true); return; }
    if (trigger && !context().blocked) {
      event.preventDefault();
      if (menu === trigger.dataset.wbMenu) closeMenu(true);
      else { openMenu(trigger.dataset.wbMenu); menuPointerDown = true; }
      return;
    }
    if (row && menu) {
      event.preventDefault(); menuPointerDown = true;
      highlightItem(menuItems().indexOf(row.querySelector('button')));
      return;
    }
    if (menuMode && !event.target.closest?.('#menubar, #workbench-menu')) {
      event.preventDefault(); event.stopImmediatePropagation();
      closeMenu(true); dismissedPointer = true; suppressClick(); return;
    }
    if (event.target.closest?.('#workbench-toolbar, [data-wb-command]')) { rememberSelection(); event.preventDefault(); }
  }, true);
  document.addEventListener('mouseover', event => {
    if (!menu || context().blocked) return;
    const trigger = event.target.closest?.('[data-wb-menu]');
    if (trigger && trigger.dataset.wbMenu !== menu) { openMenu(trigger.dataset.wbMenu); return; }
    const row = event.target.closest?.('.workbench-menu-row');
    if (row) highlightItem(menuItems().indexOf(row.querySelector('button')));
  });
  document.addEventListener('mouseup', event => {
    if (dismissedPointer) { dismissedPointer = false; suppressClick(); return; }
    const trigger = event.target.closest?.('[data-wb-menu]');
    if (!menuPointerDown && !trigger) return;
    const pressed = menuPointerDown; menuPointerDown = false; suppressClick();
    if (!pressed) return;
    const row = event.target.closest?.('.workbench-menu-row');
    if (row && menu) {
      const item = row.querySelector('button');
      if (!item.disabled) execute(item.dataset.wbCommand).catch(failed);
    } else if (!trigger && !event.target.closest?.('#workbench-menu')) closeMenu(true);
  });
  document.addEventListener('click', event => {
    if (ignoreClick) { ignoreClick = false; event.preventDefault(); event.stopImmediatePropagation(); return; }
    const trigger = event.target.closest?.('[data-wb-menu]');
    if (trigger && !context().blocked) { if (menu === trigger.dataset.wbMenu) closeMenu(true); else openMenu(trigger.dataset.wbMenu); return; }
    const command = event.target.closest?.('[data-wb-command]');
    if (command && !command.disabled) execute(command.dataset.wbCommand).catch(failed);
  }, true);
  document.addEventListener('keydown', event => {
    if (context().blocked) return;
    if (event.key === 'Alt' && !event.ctrlKey && !event.metaKey) { altPending = true; return; }
    altPending = false;
    if (event.altKey && !event.ctrlKey && !event.metaKey) {
      const letter = event.code?.startsWith('Key') ? event.code.slice(3).toLowerCase() : event.key.toLowerCase();
      const index = ['f', 'e', 'v', 'w', 'h'].indexOf(letter);
      if (index >= 0) { event.preventDefault(); openMenu(menus[index], true); return; }
    }
    if (event.key === 'F10' && !event.shiftKey) {
      event.preventDefault();
      if (menuMode) closeMenu(true); else { enterMenuMode(); focusBar(0); }
      return;
    }
    const trigger = event.target.closest?.('[data-wb-menu]');
    if (!menuMode && !trigger) return;
    if (trigger) barIndex = menus.indexOf(trigger.dataset.wbMenu);
    if (menuMode && !menu && event.key.length === 1 && !event.ctrlKey && !event.metaKey) {
      const mnemonic = ['f', 'e', 'v', 'w', 'h'].indexOf(event.key.toLowerCase());
      if (mnemonic >= 0) { event.preventDefault(); openMenu(menus[mnemonic], true); return; }
    }
    const items = menuItems(); const index = items.indexOf(document.activeElement);
    if (event.key === 'Escape') {
      event.preventDefault(); if (menu) closeDropDown(true); else closeMenu(true);
    }
    else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      if (!menu) { openMenu(menus[barIndex], event.key === 'ArrowUp' ? 'last' : true); return; }
      const next = index < 0 ? (event.key === 'ArrowDown' ? 0 : items.length - 1) : (index + (event.key === 'ArrowDown' ? 1 : items.length - 1)) % items.length;
      highlightItem(next);
    } else if (event.key === 'ArrowRight' || event.key === 'ArrowLeft') {
      event.preventDefault();
      const next = (barIndex + (event.key === 'ArrowRight' ? 1 : menus.length - 1)) % menus.length;
      if (menu) openMenu(menus[next], true); else { enterMenuMode(); focusBar(next); }
    } else if (event.key === 'Home' || event.key === 'End') {
      event.preventDefault(); if (menu) highlightItem(event.key === 'Home' ? 0 : items.length - 1); else focusBar(event.key === 'Home' ? 0 : menus.length - 1);
    } else if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      if (!menu) openMenu(menus[barIndex], true);
      else if (index >= 0) execute(items[index].dataset.wbCommand).catch(failed);
      else highlightItem(0);
    } else if (event.key === 'Tab') closeMenu(true);
    else if (event.ctrlKey || event.metaKey) closeMenu(true);
    else if (menu && event.key.length === 1 && !event.altKey) {
      event.preventDefault();
      const matches = items.map((item, i) => ({ item, i })).filter(({ item }) => item.textContent.trim().toLowerCase().startsWith(event.key.toLowerCase()));
      const next = matches.find(match => match.i > index) || matches[0];
      if (next) { highlightItem(next.i); if (matches.length === 1) execute(next.item.dataset.wbCommand).catch(failed); }
    }
  });
  document.addEventListener('keyup', event => {
    if (event.key !== 'Alt' || !altPending || context().blocked) return;
    altPending = false; event.preventDefault();
    if (menuMode) closeMenu(true); else { enterMenuMode(); focusBar(0); }
  });
  $('menubar').innerHTML = menus.map((name, index) => `<button role="menuitem" data-wb-menu="${name}" aria-haspopup="menu" aria-expanded="false" tabindex="${index ? '-1' : '0'}">${name}</button>`).join('');
  restore(); renderActivity();
  return {
    restore, refresh, refreshCommands, inspect, selected, log, execute, togglePane, closeMenu,
    isExpanded: id => layout.expanded.includes(id), toggleTree,
    pending: () => layoutWrites > 0,
    menuOpen: () => !!menu,
    snapshot: () => ({ ...layout, expanded: [...layout.expanded] }),
    reset: async () => { layout = defaults(); selection = null; applyLayout(); treeChanged(); renderInspector(); await layoutChanged(); },
    filterActivity: value => { errorsOnly = value === 'errors'; renderActivity(); },
    clearActivity: () => { messages = []; renderActivity(); },
    updateDocumentStats: (html, updated) => {
      if ($('property-words')) $('property-words').textContent = String(plain(html).split(/\s+/).filter(Boolean).length);
      if (updated && $('property-saved')) $('property-saved').textContent = updated.slice(0, 16).replace('T', ' ');
    },
  };
}
