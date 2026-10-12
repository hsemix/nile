import { button, badge, toolbar, escape, modal, field, select, value, notice, invalid, empty } from './core.js';
export const kanban = {
  title: 'Kanban board', subtitle: 'Small steps. Meaningful progress.', query: '', dialog: false, nextId: 7,
  tasks: [
    { id: 1, title: 'Map the onboarding journey', note: 'Understand the first five minutes.', lane: 0, owner: 'Maya', tag: 'Research' },
    { id: 2, title: 'Explore a warmer color palette', note: 'A little more human, a little less noise.', lane: 0, owner: 'Alex', tag: 'Design' },
    { id: 3, title: 'Build the settings screen', note: 'Profile, appearance and notifications.', lane: 1, owner: 'Leo', tag: 'Build' },
    { id: 4, title: 'Refine the navigation', note: 'Keep the important things close.', lane: 1, owner: 'Maya', tag: 'Design' },
    { id: 5, title: 'Document the component system', note: 'A shared language for the team.', lane: 2, owner: 'Alex', tag: 'Build' },
    { id: 6, title: 'Review the first prototype', note: 'Feedback captured and ready to use.', lane: 2, owner: 'Leo', tag: 'Research' }
  ],
  lanes: ['To do', 'In progress', 'Done'],
  board() {
    return this.lanes.map((name, i) => {
      const tasks = this.tasks.filter((t) => t.lane === i && (t.title + t.owner + t.tag).toLowerCase().includes(this.query.toLowerCase()));
      return `<div class="lane"><div class="row between lane-heading"><h3>${name}</h3>${badge(tasks.length, i === 1 ? 'amber' : '')}</div>${tasks.map((t) => `<div class="card task">${badge(t.tag, t.tag === 'Research' ? 'blue' : '')}<h3 style="margin-top:13px">${escape(t.title)}</h3><p>${escape(t.note)}</p><div class="row between"><span class="task-id">NS-${t.id}</span><span class="avatar">${escape(t.owner.slice(0, 2).toUpperCase())}</span></div><div class="task-actions">${i > 0 ? button('Back', 'task-back', t.id) : '<span></span>'}${button('Edit', 'task-edit', t.id)}${i < 2 ? button('Move next', 'task-next', t.id) : badge('Complete')}</div></div>`).join('') || empty('No tasks here')}</div>`;
    }).join('');
  },
  render() {
    const t = this.dialog && this.tasks.find((x) => x.id === this.dialog);
    return `${toolbar(`<input id="task-search" placeholder="Search tasks or teammates" value="${escape(this.query)}">${badge(this.tasks.length + ' tasks')}`, button('+ New task', 'task-add', '', 'primary'))}<div class="board" id="task-board">${this.board()}</div>${this.dialog ? modal(t ? 'Edit task' : 'New task', field('Task title', 'task-title', t ? t.title : '') + field('Description', 'task-note', t ? t.note : '') + '<div class="field-row">' + select('Owner', 'task-owner', ['Alex', 'Maya', 'Leo'], t ? t.owner : 'Alex') + select('Category', 'task-tag', ['Design', 'Research', 'Build'], t ? t.tag : 'Design') + '</div>', 'task-save') : ''}`;
  },
  input(id, e) { if (id === 'task-search') { this.query = e.target.value; document.getElementById('task-board').innerHTML = this.board(); } },
  action(a, v) {
    if (a === 'task-add') { this.dialog = -1; return true; }
    if (a === 'task-edit') { this.dialog = +v; return true; }
    if (a === 'close') { this.dialog = false; return true; }
    if (a === 'task-save') {
      const title = value('task-title').trim(); if (!title) { invalid('Give the task a title.'); return false; }
      let t = this.tasks.find((x) => x.id === this.dialog);
      if (!t) { t = { id: this.nextId++, lane: 0 }; this.tasks.push(t); }
      Object.assign(t, { title, note: value('task-note'), owner: value('task-owner'), tag: value('task-tag') });
      this.dialog = false; notice('Task saved'); return true;
    }
    if (a === 'task-next' || a === 'task-back') {
      const t = this.tasks.find((x) => x.id === +v); t.lane = Math.max(0, Math.min(2, t.lane + (a === 'task-next' ? 1 : -1))); notice('Moved to ' + this.lanes[t.lane]); return true;
    }
  }
};
