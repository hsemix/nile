import { button, badge, toolbar, escape, modal, field, value, notice, invalid, empty } from './core.js';
const key = (y, m, d) => `${y}-${String(m + 1).padStart(2, '0')}-${String(d).padStart(2, '0')}`;
const months = ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December'];
export const calendar = {
  title: 'Calendar', subtitle: 'Make time for the things that matter.', year: 2026, month: 9, selected: '2026-10-12', dialog: false, nextId: 5,
  events: [
    { id: 1, date: '2026-10-12', time: '09:30', title: 'Studio stand-up' },
    { id: 2, date: '2026-10-15', time: '14:00', title: 'Design review' },
    { id: 3, date: '2026-10-20', time: '10:00', title: 'Customer interviews' },
    { id: 4, date: '2026-10-23', time: '16:00', title: 'Friday catch-up' }
  ],
  grid() {
    const start = new Date(Date.UTC(this.year, this.month, 1)).getUTCDay();
    const count = new Date(Date.UTC(this.year, this.month + 1, 0)).getUTCDate();
    const cells = Math.ceil((start + count) / 7) * 7;
    return Array.from({ length: cells }, (_, i) => {
      const d = i - start + 1; if (d < 1 || d > count) return '<div class="day other"></div>';
      const date = key(this.year, this.month, d), events = this.events.filter((e) => e.date === date);
      return `<div class="day${date === this.selected ? ' selected' : ''}">${button(String(d), 'calendar-day', date)}${events.slice(0, 1).map((e) => `<button class="calendar-event" data-action="calendar-day" data-value="${date}">${escape(e.title)}</button>`).join('')}${events.length > 1 ? '<div class="tiny muted">+' + (events.length - 1) + ' more</div>' : ''}</div>`;
    }).join('');
  },
  agenda() {
    const events = this.events.filter((e) => e.date === this.selected).sort((a, b) => a.time.localeCompare(b.time));
    return `<div class="row between"><h2>Agenda · ${this.selected}</h2>${badge(events.length + ' events')}</div>${events.map((e) => `<div class="list-row"><div class="row"><span class="badge blue">${e.time}</span><b>${escape(e.title)}</b></div>${button('Remove', 'calendar-remove', e.id, 'danger')}</div>`).join('') || empty('A little breathing room. Add something to your day.')}`;
  },
  render() {
    return `${toolbar(`${button('Previous', 'calendar-prev')}<h2 style="margin:0 10px">${months[this.month]} ${this.year}</h2>${button('Next', 'calendar-next')}`, button('+ Add event', 'calendar-add', '', 'primary'))}<div class="card calendar"><div class="weekdays">${['SUN', 'MON', 'TUE', 'WED', 'THU', 'FRI', 'SAT'].map((d) => '<div>' + d + '</div>').join('')}</div><div class="month-grid" id="month-grid">${this.grid()}</div></div><div class="card agenda" id="agenda">${this.agenda()}</div>${this.dialog ? modal('New event', field('Title', 'event-title') + field('Date (YYYY-MM-DD)', 'event-date', this.selected) + field('Time (HH:MM)', 'event-time', '09:00'), 'calendar-save', 'Add event') : ''}`;
  },
  action(a, v) {
    if (a === 'calendar-prev' || a === 'calendar-next') { this.month += a === 'calendar-next' ? 1 : -1; if (this.month < 0) { this.month = 11; this.year--; } if (this.month > 11) { this.month = 0; this.year++; } this.selected = key(this.year, this.month, 1); notice(months[this.month] + ' ' + this.year); return true; }
    if (a === 'calendar-day') { this.selected = v; notice('Selected ' + v); return true; }
    if (a === 'calendar-add') { this.dialog = true; return true; }
    if (a === 'close') { this.dialog = false; return true; }
    if (a === 'calendar-remove') { this.events = this.events.filter((e) => e.id !== +v); notice('Event removed'); return true; }
    if (a === 'calendar-save') {
      const title = value('event-title').trim(), date = value('event-date').trim(), time = value('event-time').trim();
      const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(date);
      const validDate = match && new Date(Date.UTC(+match[1], +match[2] - 1, +match[3])).toISOString().slice(0, 10) === date;
      if (!title || !validDate || !/^([01]\d|2[0-3]):[0-5]\d$/.test(time)) { invalid('Enter a title, a real date and a time from 00:00 to 23:59.'); return false; }
      this.events.push({ id: this.nextId++, title, date, time }); this.year = +match[1]; this.month = +match[2] - 1; this.selected = date; this.dialog = false; notice('Event added'); return true;
    }
  }
};
