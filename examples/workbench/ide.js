import { button, badge, toolbar, escape, notice } from './core.js';
export const ide = {
  title: 'Code studio', subtitle: 'A quiet place to shape your next idea.', current: 'app.js', tabs: ['app.js', 'theme.css'],
  files: {
    'app.js': { text: "// Welcome to Northstar\nconst project = {\n  name: 'A new beginning',\n  team: ['Alex', 'Maya', 'Leo'],\n  status: 'in progress'\n};\n\nfunction greeting(name) {\n  return 'Hello, ' + name;\n}\n\nconsole.log(greeting(project.team[0]));", saved: null },
    'theme.css': { text: 'body {\n  background: #f3f6f7;\n  color: #263b46;\n  font-family: sans-serif;\n}\n\n.card {\n  border-radius: 9px;\n  padding: 22px;\n}', saved: null },
    'index.html': { text: '<!doctype html>\n<html>\n  <head><title>Northstar</title></head>\n  <body>\n    <h1>A new beginning</h1>\n    <p>Make something thoughtful.</p>\n  </body>\n</html>', saved: null },
    'README.md': { text: '# Northstar Studio\n\nA shared space for thoughtful work.\n\n## First steps\n\n- Listen carefully\n- Build with intention\n- Refine the details', saved: null }
  },
  output: 'Ready. Choose a file and start editing.\nSave keeps a snapshot for this session.',
  status() { const f = this.files[this.current]; return this.current + ' · ' + f.text.split('\n').length + ' lines · ' + f.text.length + ' characters · ' + (f.saved === f.text ? 'Saved in session' : 'Modified'); },
  render() {
    return `${toolbar(`${badge('NORTHSTAR / WORKSPACE', 'blue')}<span class="muted tiny">4 virtual files</span>`, button('Restore saved', 'ide-restore') + button('Save buffer', 'ide-save', '', 'primary'))}<div class="ide"><div class="file-tree"><h3>EXPLORER</h3><div class="muted tiny" style="padding:14px 7px">northstar /</div>${Object.keys(this.files).map((f) => button(f, 'ide-file', f, f === this.current ? 'active' : '')).join('')}</div><div class="editor-main"><div class="editor-tabs">${this.tabs.map((f) => button(f, 'ide-file', f, f === this.current ? 'active' : '')).join('')}</div><textarea id="code-editor" class="code-editor">${escape(this.files[this.current].text)}</textarea><div class="editor-status" id="editor-status">${escape(this.status())}</div><div class="output"><div style="margin-bottom:8px;color:#d3e0e4">OUTPUT</div><div id="editor-output">${escape(this.output).replace(/\n/g, '<br>')}</div></div></div></div><div class="muted tiny" style="margin-top:18px">Plain-text buffers in memory. No code execution, terminal process or disk writes.</div>`;
  },
  input(id, e) { if (id === 'code-editor') { this.files[this.current].text = e.target.value; document.getElementById('editor-status').textContent = this.status(); } },
  action(a, v) {
    if (a === 'ide-file') { this.current = v; if (!this.tabs.includes(v)) this.tabs.push(v); return true; }
    if (a === 'ide-save') { const f = this.files[this.current]; f.text = document.getElementById('code-editor').value; f.saved = f.text; this.output = 'Saved ' + this.current + ' in memory.\n' + f.text.length + ' characters captured.'; notice('Buffer saved for this session'); return true; }
    if (a === 'ide-restore') { const f = this.files[this.current]; if (f.saved !== null) { f.text = f.saved; this.output = 'Restored the saved snapshot of ' + this.current; notice('Saved buffer restored'); } else notice('Save a buffer before restoring it'); return true; }
  }
};
Object.values(ide.files).forEach((f) => { f.saved = f.text; });
