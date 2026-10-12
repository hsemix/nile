// User workflows exercised through rendered DOM controls in Nile's QuickJS host.
import { show, apps } from './app.js';
let passed = 0;
function assert(condition, message) { if (!condition) throw new Error(message); passed++; console.log('PASS ' + message); }
function click(action, value) {
  const selector = `[data-action="${action}"]` + (value === undefined ? '' : `[data-value="${value}"]`);
  const el = document.querySelector(selector); if (!el) throw new Error('Missing control: ' + selector); el.click();
}
function input(id, value, type = 'input') {
  const el = document.getElementById(id); if (!el) throw new Error('Missing field: ' + id);
  if (el.type === 'checkbox') el.checked = value; else el.value = value;
  el.dispatchEvent(new Event(type, { bubbles: true }));
}
function text(id) { return document.getElementById(id).textContent; }
try {
  show('settings'); input('settings-name', '<Alex & Maya>'); input('settings-theme', 'Dark', 'change'); input('settings-summaries', true, 'change'); click('settings-save');
  assert(text('preview-name') === '<Alex & Maya>', 'Settings escapes and previews names');
  assert(document.getElementById('settings-preview').classList.contains('dark'), 'Settings theme updates preview');
  assert(apps.settings.saved.summaries, 'Settings saves notification preferences');
  show('dashboard'); click('dashboard-period', 'Month');
  assert(text('dashboard-revenue') === '$99200' || text('dashboard-revenue') === '$99,200', 'Dashboard period updates metrics');
  input('dashboard-team', 'Design', 'change'); assert(apps.dashboard.team === 'Design', 'Dashboard filters by team');
  show('settings'); assert(text('preview-name') === '<Alex & Maya>', 'State survives application switching');
  show('kanban'); click('task-next', 1);
  assert(apps.kanban.tasks.find((t) => t.id === 1).lane === 1, 'Kanban moves task between columns');
  input('task-search', 'impossible match'); assert(document.querySelectorAll('.task').length === 0, 'Kanban filters tasks');
  input('task-search', ''); click('task-add'); click('task-save');
  assert(text('validation').length > 0 && apps.kanban.tasks.length === 6, 'Kanban rejects empty titles');
  input('task-title', 'A <thoughtful> task'); input('task-note', 'Try a new direction'); click('task-save');
  assert(apps.kanban.tasks.length === 7 && text('task-board').includes('A <thoughtful> task'), 'Kanban creates and safely renders task');
  click('task-edit', 7); input('task-title', 'A revised task'); click('task-save');
  assert(text('task-board').includes('A revised task'), 'Kanban edits task');
  show('mail'); click('mail-open', 1); assert(!apps.mail.messages[0].unread, 'Mail marks opened message as read');
  click('mail-star', 1); click('mail-folder', 'Starred'); assert(text('mail-list').includes('A fresh direction'), 'Mail shows starred message');
  click('mail-archive', 1); click('mail-folder', 'Archive'); assert(text('mail-list').includes('A fresh direction'), 'Mail archives message');
  click('mail-compose'); click('mail-send'); assert(text('validation').length > 0, 'Mail validates compose fields');
  input('mail-to', 'Maya'); input('mail-subject', 'A local draft'); input('mail-body', 'Hello from Nile.'); click('close'); click('mail-compose');
  assert(document.getElementById('mail-body').value === 'Hello from Nile.', 'Mail preserves canceled draft');
  click('mail-send'); assert(text('mail-list').includes('A local draft') && apps.mail.folder === 'Sent', 'Mail adds message to local Sent folder');
  show('ide'); input('code-editor', 'const answer = 42;'); click('ide-save'); input('code-editor', 'unsaved'); click('ide-file', 'README.md'); click('ide-file', 'app.js');
  assert(document.getElementById('code-editor').value === 'unsaved', 'IDE preserves unsaved buffers across tabs');
  click('ide-restore'); assert(document.getElementById('code-editor').value === 'const answer = 42;', 'IDE restores saved buffer');
  show('inventory'); input('inventory-search', 'mug'); assert(document.querySelectorAll('.table-row').length === 1, 'Inventory searches products');
  click('inventory-edit', 2); input('product-stock', '-1'); click('inventory-save'); assert(text('validation').length > 0, 'Inventory rejects negative stock');
  input('product-stock', '25'); click('inventory-save'); assert(text('inventory-table').includes('25 units'), 'Inventory edits stock');
  input('inventory-search', ''); click('inventory-next'); assert(text('inventory-table').includes('6–8 of 8'), 'Inventory paginates');
  click('inventory-add'); input('product-name', 'New product'); input('product-sku', 'NS-002'); click('inventory-save');
  assert(text('validation').includes('already'), 'Inventory rejects duplicate SKU');
  input('product-sku', 'NS-009'); input('product-stock', '4'); input('product-price', '12.50'); click('inventory-save');
  assert(apps.inventory.items.length === 9, 'Inventory creates product');
  show('calendar'); click('calendar-day', '2026-10-15'); assert(text('agenda').includes('Design review'), 'Calendar opens selected day agenda');
  click('calendar-add'); input('event-title', 'A better meeting'); input('event-date', '2026-02-30'); click('calendar-save');
  assert(text('validation').length > 0, 'Calendar rejects impossible dates');
  input('event-date', '2026-11-04'); input('event-time', '14:30'); click('calendar-save');
  assert(apps.calendar.month === 10 && text('agenda').includes('A better meeting'), 'Calendar creates event and navigates to month');
  click('calendar-remove', 5); assert(!text('agenda').includes('A better meeting'), 'Calendar removes event');
  click('calendar-prev'); assert(apps.calendar.month === 9, 'Calendar navigates months');
  show('media'); click('media-select', 1); click('media-select', 3); click('media-favorite'); input('media-filter', 'Favorites', 'change');
  assert(document.querySelectorAll('.media-card').length === 4, 'Media favorites selected assets');
  click('media-detail', 1); click('media-detail-favorite'); click('close'); assert(document.querySelectorAll('.media-card').length === 3, 'Media removes favorite through detail dialog');
  input('media-search', 'Blue'); assert(document.querySelectorAll('.media-card').length === 1, 'Media searches collection');
  show('slides'); click('slides-next'); assert(apps.slides.index === 1, 'Presentation navigates slides');
  input('slide-notes', 'Remember this edit'); input('slide-theme', 'Midnight', 'change'); assert(document.querySelector('.slide').classList.contains('dark'), 'Presentation switches theme');
  click('slides-present'); assert(!document.getElementById('slide-notes'), 'Presentation mode hides speaker notes');
  click('slides-present'); assert(document.getElementById('slide-notes').value === 'Remember this edit', 'Speaker notes survive theme and mode changes');
  document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true })); assert(apps.slides.index === 2, 'Presentation responds to arrow keys');
  show('dashboard');
  console.log('WORKBENCH CHECKS PASSED: ' + passed);
} catch (error) { console.log('WORKBENCH CHECKS FAILED: ' + error.message); throw error; }
