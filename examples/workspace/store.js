import sqlite from '@sqlite';
import { fs, cwd } from '@sys';
import { path, variable } from '@env';
import { uid, validId, title, dueDate, plain, checkNoteSize, statuses, priorities, validateBackup } from './model.js';

const configuredDir = variable('NORTHSTAR_DATA_DIR') || path('appdata', 'Nile/Northstar');
export const dataDir = /^(?:\/|[A-Za-z]:[\\/]|\\\\)/.test(configuredDir) ? configuredDir : cwd() + '/' + configuredDir;
let db;
let queue = Promise.resolve();
export function serial(action) {
  const result = queue.then(action);
  queue = result.catch(() => {});
  return result;
}
const stamp = () => new Date().toISOString();
const columns = {
  projects: ['id', 'title', 'description', 'archived', 'updated'],
  tasks: ['id', 'project_id', 'title', 'status', 'priority', 'due', 'updated'],
  notes: ['id', 'project_id', 'title', 'html', 'search_text', 'updated'],
  attachments: ['id', 'project_id', 'title', 'path', 'size', 'updated']
};
async function insert(table, row) {
  const fields = columns[table];
  await db.execute('INSERT INTO ' + table + ' (' + fields.join(',') + ') VALUES (' + fields.map(() => '?').join(',') + ')', fields.map(key => row[key]));
}
async function readAll(table) {
  const rows = [];
  let after = '';
  const count = table === 'notes' ? 1 : 25;
  while (true) {
    const page = await db.query('SELECT * FROM ' + table + ' WHERE id>? ORDER BY id LIMIT ' + count, [after]);
    rows.push(...page);
    if (page.length < count) break;
    after = page[page.length - 1].id;
  }
  return rows.sort((a, b) => b.updated.localeCompare(a.updated));
}
async function transaction(action) {
  await db.execute('BEGIN IMMEDIATE');
  try { const value = await action(); await db.execute('COMMIT'); return value; }
  catch (error) { await db.execute('ROLLBACK'); throw error; }
}
export async function init() {
  await fs.mkdir(dataDir + '/attachments');
  await fs.mkdir(dataDir + '/drafts');
  db = await sqlite.open(dataDir + '/workspace.db');
  await db.execute('PRAGMA foreign_keys = ON');
  const version = (await db.query('PRAGMA user_version'))[0].user_version;
  if (version > 1) throw new Error('This workspace was created by a newer app.');
  if (version < 1) await transaction(async () => {
    await db.execute('CREATE TABLE projects (id TEXT PRIMARY KEY, title TEXT NOT NULL, description TEXT NOT NULL, archived INTEGER NOT NULL DEFAULT 0, updated TEXT NOT NULL)');
    await db.execute('CREATE TABLE tasks (id TEXT PRIMARY KEY, project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE, title TEXT NOT NULL, status TEXT NOT NULL, priority TEXT NOT NULL, due TEXT NOT NULL, updated TEXT NOT NULL)');
    await db.execute('CREATE TABLE notes (id TEXT PRIMARY KEY, project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE, title TEXT NOT NULL, html TEXT NOT NULL, search_text TEXT NOT NULL, updated TEXT NOT NULL)');
    await db.execute('CREATE TABLE attachments (id TEXT PRIMARY KEY, project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE, title TEXT NOT NULL, path TEXT NOT NULL, size INTEGER NOT NULL, updated TEXT NOT NULL)');
    await db.execute('CREATE INDEX tasks_project ON tasks(project_id)');
    await db.execute('CREATE INDEX notes_project ON notes(project_id)');
    await db.execute('CREATE INDEX attachments_project ON attachments(project_id)');
    await db.execute('CREATE TABLE preferences (key TEXT PRIMARY KEY, value TEXT NOT NULL)');
    await db.execute('PRAGMA user_version = 1');
  });
  return load();
}
export async function load() {
  return serial(async () => {
    const result = {};
    for (const table of Object.keys(columns)) result[table] = await readAll(table);
    const preferences = await db.query('SELECT key, value FROM preferences');
    result.preferences = {};
    for (const row of preferences) { try { result.preferences[row.key] = JSON.parse(row.value); } catch (_) {} }
    return result;
  });
}
export function preference(key, value) { return serial(() => db.execute('INSERT OR REPLACE INTO preferences (key,value) VALUES (?,?)', [key, JSON.stringify(value)])); }
export function saveProject(project) {
  return serial(async () => {
    const row = { id: project.id || uid(), title: title(project.title), description: String(project.description || ''), archived: project.archived ? 1 : 0, updated: stamp() };
    if (row.description.length > 2000) throw new Error('Keep the project description under 2,000 characters.');
    await db.execute('INSERT INTO projects (id,title,description,archived,updated) VALUES (?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET title=excluded.title,description=excluded.description,archived=excluded.archived,updated=excluded.updated', columns.projects.map(key => row[key]));
    return row;
  });
}
export function saveTask(task) {
  return serial(async () => {
    if (!statuses.includes(task.status) || !priorities.includes(task.priority)) throw new Error('Choose a valid task status and priority.');
    const row = { ...task, id: task.id || uid(), title: title(task.title), due: dueDate(task.due), updated: stamp() };
    await db.execute('INSERT INTO tasks (id,project_id,title,status,priority,due,updated) VALUES (?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET title=excluded.title,status=excluded.status,priority=excluded.priority,due=excluded.due,updated=excluded.updated', columns.tasks.map(key => row[key]));
    return row;
  });
}
export function bulkTasks(ids, status) {
  return serial(() => transaction(async () => {
    if (!statuses.includes(status)) throw new Error('Invalid task status.');
    for (const id of ids) await db.execute('UPDATE tasks SET status=?,updated=? WHERE id=?', [status, stamp(), id]);
  }));
}
export function saveNote(note) {
  return serial(async () => {
    const row = { ...note, id: note.id || uid(), title: title(note.title), html: String(note.html || ''), updated: stamp() };
    checkNoteSize(row.html, row.title);
    row.search_text = plain(row.html);
    await db.execute('INSERT INTO notes (id,project_id,title,html,search_text,updated) VALUES (?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET title=excluded.title,html=excluded.html,search_text=excluded.search_text,updated=excluded.updated', columns.notes.map(key => row[key]));
    return row;
  });
}
export function remove(table, id) {
  if (!columns[table]) return Promise.reject(new Error('Unknown record type.'));
  // Managed blobs remain until cleanup/restore; removal never destroys external files.
  return serial(() => db.execute('DELETE FROM ' + table + ' WHERE id=?', [id]));
}
export function search(query) {
  return serial(() => db.query("SELECT 'project' AS kind,id,id AS project_id,title FROM projects WHERE title LIKE ? OR description LIKE ? UNION ALL SELECT 'task',id,project_id,title FROM tasks WHERE title LIKE ? UNION ALL SELECT 'note',id,project_id,title FROM notes WHERE title LIKE ? OR search_text LIKE ? UNION ALL SELECT 'attachment',id,project_id,title FROM attachments WHERE title LIKE ? LIMIT 100", Array(6).fill('%' + query.replace(/[\\%_]/g, ' ') + '%')));
}
export function attach(projectId, source) {
  return serial(async () => {
    const stat = await fs.stat(source);
    if (!stat.isFile || stat.st_size > 10 * 1024 * 1024) throw new Error('Choose a file up to 10 MB.');
    const id = uid();
    const destination = dataDir + '/attachments/' + id;
    await fs.copyfile(source, destination);
    const copied = await fs.stat(destination);
    if (copied.st_size > 10 * 1024 * 1024) { await fs.unlink(destination); throw new Error('The copied file exceeds 10 MB.'); }
    const row = { id, project_id: projectId, title: source.split(/[\\/]/).pop(), path: destination, size: copied.st_size, updated: stamp() };
    try { await insert('attachments', row); } catch (error) { await fs.unlink(destination); throw error; }
    return row;
  });
}
function encode(bytes) {
  let binary = '';
  for (let i = 0; i < bytes.length; i += 8192) binary += String.fromCharCode(...bytes.slice(i, i + 8192));
  return btoa(binary);
}
export async function backup(destination) {
  // Serialize the snapshot and blob reads with mutations.
  return serial(async () => {
    const result = { format: 'northstar-workspace', version: 1, created: stamp() };
    for (const table of Object.keys(columns)) result[table] = await readAll(table);
    for (const row of result.attachments) { row.data = encode(new Uint8Array(await fs.readFile(row.path))); delete row.path; }
    const temporary = destination + '.northstar-' + uid() + '.tmp';
    const json = JSON.stringify(result);
    if (new TextEncoder().encode(json).length > 150 * 1024 * 1024) throw new Error('This workspace exceeds the 150 MB portable backup limit.');
    await fs.writeFile(temporary, json);
    await fs.rename(temporary, destination);
  });
}
export async function readBackup(source) {
  const stat = await fs.stat(source);
  if (!stat.isFile || stat.st_size > 150 * 1024 * 1024) throw new Error('Backup must be a file under 150 MB.');
  return validateBackup(JSON.parse(new TextDecoder().decode(await fs.readFile(source))));
}
export function restore(data) {
  validateBackup(data);
  return serial(async () => {
    // New unique blob names mean rollback leaves the old workspace intact.
    const staged = [];
    try {
      for (const attachment of data.attachments) {
        const destination = dataDir + '/attachments/' + uid();
        const bytes = Uint8Array.from(atob(attachment.data), character => character.charCodeAt(0));
        await fs.writeFile(destination, bytes);
        staged.push({ ...attachment, path: destination });
      }
      await transaction(async () => {
        await db.execute('DELETE FROM projects');
        for (const project of data.projects) await insert('projects', project);
        for (const task of data.tasks) await insert('tasks', task);
        for (const note of data.notes) await insert('notes', { ...note, search_text: plain(note.html) });
        for (const attachment of staged) await insert('attachments', attachment);
        await db.execute('DELETE FROM preferences');
      });
    } catch (error) {
      for (const row of staged) { try { await fs.unlink(row.path); } catch (_) {} }
      throw error;
    }
  });
}
export function writeDraft(note) {
  if (!validId(note.id)) throw new Error('Invalid draft ID.');
  const destination = dataDir + '/drafts/' + note.id + '.json';
  fs.writeFileSync(destination + '.tmp', JSON.stringify(note));
  fs.renameSync(destination + '.tmp', destination);
}
export function discardDraft(id) { if (!validId(id)) throw new Error('Invalid draft ID.'); if (fs.statSync(dataDir + '/drafts/' + id + '.json')) fs.unlinkSync(dataDir + '/drafts/' + id + '.json'); }
export function drafts() {
  return (fs.readdirSync(dataDir + '/drafts') || []).filter(file => file.name.endsWith('.json')).map(file => {
    try { return JSON.parse(new TextDecoder().decode(fs.readFileSync(dataDir + '/drafts/' + file.name))); } catch (_) { return null; }
  }).filter(note => note && validId(note.id));
}
