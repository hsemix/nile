// Appended to app.js only inside the disposable integration-test copy.
import testSqlite from '@sqlite';
const checksHeartbeat = setInterval(() => {}, 20);
async function runChecks() {
  const results = [];
  const assert = (condition, message) => { if (!condition) throw new Error(message); results.push(message); };
  const waitReady = async () => { for (let i = 0; i < 100 && !ready; i++) await new Promise(resolve => setTimeout(resolve, 30)); if (!ready) throw new Error('App did not start'); };
  const root = store.dataDir;
  try {
    await waitReady();
    assert(state.projects.length === 0, 'Fresh workspace is empty');
    document.querySelector('[data-action="new-project"]').click();
    for (let i = 0; i < 50 && busy; i++) await new Promise(resolve => setTimeout(resolve, 10));
    assert(!!$('project-title'), 'Click event opens the create-project dialog');
    $('project-title').value = 'Field guide'; $('project-description').value = 'A complete project, from idea to delivery.';
    await action('save-project', '');
    const project = projectFor();
    assert(project.title === 'Field guide' && $('content').textContent.includes('Field guide'), 'Create project through the UI');
    await action('new-task', project.id); $('task-title').value = 'Research the brief'; $('task-priority').value = 'High'; $('task-due').value = '2026-10-25'; await action('save-task', project.id);
    const task = state.tasks[0];
    assert(task.title === 'Research the brief' && task.priority === 'High', 'Create task with priority and deadline');
    await action('edit-task', task.id); $('task-title').value = ''; let invalid = false;
    try { await action('save-task', task.id); } catch (_) { invalid = true; }
    assert(invalid && (await store.load()).tasks[0].title === 'Research the brief', 'Invalid task edit preserves saved data'); dismiss();
    selectedTasks.add(task.id); await action('bulk-done', '');
    assert(state.tasks[0].status === 'Done', 'Bulk task completion persists');
    await action('new-note', project.id); const note = noteFor();
    $('note-title').value = 'Discovery notes'; $('editor').innerHTML = '<h2>What we learned</h2><p><b>Clarity</b> makes work easier. UniqueSearchPhrase</p>';
    captureNote();
    assert(store.drafts().length === 1, 'Editing writes a recovery draft before autosave');
    await new Promise(resolve => setTimeout(resolve, 1100));
    assert(!dirty, 'Autosave timer commits a document without a manual save');
    assert(!dirty && store.drafts().length === 0 && state.notes[0].title === 'Discovery notes', 'Save commits note and clears recovery draft');
    assert((await store.search('UniqueSearchPhrase')).some(row => row.id === note.id), 'SQLite search finds document body');
    const largeNotes = [];
    for (let index = 0; index < 3; index++) largeNotes.push(await store.saveNote({ project_id: project.id, title: 'Large note ' + index, html: '<p>' + 'Large note body '.repeat(14000) + '</p>' }));
    assert((await store.load()).notes.length === 4, 'Paged loading handles notes larger than a single native result packet in total');
    let tooLarge = false;
    try { await store.saveNote({ project_id: project.id, title: 'Too large', html: 'x'.repeat(700000) }); } catch (_) { tooLarge = true; }
    assert(tooLarge && (await store.load()).notes.length === 4, 'Oversized notes are rejected before an unreadable record can be saved');
    for (const largeNote of largeNotes) await store.remove('notes', largeNote.id);
    assert(!cleanHtml('<script>evil()</script><p onclick="evil()">safe</p><img src="https://remote/x"><iframe src="file:///x"></iframe>').includes('evil') && !cleanHtml('<img src="file:///secret">').includes('secret'), 'Imported note markup cannot load scripts or external assets');
    assert(!/<script|<img|onclick|onerror/i.test(cleanHtml('<<script>script>evil()<<script>/script><img src="file:///secret" onerror="evil()">')) && cleanHtml('<p>A &amp; B &lt; C</p>') === '<p>A &amp; B &lt; C</p>', 'Malformed imported markup remains inert and text entities survive');
    await fs.writeFile(root + '/fixture.txt', 'Attachment round trip.');
    const originalPicker = Window.this.selectFile;
    Window.this.selectFile = () => root + '/fixture.txt';
    await action('attach', project.id);
    const attachment = state.attachments[0];
    await fs.writeFile(root + '/fixture.txt', 'Original changed.');
    assert(new TextDecoder().decode(await fs.readFile(attachment.path)) === 'Attachment round trip.', 'Attachments are managed copies');
    await open(projectTab(project.id, 'files')); await previewAttachment(attachment.id);
    assert($('file-preview').textContent.includes('Attachment round trip.'), 'Text attachment preview renders');
    Window.this.selectFile = () => root + '/backup.json';
    await action('backup', ''); const backup = await store.readBackup(root + '/backup.json');
    assert(backup.attachments.length === 1 && backup.attachments[0].data && !backup.attachments[0].path, 'Portable backup contains attachment bytes');
    await store.saveTask({ ...task, status: 'In progress' });
    await action('restore', '');
    assert(!!modal && $('overlay').textContent.includes('Replace this workspace'), 'Restore previews contents and asks before replacement');
    await action('confirm', '');
    assert((await fs.readdir(root)).some(file => file.name.startsWith('before-restore-')), 'UI restore creates a safety backup before replacement');
    assert(state.tasks[0].status === 'Done' && new TextDecoder().decode(await fs.readFile(state.attachments[0].path)) === 'Attachment round trip.', 'Restore atomically replaces records and restores file bytes');
    let rejected = false;
    try { await store.restore({ ...backup, tasks: [{ ...backup.tasks[0], project_id: 'missing' }] }); } catch (_) { rejected = true; }
    assert(rejected && (await store.load()).projects.length === 1, 'Invalid restore leaves workspace intact');
    // A write failure during a restore must roll back the database transaction.
    const broken = JSON.parse(JSON.stringify(backup)); broken.notes[0].html = null;
    rejected = false; try { await store.restore(broken); } catch (_) { rejected = true; }
    assert(rejected && (await store.load()).notes[0].title === 'Discovery notes', 'Malformed backup is rejected before replacement');
    const testDb = await testSqlite.open(root + '/workspace.db');
    await testDb.execute("CREATE TRIGGER reject_restore BEFORE INSERT ON notes BEGIN SELECT RAISE(ABORT, 'forced restore failure'); END");
    await store.saveTask({ ...state.tasks[0], status: 'In progress' });
    rejected = false; try { await store.restore(backup); } catch (_) { rejected = true; }
    assert(rejected && (await store.load()).tasks[0].status === 'In progress' && (await store.load()).notes.length === 1, 'Mid-restore SQL failure rolls back all replacement records');
    await testDb.execute('DROP TRIGGER reject_restore'); await testDb.close();
    await store.restore(backup); await reload();
    await open(projectTab(project.id)); await action('archive', project.id);
    assert(state.projects[0].archived === 1 && !$('projects').textContent.includes('Field guide'), 'Archive hides project from active navigation');
    await action('archive', project.id); assert(state.projects[0].archived === 0, 'Unarchive restores project');
    await action('theme', 'dark'); assert(document.documentElement.getAttribute('data-theme') === 'dark', 'Theme persists');
    Window.this.selectFile = () => root + '/report.pdf';
    await action('report', state.projects[0].id);
    assert(new TextDecoder().decode((await fs.readFile(root + '/report.pdf')).slice(0, 5)) === '%PDF-', 'In-app PDF export creates a PDF through the real renderer');
    Window.this.selectFile = originalPicker;
    await open(noteTab(state.notes[0]));
    await session();
    assert((await store.load()).preferences.session.active === active, 'Open tabs and active document persist');
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', ctrlKey: true, bubbles: true, cancelable: true }));
    await new Promise(resolve => setTimeout(resolve, 30));
    assert(!!modal && $('overlay').textContent.includes('Search everything'), 'Keyboard command opens command palette');
    dismiss();
    // Keep a recoverable edit for the restart check without replacing the saved note.
    store.writeDraft({ ...state.notes[0], html: '<p>Recovered after restart</p>', updated: now() });
    fs.writeFileSync(root + '/checks.json', JSON.stringify({ ok: true, results }));
    console.log('NORTHSTAR CHECKS PASSED: ' + results.length);
  } catch (error) {
    fs.writeFileSync(root + '/checks.json', JSON.stringify({ ok: false, results, error: error.message, stack: error.stack }));
    console.error('NORTHSTAR CHECKS FAILED: ' + error.stack);
  }
  clearInterval(checksHeartbeat);
}
runChecks();
