import styles from './styles.js';
import { escape, notice } from './core.js';
import { settings } from './settings.js';
import { dashboard } from './dashboard.js';
import { kanban } from './kanban.js';
import { mail } from './mail.js';
import { ide } from './ide.js';
import { inventory } from './inventory.js';
import { calendar } from './calendar.js';
import { media } from './media.js';
import { slides } from './slides.js';
export const apps = { settings, dashboard, kanban, mail, ide, inventory, calendar, media, slides };
const labels = { settings: 'Settings', dashboard: 'Dashboard', kanban: 'Kanban board', mail: 'Mail', ide: 'Code studio', inventory: 'Inventory', calendar: 'Calendar', media: 'Media library', slides: 'Presentations' };
const sheet = document.createElement('style'); sheet.textContent = styles; document.head.appendChild(sheet);
document.body.innerHTML = `<div class="shell"><div class="sidebar"><div class="brand">northstar<span class="brand-dot">.</span></div><div class="brand-caption">NILE APPLICATION STUDIO</div><div class="nav-caption">YOUR WORKSPACE</div><div class="app-nav">${Object.keys(apps).map((key, i) => `<button data-app="${key}"><span class="nav-number">0${i + 1}</span>${labels[key]}</button>`).join('')}</div><div class="sidebar-footer">Nine interfaces. One engine.<br>Local sample data · session only</div></div><div class="main"><div class="topbar"><div class="breadcrumb">Workspace / <b id="breadcrumb"></b></div><div class="row"><span class="badge">Local demo</span><span class="avatar">AD</span></div></div><div class="content" id="content"></div></div></div><div class="statusbar"><span>Nile · HTML / CSS / JavaScript</span><span id="notice">Ready</span></div>`;
export let current = 'settings';
export function render() {
  const app = apps[current];
  document.getElementById('content').innerHTML = `<div class="page-heading"><div class="eyebrow">NORTHSTAR / ${escape(labels[current].toUpperCase())}</div><h1>${escape(app.title)}</h1><div class="muted">${escape(app.subtitle)}</div></div>${app.render()}`;
}
export function show(key) {
  if (!apps[key]) key = 'settings'; current = key; document.body.dataset.current = key;
  document.querySelectorAll('[data-app]').forEach((b) => b.classList.toggle('active', b.dataset.app === key));
  document.getElementById('breadcrumb').textContent = labels[key];
  render(); document.getElementById('content').scrollTop = 0; notice(labels[key] + ' ready');
}
document.addEventListener('click', (e) => {
  const nav = e.target.closest('[data-app]'); if (nav) { show(nav.dataset.app); return; }
  const b = e.target.closest('[data-action]'); if (!b || b.disabled) return;
  if (apps[current].action?.(b.dataset.action, b.dataset.value)) render();
});
for (const type of ['input', 'change']) document.addEventListener(type, (e) => {
  // Text controls use input; select/checkbox controls use change.
  const choice = e.target.localName === 'select' || e.target.type === 'checkbox';
  if ((type === 'change') !== choice) return;
  if (apps[current].input?.(e.target.id, e)) render();
});
document.addEventListener('keydown', (e) => {
  if (current !== 'slides' || ['input', 'textarea', 'select'].includes(e.target.localName)) return;
  if (e.key === 'ArrowRight' || e.key === 'ArrowLeft') { apps.slides.action(e.key === 'ArrowRight' ? 'slides-next' : 'slides-prev'); render(); e.preventDefault(); }
  if (e.key === 'Escape' && apps.slides.presenting) { apps.slides.action('slides-present'); render(); }
});
show(document.body.dataset.start || 'settings');
console.log('workbench ready: ' + Object.keys(apps).join(', '));
