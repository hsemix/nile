import { button, badge, toolbar, escape, modal, field, value, notice, invalid, empty } from './core.js';
export const mail = {
  title: 'Mail', subtitle: 'A little less noise in your inbox.', folder: 'Inbox', selected: 1, query: '', compose: false, nextId: 5,
  draft: { to: '', subject: '', body: '' },
  messages: [
    { id: 1, from: 'Maya Chen', subject: 'A fresh direction for Northstar', preview: 'The first round of concepts is ready.', body: 'Hi Alex,\n\nThe first round of concepts is ready for a look. We explored a warmer palette, a simpler navigation, and a more focused onboarding experience.\n\nI would love your thoughts before our review on Thursday.\n\nThanks,\nMaya', time: '9:42 AM', folder: 'Inbox', unread: true, star: false },
    { id: 2, from: 'Leo Martin', subject: 'Prototype notes', preview: 'A few things worth trying next.', body: 'The prototype is feeling good. I have captured the usability notes and added three tasks to the board.\n\nThe next step is a short review with the team.', time: '8:16 AM', folder: 'Inbox', unread: true, star: true },
    { id: 3, from: 'Nora Patel', subject: 'Friday studio catch-up', preview: 'Bring one thing you are excited about.', body: 'Let us take a little time on Friday to share what we have been working on. Bring one idea, one question, or one detail you are excited about.', time: 'Yesterday', folder: 'Inbox', unread: false, star: false },
    { id: 4, from: 'Alex Davis', subject: 'Welcome to the project', preview: 'A shared space for our next chapter.', body: 'Welcome, everyone. This is a shared space for our next chapter. I am looking forward to building something thoughtful together.', time: 'Monday', folder: 'Sent', unread: false, star: false }
  ],
  filtered() { return this.messages.filter((m) => (this.folder === 'Starred' ? m.star && m.folder !== 'Archive' : m.folder === this.folder) && (m.from + m.subject + m.body).toLowerCase().includes(this.query.toLowerCase())); },
  list() { return this.filtered().map((m) => `<button class="message${m.id === this.selected ? ' active' : ''}" data-action="mail-open" data-value="${m.id}"><div class="row between"><span>${escape(m.from)}</span><span class="muted tiny">${escape(m.time)}</span></div><strong>${m.unread ? '• ' : ''}${escape(m.subject)}</strong><span class="muted">${escape(m.preview)}</span></button>`).join('') || empty('No messages'); },
  reading() {
    const m = this.filtered().find((x) => x.id === this.selected);
    return m ? `<div class="row between"><span class="eyebrow">${escape(m.folder.toUpperCase())}</span><div class="row">${button(m.star ? 'Unstar' : 'Star', 'mail-star', m.id)}${button('Archive', 'mail-archive', m.id)}</div></div><h2>${escape(m.subject)}</h2><div class="row"><div class="avatar">${escape(m.from.slice(0, 2).toUpperCase())}</div><div><b>${escape(m.from)}</b><div class="muted tiny">${escape(m.time)} · to ${m.folder === 'Sent' ? 'your recipient' : 'you'}</div></div></div><div class="mail-text">${escape(m.body).replace(/\n/g, '<br>')}</div><div style="margin-top:28px">${button('Reply', 'mail-reply', m.id)}</div>` : empty('Choose a message to read');
  },
  render() {
    return `${toolbar(`<input id="mail-search" placeholder="Search messages" value="${escape(this.query)}">${badge(this.messages.filter((m) => m.unread && m.folder === 'Inbox').length + ' unread')}`, button('+ Compose', 'mail-compose', '', 'primary'))}<div class="mail-layout"><div class="folders">${['Inbox', 'Starred', 'Sent', 'Archive'].map((f) => button(f, 'mail-folder', f, this.folder === f ? 'active' : '')).join('')}</div><div class="messages" id="mail-list">${this.list()}</div><div class="mail-body" id="mail-reading">${this.reading()}</div></div>${this.compose ? modal('Compose message', field('To', 'mail-to', this.draft.to) + field('Subject', 'mail-subject', this.draft.subject) + `<label class="field"><span>Message</span><textarea id="mail-body">${escape(this.draft.body)}</textarea></label><div class="muted tiny">Adds to the local Sent folder. No email is transmitted.</div>`, 'mail-send', 'Add to Sent') : ''}`;
  },
  input(id, e) {
    if (id === 'mail-search') { this.query = e.target.value; document.getElementById('mail-list').innerHTML = this.list(); document.getElementById('mail-reading').innerHTML = this.reading(); }
    const key = { 'mail-to': 'to', 'mail-subject': 'subject', 'mail-body': 'body' }[id]; if (key) this.draft[key] = e.target.value;
  },
  action(a, v) {
    if (a === 'mail-folder') { this.folder = v; this.selected = this.filtered()[0]?.id || 0; return true; }
    if (a === 'mail-open') { this.selected = +v; this.messages.find((m) => m.id === +v).unread = false; return true; }
    if (a === 'mail-star' || a === 'mail-archive') { const m = this.messages.find((x) => x.id === +v); if (a === 'mail-star') m.star = !m.star; else { m.folder = 'Archive'; notice('Message archived'); } return true; }
    if (a === 'mail-compose') { this.compose = true; return true; }
    if (a === 'mail-reply') { const m = this.messages.find((x) => x.id === +v); this.draft = { to: m.from, subject: 'Re: ' + m.subject, body: '' }; this.compose = true; return true; }
    if (a === 'close') { this.compose = false; return true; }
    if (a === 'mail-send') {
      const to = value('mail-to').trim(), subject = value('mail-subject').trim(), body = value('mail-body');
      if (!to || !subject || !body.trim()) { invalid('Fill in the recipient, subject and message.'); return false; }
      const m = { id: this.nextId++, from: 'Alex Davis', subject, preview: body.slice(0, 60), body, time: 'Just now', folder: 'Sent', star: false, unread: false, to };
      this.messages.push(m); this.folder = 'Sent'; this.selected = m.id; this.compose = false; this.draft = { to: '', subject: '', body: '' }; notice('Message added to local Sent folder'); return true;
    }
  }
};
