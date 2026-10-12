import { button, badge, toolbar, escape, modal, field, select, value, notice, invalid, empty } from './core.js';
export const inventory = {
  title: 'Inventory', subtitle: 'Everything in its place. Every detail in view.', query: '', category: 'All categories', sort: 'Name', page: 0, dialog: false, nextId: 9,
  items: [
    { id: 1, name: 'Studio notebook', sku: 'NS-001', category: 'Stationery', stock: 48, price: 12 },
    { id: 2, name: 'Ceramic coffee mug', sku: 'NS-002', category: 'Accessories', stock: 8, price: 24 },
    { id: 3, name: 'Cotton tote bag', sku: 'NS-003', category: 'Accessories', stock: 32, price: 18 },
    { id: 4, name: 'Desk organizer', sku: 'NS-004', category: 'Workspace', stock: 15, price: 36 },
    { id: 5, name: 'Mechanical pencil', sku: 'NS-005', category: 'Stationery', stock: 72, price: 9 },
    { id: 6, name: 'Weekly planner', sku: 'NS-006', category: 'Stationery', stock: 6, price: 16 },
    { id: 7, name: 'Linen desk mat', sku: 'NS-007', category: 'Workspace', stock: 21, price: 42 },
    { id: 8, name: 'Brass bookmark', sku: 'NS-008', category: 'Accessories', stock: 0, price: 14 }
  ],
  filtered() { return this.items.filter((i) => i.name.toLowerCase().includes(this.query.toLowerCase()) && (this.category === 'All categories' || i.category === this.category)).sort((a, b) => this.sort === 'Stock' ? a.stock - b.stock : a.name.localeCompare(b.name)); },
  table() {
    const list = this.filtered(); this.page = Math.max(0, Math.min(this.page, Math.ceil(list.length / 5) - 1));
    return `<div class="table-head"><span>PRODUCT</span><span>CATEGORY</span><span>STOCK</span><span>PRICE</span><span>ACTION</span></div>${list.slice(this.page * 5, this.page * 5 + 5).map((i) => `<div class="table-row"><div><b>${escape(i.name)}</b><div class="product-sku">${escape(i.sku)}</div></div><span class="muted tiny">${i.category}</span><span>${badge(i.stock + ' units', i.stock === 0 ? 'red' : i.stock < 10 ? 'amber' : '')}</span><span>$${i.price.toFixed(2)}</span><div>${button('Edit', 'inventory-edit', i.id)}</div></div>`).join('') || ''}${list.length ? '' : empty('No matching products')}<div class="table-foot"><span class="muted tiny">${list.length ? this.page * 5 + 1 : 0}–${Math.min(list.length, this.page * 5 + 5)} of ${list.length} products</span><div class="row"><button data-action="inventory-prev"${this.page === 0 ? ' disabled' : ''}>Previous</button><button data-action="inventory-next"${(this.page + 1) * 5 >= list.length ? ' disabled' : ''}>Next</button></div></div>`;
  },
  render() {
    const i = this.items.find((x) => x.id === this.dialog);
    return `<div class="cards"><div class="card"><div class="metric-label">TOTAL PRODUCTS</div><div class="metric-value">${this.items.length}</div><div class="muted tiny">Across three categories</div></div><div class="card"><div class="metric-label">INVENTORY VALUE</div><div class="metric-value">$${this.items.reduce((a, x) => a + x.stock * x.price, 0).toLocaleString()}</div><div class="metric-change">At listed unit prices</div></div><div class="card"><div class="metric-label">LOW STOCK</div><div class="metric-value">${this.items.filter((x) => x.stock < 10).length}</div><div class="muted tiny">Fewer than 10 units remaining</div></div></div>${toolbar(`<input id="inventory-search" placeholder="Search products" value="${escape(this.query)}"><select id="inventory-category">${['All categories', 'Stationery', 'Accessories', 'Workspace'].map((c) => `<option${c === this.category ? ' selected' : ''}>${c}</option>`).join('')}</select><select id="inventory-sort"><option${this.sort === 'Name' ? ' selected' : ''}>Name</option><option${this.sort === 'Stock' ? ' selected' : ''}>Stock</option></select>`, button('+ Add product', 'inventory-add', '', 'primary'))}<div class="card table" id="inventory-table">${this.table()}</div>${this.dialog ? modal(i ? 'Edit product' : 'Add product', field('Product name', 'product-name', i ? i.name : '') + field('SKU', 'product-sku', i ? i.sku : 'NS-' + String(this.nextId).padStart(3, '0')) + select('Category', 'product-category', ['Stationery', 'Accessories', 'Workspace'], i ? i.category : 'Stationery') + '<div class="field-row">' + field('Stock (whole units)', 'product-stock', i ? i.stock : 0) + field('Price ($)', 'product-price', i ? i.price : 0) + '</div>', 'inventory-save') : ''}`;
  },
  input(id, e) {
    const key = { 'inventory-search': 'query', 'inventory-category': 'category', 'inventory-sort': 'sort' }[id];
    if (key) { this[key] = e.target.value; this.page = 0; document.getElementById('inventory-table').innerHTML = this.table(); }
  },
  action(a, v) {
    if (a === 'inventory-add') { this.dialog = -1; return true; }
    if (a === 'inventory-edit') { this.dialog = +v; return true; }
    if (a === 'close') { this.dialog = false; return true; }
    if (a === 'inventory-next' || a === 'inventory-prev') { this.page += a === 'inventory-next' ? 1 : -1; return true; }
    if (a === 'inventory-save') {
      const name = value('product-name').trim(), sku = value('product-sku').trim(), stockText = value('product-stock').trim(), priceText = value('product-price').trim(), stock = Number(stockText), price = Number(priceText);
      if (!name || !sku || !stockText || !priceText || !Number.isInteger(stock) || stock < 0 || !Number.isFinite(price) || price < 0) { invalid('Enter a name, SKU, whole nonnegative stock and a valid price.'); return false; }
      if (this.items.some((x) => x.sku === sku && x.id !== this.dialog)) { invalid('This SKU is already in use.'); return false; }
      let i = this.items.find((x) => x.id === this.dialog); if (!i) { i = { id: this.nextId++ }; this.items.push(i); }
      Object.assign(i, { name, sku, stock, price, category: value('product-category') }); this.dialog = false; notice('Product saved in session'); return true;
    }
  }
};
