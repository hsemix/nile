// Pure domain rules shared by the application and the integration checks.
export const statuses = ['To do', 'In progress', 'Done'];
export const priorities = ['Low', 'Normal', 'High'];
export const escape = value => String(value == null ? '' : value).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
export const uid = () => Date.now().toString(36) + '-' + Math.random().toString(36).slice(2, 12);
export const validId = value => typeof value === 'string' && /^[a-zA-Z0-9_-]{1,128}$/.test(value);
export const today = () => {
  const date = new Date();
  return date.getFullYear() + '-' + String(date.getMonth() + 1).padStart(2, '0') + '-' + String(date.getDate()).padStart(2, '0');
};
export function title(value) {
  const result = String(value || '').trim();
  if (!result || result.length > 160) throw new Error('Use a title between 1 and 160 characters.');
  return result;
}
export function dueDate(value) {
  if (!value) return '';
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value) || !Number.isFinite(Date.parse(value + 'T12:00:00Z')) || new Date(value + 'T12:00:00Z').toISOString().slice(0, 10) !== value) throw new Error('Enter a valid deadline.');
  return value;
}
export function plain(html) { return String(html).replace(/<[^>]*>/g, ' ').replace(/&[a-z#0-9]+;/gi, ' ').replace(/\s+/g, ' ').trim(); }
export function checkNoteSize(html, name) {
  if (new TextEncoder().encode(JSON.stringify({ html, title: name, search_text: plain(html) })).length > 650000) throw new Error('This note exceeds the 650 KB storage limit. Split it into smaller notes or attach large images as files.');
}
export function validateBackup(data) {
  if (!data || data.format !== 'northstar-workspace' || data.version !== 1) throw new Error('This is not a supported Northstar backup.');
  for (const table of ['projects', 'tasks', 'notes', 'attachments']) if (!Array.isArray(data[table]) || data[table].length > 100000) throw new Error('Invalid backup table: ' + table);
  const ids = {};
  for (const table of ['projects', 'tasks', 'notes', 'attachments']) {
    ids[table] = new Set();
    for (const row of data[table]) {
      if (!row || !validId(row.id) || ids[table].has(row.id)) throw new Error('Invalid or duplicate record ID.');
      ids[table].add(row.id);
      if (typeof row.title !== 'string' || typeof row.updated !== 'string') throw new Error('Invalid record title or timestamp.');
      if (table === 'attachments') {
        if (!row.title.trim() || row.title.length > 1024) throw new Error('Invalid attachment name.');
      } else if (title(row.title) !== row.title) throw new Error('Invalid record title.');
    }
  }
  for (const project of data.projects) if (![0, 1].includes(project.archived) || typeof project.description !== 'string' || project.description.length > 2000) throw new Error('Invalid project.');
  for (const table of ['tasks', 'notes', 'attachments']) for (const row of data[table]) if (!ids.projects.has(row.project_id)) throw new Error('A backup record refers to a missing project.');
  for (const task of data.tasks) if (!statuses.includes(task.status) || !priorities.includes(task.priority) || typeof task.due !== 'string' || dueDate(task.due) !== task.due) throw new Error('Invalid task.');
  for (const note of data.notes) {
    if (typeof note.html !== 'string') throw new Error('Invalid note.');
    checkNoteSize(note.html, note.title);
  }
  for (const file of data.attachments) {
    if (!Number.isSafeInteger(file.size) || file.size < 0 || file.size > 10 * 1024 * 1024 || typeof file.data !== 'string' || !/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(file.data)) throw new Error('Invalid attachment.');
    if (file.data.length / 4 * 3 - (file.data.endsWith('==') ? 2 : file.data.endsWith('=') ? 1 : 0) !== file.size) throw new Error('Attachment size mismatch.');
  }
  return data;
}
