export const escape = (value) => String(value == null ? '' : value).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
export const button = (text, action, value = '', cls = '') => `<button class="${cls}" data-action="${action}" data-value="${escape(value)}">${escape(text)}</button>`;
export const badge = (text, cls = '') => `<span class="badge ${cls}">${escape(text)}</span>`;
export const empty = (text) => `<div class="empty">${escape(text)}</div>`;
export const field = (label, id, value = '', type = 'text') => `<label class="field"><span>${escape(label)}</span><input id="${id}" type="${type}" value="${escape(value)}"></label>`;
export const select = (label, id, options, value) => `<label class="field">${label ? '<span>' + escape(label) + '</span>' : ''}<select id="${id}" title="${escape(label || id)}">${options.map((o) => `<option value="${escape(o)}"${o === value ? ' selected' : ''}>${escape(o)}</option>`).join('')}</select></label>`;
export const modal = (title, content, saveAction, saveLabel = 'Save') => `<div class="scrim"><div class="dialog"><div class="row between"><h2>${escape(title)}</h2>${button('Close', 'close')}</div>${content}<div class="row dialog-actions">${button('Cancel', 'close')}${button(saveLabel, saveAction, '', 'primary')}</div><div id="validation" class="validation"></div></div></div>`;
export function notice(text) { document.getElementById('notice').textContent = text; console.log('workbench:', text); }
export function value(id) { return document.getElementById(id).value; }
export function invalid(text) { document.getElementById('validation').textContent = text; notice(text); }
export const toolbar = (left, right = '') => `<div class="toolbar row between"><div class="row">${left}</div><div class="row">${right}</div></div>`;
