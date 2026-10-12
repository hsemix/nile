import { button, badge, toolbar, escape, modal, notice, empty } from './core.js';
export const media = {
  title: 'Media library', subtitle: 'A collection of things worth keeping.', query: '', filter: 'All assets', selected: [], detail: 0,
  items: [
    { id: 1, title: 'Quiet morning', kind: 'Photos', size: '640 × 480', color: '#bac9ae', image: '../images/photo.png', favorite: false },
    { id: 2, title: 'Sage study', kind: 'Artwork', size: 'CSS composition', color: '#8dbba7', favorite: true },
    { id: 3, title: 'Blue hour', kind: 'Artwork', size: 'CSS composition', color: '#8cabc3', favorite: false },
    { id: 4, title: 'Warm terracotta', kind: 'Artwork', size: 'CSS composition', color: '#cd9e86', favorite: false },
    { id: 5, title: 'Paper texture', kind: 'Textures', size: 'Local tile', color: '#d7c6a5', image: '../images/tile.png', favorite: true },
    { id: 6, title: 'A little sunshine', kind: 'Artwork', size: 'CSS composition', color: '#d5be7b', favorite: false }
  ],
  filtered() { return this.items.filter((i) => i.title.toLowerCase().includes(this.query.toLowerCase()) && (this.filter === 'All assets' || this.filter === 'Favorites' && i.favorite || i.kind === this.filter)); },
  art(i, detail = false) { return i.image ? `<div class="${detail ? 'detail-art' : 'media-image'}"><img src="${i.image}" alt="${escape(i.title)}"></div>` : `<div class="art${detail ? ' detail-art' : ''}" style="background:${i.color}"><div class="${i.id % 2 ? 'art-disc' : 'art-block'}"></div></div>`; },
  grid() {
    return this.filtered().map((i) => `<div class="card media-card${this.selected.includes(i.id) ? ' selected' : ''}">${this.art(i)}<div class="media-caption"><div class="row between"><h3>${escape(i.title)}</h3>${i.favorite ? badge('Favorite') : ''}</div><div class="muted tiny">${i.kind} · ${i.size}</div><div class="row between" style="margin-top:13px">${button(this.selected.includes(i.id) ? 'Selected' : 'Select', 'media-select', i.id, this.selected.includes(i.id) ? 'active' : '')}${button('Details', 'media-detail', i.id)}</div></div></div>`).join('') || empty('No assets match this filter');
  },
  render() {
    const i = this.items.find((x) => x.id === this.detail);
    return `${toolbar(`<input id="media-search" placeholder="Search your collection" value="${escape(this.query)}"><select id="media-filter">${['All assets', 'Photos', 'Artwork', 'Textures', 'Favorites'].map((f) => `<option${this.filter === f ? ' selected' : ''}>${f}</option>`).join('')}</select>`, `<span id="media-count" class="muted tiny">${this.selected.length} selected</span>${button('Favorite selected', 'media-favorite')}${button('Clear selection', 'media-clear')}`)}<div class="gallery" id="media-grid">${this.grid()}</div><div class="muted tiny" style="margin-top:20px">Existing local images and CSS artwork. No downloads or uploads.</div>${i ? modal(i.title, this.art(i, true) + `<div class="row between">${badge(i.kind)}<span class="muted">${i.size}</span></div><p class="muted" style="margin-top:16px">${i.image ? 'An existing image bundled with Nile examples.' : 'A geometric study rendered directly with CSS.'}</p>`, 'media-detail-favorite', i.favorite ? 'Remove favorite' : 'Add favorite') : ''}`;
  },
  input(id, e) { const key = { 'media-search': 'query', 'media-filter': 'filter' }[id]; if (key) { this[key] = e.target.value; document.getElementById('media-grid').innerHTML = this.grid(); } },
  action(a, v) {
    if (a === 'media-select') { const id = +v; this.selected = this.selected.includes(id) ? this.selected.filter((x) => x !== id) : [...this.selected, id]; return true; }
    if (a === 'media-detail') { this.detail = +v; return true; }
    if (a === 'close') { this.detail = 0; return true; }
    if (a === 'media-favorite') { if (!this.selected.length) { notice('Select one or more assets first'); return false; } this.items.forEach((i) => { if (this.selected.includes(i.id)) i.favorite = true; }); notice(this.selected.length + ' assets favorited'); return true; }
    if (a === 'media-detail-favorite') { const i = this.items.find((x) => x.id === this.detail); i.favorite = !i.favorite; notice(i.favorite ? 'Added to favorites' : 'Favorite removed'); return true; }
    if (a === 'media-clear') { this.selected = []; notice('Selection cleared'); return true; }
  }
};
