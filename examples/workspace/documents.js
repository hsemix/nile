import { fs, spawn, exepath } from '@sys';
import { escape, uid } from './model.js';
import { dataDir } from './store.js';

// Keep imported/restored note markup inert. No scripts, event handlers, remote
// assets, file URLs, embedded frames or arbitrary CSS enter the editor/report.
export function cleanHtml(html) {
  const allowed = new Set('p div span br h1 h2 h3 h4 b strong i em u s strike sub sup ul ol li blockquote pre code hr table thead tbody tr td th a img'.split(' '));
  const input = String(html).replace(/<!--[\s\S]*?-->/g, '').replace(/<(script|style|iframe|object)\b[^>]*>[\s\S]*?<\/\1\s*>/gi, '');
  const text = value => value.replace(/&(?!(?:amp|lt|gt|quot|apos|nbsp|#\d{1,7}|#x[a-f0-9]{1,6});)/gi, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
  const renderTag = (tag, name, attributes) => {
    name = name.toLowerCase();
    if (!allowed.has(name)) return '';
    if (tag.startsWith('</')) return '</' + name + '>';
    let output = '<' + name;
    const style = attributes.match(/\bstyle\s*=\s*(["'])(.*?)\1/i);
    if (style) {
      const safe = style[2].split(';').filter(rule => /^(color|background-color|font-size|font-family|font-weight|font-style|text-align|text-decoration|margin-left|padding-left)\s*:\s*[a-z0-9\s#.,%()-]+$/i.test(rule.trim()) && !/url|expression|var\s*\(/i.test(rule)).join(';');
      if (safe) output += ' style="' + escape(safe) + '"';
    }
    if (name === 'img') {
      const source = attributes.match(/\bsrc\s*=\s*(["'])(data:image\/(?:png|jpeg|webp|gif);base64,[a-z0-9+/=]+)\1/i);
      if (!source) return '';
      output += ' src="' + source[2] + '"';
    }
    for (const key of ['colspan', 'rowspan']) {
      const match = attributes.match(new RegExp('\\b' + key + '\\s*=\\s*["\']?(\\d{1,2})', 'i'));
      if (match) output += ' ' + key + '="' + match[1] + '"';
    }
    return output + '>';
  };
  // Escape every unmatched fragment, so removing a malformed tag cannot join
  // surrounding characters into a newly executable tag.
  const pattern = /<\/?([a-z][a-z0-9:-]*)\b([^>]*)>/gi;
  let result = '';
  let cursor = 0;
  let match;
  while ((match = pattern.exec(input))) {
    result += text(input.slice(cursor, match.index));
    result += renderTag(match[0], match[1], match[2]);
    cursor = pattern.lastIndex;
  }
  return result + text(input.slice(cursor));
}
export function reportHtml(project, state) {
  const tasks = state.tasks.filter(row => row.project_id === project.id);
  const notes = state.notes.filter(row => row.project_id === project.id);
  const attachments = state.attachments.filter(row => row.project_id === project.id);
  return '<!doctype html><html><head><meta charset="utf-8"><title>' + escape(project.title) + '</title><style>' +
    '@page { size:A4; margin:20mm; @bottom-center { content:"Page " counter(page) " of " counter(pages); font-size:9pt; } }' +
    'body{font:12px sans-serif;color:#273345}h1{font-size:30px}h2{font-size:20px;margin-top:28px}h3{font-size:16px}p{line-height:1.6}table{border-collapse:collapse;width:100%}td,th{border:1px solid #dce3ee;padding:9px;text-align:left}thead{background:#edf2fa}img{max-width:100%}.meta{color:#7c899a;font-size:10px}.note{break-before:page}tr{break-inside:avoid}' +
    '</style></head><body><p class="meta">NORTHSTAR / PROJECT REPORT · ' + escape(new Date().toLocaleDateString()) + '</p><h1>' + escape(project.title) + '</h1><p>' + escape(project.description) + '</p><h2>Tasks</h2><table><thead><tr><th>Task</th><th>Status</th><th>Priority</th><th>Due</th></tr></thead><tbody>' +
    tasks.map(task => '<tr><td>' + escape(task.title) + '</td><td>' + escape(task.status) + '</td><td>' + escape(task.priority) + '</td><td>' + escape(task.due || '—') + '</td></tr>').join('') + '</tbody></table><h2>Attachments</h2>' +
    (attachments.length ? '<ul>' + attachments.map(file => '<li>' + escape(file.title) + ' (' + Math.ceil(file.size / 1024) + ' KB)</li>').join('') + '</ul>' : '<p>No attachments.</p>') +
    notes.map(note => '<div class="note"><h2>' + escape(note.title) + '</h2>' + cleanHtml(note.html) + '</div>').join('') + '</body></html>';
}
export async function exportPdf(destination, html) {
  const source = dataDir + '/report-' + uid() + '.html';
  const temporary = dataDir + '/report-' + uid() + '.pdf';
  await fs.writeFile(source, html);
  try {
    // Reuse the engine's vector PDF renderer in a child process; no shell parsing.
    const process = spawn([exepath(), source, '--pdf', temporary], { stdin: 'ignore', stdout: 'pipe', stderr: 'pipe' });
    const drain = async stream => { let result = ''; let chunk; while ((chunk = await stream.read(4096)) !== undefined) result += new TextDecoder().decode(chunk); stream.close(); return result; };
    const [result, output, errors] = await Promise.all([process.wait(), drain(process.stdout), drain(process.stderr)]);
    if (result.exit_status !== 0 || !fs.statSync(temporary)) throw new Error('PDF export failed: ' + (errors || output));
    await fs.copyfile(temporary, destination);
  } finally {
    for (const file of [source, temporary]) if (fs.statSync(file)) await fs.unlink(file);
  }
}
