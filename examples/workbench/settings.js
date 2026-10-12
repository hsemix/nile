import { button, select, field, badge, notice, value } from './core.js';
export const settings = {
  title: 'Settings', subtitle: 'A workspace that feels like yours.',
  state: { name: 'Alex Davis', team: 'Northstar Studio', theme: 'Light', density: 'Comfortable', notifications: true, summaries: false },
  saved: null,
  render() {
    const s = this.state;
    return `<div class="two-columns"><div class="grow"><div class="card section"><div class="row between"><h2>Profile & workspace</h2>${badge('Personal')}</div><div class="field-row">${field('Display name', 'settings-name', s.name)}${field('Workspace', 'settings-team', s.team)}</div><div class="muted">Used in this showcase. Changes stay available when you switch apps.</div></div><div class="card section"><h2>Appearance</h2><div class="setting-row"><div><h3>Color theme</h3><div class="muted">Preview the theme in the panel on the right.</div></div>${select('', 'settings-theme', ['Light', 'Dark'], s.theme)}</div><div class="setting-row"><div><h3>Interface density</h3><div class="muted">Adjust spacing in the settings preview.</div></div>${select('', 'settings-density', ['Comfortable', 'Compact'], s.density)}</div></div><div class="card section"><h2>Notifications</h2><label class="setting-row"><div><h3>Activity updates</h3><div class="muted">Show updates from your team.</div></div><input id="settings-notifications" type="checkbox"${s.notifications ? ' checked' : ''}></label><label class="setting-row"><div><h3>Weekly summary</h3><div class="muted">Include a summary in the preference profile.</div></div><input id="settings-summaries" type="checkbox"${s.summaries ? ' checked' : ''}></label></div><div class="row">${button('Save preferences', 'settings-save', '', 'primary')}${button('Reset defaults', 'settings-reset')}<span class="muted tiny" id="settings-saved">${this.saved ? 'Preferences saved for this session' : 'Not saved yet'}</span></div></div><div class="card side-card"><h2>Live preview</h2><div id="settings-preview" class="preview-card${s.theme === 'Dark' ? ' dark' : ''}"><h3 id="preview-name">${s.name.replace(/</g, '&lt;')}</h3><div class="muted tiny">Your personal workspace</div><div class="preview-line"></div><div class="preview-line short"></div><span id="preview-density" class="badge">${s.density}</span></div><h3>Make it your own</h3><p class="muted">Try a dark preview, turn on notifications, and save your preferences.</p><div class="muted tiny">Session only · no account changes</div></div></div>`;
  },
  input(id, e) {
    const keys = { 'settings-name': 'name', 'settings-team': 'team', 'settings-theme': 'theme', 'settings-density': 'density', 'settings-notifications': 'notifications', 'settings-summaries': 'summaries' };
    const key = keys[id]; if (!key) return;
    this.state[key] = e.target.type === 'checkbox' ? e.target.checked : e.target.value;
    document.getElementById('preview-name').textContent = this.state.name;
    document.getElementById('settings-preview').classList.toggle('dark', this.state.theme === 'Dark');
    document.getElementById('preview-density').textContent = this.state.density;
    document.getElementById('settings-preview').style.padding = this.state.density === 'Compact' ? '12px' : '22px';
    document.getElementById('settings-saved').textContent = 'Unsaved changes';
  },
  action(action) {
    if (action === 'settings-save') { this.state.name = value('settings-name'); this.state.team = value('settings-team'); this.saved = { ...this.state }; notice('Preferences saved for this session'); return true; }
    if (action === 'settings-reset') { this.state = { name: 'Alex Davis', team: 'Northstar Studio', theme: 'Light', density: 'Comfortable', notifications: true, summaries: false }; this.saved = null; notice('Default preferences restored'); return true; }
  }
};
