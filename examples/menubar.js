const menus = document.querySelectorAll('.menu');
let openMenu = null, previousFocus = null, drafts = 0;
function closeMenu(restore = false) {
  for (const menu of menus) { menu.classList.remove('open'); menu.querySelector('[data-menu]').setAttribute('aria-expanded', 'false'); }
  openMenu = null;
  if (restore && previousFocus) previousFocus.focus();
}
function open(menu, focusItem = false) {
  closeMenu(); openMenu = menu; previousFocus = menu.querySelector('[data-menu]');
  menu.classList.add('open'); previousFocus.setAttribute('aria-expanded', 'true');
  if (focusItem) items()[0].focus();
}
function items() { return openMenu ? openMenu.querySelectorAll('[data-command]').filter((b) => !b.disabled) : []; }
function command(name) {
  if (name === 'new') document.getElementById('status').textContent = 'New draft ' + (++drafts) + ' created in memory';
  if (name === 'details') {
    const hidden = document.getElementById('details').classList.toggle('hidden');
    document.getElementById('details-label').textContent = hidden ? 'Show details' : 'Hide details';
    document.getElementById('status').textContent = hidden ? 'Details hidden' : 'Details shown';
  }
  if (name === 'title') { document.title = 'My Nile workspace'; document.getElementById('status').textContent = 'Window title changed'; }
  if (name === 'about') document.getElementById('status').textContent = 'An application-drawn menu using Nile HTML/CSS/JS';
  if (name === 'close') window.close();
  console.log('menu command:', name); closeMenu(true);
}
document.addEventListener('click', (event) => {
  const trigger = event.target.closest('[data-menu]');
  if (trigger) { const menu = trigger.closest('.menu'); if (menu === openMenu) closeMenu(); else open(menu); return; }
  const item = event.target.closest('[data-command]');
  if (item && !item.disabled) { command(item.dataset.command); return; }
  if (!event.target.closest('.menu')) closeMenu();
});
for (const menu of menus) menu.querySelector('[data-menu]').addEventListener('mouseover', () => { if (openMenu && openMenu !== menu) open(menu); });
document.addEventListener('keydown', (event) => {
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'n') { command('new'); event.preventDefault(); return; }
  const trigger = event.target.closest('[data-menu]');
  if (trigger && ['ArrowDown', 'Enter', ' '].includes(event.key)) { open(trigger.closest('.menu'), true); event.preventDefault(); return; }
  if (!openMenu) return;
  const choices = items(), index = choices.indexOf(document.activeElement);
  if (event.key === 'Escape') closeMenu(true);
  else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') choices[(index + (event.key === 'ArrowDown' ? 1 : -1) + choices.length) % choices.length].focus();
  else if (event.key === 'ArrowRight' || event.key === 'ArrowLeft') open(menus[(menus.indexOf(openMenu) + (event.key === 'ArrowRight' ? 1 : -1) + menus.length) % menus.length], true);
  else if (event.key === 'Enter' || event.key === ' ') { const item = event.target.closest('[data-command]'); if (item) command(item.dataset.command); }
  else return;
  event.preventDefault();
});
console.log('menubar ready');
