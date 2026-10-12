// An interactive ribbon design demo, not a rich-text editor. Formatting targets
// the Overview paragraph so commands have a visible result without contenteditable.
const sample = document.getElementById('sample');
const status = document.getElementById('status');
const report = (text) => { status.textContent = text; console.log('ribbon:', text); };
let formatting = { bold: false, italic: false, underline: false, strike: false, highlight: false, color: false, align: 'left', size: 15, font: 'serif', indent: 0, spacing: 1.7, border: false };
let history = [], future = [], zoom = 100;
function checkpoint() { history.push(JSON.stringify(formatting)); future = []; }
function apply() {
  sample.style.fontWeight = formatting.bold ? '700' : '400';
  sample.style.fontStyle = formatting.italic ? 'italic' : 'normal';
  sample.style.textDecoration = [formatting.underline ? 'underline' : '', formatting.strike ? 'line-through' : ''].join(' ').trim() || 'none';
  sample.style.backgroundColor = formatting.highlight ? '#fff0aa' : 'transparent';
  sample.style.color = formatting.color ? '#b64449' : '#4d5563';
  sample.style.textAlign = formatting.align;
  sample.style.fontSize = formatting.size + 'px';
  sample.style.fontFamily = formatting.font;
  sample.style.marginLeft = formatting.indent + 'px';
  sample.style.lineHeight = String(formatting.spacing);
  sample.style.border = formatting.border ? '1px solid #91a2c6' : '0';
  document.querySelectorAll('[data-format]').forEach((b) => b.classList.toggle('selected', formatting[b.dataset.format]));
  document.querySelectorAll('[data-align]').forEach((b) => b.classList.toggle('selected', b.dataset.align === formatting.align));
}
const menus = {
  File: [['Document', [['▤', 'New copy', 'new'], ['▣', 'Save', 'save'], ['▧', 'Print preview', 'preview']]], ['About', [['W', 'Ribbon demo', 'about']]]],
  Insert: [['Pages', [['▤', 'Cover page', 'cover'], ['+', 'Page break', 'page-break']]], ['Illustrations', [['▦', 'Table', 'table'], ['▧', 'Picture', 'picture'], ['○', 'Shapes', 'shapes']]], ['Text', [['Aa', 'Text box', 'text-box'], ['Ω', 'Symbol', 'symbol']]]],
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
document.querySelectorAll('[data-format]').forEach((button) => button.addEventListener('click', () => {
  checkpoint(); const key = button.dataset.format; formatting[key] = !formatting[key]; apply(); report(key + (formatting[key] ? ' on' : ' off') + ' · Overview');
}));
document.querySelectorAll('[data-align]').forEach((button) => button.addEventListener('click', () => {
  checkpoint(); formatting.align = button.dataset.align; apply(); report('Align ' + formatting.align + ' · Overview');
}));
document.querySelectorAll('[data-style]').forEach((button) => button.addEventListener('click', () => {
  checkpoint(); const style = button.dataset.style;
  formatting.size = style === 'title' ? 28 : style === 'heading' ? 22 : 15;
  formatting.bold = style === 'heading'; formatting.color = style !== 'normal'; apply();
  document.querySelectorAll('[data-style]').forEach((b) => b.classList.toggle('selected', b === button));
  report(style + ' style · Overview');
}));
document.getElementById('font-family').addEventListener('change', (e) => { checkpoint(); formatting.font = e.target.value; apply(); report('Font changed · Overview'); });
document.getElementById('font-size').addEventListener('change', (e) => { checkpoint(); formatting.size = +e.target.value; apply(); report('Font size ' + formatting.size + ' · Overview'); });
document.getElementById('autosave').addEventListener('click', (e) => { const off = e.currentTarget.classList.toggle('off'); report('AutoSave ' + (off ? 'off' : 'on') + ' · design preview'); });
document.querySelectorAll('[data-section]').forEach((b) => b.addEventListener('click', () => {
  document.getElementById(b.dataset.section).scrollIntoView();
  document.querySelectorAll('[data-section]').forEach((n) => n.classList.toggle('current', n === b));
  report(b.textContent);
}));
const words = () => document.getElementById('paper').textContent.trim().split(/\s+/).length;
document.getElementById('word-count').textContent = words() + ' words';
document.getElementById('find-input').addEventListener('input', (e) => {
  const query = e.target.value.toLowerCase();
  const matches = query ? document.getElementById('paper').textContent.toLowerCase().split(query).length - 1 : 0;
  report(query ? matches + ' matches for “' + e.target.value + '”' : 'Ready');
});
document.addEventListener('click', (e) => {
  const button = e.target.closest('[data-action]'); if (!button) return;
  const action = button.dataset.action;
  if (action === 'undo' || action === 'redo') {
    const from = action === 'undo' ? history : future, to = action === 'undo' ? future : history;
    if (from.length) { to.push(JSON.stringify(formatting)); formatting = JSON.parse(from.pop()); apply(); report(action + ' formatting'); }
    else report('Nothing to ' + action);
  } else if (action === 'grow' || action === 'shrink') {
    checkpoint(); formatting.size = Math.max(10, Math.min(32, formatting.size + (action === 'grow' ? 1 : -1))); apply(); report('Font size ' + formatting.size);
  } else if (action === 'clear') {
    checkpoint(); formatting = { bold: false, italic: false, underline: false, strike: false, highlight: false, color: false, align: 'left', size: 15, font: 'serif', indent: 0, spacing: 1.7, border: false }; apply(); report('Formatting cleared');
  } else if (action === 'indent' || action === 'outdent' || action === 'spacing' || action === 'borders') {
    checkpoint();
    if (action === 'spacing') formatting.spacing = formatting.spacing === 1.7 ? 2 : 1.7;
    else if (action === 'borders') formatting.border = !formatting.border;
    else formatting.indent = Math.max(0, formatting.indent + (action === 'indent' ? 16 : -16));
    apply(); report(action + ' · Overview');
  } else if (action.indexOf('zoom-') === 0) {
    zoom = action === 'zoom-reset' ? 100 : Math.max(70, Math.min(130, zoom + (action === 'zoom-in' ? 10 : -10)));
    document.getElementById('paper').style.transform = 'scale(' + zoom / 100 + ')';
    document.getElementById('paper').style.transformOrigin = 'top center';
    document.getElementById('zoom-label').textContent = zoom + '%'; report('Zoom ' + zoom + '%');
  } else if (action === 'navigation' || action === 'find' || action === 'contents') {
    const nav = document.getElementById('navigation'); nav.classList.toggle('hidden', action === 'navigation' ? !nav.classList.contains('hidden') : false);
    if (action === 'find') document.getElementById('find-input').focus(); report('Navigation');
  } else if (action === 'ruler') {
    document.querySelector('.ruler').classList.toggle('hidden'); report('Ruler toggled');
  } else if (action === 'word-count') report(words() + ' words in document');
  else report(button.textContent.trim() + ' · design preview');
});
console.log('word ribbon ready: Home, Insert, Layout, References, Review, View, File');
