// ---- rich-text editing: Range, Selection, execCommand ----------------------
// Spliced into the prelude's closure (see `/*@EDIT@*/`), so it shares `N`, `wrap` and the DOM
// classes. The engine keeps the selection (so it can draw and move it); this file edits the DOM
// around it. Selection offsets are UTF-16 units in text nodes and child indices in elements.

const BLOCK = new Set(["p", "div", "h1", "h2", "h3", "h4", "h5", "h6", "li", "ul", "ol", "blockquote", "pre", "table",
  "thead", "tbody", "tfoot", "tr", "td", "th", "section", "article", "header", "footer", "main", "nav", "aside", "form",
  "fieldset", "figure", "figcaption", "details", "summary", "hr", "dl", "dt", "dd", "caption"]);
const isEl = (n) => !!n && n.nodeType === 1;
const isTx = (n) => !!n && n.nodeType === 3;
const lname = (n) => (isEl(n) ? n.localName : "");
const isBlock = (n) => isEl(n) && BLOCK.has(n.localName);
const kids = (n) => n.childNodes;
const nlen = (n) => (isTx(n) ? n.data.length : kids(n).length);
const idx = (n) => kids(n.parentNode).indexOf(n);
const isLow = (c) => { const x = c.charCodeAt(0); return x >= 0xdc00 && x <= 0xdfff; };
const pt = (node, offset) => ({ node, offset });
const NBSP = " ";

Node.prototype.cloneNode = function (deep) {
  if (isTx(this)) return documentObj.createTextNode(this.data);
  const c = documentObj.createElement(this.localName);
  for (const a of this.getAttributeNames()) c.setAttribute(a, this.getAttribute(a));
  if (deep) for (const k of kids(this)) c.appendChild(k.cloneNode(true));
  return c;
};
Text.prototype.splitText = function (offset) {
  const d = this.data;
  const r = documentObj.createTextNode(d.slice(offset));
  this.data = d.slice(0, offset);
  if (this.parentNode) this.parentNode.insertBefore(r, this.nextSibling);
  return r;
};
Object.defineProperty(Element.prototype, "contentEditable", {
  configurable: true,
  get() { const v = this.getAttribute("contenteditable"); return v === null ? "inherit" : v === "" ? "true" : v.toLowerCase(); },
  set(v) { v === "inherit" ? this.removeAttribute("contenteditable") : this.setAttribute("contenteditable", String(v)); },
});
const VOID = new Set(["br", "hr", "img", "input", "meta", "link", "area", "base", "col", "embed", "source", "track", "wbr"]);
if (!Object.getOwnPropertyDescriptor(Element.prototype, "outerHTML")) {
  Object.defineProperty(Element.prototype, "outerHTML", {
    configurable: true,
    get() {
      const esc = (v) => String(v).replace(/&/g, "&amp;").replace(/"/g, "&quot;");
      const attrs = this.getAttributeNames().map((a) => ` ${a}="${esc(this.getAttribute(a))}"`).join("");
      return VOID.has(this.localName) ? `<${this.localName}${attrs}>` : `<${this.localName}${attrs}>${this.innerHTML}</${this.localName}>`;
    },
  });
}
Object.defineProperty(Element.prototype, "isContentEditable", { configurable: true, get() { return !!hostOf(this); } });

// ---- positions ---------------------------------------------------------------
function hostOf(n) {
  for (let e = isTx(n) ? n.parentNode : n; isEl(e); e = e.parentNode) {
    const v = e.getAttribute("contenteditable");
    if (v !== null) return v.trim().toLowerCase() === "false" ? null : e;
  }
  return null;
}
function getPts() {
  const s = N.selGet();
  return s.length < 4 ? null : { a: pt(wrap(s[0]), s[1]), f: pt(wrap(s[2]), s[3]) };
}
function setPts(a, f) { const g2 = f || a; N.selSet(a.node._id, a.offset, g2.node._id, g2.offset); }
function pathOf(n, root) { const p = []; for (; n && n !== root; n = n.parentNode) p.unshift(idx(n)); return p; }
function order(a, b) {
  if (a.node === b.node) return Math.sign(a.offset - b.offset);
  if (a.node.contains(b.node)) { let c = b.node; while (c.parentNode !== a.node) c = c.parentNode; return idx(c) < a.offset ? 1 : -1; }
  if (b.node.contains(a.node)) return -order(b, a);
  const chain = (n) => { const p = []; for (; n; n = n.parentNode) p.unshift(n); return p; };
  const pa = chain(a.node), pb = chain(b.node);
  let i = 0;
  while (pa[i] === pb[i]) i++;
  return idx(pa[i]) < idx(pb[i]) ? -1 : 1;
}
function commonAncestor(a, b) {
  const ca = new Set();
  for (let n = a; n; n = n.parentNode) ca.add(n);
  for (let n = b; n; n = n.parentNode) if (ca.has(n)) return n;
  return documentObj;
}
function childOnPath(ancestor, n) { while (n.parentNode !== ancestor) n = n.parentNode; return n; }
function closestBlock(n, host) {
  for (; n && n !== host; n = n.parentNode) if (isBlock(n)) return n;
  return host;
}
const isLeaf = (n) => isTx(n) || lname(n) === "br" || lname(n) === "img" || (isBlock(n) && kids(n).length === 0);
function firstLeafOf(n) {
  if (isLeaf(n)) return n;
  for (const c of kids(n)) { const l = firstLeafOf(c); if (l) return l; }
  return null;
}
function lastLeafOf(n) {
  if (isLeaf(n)) return n;
  const k = kids(n);
  for (let i = k.length - 1; i >= 0; i--) { const l = lastLeafOf(k[i]); if (l) return l; }
  return null;
}
function nextNode(n, host, skipKids) {
  if (!skipKids && isEl(n) && n.firstChild) return n.firstChild;
  for (; n && n !== host; n = n.parentNode) if (n.nextSibling) return n.nextSibling;
  return null;
}
function prevNode(n, host) {
  if (n === host) return null;
  let p = n.previousSibling;
  if (p) { while (p.lastChild) p = p.lastChild; return p; }
  const par = n.parentNode;
  return par === host ? null : par;
}
function leafAfter(n, host) { for (let c = nextNode(n, host, true); c; c = nextNode(c, host)) if (isLeaf(c)) return c; return null; }
function leafBefore(n, host) { for (let c = prevNode(n, host); c; c = prevNode(c, host)) if (isLeaf(c)) return c; return null; }

// A cursor is {leaf, pos}: a text node and character offset, or 0/1 before/after a <br>, an
// image or an empty block.
function toCursor(p, host) {
  if (isTx(p.node)) return { leaf: p.node, pos: p.offset };
  const k = kids(p.node);
  for (let i = p.offset; i < k.length; i++) { const l = firstLeafOf(k[i]); if (l) return { leaf: l, pos: 0 }; }
  for (let i = Math.min(p.offset, k.length) - 1; i >= 0; i--) { const l = lastLeafOf(k[i]); if (l) return { leaf: l, pos: isTx(l) ? l.data.length : 1 }; }
  return null;
}
function toPoint(c) {
  const l = c.leaf;
  if (isTx(l)) return pt(l, c.pos);
  if (isBlock(l)) return pt(l, 0);
  return pt(l.parentNode, idx(l) + (c.pos ? 1 : 0));
}
const sameLine = (a, b, host) => closestBlock(a, host) === closestBlock(b, host);
// Move one position. Returns {cursor, kind: "char" | "block" | "br" | "void", ch} or null.
function step(c, dir, host) {
  const l = c.leaf;
  if (isTx(l)) {
    if (dir > 0 && c.pos < l.data.length) {
      const n = /[\ud800-\udbff]/.test(l.data[c.pos]) ? 2 : 1;
      return { cursor: { leaf: l, pos: c.pos + n }, kind: "char", ch: l.data.slice(c.pos, c.pos + n) };
    }
    if (dir < 0 && c.pos > 0) {
      const n = c.pos > 1 && isLow(l.data[c.pos - 1]) ? 2 : 1;
      return { cursor: { leaf: l, pos: c.pos - n }, kind: "char", ch: l.data.slice(c.pos - n, c.pos) };
    }
  } else if (lname(l) === "br" || lname(l) === "img") {
    // Over the leaf itself.
    if (dir > 0 && c.pos === 0) return { cursor: { leaf: l, pos: 1 }, kind: lname(l) === "br" ? "br" : "void", ch: "\n", over: l };
    if (dir < 0 && c.pos === 1) return { cursor: { leaf: l, pos: 0 }, kind: lname(l) === "br" ? "br" : "void", ch: "\n", over: l };
  }
  // Leave this leaf for the neighbouring one.
  let o = dir > 0 ? leafAfter(l, host) : leafBefore(l, host);
  while (o && isTx(o) && o.data === "") o = dir > 0 ? leafAfter(o, host) : leafBefore(o, host);
  if (!o) return null;
  const crossing = !sameLine(l, o, host);
  if (isTx(o)) {
    if (crossing) return { cursor: { leaf: o, pos: dir > 0 ? 0 : o.data.length }, kind: "block", ch: "\n" };
    // Same block: the boundary position is the one just left, so step a character further.
    return step({ leaf: o, pos: dir > 0 ? 0 : o.data.length }, dir, host) || { cursor: { leaf: o, pos: dir > 0 ? 0 : o.data.length }, kind: "char", ch: "" };
  }
  if (lname(o) === "br" || lname(o) === "img") {
    // The near side of it is where we already are visually; go over it.
    return step({ leaf: o, pos: dir > 0 ? 0 : 1 }, dir, host);
  }
  return { cursor: { leaf: o, pos: dir > 0 ? 0 : 1 }, kind: "block", ch: "\n" };
}

// ---- Range and Selection -------------------------------------------------------
class Range {
  constructor() { this.startContainer = this.endContainer = documentObj; this.startOffset = this.endOffset = 0; }
  get collapsed() { return this.startContainer === this.endContainer && this.startOffset === this.endOffset; }
  get commonAncestorContainer() { return commonAncestor(this.startContainer, this.endContainer); }
  setStart(n, o) { this.startContainer = n; this.startOffset = o; if (order(pt(this.endContainer, this.endOffset), pt(n, o)) < 0) { this.endContainer = n; this.endOffset = o; } }
  setEnd(n, o) { this.endContainer = n; this.endOffset = o; if (order(pt(this.startContainer, this.startOffset), pt(n, o)) > 0) { this.startContainer = n; this.startOffset = o; } }
  setStartBefore(n) { this.setStart(n.parentNode, idx(n)); }
  setStartAfter(n) { this.setStart(n.parentNode, idx(n) + 1); }
  setEndBefore(n) { this.setEnd(n.parentNode, idx(n)); }
  setEndAfter(n) { this.setEnd(n.parentNode, idx(n) + 1); }
  selectNode(n) { this.startContainer = this.endContainer = n.parentNode; this.startOffset = idx(n); this.endOffset = idx(n) + 1; }
  selectNodeContents(n) { this.startContainer = this.endContainer = n; this.startOffset = 0; this.endOffset = nlen(n); }
  collapse(toStart) { if (toStart) { this.endContainer = this.startContainer; this.endOffset = this.startOffset; } else { this.startContainer = this.endContainer; this.startOffset = this.endOffset; } }
  cloneRange() { const r = new Range(); r.startContainer = this.startContainer; r.startOffset = this.startOffset; r.endContainer = this.endContainer; r.endOffset = this.endOffset; return r; }
  toString() { return textBetween(pt(this.startContainer, this.startOffset), pt(this.endContainer, this.endOffset)); }
  deleteContents() { deleteRange(pt(this.startContainer, this.startOffset), pt(this.endContainer, this.endOffset), hostOf(this.startContainer)); this.collapse(true); }
  insertNode(n) {
    const { startContainer: c, startOffset: o } = this;
    if (isTx(c)) { const r = c.splitText(o); c.parentNode.insertBefore(n, r); } else c.insertBefore(n, kids(c)[o] || null);
  }
  surroundContents(wrapper) { const t = textNodesBetween(pt(this.startContainer, this.startOffset), pt(this.endContainer, this.endOffset)); if (t.length) { t[0].parentNode.insertBefore(wrapper, t[0]); for (const x of t) wrapper.appendChild(x); } }
  getBoundingClientRect() { return documentObj.documentElement.getBoundingClientRect(); }
}
// A fragment is a detached <div> marked as one: nodeType 11, and inserting it moves its children.
function makeFragment() {
  const f = documentObj.createElement("div");
  Object.defineProperty(f, "nodeType", { value: 11 });
  Object.defineProperty(f, "__fragment", { value: true });
  return f;
}
documentObj.createDocumentFragment = makeFragment;
const origAppend = Node.prototype.appendChild, origInsert = Node.prototype.insertBefore;
Node.prototype.appendChild = function (c) {
  if (c && c.__fragment) { for (const k of kids(c)) origAppend.call(this, k); return c; }
  return origAppend.call(this, c);
};
Node.prototype.insertBefore = function (c, ref) {
  if (c && c.__fragment) { for (const k of kids(c)) origInsert.call(this, k, ref); return c; }
  return origInsert.call(this, c, ref);
};
// Keep only what lies between `a` and `b` inside a deep copy of `root` (points are in the copy).
function trimTo(root, a, b) {
  const cut = (p, after) => {
    let node = p.node, off = p.offset;
    if (isTx(node)) {
      node.data = after ? node.data.slice(0, off) : node.data.slice(off);
      off = after ? idx(node) + 1 : idx(node);
      node = node.parentNode;
    }
    for (;;) {
      const ks = kids(node);
      for (const k of after ? ks.slice(off) : ks.slice(0, off)) k.remove();
      if (node === root) break;
      off = after ? idx(node) + 1 : idx(node);
      node = node.parentNode;
    }
  };
  cut(b, true);
  cut(a, false);
}
Range.prototype.cloneContents = function () {
  const frag = makeFragment();
  const a = pt(this.startContainer, this.startOffset), b = pt(this.endContainer, this.endOffset);
  if (order(a, b) >= 0) return frag;
  const common = commonAncestor(a.node, b.node);
  if (isTx(common)) { frag.appendChild(documentObj.createTextNode(common.data.slice(a.offset, b.offset))); return frag; }
  const copy = common.cloneNode(true);
  const inCopy = (p) => {
    let n = copy;
    for (const i of pathOf(p.node, common)) n = kids(n)[i];
    return pt(n, p.offset);
  };
  trimTo(copy, inCopy(a), inCopy(b));
  for (const k of kids(copy)) frag.appendChild(k);
  return frag;
};
Range.prototype.extractContents = function () {
  const frag = this.cloneContents();
  this.deleteContents();
  return frag;
};
Range.prototype.createContextualFragment = function (html) {
  const frag = makeFragment();
  frag.innerHTML = String(html);
  return frag;
};
Object.assign(Range, { START_TO_START: 0, START_TO_END: 1, END_TO_END: 2, END_TO_START: 3 });
Range.prototype.compareBoundaryPoints = function (how, other) {
  const mine = how === 0 || how === 3 ? pt(this.startContainer, this.startOffset) : pt(this.endContainer, this.endOffset);
  const theirs = how === 0 || how === 1 ? pt(other.startContainer, other.startOffset) : pt(other.endContainer, other.endOffset);
  return order(mine, theirs);
};
Range.prototype.comparePoint = function (node, offset) {
  const p = pt(node, offset);
  if (order(p, pt(this.startContainer, this.startOffset)) < 0) return -1;
  if (order(p, pt(this.endContainer, this.endOffset)) > 0) return 1;
  return 0;
};
Range.prototype.isPointInRange = function (node, offset) { return this.comparePoint(node, offset) === 0; };
Range.prototype.intersectsNode = function (node) {
  const at = node.parentNode ? idx(node) : 0, parent = node.parentNode || node;
  return order(pt(parent, at), pt(this.endContainer, this.endOffset)) < 0 && order(pt(parent, node.parentNode ? at + 1 : nlen(node)), pt(this.startContainer, this.startOffset)) > 0;
};
Range.prototype.detach = function () {};
documentObj.createRange = () => new Range();
g.Range = Range;
function textBetween(a, b) {
  return textNodesBetween(a, b, true).map((t) => t.text).join("");
}
// Text nodes (or, with `slices`, {node, text} pieces) between two points, in order.
function textNodesBetween(a, b, slices) {
  if (order(a, b) > 0) [a, b] = [b, a];
  const out = [];
  const root = commonAncestor(a.node, b.node);
  for (let n = nextNode(root, root); n; n = nextNode(n, root)) {
    if (!isTx(n)) continue;
    if (order(pt(n, n.data.length), a) <= 0 && !(n === a.node && a.offset === 0 && n.data === "")) continue;
    if (order(pt(n, 0), b) >= 0) break;
    const from = n === a.node ? a.offset : 0, to = n === b.node ? b.offset : n.data.length;
    out.push(slices ? { node: n, text: n.data.slice(from, to) } : n);
  }
  if (isTx(root) && order(pt(root, 0), b) < 0) out.push(slices ? { node: root, text: root.data.slice(a.offset, b.offset) } : root);
  return out;
}

const selection = {
  get anchorNode() { const p = getPts(); return p ? p.a.node : null; },
  get anchorOffset() { const p = getPts(); return p ? p.a.offset : 0; },
  get focusNode() { const p = getPts(); return p ? p.f.node : null; },
  get focusOffset() { const p = getPts(); return p ? p.f.offset : 0; },
  get isCollapsed() { const p = getPts(); return p ? p.a.node === p.f.node && p.a.offset === p.f.offset : N.pageSelection() === ""; },
  get rangeCount() { return getPts() || N.pageSelection() !== "" ? 1 : 0; },
  get type() { const p = getPts(); return !p && N.pageSelection() === "" ? "None" : this.isCollapsed ? "Caret" : "Range"; },
  get direction() { const p = getPts(); return !p ? "none" : order(p.a, p.f) <= 0 ? "forward" : "backward"; },
  toString: () => N.pageSelection(),
  getRangeAt(i) {
    const p = getPts();
    if (!p || i !== 0) throw new Error("IndexSizeError: no range at that index");
    const r = new Range();
    const [s, e] = order(p.a, p.f) <= 0 ? [p.a, p.f] : [p.f, p.a];
    r.startContainer = s.node; r.startOffset = s.offset; r.endContainer = e.node; r.endOffset = e.offset;
    return r;
  },
  addRange(r) { setPts(pt(r.startContainer, r.startOffset), pt(r.endContainer, r.endOffset)); },
  removeAllRanges() { N.selClear(); N.clearPageSelection(); },
  empty() { this.removeAllRanges(); },
  removeRange() { this.removeAllRanges(); },
  collapse(n, o) { if (n === null) return this.removeAllRanges(); setPts(pt(n, o || 0)); },
  setPosition(n, o) { this.collapse(n, o); },
  collapseToStart() { const p = getPts(); if (p) { const s = order(p.a, p.f) <= 0 ? p.a : p.f; setPts(s); } },
  collapseToEnd() { const p = getPts(); if (p) { const e = order(p.a, p.f) <= 0 ? p.f : p.a; setPts(e); } },
  extend(n, o) { const p = getPts(); setPts(p ? p.a : pt(n, o), pt(n, o || 0)); },
  setBaseAndExtent(an, ao, fn, fo) { setPts(pt(an, ao), pt(fn, fo)); },
  selectAllChildren(n) { setPts(pt(n, 0), pt(n, nlen(n))); },
  containsNode(n, partial) { const p = getPts(); if (!p) return false; const [s, e] = order(p.a, p.f) <= 0 ? [p.a, p.f] : [p.f, p.a]; const a = pt(n.parentNode, idx(n)), b = pt(n.parentNode, idx(n) + 1); return partial ? order(a, e) < 0 && order(b, s) > 0 : order(a, s) >= 0 && order(b, e) <= 0; },
  deleteFromDocument() { documentObj.execCommand("delete"); },
};
g.getSelection = () => selection;
documentObj.getSelection = () => selection;

// ---- helpers: deleting, splitting, tidying ---------------------------------------
function removeId(el) { if (isEl(el)) el.removeAttribute("id"); return el; }
const isEmptyInline = (n) => (isTx(n) && n.data === "") || (isEl(n) && !isBlock(n) && lname(n) !== "br" && lname(n) !== "img" && kids(n).every(isEmptyInline));
function ensureContent(block, host) {
  if (!isEl(block) || block === host) return;
  if (kids(block).every(isEmptyInline)) {
    for (const c of kids(block)) c.remove();
    block.appendChild(documentObj.createElement("br"));
  }
}
function unwrap(el) {
  const p = el.parentNode;
  for (const c of kids(el)) p.insertBefore(c, el);
  el.remove();
}
function sameAttrs(a, b) {
  const na = a.getAttributeNames().sort(), nb = b.getAttributeNames().sort();
  return na.length === nb.length && na.every((n, i) => n === nb[i] && a.getAttribute(n) === b.getAttribute(n));
}
// Merge adjacent identical inline elements, and drop empty ones.
function tidy(root) {
  for (const c of kids(root)) if (isEl(c) && !isBlock(c) && lname(c) !== "br" && lname(c) !== "img" && kids(c).every(isEmptyInline)) c.remove();
  const k = kids(root);
  for (let i = k.length - 1; i > 0; i--) {
    const a = k[i - 1], b = k[i];
    if (isEl(a) && isEl(b) && !isBlock(a) && lname(a) !== "br" && lname(a) !== "img" && a.localName === b.localName && sameAttrs(a, b)) {
      while (b.firstChild) a.appendChild(b.firstChild);
      b.remove();
    }
  }
  for (const c of kids(root)) if (isEl(c)) tidy(c);
}
// Cut the tree at `p`, up to and including `limit`; returns the new right-hand copy of `limit`.
function splitTreeAt(p, limit) {
  let node = p.node, offset = p.offset;
  if (isTx(node)) { const r = node.splitText(offset); offset = idx(r); node = r.parentNode; }
  while (node !== limit) {
    const par = node.parentNode;
    const clone = removeId(node.cloneNode(false));
    for (const c of kids(node).slice(offset)) clone.appendChild(c);
    par.insertBefore(clone, node.nextSibling);
    offset = idx(node) + 1;
    node = par;
  }
  const clone = removeId(limit.cloneNode(false));
  for (const c of kids(limit).slice(offset)) clone.appendChild(c);
  limit.parentNode.insertBefore(clone, limit.nextSibling);
  return clone;
}
function pruneEmpty(n, host) {
  while (n && n !== host && isEl(n) && kids(n).length === 0) { const p = n.parentNode; n.remove(); n = p; }
}
function mergeBlocks(s, e, host) {
  const only = kids(s).filter((c) => !isEmptyInline(c));
  if (only.length === 1 && lname(only[0]) === "br") for (const c of kids(s)) c.remove();
  const parent = e.parentNode;
  for (const c of kids(e)) s.appendChild(c);
  e.remove();
  pruneEmpty(parent, host);
}
function truncate(p, stop, after) {
  let n = p.node;
  if (isTx(n)) n.data = after ? n.data.slice(0, p.offset) : n.data.slice(p.offset);
  else for (const c of after ? kids(n).slice(p.offset) : kids(n).slice(0, p.offset)) c.remove();
  while (n !== stop) {
    const par = n.parentNode, sib = kids(par), i = sib.indexOf(n);
    for (const c of after ? sib.slice(i + 1) : sib.slice(0, i)) c.remove();
    n = par;
  }
}
// ---- tables --------------------------------------------------------------------------
const isCell = (n) => lname(n) === "td" || lname(n) === "th";
function cellOf(n, host) {
  for (; n && n !== host; n = n.parentNode) if (isCell(n)) return n;
  return null;
}
// Deleting across cells empties what the selection covers and keeps the table's shape.
function deleteAcrossCells(a, b, host) {
  const ca = cellOf(a.node, host), cb = cellOf(b.node, host);
  const clear = (cell) => { for (const c of kids(cell)) c.remove(); cell.appendChild(documentObj.createElement("br")); };
  if (ca) truncate(a, ca, true);
  if (cb) truncate(b, cb, false);
  for (const cell of host.querySelectorAll("td, th")) {
    if (cell === ca || cell === cb) continue;
    if (order(pt(cell, 0), a) >= 0 && order(pt(cell, nlen(cell)), b) <= 0) clear(cell);
  }
  for (const cell of [ca, cb]) if (cell) ensureContent(cell, host);
  return ca ? leafToPoint(a.node.isConnected ? a : pt(ca, 0), host) : a;
}
// Tab and Shift+Tab move between cells; Tab in the last cell adds a row.
function tabInTable(back, host) {
  const sp = getPts();
  const cell = sp && cellOf(sp.f.node, host);
  if (!cell) return false;
  let target;
  const sibling = back ? cell.previousElementSibling : cell.nextElementSibling;
  if (sibling && isCell(sibling)) target = sibling;
  else {
    const row = cell.parentNode;
    let other = back ? row.previousElementSibling : row.nextElementSibling;
    if (!other && row.parentNode && lname(row.parentNode) !== "table") {
      const group = back ? row.parentNode.previousElementSibling : row.parentNode.nextElementSibling;
      other = group && (back ? group.lastElementChild : group.firstElementChild);
    }
    if (!other && !back) {
      checkpoint(host, "structure");
      other = row.cloneNode(false);
      for (const c of kids(row)) { const nc = c.cloneNode(false); nc.appendChild(documentObj.createElement("br")); other.appendChild(nc); }
      row.parentNode.insertBefore(other, row.nextSibling);
      fireInput(host, "insertTableRow");
    }
    if (!other) return true;
    const cells = kids(other).filter(isCell);
    target = back ? cells[cells.length - 1] : cells[0];
  }
  if (target) setPts(leafToPoint(pt(target, 0), host));
  return true;
}
function insertTable(rows, cols, host) {
  const caret = deleteSelectionIn(host) || getPts().f;
  const table = documentObj.createElement("table");
  table.setAttribute("style", "border-collapse: collapse; width: 100%");
  const body = documentObj.createElement("tbody");
  for (let r = 0; r < rows; r++) {
    const tr = documentObj.createElement("tr");
    for (let c = 0; c < cols; c++) {
      const td = documentObj.createElement("td");
      td.setAttribute("style", "border: 1px solid #9aa0a6; padding: 4px 6px; min-width: 24px");
      td.appendChild(documentObj.createElement("br"));
      tr.appendChild(td);
    }
    body.appendChild(tr);
  }
  table.appendChild(body);
  const block = closestBlock(caret.node, host);
  const after = documentObj.createElement("p");
  after.appendChild(documentObj.createElement("br"));
  if (block === host) {
    const end = insertNodesAt(caret, [table, after]);
    void end;
  } else {
    const right = splitTreeAt(textPointForInsert(caret), block);
    const leftEmpty = kids(block).every(isEmptyInline);
    block.parentNode.insertBefore(table, right);
    if (leftEmpty) block.remove();
    if (kids(right).every(isEmptyInline)) { dropPlaceholder(right); right.appendChild(documentObj.createElement("br")); }
  }
  if (!table.nextSibling) table.parentNode.appendChild(after);
  setPts(leafToPoint(pt(kids(kids(kids(table)[0])[0])[0], 0), host));
}

// Remove everything between two points, joining the blocks they were in. Returns the caret.
function deleteRange(a, b, host) {
  if (order(a, b) > 0) [a, b] = [b, a];
  if (order(a, b) === 0) return a;
  if (cellOf(a.node, host) !== cellOf(b.node, host)) return deleteAcrossCells(a, b, host);
  const startBlock = closestBlock(a.node, host), endBlock = closestBlock(b.node, host);
  let caret;
  if (isTx(a.node) && a.node === b.node) {
    a.node.data = a.node.data.slice(0, a.offset) + a.node.data.slice(b.offset);
    caret = pt(a.node, a.offset);
  } else {
    const common = commonAncestor(a.node, b.node);
    const firstPartial = a.node === common ? null : childOnPath(common, a.node);
    const lastPartial = b.node === common ? null : childOnPath(common, b.node);
    const ck = kids(common);
    const si = a.node === common ? a.offset : ck.indexOf(firstPartial) + 1;
    const ei = b.node === common ? b.offset : ck.indexOf(lastPartial);
    const between = ck.slice(si, ei);
    if (a.node !== common) truncate(a, firstPartial, true);
    if (b.node !== common) truncate(b, lastPartial, false);
    for (const n of between) n.remove();
    caret = a.node === common ? pt(common, a.offset) : pt(a.node, isTx(a.node) ? a.offset : nlen(a.node));
    if (startBlock !== endBlock && endBlock.isConnected && startBlock.isConnected && host && host.contains(endBlock) && !startBlock.contains(endBlock) && !endBlock.contains(startBlock)) mergeBlocks(startBlock, endBlock, host);
  }
  return cleanup(caret, startBlock, host);
}
function cleanup(caret, block, host) {
  if (block && block.isConnected) {
    // Drop empty text and inline wrappers, except where the caret is.
    const walk = (n) => {
      for (const c of kids(n)) {
        if (c === caret.node) continue;
        if (isTx(c) && c.data === "") c.remove();
        else if (isEl(c) && !isBlock(c)) { walk(c); if (kids(c).length === 0 && lname(c) !== "br" && lname(c) !== "img") c.remove(); }
      }
    };
    walk(block);
    ensureContent(block, host);
  }
  if (!caret.node.isConnected || (isEl(caret.node) && caret.offset > nlen(caret.node))) caret = block && block.isConnected ? leafToPoint(pt(block, 0), host) : caret;
  return caret;
}
function leafToPoint(p, host) { const c = toCursor(p, host); return c ? toPoint(c) : p; }

// ---- typing --------------------------------------------------------------------
function firstTextInBlock(n, host) {
  const b = closestBlock(n, host);
  for (let l = leafBefore(n, host); l; l = leafBefore(l, host)) {
    if (closestBlock(l, host) !== b) return true;
    if (lname(l) === "br") return true;
    if (isTx(l) && l.data !== "") return false;
    if (lname(l) === "img") return false;
  }
  return true;
}
function lastTextInBlock(n, host) {
  const b = closestBlock(n, host);
  for (let l = leafAfter(n, host); l; l = leafAfter(l, host)) {
    if (closestBlock(l, host) !== b) return true;
    if (lname(l) === "br") return true;
    if (isTx(l) && l.data !== "") return false;
    if (lname(l) === "img") return false;
  }
  return true;
}
// Keep every space visible: runs of spaces alternate with no-break spaces, and a lone space at
// a line's start or end is a no-break space. Replacing characters one for one leaves the
// caret where it is.
function fixSpaces(node, host) {
  const d = node.data;
  if (!/[  ]/.test(d)) return;
  const first = firstTextInBlock(node, host), last = lastTextInBlock(node, host);
  const out = d.split("");
  for (let i = 0; i < out.length;) {
    if (out[i] !== " " && out[i] !== NBSP) { i++; continue; }
    let j = i;
    while (j < out.length && (out[j] === " " || out[j] === NBSP)) j++;
    const atStart = first && i === 0, atEnd = last && j === out.length;
    for (let k = i; k < j; k++) out[k] = atStart || atEnd || (k - i) % 2 === 1 ? NBSP : " ";
    // A single space between words is an ordinary space, even at a block's edge when text follows.
    i = j;
  }
  const s = out.join("");
  if (s !== d) node.data = s;
}
function textPointForInsert(p) {
  if (isTx(p.node)) return p;
  const k = kids(p.node);
  // An empty line holds just a placeholder <br>.
  if (k.length && k.every((c) => lname(c) === "br" || isEmptyInline(c)) && k.filter((c) => lname(c) === "br").length === 1) {
    for (const c of k) c.remove();
    p = pt(p.node, 0);
  }
  const before = kids(p.node)[p.offset - 1], after = kids(p.node)[p.offset];
  if (isTx(before)) return pt(before, before.data.length);
  if (isTx(after)) return pt(after, 0);
  const t = documentObj.createTextNode("");
  p.node.insertBefore(t, after || null);
  return pt(t, 0);
}
function insertTextAt(p, text, host) {
  p = textPointForInsert(p);
  const d = p.node.data;
  p.node.data = d.slice(0, p.offset) + text + d.slice(p.offset);
  fixSpaces(p.node, host);
  return pt(p.node, p.offset + text.length);
}
function insertLineBreak(p, host) {
  const br = documentObj.createElement("br");
  let parent;
  if (isTx(p.node)) { const r = p.node.splitText(p.offset); parent = p.node.parentNode; parent.insertBefore(br, r); } else { parent = p.node; parent.insertBefore(br, kids(parent)[p.offset] || null); }
  // A <br> that ends a block needs another to show the empty line.
  if (!br.nextSibling) parent.appendChild(documentObj.createElement("br"));
  return pt(br.parentNode, idx(br) + 1);
}
function splitBlock(p, host) {
  const block = closestBlock(p.node, host);
  if (block === host || isCell(block)) return insertLineBreak(p, host);
  const empty = kids(block).every(isEmptyInline) || (kids(block).length === 1 && lname(kids(block)[0]) === "br");
  if (lname(block) === "li" && empty) return outdentItem(block, host);
  const right = splitTreeAt(p, block);
  ensureContent(block, host);
  const rightEmpty = kids(right).every(isEmptyInline);
  ensureContent(right, host);
  // Pressing Enter at the end of a heading starts a paragraph.
  let target = right;
  if (/^h[1-6]$/.test(lname(right)) && rightEmpty) {
    const para = documentObj.createElement("p");
    for (const a of right.getAttributeNames()) if (a === "style" || a === "class") para.setAttribute(a, right.getAttribute(a));
    while (right.firstChild) para.appendChild(right.firstChild);
    right.parentNode.insertBefore(para, right);
    right.remove();
    target = para;
  }
  return leafToPoint(pt(target, 0), host);
}
// Take a list item out of its list as a paragraph (Enter on an empty item, or Backspace at its start).
function outdentItem(li, host) {
  const list = li.parentNode;
  const para = documentObj.createElement("p");
  for (const c of kids(li)) para.appendChild(c);
  ensureContent(para, host);
  if (!li.nextSibling) { list.parentNode.insertBefore(para, list.nextSibling); li.remove(); }
  else if (!li.previousSibling) { list.parentNode.insertBefore(para, list); li.remove(); }
  else { const right = splitTreeAt(pt(list, idx(li) + 1), list); list.parentNode.insertBefore(para, right); li.remove(); }
  pruneEmpty(list, host);
  return leafToPoint(pt(para, 0), host);
}

// ---- inline formatting -------------------------------------------------------------
const sty = (el, p) => el.style.getPropertyValue(p);
const norm = (v) => String(v).trim().toLowerCase();
const FORMATS = {
  bold: { read: (el) => (/^(b|strong)$/.test(el.localName) || /^(bold|bolder|[6-9]00)$/.test(sty(el, "font-weight")) ? true : sty(el, "font-weight") === "normal" || sty(el, "font-weight") === "400" ? false : null), make: () => documentObj.createElement("b"), styleProps: ["font-weight"], tags: ["b", "strong"] },
  italic: { read: (el) => (/^(i|em)$/.test(el.localName) || sty(el, "font-style") === "italic" ? true : sty(el, "font-style") === "normal" ? false : null), make: () => documentObj.createElement("i"), styleProps: ["font-style"], tags: ["i", "em"] },
  underline: { read: (el) => (el.localName === "u" || /underline/.test(sty(el, "text-decoration")) ? true : null), make: () => documentObj.createElement("u"), styleProps: ["text-decoration"], tags: ["u"] },
  strike: { read: (el) => (/^(s|strike|del)$/.test(el.localName) || /line-through/.test(sty(el, "text-decoration")) ? true : null), make: () => documentObj.createElement("s"), styleProps: ["text-decoration"], tags: ["s", "strike", "del"] },
  sub: { read: (el) => (el.localName === "sub" ? true : null), make: () => documentObj.createElement("sub"), styleProps: [], tags: ["sub"] },
  sup: { read: (el) => (el.localName === "sup" ? true : null), make: () => documentObj.createElement("sup"), styleProps: [], tags: ["sup"] },
  foreColor: { value: true, read: (el) => (el.localName === "font" && el.getAttribute("color") ? norm(el.getAttribute("color")) : sty(el, "color") ? norm(sty(el, "color")) : null), make: (v) => spanWith("color", v), styleProps: ["color"], tags: [] },
  hiliteColor: { value: true, read: (el) => (sty(el, "background-color") ? norm(sty(el, "background-color")) : null), make: (v) => spanWith("background-color", v), styleProps: ["background-color"], tags: [] },
  fontName: { value: true, read: (el) => (sty(el, "font-family") ? sty(el, "font-family").replace(/["']/g, "") : null), make: (v) => spanWith("font-family", v), styleProps: ["font-family"], tags: [] },
  fontSize: { value: true, read: (el) => (sty(el, "font-size") ? norm(sty(el, "font-size")) : null), make: (v) => spanWith("font-size", v), styleProps: ["font-size"], tags: [] },
};
function spanWith(prop, v) { const s = documentObj.createElement("span"); s.style.setProperty(prop, v); return s; }
function stripFormat(el, key) {
  const f = FORMATS[key];
  if (f.tags.includes(el.localName)) { unwrap(el); return; }
  for (const p of f.styleProps) {
    if (key === "underline" || key === "strike") {
      const rest = sty(el, p).split(/\s+/).filter((w) => w && w !== (key === "underline" ? "underline" : "line-through"));
      if (rest.length) el.style.setProperty(p, rest.join(" ")); else el.style.removeProperty(p);
    } else el.style.removeProperty(p);
  }
  if (el.localName === "font" && key === "foreColor") el.removeAttribute("color");
  if ((el.localName === "span" || el.localName === "font") && el.getAttributeNames().length === 0) unwrap(el);
}
// What a text node's formatting is for `key` (true/false for toggles, a string for values).
function formatAt(n, key, host) {
  const f = FORMATS[key];
  for (let e = isTx(n) ? n.parentNode : n; isEl(e) && e !== host; e = e.parentNode) {
    const v = f.read(e);
    if (v !== null) return v;
  }
  return f.value ? null : false;
}
function matchingAncestor(n, key, host) {
  const f = FORMATS[key];
  for (let e = n.parentNode; isEl(e) && e !== host; e = e.parentNode) if (f.read(e) !== null && !(f.read(e) === false)) return e;
  return null;
}
// Give a text node its own copy of ancestor `a`, so formatting can be stripped from just that node.
function isolate(t, a) {
  const after = splitTreeAt(pt(t.parentNode, idx(t) + 1), a);
  if (kids(after).every(isEmptyInline)) after.remove();
  const mine = splitTreeAt(pt(t.parentNode, idx(t)), a);
  if (kids(a).every(isEmptyInline)) a.remove();
  return mine;
}
function selectedPoints() {
  const p = getPts();
  if (!p) return null;
  const [s, e] = order(p.a, p.f) <= 0 ? [p.a, p.f] : [p.f, p.a];
  return { s, e, backward: order(p.a, p.f) > 0 };
}
// Cut the text nodes at the selection's ends so the selection covers whole nodes; returns them.
function coveredText(host) {
  const sp = selectedPoints();
  if (!sp) return [];
  let { s, e } = sp;
  // End first, so the start node's offsets stay valid when both are in one node.
  if (isTx(e.node) && e.offset > 0 && e.offset < e.node.data.length) e.node.splitText(e.offset);
  if (isTx(s.node) && s.offset > 0 && s.offset < s.node.data.length) {
    const r = s.node.splitText(s.offset);
    if (s.node === e.node) e = pt(r, e.offset - s.offset);
    s = pt(r, 0);
  }
  const out = [];
  for (let n = nextNode(host, host); n; n = nextNode(n, host)) {
    if (!isTx(n) || n.data === "") continue;
    if (order(pt(n, n.data.length), s) <= 0) continue;
    if (order(pt(n, 0), e) >= 0) break;
    out.push(n);
  }
  return out;
}
let pending = null; // formats the next typed character takes: {bold: true, ...}
function applyFormat(key, value, host) {
  const sp = selectedPoints();
  if (!sp) return false;
  const f = FORMATS[key];
  if (sp.s.node === sp.e.node && sp.s.offset === sp.e.offset) {
    // A caret: the next typing takes the format.
    const here = pending && key in pending ? pending[key] : formatAt(toCursor(sp.s, host) ? toCursor(sp.s, host).leaf : sp.s.node, key, host);
    pending = Object.assign({}, pending);
    pending[key] = f.value ? value : value === undefined ? !here : value;
    return true;
  }
  const nodes = coveredText(host);
  if (!nodes.length) return false;
  let want;
  if (f.value) want = value || null;
  else want = value === undefined ? !nodes.every((n) => formatAt(n, key, host)) : !!value;
  for (const t of nodes) {
    if (!want) {
      for (let a = matchingAncestor(t, key, host); a; a = matchingAncestor(t, key, host)) stripFormat(isolate(t, a), key);
    } else if (formatAt(t, key, host) !== (f.value ? want : true)) {
      // Replace a different value rather than stacking on it.
      if (f.value) for (let a = matchingAncestor(t, key, host); a; a = matchingAncestor(t, key, host)) stripFormat(isolate(t, a), key);
      const w = f.make(want);
      t.parentNode.insertBefore(w, t);
      w.appendChild(t);
    }
  }
  const first = nodes[0], last = nodes[nodes.length - 1];
  tidyAround(first, host); tidyAround(last, host);
  setPts(pt(first, 0), pt(last, last.data.length));
  return true;
}
function tidyAround(n, host) { const b = closestBlock(n, host); tidy(b === host ? host : b); }

// ---- blocks ------------------------------------------------------------------------------
function selectedBlocks(host) {
  const sp = selectedPoints();
  if (!sp) return [];
  const blocks = [];
  const add = (n) => { let b = closestBlock(n, host); if (b === host) b = wrapInline(host); if (!blocks.includes(b)) blocks.push(b); };
  const c0 = toCursor(sp.s, host), c1 = toCursor(sp.e, host);
  if (!c0) { add(sp.s.node); return blocks; }
  add(c0.leaf);
  if (c1 && c1.leaf !== c0.leaf) {
    for (let n = leafAfter(c0.leaf, host); n; n = leafAfter(n, host)) {
      add(n);
      if (n === c1.leaf) break;
    }
  }
  return blocks;
}
// A host whose content is bare inline runs: put it in a <div> so block commands have a block.
function wrapInline(host) {
  if (kids(host).some(isBlock)) return host;
  const d = documentObj.createElement("div");
  for (const c of kids(host)) d.appendChild(c);
  host.appendChild(d);
  return d;
}
function renameBlock(el, tag) {
  if (el.localName === tag) return el;
  const n = documentObj.createElement(tag);
  for (const a of el.getAttributeNames()) if (a !== "id") n.setAttribute(a, el.getAttribute(a));
  while (el.firstChild) n.appendChild(el.firstChild);
  el.parentNode.insertBefore(n, el);
  el.remove();
  return n;
}
function remapSelection(map) {
  const p = getPts();
  if (!p) return;
  const fix = (q) => (map.has(q.node) ? pt(map.get(q.node), q.offset) : q);
  setPts(fix(p.a), fix(p.f));
}
function setList(kind, host) {
  const blocks = selectedBlocks(host);
  if (!blocks.length) return false;
  const tag = kind === "ul" ? "ul" : "ol";
  const allIn = blocks.every((b) => lname(b) === "li" && lname(b.parentNode) === tag);
  const moved = new Map();
  if (allIn) {
    for (const li of blocks) {
      const list = li.parentNode;
      const para = documentObj.createElement("p");
      while (li.firstChild) para.appendChild(li.firstChild);
      if (!li.nextSibling) list.parentNode.insertBefore(para, list.nextSibling);
      else if (!li.previousSibling) list.parentNode.insertBefore(para, list);
      else { const right = splitTreeAt(pt(list, idx(li) + 1), list); list.parentNode.insertBefore(para, right); }
      moved.set(li, para);
      li.remove();
      pruneEmpty(list, host);
    }
  } else {
    let list = null;
    for (const b of blocks) {
      if (lname(b) === "li") {
        if (lname(b.parentNode) !== tag) { const l = renameBlock(b.parentNode, tag); list = l; moved.set(b.parentNode, l); }
        else list = b.parentNode;
        continue;
      }
      if (!list || list.nextSibling !== b) {
        const prev = b.previousSibling;
        list = prev && lname(prev) === tag ? prev : (() => { const l = documentObj.createElement(tag); b.parentNode.insertBefore(l, b); return l; })();
      }
      const li = documentObj.createElement("li");
      for (const a of b.getAttributeNames()) if (a === "style") li.setAttribute(a, b.getAttribute(a));
      while (b.firstChild) li.appendChild(b.firstChild);
      list.appendChild(li);
      moved.set(b, li);
      b.remove();
      // Absorb a following list of the same kind.
      const nx = list.nextSibling;
      if (nx && lname(nx) === tag) { while (nx.firstChild) list.appendChild(nx.firstChild); nx.remove(); }
    }
  }
  remapSelection(moved);
  return true;
}
function setBlockStyle(host, css) {
  const blocks = selectedBlocks(host);
  for (const b of blocks) for (const d of String(css).split(";")) { const i = d.indexOf(":"); if (i > 0) b.style.setProperty(d.slice(0, i).trim(), d.slice(i + 1).trim()); }
  return blocks.length > 0;
}
function indentBlocks(host, dir) {
  const blocks = selectedBlocks(host);
  for (const b of blocks) {
    const cur = parseFloat(sty(b, "margin-left")) || 0;
    const next = Math.max(0, cur + dir * 40);
    if (next) b.style.setProperty("margin-left", next + "px"); else b.style.removeProperty("margin-left");
  }
  return blocks.length > 0;
}

// ---- history ---------------------------------------------------------------------------------
const histories = new WeakMap();
let chain = false; // the last command was typing, so more typing joins its undo step
function hist(host) { let h = histories.get(host); if (!h) histories.set(host, (h = { undo: [], redo: [], kind: null, t: 0 })); return h; }
function serialize(p, host) { return p && host.contains(p.node) ? { path: pathOf(p.node, host), offset: p.offset } : null; }
function snapshot(host) { const s = getPts(); return { html: host.innerHTML, a: s && serialize(s.a, host), f: s && serialize(s.f, host) }; }
function resolve(s, host) {
  if (!s) return null;
  let n = host;
  for (const i of s.path) { n = kids(n)[i]; if (!n) return null; }
  return pt(n, Math.min(s.offset, nlen(n)));
}
function checkpoint(host, kind) {
  const h = hist(host), now = N.now();
  if (kind === "typing" && chain && h.kind === "typing" && now - h.t < 2000) { h.t = now; return; }
  h.undo.push(snapshot(host));
  if (h.undo.length > 200) h.undo.shift();
  h.redo.length = 0;
  h.kind = kind; h.t = now;
  chain = kind === "typing";
}
function restore(host, s) {
  host.innerHTML = s.html;
  const a = resolve(s.a, host), f = resolve(s.f, host);
  if (a) setPts(a, f || a);
  pending = null; chain = false;
}
function undo(host) { const h = hist(host), s = h.undo.pop(); if (!s) return false; h.redo.push(snapshot(host)); restore(host, s); h.kind = null; return true; }
function redo(host) { const h = hist(host), s = h.redo.pop(); if (!s) return false; h.undo.push(snapshot(host)); restore(host, s); h.kind = null; return true; }

// ---- commands -----------------------------------------------------------------------------------
let ownEvent = false;
function fireInput(host, type, data) {
  const ev = new Event("input", { bubbles: true });
  ev.inputType = type; ev.data = data === undefined ? null : data;
  host.dispatchEvent(ev);
  ownEvent = true;
  try { documentObj.dispatchEvent(new Event("selectionchange")); } finally { ownEvent = false; }
}
// Run `fn` as one undoable edit on the host of the selection.
function edit(kind, type, data, fn) {
  const sp = getPts();
  if (!sp) return false;
  const host = hostOf(sp.f.node);
  if (!host) return false;
  if (kind !== "none") {
    const pre = new Event("beforeinput", { bubbles: true, cancelable: true });
    pre.inputType = type; pre.data = data === undefined ? null : data;
    if (!host.dispatchEvent(pre)) return true;
    checkpoint(host, kind);
  }
  const done = fn(host, sp);
  if (done !== false && kind !== "none") fireInput(host, type, data);
  return done !== false;
}
function deleteSelectionIn(host) {
  const sp = selectedPoints();
  if (!sp || (sp.s.node === sp.e.node && sp.s.offset === sp.e.offset)) return null;
  const caret = deleteRange(sp.s, sp.e, host);
  setPts(caret);
  return caret;
}
function typeText(text, host) {
  let caret = deleteSelectionIn(host);
  if (!caret) caret = getPts().f;
  const want = pending;
  pending = null;
  if (want && Object.keys(want).length) caret = insertStyled(caret, text, want, host);
  else caret = insertTextAt(caret, text, host);
  setPts(caret);
}
function insertStyled(p, text, want, host) {
  // Leave any formatting being switched off, then insert the text wrapped in what is switched on.
  p = textPointForInsert(p);
  let at = p;
  for (const key of Object.keys(want)) {
    const f = FORMATS[key];
    if (f.value) continue;
    if (want[key] === false && formatAt(at.node, key, host)) {
      const a = matchingAncestor(at.node, key, host);
      if (a) {
        const right = splitTreeAt(at, a);
        at = pt(a.parentNode, idx(a) + 1);
        if (kids(a).every(isEmptyInline)) { at = pt(a.parentNode, idx(a)); a.remove(); }
        if (kids(right).every(isEmptyInline)) right.remove();
      }
    }
  }
  const t = documentObj.createTextNode(text);
  if (isTx(at.node)) { const r = at.node.splitText(at.offset); at.node.parentNode.insertBefore(t, r); } else at.node.insertBefore(t, kids(at.node)[at.offset] || null);
  let top = t;
  for (const key of Object.keys(want)) {
    const f = FORMATS[key], v = want[key];
    if (f.value ? v && formatAt(t, key, host) !== v : v === true && !formatAt(t, key, host)) {
      const w = f.make(v);
      top.parentNode.insertBefore(w, top);
      w.appendChild(top);
      top = w;
    }
  }
  fixSpaces(t, host);
  return pt(t, text.length);
}
// A word's worth of steps from `c`: skip separators, then letters and digits.
function wordStep(c, dir, host) {
  const isW = (ch) => /[\p{L}\p{N}_]/u.test(ch);
  let cur = step(c, dir, host);
  if (!cur || cur.kind !== "char") return cur;
  let guard = 0;
  while (guard++ < 10000 && !isW(cur.ch)) { const nx = step(cur.cursor, dir, host); if (!nx || nx.kind !== "char") break; cur = nx; }
  while (guard++ < 20000 && isW(cur.ch)) { const nx = step(cur.cursor, dir, host); if (!nx || nx.kind !== "char" || !isW(nx.ch)) break; cur = nx; }
  return cur;
}
function backspace(host, forward, word) {
  if (deleteSelectionIn(host)) return;
  const sp = getPts();
  const c = toCursor(sp.f, host);
  if (!c) return;
  const s = word ? wordStep(c, forward ? 1 : -1, host) : step(c, forward ? 1 : -1, host);
  if (!s) return;
  const a = toPoint(c), b = toPoint(s.cursor);
  // Backspace at the start of a list item takes it out of the list.
  const block = closestBlock(sp.f.node, host);
  if (!forward && lname(block) === "li" && s.kind === "block" && !(block.previousSibling && lname(block.previousSibling) === "li")) {
    setPts(outdentItem(block, host));
    return;
  }
  // Backspace and Delete don't merge one cell into the next.
  if (cellOf(a.node, host) !== cellOf(b.node, host)) return;
  setPts(deleteRange(a, b, host));
}
function moveCaret(dir, shift, unit, host) {
  const sp = getPts();
  if (!shift && order(sp.a, sp.f) !== 0) {
    // Collapse to the near edge of the selection first.
    const [s, e] = order(sp.a, sp.f) <= 0 ? [sp.a, sp.f] : [sp.f, sp.a];
    setPts(dir < 0 ? s : e);
    return;
  }
  const c = toCursor(sp.f, host);
  if (!c) return;
  const s = unit === "word" ? wordStep(c, dir, host) : step(c, dir, host);
  if (!s) return;
  const p = toPoint(s.cursor);
  setPts(shift ? sp.a : p, p);
}
function selectAllIn(host) {
  const first = firstLeafOf(host), last = lastLeafOf(host);
  const a = first ? toPoint({ leaf: first, pos: 0 }) : pt(host, 0);
  const b = last ? toPoint({ leaf: last, pos: isTx(last) ? last.data.length : 1 }) : pt(host, nlen(host));
  setPts(a, b);
}
function docEdge(end, shift, host) {
  const sp = getPts();
  const leaf = end ? lastLeafOf(host) : firstLeafOf(host);
  const p = leaf ? toPoint({ leaf, pos: end ? (isTx(leaf) ? leaf.data.length : 1) : 0 }) : pt(host, 0);
  setPts(shift ? sp.a : p, p);
}

// Called by the engine for every key pressed in an editing host (after movement keys it handles
// itself). Returns whether it did something.
Object.defineProperty(g, "__editKey", {
  value(key, shift, ctrl, alt, meta) {
    const sp = getPts();
    const host = sp && hostOf(sp.f.node);
    if (!host) return false;
    const cmd = ctrl || meta;
    if (cmd) {
      switch (String(key).toLowerCase()) {
        case "a": selectAllIn(host); return true;
        case "z": return shift ? execCommand("redo") : execCommand("undo");
        case "y": return execCommand("redo");
        case "b": return execCommand("bold");
        case "i": return execCommand("italic");
        case "u": return execCommand("underline");
        case "home": docEdge(false, shift, host); return true;
        case "end": docEdge(true, shift, host); return true;
        case "arrowleft": moveCaret(-1, shift, "word", host); return true;
        case "arrowright": moveCaret(1, shift, "word", host); return true;
        case "backspace": return edit("delete", "deleteWordBackward", undefined, (h) => backspace(h, false, true));
        case "delete": return edit("delete", "deleteWordForward", undefined, (h) => backspace(h, true, true));
      }
      return false;
    }
    switch (key) {
      case "ArrowLeft": moveCaret(-1, shift, alt ? "word" : "char", host); pending = null; return true;
      case "ArrowRight": moveCaret(1, shift, alt ? "word" : "char", host); pending = null; return true;
      case "Backspace": return edit("delete", "deleteContentBackward", undefined, (h) => backspace(h, false, alt));
      case "Delete": return edit("delete", "deleteContentForward", undefined, (h) => backspace(h, true, alt));
      case "Enter": return shift ? execCommand("insertLineBreak") : execCommand("insertParagraph");
      case "Tab": return tabInTable(shift, host);
    }
    if ([...String(key)].length === 1) return edit("typing", "insertText", key, (h) => typeText(key, h));
    return false;
  },
});
// ---- IME composition ---------------------------------------------------------------------
// While the user composes, the preedit text sits in the page as an underlined span (so it flows
// and wraps like real text) without touching the undo history or firing `input`. Committing takes
// the span out and inserts the final text as ordinary typing.
let imeSpan = null;
function imeRemove() {
  if (!imeSpan) return null;
  const span = imeSpan;
  imeSpan = null;
  const parent = span.parentNode;
  if (!parent) return null;
  const at = pt(parent, idx(span));
  span.remove();
  return at;
}
Object.defineProperty(g, "__imePreedit", {
  value(text) {
    text = String(text);
    if (!text) {
      const at = imeRemove();
      if (at) setPts(at);
      return true;
    }
    if (!imeSpan) {
      const sp = getPts();
      const host = sp && hostOf(sp.f.node);
      if (!host) return false;
      let caret = deleteSelectionIn(host) || getPts().f;
      caret = textPointForInsert(caret);
      const right = caret.node.splitText(caret.offset);
      imeSpan = documentObj.createElement("span");
      imeSpan.setAttribute("data-nile-ime", "");
      imeSpan.setAttribute("style", "text-decoration: underline");
      imeSpan.appendChild(documentObj.createTextNode(text));
      caret.node.parentNode.insertBefore(imeSpan, right);
    } else {
      imeSpan.firstChild.data = text;
    }
    setPts(pt(imeSpan.firstChild, text.length));
    return true;
  },
});
Object.defineProperty(g, "__imeCommit", {
  value(text) {
    const at = imeRemove();
    if (at) setPts(at);
    return String(text) ? execCommand("insertText", false, String(text)) : true;
  },
});
Object.defineProperty(g, "__imeActive", { value() { return !!imeSpan; } });

Object.defineProperty(g, "__editPaste", {
  value(text) { return execCommand("insertText", false, String(text)); },
});

// ---- rich copy and paste ------------------------------------------------------------
// The HTML of the selection, for the clipboard.
Object.defineProperty(g, "__selectionHtml", {
  value() {
    const sp = selectedPoints();
    if (!sp || order(sp.s, sp.e) === 0) return "";
    const r = new Range();
    r.startContainer = sp.s.node; r.startOffset = sp.s.offset; r.endContainer = sp.e.node; r.endOffset = sp.e.offset;
    return r.cloneContents().innerHTML;
  },
});
const PASTE_TAGS = new Set(["b", "i", "u", "s", "sub", "sup", "a", "br", "p", "h1", "h2", "h3", "h4", "h5", "h6", "ul", "ol", "li", "blockquote", "pre", "code", "span", "img"]);
const PASTE_RENAME = { strong: "b", em: "i", strike: "s", del: "s", div: "p", font: "span", section: "p", article: "p", header: "p", footer: "p", main: "p" };
const PASTE_DROP = new Set(["script", "style", "head", "title", "meta", "link", "template", "iframe", "object", "embed", "svg", "noscript", "form", "input", "button", "select", "textarea"]);
const PASTE_STYLES = new Set(["color", "background-color", "font-weight", "font-style", "text-decoration", "text-decoration-line", "font-size", "font-family", "text-align"]);
function cleanStyle(css) {
  const keep = [];
  for (const decl of String(css).split(";")) {
    const i = decl.indexOf(":");
    if (i < 0) continue;
    const k = decl.slice(0, i).trim().toLowerCase(), v = decl.slice(i + 1).trim();
    if (PASTE_STYLES.has(k) && v && !/url\(|expression|javascript/i.test(v)) keep.push(k + ": " + v);
  }
  return keep.join("; ");
}
const SAFE_URL = /^\s*(https?:|file:|data:image\/|mailto:|#|\/|\.|[a-z0-9_-]+(\/|\.|$))/i;
// Copy `src`'s children into `dst`, keeping only formatting the editor understands.
function cleanInto(src, dst) {
  for (const n of kids(src)) {
    if (isTx(n)) { dst.appendChild(documentObj.createTextNode(n.data)); continue; }
    if (!isEl(n)) continue;
    let tag = lname(n);
    if (PASTE_DROP.has(tag)) continue;
    tag = PASTE_RENAME[tag] || tag;
    if (!PASTE_TAGS.has(tag)) { cleanInto(n, dst); continue; }
    const el = documentObj.createElement(tag);
    if (tag === "a") {
      const href = n.getAttribute("href");
      if (href && SAFE_URL.test(href) && !/^\s*(javascript|vbscript):/i.test(href)) el.setAttribute("href", href);
    } else if (tag === "img") {
      const s = n.getAttribute("src");
      if (!s || !SAFE_URL.test(s)) continue;
      el.setAttribute("src", s);
      for (const a of ["width", "height", "alt"]) { const v = n.getAttribute(a); if (v !== null && /^[\w .%-]*$/.test(v)) el.setAttribute(a, v); }
    }
    const css = n.getAttribute("style") || (lname(n) === "font" && n.getAttribute("color") ? "color: " + n.getAttribute("color") : "");
    if (css) { const c = cleanStyle(css); if (c) el.setAttribute("style", c); }
    cleanInto(n, el);
    dst.appendChild(el);
  }
}
const PASTE_BLOCKS = new Set(["p", "h1", "h2", "h3", "h4", "h5", "h6", "ul", "ol", "blockquote", "pre", "li"]);
const MERGEABLE = new Set(["p", "h1", "h2", "h3", "h4", "h5", "h6", "blockquote", "pre", "li"]);
// Clean pasted HTML into a list of blocks (loose inline runs get a paragraph of their own).
function pastedBlocks(html) {
  const tmp = documentObj.createElement("div");
  tmp.innerHTML = String(html);
  const clean = documentObj.createElement("div");
  cleanInto(tmp, clean);
  const blocks = [];
  let run = null;
  for (const n of kids(clean)) {
    if (isEl(n) && PASTE_BLOCKS.has(lname(n))) { run = null; blocks.push(n); continue; }
    if (isTx(n) && !/\S/.test(n.data) && !run) continue; // whitespace between blocks
    if (!run) { run = documentObj.createElement("p"); blocks.push(run); }
    run.appendChild(n.nodeType === 3 ? documentObj.createTextNode(n.data.replace(/\s+/g, " ")) : n);
  }
  return blocks.filter((b) => kids(b).length > 0);
}
function insertNodesAt(at, nodes) {
  at = textPointForInsert(at);
  let last = null;
  for (const n of nodes) {
    if (isTx(at.node)) { const r = at.node.splitText(at.offset); at.node.parentNode.insertBefore(n, r); at = pt(at.node.parentNode, idx(n) + 1); } else { at.node.insertBefore(n, kids(at.node)[at.offset] || null); at = pt(at.node, at.offset + 1); }
    last = n;
  }
  return last ? pt(last.parentNode, idx(last) + 1) : at;
}
function dropPlaceholder(block) {
  for (const c of kids(block)) if ((isTx(c) && c.data === "") || lname(c) === "br") c.remove();
}
function pasteBlocks(blocks, host) {
  let caret = deleteSelectionIn(host) || getPts().f;
  if (blocks.length === 1) {
    // One paragraph's worth goes into the text at the caret.
    const end = insertNodesAt(caret, kids(blocks[0]).slice());
    setPts(leafToPoint(end, host));
    return;
  }
  const block = closestBlock(caret.node, host);
  if (block === host) {
    setPts(leafToPoint(insertNodesAt(caret, blocks), host));
    return;
  }
  const right = splitTreeAt(textPointForInsert(caret), block);
  if (kids(block).every(isEmptyInline)) dropPlaceholder(block);
  if (kids(right).every(isEmptyInline)) dropPlaceholder(right);
  let mids = blocks.slice();
  if (MERGEABLE.has(lname(mids[0])) && lname(block) !== "li") {
    const first = mids.shift();
    for (const c of kids(first).slice()) block.appendChild(c);
  }
  let tail = null;
  if (mids.length && MERGEABLE.has(lname(mids[mids.length - 1])) && lname(right) !== "li") tail = mids.pop();
  for (const m of mids) block.parentNode.insertBefore(m, right);
  let end = pt(right, 0);
  if (tail) {
    // The tail's text leads the right-hand block; the caret sits where it ends.
    const moved = kids(tail).slice();
    const before = kids(right)[0] || null;
    for (const c of moved) right.insertBefore(c, before);
    end = pt(right, moved.length);
  }
  ensureContent(block, host);
  ensureContent(right, host);
  setPts(leafToPoint(end, host));
}
Object.defineProperty(g, "__editPasteHtml", {
  value(html, text) {
    const blocks = pastedBlocks(html);
    if (!blocks.length) return text ? execCommand("insertText", false, String(text)) : false;
    return edit("structure", "insertFromPaste", undefined, (h) => pasteBlocks(blocks, h));
  },
});

// A host that was focused by a script has no caret yet; put one at its start.
function ensureSelection() {
  let sp = getPts();
  if (sp) return sp;
  const h = documentObj.activeElement && hostOf(documentObj.activeElement);
  if (!h) return null;
  const first = firstLeafOf(h);
  setPts(first ? toPoint({ leaf: first, pos: 0 }) : pt(h, 0));
  return getPts();
}
function execCommand(cmd, ui, value) {
  cmd = String(cmd).toLowerCase();
  const sp = ensureSelection();
  const host = sp && hostOf(sp.f.node);
  if (!host) return false;
  const inline = (key, v) => edit("format", cmd, v, (h) => applyFormat(key, v, h));
  switch (cmd) {
    case "bold": return inline("bold");
    case "italic": return inline("italic");
    case "underline": return inline("underline");
    case "strikethrough": return inline("strike");
    case "subscript": return inline("sub");
    case "superscript": return inline("sup");
    case "forecolor": return inline("foreColor", String(value));
    case "backcolor": case "hilitecolor": return inline("hiliteColor", String(value));
    case "fontname": return inline("fontName", String(value));
    case "fontsize": return inline("fontSize", /^\d+$/.test(String(value)) ? ["10px", "13px", "16px", "18px", "24px", "32px", "48px"][Math.max(1, Math.min(7, +value)) - 1] : String(value));
    case "fontsizepx": return inline("fontSize", /px$/.test(String(value)) ? String(value) : value + "px");
    case "removeformat": return edit("format", cmd, undefined, (h) => { for (const k of Object.keys(FORMATS)) { const nodes = coveredText(h); for (const t of nodes) for (let a = matchingAncestor(t, k, h); a; a = matchingAncestor(t, k, h)) stripFormat(isolate(t, a), k); } });
    case "undo": return undo(host);
    case "redo": return redo(host);
    case "selectall": selectAllIn(host); return true;
    case "inserttext": return edit("typing", "insertText", String(value), (h) => {
      const lines = String(value).replace(/\r\n?/g, "\n").split("\n");
      lines.forEach((line, i) => {
        if (i > 0) { const caret = splitBlock(deleteSelectionIn(h) || getPts().f, h); setPts(caret); }
        if (line) typeText(line, h);
      });
    });
    case "insertparagraph": return edit("structure", "insertParagraph", undefined, (h) => { const c = deleteSelectionIn(h) || getPts().f; setPts(splitBlock(c, h)); pending = null; });
    case "insertlinebreak": return edit("structure", "insertLineBreak", undefined, (h) => { const c = deleteSelectionIn(h) || getPts().f; setPts(insertLineBreak(c, h)); });
    case "delete": return edit("delete", "deleteContentBackward", undefined, (h) => backspace(h, false, false));
    case "forwarddelete": return edit("delete", "deleteContentForward", undefined, (h) => backspace(h, true, false));
    case "justifyleft": case "justifycenter": case "justifyright": case "justifyfull":
      return edit("format", cmd, undefined, (h) => setBlockStyle(h, "text-align:" + { justifyleft: "left", justifycenter: "center", justifyright: "right", justifyfull: "justify" }[cmd]));
    case "insertunorderedlist": return edit("structure", cmd, undefined, (h) => setList("ul", h));
    case "insertorderedlist": return edit("structure", cmd, undefined, (h) => setList("ol", h));
    case "indent": return edit("format", cmd, undefined, (h) => indentBlocks(h, 1));
    case "outdent": return edit("format", cmd, undefined, (h) => indentBlocks(h, -1));
    case "formatblock": return edit("structure", cmd, undefined, (h) => {
      const tag = String(value).replace(/[<>]/g, "").toLowerCase();
      const map = new Map();
      for (const b of selectedBlocks(h)) if (lname(b) !== "li" && b !== h) map.set(b, renameBlock(b, tag));
      remapSelection(map);
    });
    case "blockstyle": return edit("format", cmd, undefined, (h) => setBlockStyle(h, String(value)));
    case "inserthorizontalrule": return edit("structure", cmd, undefined, (h) => {
      const c = deleteSelectionIn(h) || getPts().f;
      const block = closestBlock(c.node, h);
      const hr = documentObj.createElement("hr");
      block.parentNode.insertBefore(hr, block.nextSibling);
    });
    case "createlink": return edit("structure", cmd, undefined, (h) => {
      for (const t of coveredText(h)) { const a = documentObj.createElement("a"); a.setAttribute("href", String(value)); t.parentNode.insertBefore(a, t); a.appendChild(t); }
    });
    case "unlink": return edit("structure", cmd, undefined, (h) => { for (const t of coveredText(h)) for (let e = t.parentNode; isEl(e) && e !== h; e = e.parentNode) if (e.localName === "a") { unwrap(e); break; } });
    case "insertimage": return edit("structure", cmd, undefined, (h) => {
      const c = deleteSelectionIn(h) || getPts().f;
      const img = documentObj.createElement("img");
      img.setAttribute("src", String(value));
      if (isTx(c.node)) { const r = c.node.splitText(c.offset); c.node.parentNode.insertBefore(img, r); } else c.node.insertBefore(img, kids(c.node)[c.offset] || null);
      setPts(pt(img.parentNode, idx(img) + 1));
    });
    case "inserttable": return edit("structure", cmd, undefined, (h) => {
      const m = /^\s*(\d+)\s*[x,*]\s*(\d+)\s*$/i.exec(String(value));
      const [r, c] = m ? [Math.min(+m[1], 50), Math.min(+m[2], 20)] : [2, 2];
      insertTable(Math.max(r, 1), Math.max(c, 1), h);
    });
    case "inserthtml": return edit("structure", cmd, undefined, (h) => {
      const c = deleteSelectionIn(h) || getPts().f;
      const tmp = documentObj.createElement("div");
      tmp.innerHTML = String(value);
      const nodes = kids(tmp);
      let at = textPointForInsert(c);
      let last = null;
      for (const n of nodes) {
        if (isTx(at.node)) { const r = at.node.splitText(at.offset); at.node.parentNode.insertBefore(n, r); at = pt(at.node.parentNode, idx(n) + 1); } else { at.node.insertBefore(n, kids(at.node)[at.offset] || null); at = pt(at.node, at.offset + 1); }
        last = n;
      }
      if (last) setPts(leafToPoint(pt(last.parentNode, idx(last) + 1), h));
    });
    case "copy": case "cut": case "paste": N.clipboardRequest(cmd); return true;
  }
  return false;
}
documentObj.execCommand = execCommand;
documentObj.queryCommandSupported = (cmd) => /^(bold|italic|underline|strikethrough|subscript|superscript|forecolor|backcolor|hilitecolor|fontname|fontsize|fontsizepx|removeformat|undo|redo|selectall|inserttext|insertparagraph|insertlinebreak|delete|forwarddelete|justify(left|center|right|full)|insertunorderedlist|insertorderedlist|indent|outdent|formatblock|blockstyle|inserthorizontalrule|createlink|unlink|insertimage|inserthtml|inserttable)$/.test(String(cmd).toLowerCase());
documentObj.queryCommandEnabled = (cmd) => {
  const sp = getPts();
  const host = sp && hostOf(sp.f.node);
  if (!host) return false;
  const c = String(cmd).toLowerCase();
  if (c === "undo") return hist(host).undo.length > 0;
  if (c === "redo") return hist(host).redo.length > 0;
  return documentObj.queryCommandSupported(c);
};
documentObj.queryCommandState = (cmd) => {
  const sp = selectedPoints();
  const host = sp && hostOf(sp.s.node);
  if (!host) return false;
  const c = String(cmd).toLowerCase();
  const key = { bold: "bold", italic: "italic", underline: "underline", strikethrough: "strike", subscript: "sub", superscript: "sup" }[c];
  if (key) {
    const collapsed = sp.s.node === sp.e.node && sp.s.offset === sp.e.offset;
    if (collapsed) {
      if (pending && key in pending) return !!pending[key];
      const cur = toCursor(sp.s, host);
      return !!formatAt(cur ? cur.leaf : sp.s.node, key, host);
    }
    const nodes = textNodesBetween(sp.s, sp.e).filter((n) => /\S/.test(n.data));
    return nodes.length > 0 && nodes.every((n) => formatAt(n, key, host));
  }
  const blocks = selectedBlocks(host);
  if (c === "insertunorderedlist" || c === "insertorderedlist") return blocks.length > 0 && blocks.every((b) => lname(b) === "li" && lname(b.parentNode) === (c === "insertunorderedlist" ? "ul" : "ol"));
  const align = { justifyleft: "left", justifycenter: "center", justifyright: "right", justifyfull: "justify" }[c];
  if (align) return blocks.length > 0 && blocks.every((b) => (sty(b, "text-align") || "left") === align);
  return false;
};
documentObj.queryCommandValue = (cmd) => {
  const sp = selectedPoints();
  const host = sp && hostOf(sp.s.node);
  if (!host) return "";
  const c = String(cmd).toLowerCase();
  const cur = toCursor(sp.s, host);
  const leaf = cur ? cur.leaf : sp.s.node;
  const key = { forecolor: "foreColor", backcolor: "hiliteColor", hilitecolor: "hiliteColor", fontname: "fontName", fontsize: "fontSize", fontsizepx: "fontSize" }[c];
  if (key) return formatAt(leaf, key, host) || "";
  if (c === "formatblock") return lname(closestBlock(leaf, host));
  return "";
};
// The user moved the caret (mouse, up/down): typing starts a new undo step and drops a pending format.
documentObj.addEventListener("selectionchange", () => { if (!ownEvent) { chain = false; pending = null; } });
