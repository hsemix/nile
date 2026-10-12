import { button, badge, toolbar, escape, notice } from './core.js';
export const slides = {
  title: 'Presentations', subtitle: 'Give your ideas a little more room.', index: 0, theme: 'Sage', presenting: false,
  deck: [
    { title: 'Good ideas deserve a great beginning.', kicker: 'NORTHSTAR / A NEW CHAPTER', text: 'A thoughtful approach to building something useful, beautiful and human.', notes: 'Open with the purpose: we want a simpler, more thoughtful experience.' },
    { title: 'Start with listening.', kicker: '01 / DISCOVER', text: 'Ask better questions. Understand the everyday moments. Find the opportunity hiding in plain sight.', notes: 'Share the research plan: interviews, observation and a review of the current experience.' },
    { title: 'Build with intention.', kicker: '02 / DESIGN', text: 'Explore possibilities together. Make a clear choice. Turn the strongest ideas into something people can try.', notes: 'Introduce the prototype and the three core design principles.' },
    { title: 'Make every detail count.', kicker: '03 / DELIVER', text: 'Refine the experience, test the important paths, and leave room for the next chapter.', notes: 'Close with the timeline and invite questions from the team.' }
  ],
  render() {
    const s = this.deck[this.index];
    return `${toolbar(`<span class="muted tiny">Project proposal</span>${badge(this.deck.length + ' slides')}`, `<select id="slide-theme">${['Sage', 'Midnight', 'Blue'].map((t) => `<option${this.theme === t ? ' selected' : ''}>${t}</option>`).join('')}</select>${button(this.presenting ? 'Exit presentation' : 'Present', 'slides-present', '', 'primary')}`)}<div class="slides-layout${this.presenting ? ' presentation' : ''}"><div class="thumbnails">${this.deck.map((s, i) => `<button class="thumbnail${i === this.index ? ' active' : ''}" data-action="slides-go" data-value="${i}"><small>SLIDE ${i + 1}</small>${escape(s.title)}</button>`).join('')}</div><div class="slide-stage"><div class="slide${this.theme === 'Midnight' ? ' dark' : this.theme === 'Blue' ? ' blue' : ''}"><div class="slide-kicker">${escape(s.kicker)}</div><h2>${escape(s.title)}</h2><p>${escape(s.text)}</p><div class="slide-footer">NORTHSTAR STUDIO <span style="margin-left:30px">${String(this.index + 1).padStart(2, '0')} / 04</span></div></div><div class="row between" style="margin-top:16px"><button data-action="slides-prev"${this.index === 0 ? ' disabled' : ''}>Previous</button><span class="muted tiny">Slide ${this.index + 1} of ${this.deck.length} · Arrow keys to navigate</span><button data-action="slides-next"${this.index === this.deck.length - 1 ? ' disabled' : ''}>Next</button></div>${this.presenting ? '' : `<div class="card slide-notes"><b class="tiny">SPEAKER NOTES</b><label class="field" style="margin-top:12px;margin-bottom:0"><textarea id="slide-notes" style="height:73px">${escape(s.notes)}</textarea></label></div>`}</div></div>`;
  },
  input(id, e) { if (id === 'slide-theme') { this.theme = e.target.value; return true; } if (id === 'slide-notes') this.deck[this.index].notes = e.target.value; },
  action(a, v) {
    if (a === 'slides-go' || a === 'slides-next' || a === 'slides-prev') { this.index = Math.max(0, Math.min(this.deck.length - 1, a === 'slides-go' ? +v : this.index + (a === 'slides-next' ? 1 : -1))); notice('Slide ' + (this.index + 1)); return true; }
    if (a === 'slides-present') { this.presenting = !this.presenting; notice(this.presenting ? 'Presentation mode · Escape to exit' : 'Presentation ended'); return true; }
  }
};
