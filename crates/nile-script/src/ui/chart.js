// @nile/ui/chart: <nile-chart>, bar, line, area, pie and donut charts drawn as SVG.
//
//   <nile-chart id="sales" type="bar" height="260"></nile-chart>
//   sales.data = { labels: ["Jan", "Feb", "Mar"], series: [{ name: "2026", values: [120, 90, 150] }, { name: "2025", values: [80, 110, 95] }] };
//   pie.data = [{ label: "Cash", value: 62 }, { label: "Card", value: 31 }, { label: "Other", value: 7 }];   // one series, per-slice labels
//
// Attributes: type (bar, line, area, pie, donut), height (px, default 260), legend (bottom, top, none), stacked
// (bar/area), smooth (line/area), min / max (value axis; default 0 and the data's maximum, rounded up), grid
// ("false" hides the rules), points ("false" hides line markers). Properties: data, colors (array), format(value)
// (text for the axis and the tooltip), title. A series may carry its own `color`. Hovering shows a tooltip with the
// values at that point; clicking a legend entry hides or shows its series; clicking the plot raises `chartclick`
// (detail: { index, label, series: [{ name, value }] }). The chart redraws when its size or data changes.
// Style with --nile-chart-text / -grid / -tooltip-bg / -tooltip-color, or .nile-chart-tooltip, .nile-chart-legend classes.

import { style, emit, flagAttr, numberAttr } from "@nile/ui/base";

const CSS = `
:where(nile-chart) { display: block; position: relative; color: var(--nile-chart-text, inherit); }
:where(nile-chart) > .nile-chart-plot { position: relative; width: 100%; }
:where(nile-chart) svg { display: block; }
:where(.nile-chart-legend) { display: flex; flex-wrap: wrap; justify-content: center; gap: 4px 14px; padding: 6px 4px; font-size: 13px; }
:where(.nile-chart-key) { display: inline-flex; align-items: center; gap: 6px; cursor: pointer; }
:where(.nile-chart-key.nile-off) { opacity: 0.4; text-decoration: line-through; }
:where(.nile-chart-swatch) { display: inline-block; width: 10px; height: 10px; border-radius: 2px; }
:where(.nile-chart-tooltip) { position: absolute; z-index: 5; display: none; pointer-events: none; padding: 6px 9px; border-radius: 5px; font-size: 12px; line-height: 1.5; white-space: nowrap;
  background: var(--nile-chart-tooltip-bg, rgba(30, 41, 59, 0.94)); color: var(--nile-chart-tooltip-color, #fff); box-shadow: 0 4px 14px rgba(0, 0, 0, 0.3); }
:where(.nile-chart-cursor) { position: absolute; display: none; pointer-events: none; background: rgba(127, 127, 127, 0.14); }
`;

const PALETTE = ["#2563eb", "#f59e0b", "#10b981", "#ef4444", "#8b5cf6", "#06b6d4", "#ec4899", "#84cc16"];

function esc(s) {
  return String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

/** 1,234.5 style number. */
function plain(v) {
  const n = Number(v);
  if (!Number.isFinite(n)) return String(v);
  const fixed = Math.abs(n) >= 100 || n === Math.round(n) ? String(Math.round(n * 100) / 100) : String(Math.round(n * 100) / 100);
  const parts = fixed.split(".");
  parts[0] = parts[0].replace(/\B(?=(\d{3})+(?!\d))/g, ",");
  return parts.join(".");
}

/** Round tick positions covering [lo, hi]: about `count` steps of 1, 2, 2.5, 5 x 10^n. */
export function niceScale(lo, hi, count) {
  if (!(hi > lo)) hi = lo + 1;
  const raw = (hi - lo) / Math.max(1, count || 5);
  const mag = Math.pow(10, Math.floor(Math.log10(raw)));
  const step = [1, 2, 2.5, 5, 10].map((m) => m * mag).find((s) => s >= raw) || 10 * mag;
  const start = Math.floor(lo / step) * step;
  const end = Math.ceil(hi / step) * step;
  const ticks = [];
  for (let v = start; v <= end + step / 1000; v += step) ticks.push(Math.round(v / step * 1e6) / 1e6 * step);
  return { min: start, max: end, step: step, ticks: ticks };
}

function normalise(data) {
  if (Array.isArray(data)) {
    return { labels: data.map((d) => String(d.label)), series: [{ name: "", values: data.map((d) => Number(d.value)), colors: data.map((d) => d.color) }] };
  }
  const d = data || { labels: [], series: [] };
  return { labels: (d.labels || []).map(String), series: (d.series || []).map((s) => ({ name: s.name || "", values: (s.values || []).map(Number), color: s.color })) };
}

export function defineChart() {
  if (typeof customElements === "undefined" || typeof HTMLElement === "undefined") return;
  if (customElements.get && customElements.get("nile-chart")) return;
  style("chart", CSS);

  class NileChart extends HTMLElement {
    static get observedAttributes() {
      return ["type", "height", "legend", "stacked", "smooth", "min", "max", "grid", "points"];
    }
    constructor() {
      super();
      this._data = { labels: [], series: [] };
      this._hidden = {};
      this._colors = PALETTE;
      this._fmt = null;
    }
    connectedCallback() {
      if (this._plot) return;
      this._legendTop = document.createElement("div");
      this._legendTop.className = "nile-chart-legend";
      this._plot = document.createElement("div");
      this._plot.className = "nile-chart-plot";
      this._legendBottom = document.createElement("div");
      this._legendBottom.className = "nile-chart-legend";
      this._tip = document.createElement("div");
      this._tip.className = "nile-chart-tooltip";
      this._cursor = document.createElement("div");
      this._cursor.className = "nile-chart-cursor";
      this.appendChild(this._legendTop);
      this.appendChild(this._plot);
      this.appendChild(this._legendBottom);
      this._plot.addEventListener("mousemove", (e) => this._hover(e));
      this._plot.addEventListener("mouseleave", () => this._unhover());
      this._plot.addEventListener("click", (e) => this._click(e));
      if (typeof ResizeObserver !== "undefined") {
        this._ro = new ResizeObserver(() => {
          const w = Math.round(this._plot.getBoundingClientRect().width);
          if (w !== this._w) this._draw();
        });
        this._ro.observe(this);
      }
      this._draw();
    }
    disconnectedCallback() {
      if (this._ro) this._ro.disconnect();
    }
    attributeChangedCallback() {
      if (this._plot) this._draw();
    }
    get data() {
      return this._data;
    }
    set data(d) {
      this._data = normalise(d);
      this._hidden = {};
      if (this._plot) this._draw();
    }
    get colors() {
      return this._colors;
    }
    set colors(c) {
      this._colors = c && c.length ? c : PALETTE;
      if (this._plot) this._draw();
    }
    get format() {
      return this._fmt;
    }
    set format(fn) {
      this._fmt = fn;
      if (this._plot) this._draw();
    }
    _f(v) {
      return this._fmt ? this._fmt(v) : plain(v);
    }
    _color(series, i) {
      return series.color || this._colors[i % this._colors.length];
    }
    // ---- drawing ------------------------------------------------------------------------------------------------------
    _draw() {
      if (!this._plot) return;
      const type = this.getAttribute("type") || "bar";
      const height = numberAttr(this, "height", 260);
      const box = this._plot.getBoundingClientRect();
      const width = Math.round(box.width) || Math.round(this.getBoundingClientRect().width) || 0;
      this._w = width;
      this._plot.style.height = height + "px";
      this._drawLegend(type);
      if (width < 40) return;
      const d = this._data;
      const show = d.series.map((s, i) => !this._hidden[i]);
      const body = type === "pie" || type === "donut" ? this._pie(width, height, type === "donut") : this._axes(type, width, height, show);
      this._plot.innerHTML = "";
      const holder = document.createElement("div");
      holder.innerHTML = '<svg xmlns="http://www.w3.org/2000/svg" width="' + width + '" height="' + height + '" viewBox="0 0 ' + width + " " + height + '" font-family="sans-serif" font-size="12" fill="currentColor">' + body + "</svg>";
      this._plot.appendChild(holder.firstChild);
      this._plot.appendChild(this._cursor);
      this._plot.appendChild(this._tip);
    }
    _drawLegend(type) {
      const where = this.getAttribute("legend") || (this._data.series.length > 1 || type === "pie" || type === "donut" ? "bottom" : "none");
      this._legendTop.style.display = where === "top" ? "" : "none";
      this._legendBottom.style.display = where === "bottom" ? "" : "none";
      const host = where === "top" ? this._legendTop : this._legendBottom;
      host.innerHTML = "";
      if (where === "none") return;
      const pie = type === "pie" || type === "donut";
      const entries = pie ? this._data.labels.map((l, i) => ({ name: l, color: this._sliceColor(i) })) : this._data.series.map((s, i) => ({ name: s.name, color: this._color(s, i) }));
      entries.forEach((e, i) => {
        const key = document.createElement("span");
        key.className = "nile-chart-key" + (this._hidden[i] ? " nile-off" : "");
        const sw = document.createElement("span");
        sw.className = "nile-chart-swatch";
        sw.style.background = e.color;
        key.appendChild(sw);
        key.appendChild(document.createTextNode(e.name));
        key.addEventListener("click", () => {
          this._hidden[i] = !this._hidden[i];
          this._draw();
        });
        host.appendChild(key);
      });
    }
    _sliceColor(i) {
      const s = this._data.series[0];
      return (s && s.colors && s.colors[i]) || this._colors[i % this._colors.length];
    }
    _axes(type, width, height, show) {
      const d = this._data;
      const stacked = this.hasAttribute("stacked") && (type === "bar" || type === "area");
      const n = d.labels.length;
      const live = d.series.map((s, i) => i).filter((i) => show[i]);
      // Value range.
      let hi = -Infinity, lo = Infinity;
      for (let c = 0; c < n; c++) {
        let pos = 0, neg = 0;
        live.forEach((i) => {
          const v = d.series[i].values[c] || 0;
          if (stacked) {
            if (v >= 0) pos += v;
            else neg += v;
          } else {
            hi = Math.max(hi, v);
            lo = Math.min(lo, v);
          }
        });
        if (stacked) {
          hi = Math.max(hi, pos);
          lo = Math.min(lo, neg);
        }
      }
      if (!Number.isFinite(hi)) hi = 1;
      if (!Number.isFinite(lo)) lo = 0;
      lo = Math.min(lo, 0);
      const min = this.hasAttribute("min") ? numberAttr(this, "min", lo) : lo;
      const maxWanted = this.hasAttribute("max") ? numberAttr(this, "max", hi) : hi;
      const scale = niceScale(min, maxWanted, Math.max(2, Math.round(height / 50)));
      const vmin = this.hasAttribute("min") ? min : scale.min;
      const vmax = this.hasAttribute("max") ? maxWanted : scale.max;
      const labels = scale.ticks.filter((t) => t >= vmin - 1e-9 && t <= vmax + 1e-9).map((t) => this._f(t));
      const left = Math.max(34, Math.max.apply(null, labels.map((l) => l.length)) * 7 + 14);
      const top = 12, right = 14, bottom = 28;
      const w = width - left - right, h = height - top - bottom;
      this._geo = { type: type, left: left, top: top, w: w, h: h, n: n, live: live, stacked: stacked, vmin: vmin, vmax: vmax };
      const y = (v) => top + h - ((v - vmin) / (vmax - vmin || 1)) * h;
      const band = w / Math.max(1, n);
      let out = "";
      const grid = flagAttr(this, "grid", true);
      scale.ticks.forEach((t) => {
        if (t < vmin - 1e-9 || t > vmax + 1e-9) return;
        const ty = y(t);
        if (grid) out += '<line x1="' + left + '" x2="' + (left + w) + '" y1="' + ty + '" y2="' + ty + '" stroke="currentColor" stroke-opacity="0.14"/>';
        out += '<text x="' + (left - 8) + '" y="' + (ty + 4) + '" text-anchor="end" fill-opacity="0.7">' + esc(this._f(t)) + "</text>";
      });
      out += '<line x1="' + left + '" x2="' + (left + w) + '" y1="' + y(Math.max(vmin, 0)) + '" y2="' + y(Math.max(vmin, 0)) + '" stroke="currentColor" stroke-opacity="0.4"/>';
      // Category labels (thinned when they would collide).
      const every = Math.max(1, Math.ceil((Math.max.apply(null, d.labels.map((l) => l.length).concat([1])) * 7 + 8) / band));
      d.labels.forEach((l, c) => {
        if (c % every) return;
        out += '<text x="' + (left + band * (c + 0.5)) + '" y="' + (height - 9) + '" text-anchor="middle" fill-opacity="0.8">' + esc(l) + "</text>";
      });
      if (type === "bar") {
        const group = stacked ? 1 : Math.max(1, live.length);
        const gap = Math.min(band * 0.25, 18);
        const bw = Math.max(1, (band - gap) / group);
        const base = y(Math.max(vmin, 0));
        for (let c = 0; c < n; c++) {
          let pos = 0, neg = 0;
          live.forEach((i, k) => {
            const v = d.series[i].values[c] || 0;
            let from, to;
            if (stacked) {
              if (v >= 0) {
                from = pos;
                pos += v;
                to = pos;
              } else {
                from = neg;
                neg += v;
                to = neg;
              }
            } else {
              from = 0;
              to = v;
            }
            const x = left + band * c + gap / 2 + (stacked ? 0 : bw * k);
            const y1 = y(to), y0 = y(from);
            const rect = Math.abs(y0 - y1);
            if (rect > 0) out += '<rect x="' + x + '" y="' + Math.min(y0, y1) + '" width="' + Math.max(1, bw - (stacked ? 0 : 1)) + '" height="' + rect + '" rx="2" fill="' + this._color(d.series[i], i) + '"/>';
          });
        }
        void base;
      } else {
        const smooth = this.hasAttribute("smooth");
        const dots = flagAttr(this, "points", true);
        const acc = new Array(n).fill(0);
        const floor = y(Math.max(vmin, 0));
        let below = null; // the layer underneath (stacked areas fill between two lines)
        live.forEach((i) => {
          const s = d.series[i];
          const color = this._color(s, i);
          const pts = [];
          for (let c = 0; c < n; c++) {
            const v = s.values[c] || 0;
            const level = stacked ? (acc[c] += v) : v;
            pts.push([left + band * (c + 0.5), y(level), c]);
          }
          const path = smooth ? curve(pts) : "M" + pts.map((p) => p[0] + " " + p[1]).join("L");
          if (type === "area" && pts.length) {
            const lower = below ? below.slice().reverse().map((p) => "L" + p[0] + " " + p[1]).join("") : "L" + pts[pts.length - 1][0] + " " + floor + "L" + pts[0][0] + " " + floor;
            out += '<path d="' + path + lower + 'Z" fill="' + color + '" fill-opacity="0.3"/>';
          }
          if (stacked) below = pts;
          if (pts.length) out += '<path d="' + path + '" fill="none" stroke="' + color + '" stroke-width="2" stroke-linejoin="round"/>';
          if (dots) pts.forEach((p) => (out += '<circle cx="' + p[0] + '" cy="' + p[1] + '" r="3" fill="' + color + '"/>'));
        });
      }
      return out;
    }
    _pie(width, height, donut) {
      const d = this._data;
      const s = d.series[0] || { values: [] };
      const vals = s.values.map((v, i) => (this._hidden[i] ? 0 : Math.max(0, v || 0)));
      const total = vals.reduce((a, b) => a + b, 0);
      const r = Math.max(10, Math.min(width, height) / 2 - 10);
      const cx = width / 2, cy = height / 2;
      this._geo = { type: donut ? "donut" : "pie", cx: cx, cy: cy, r: r, inner: donut ? r * 0.58 : 0, total: total, vals: vals };
      if (!total) return '<text x="' + cx + '" y="' + cy + '" text-anchor="middle" fill-opacity="0.6">No data</text>';
      let out = "";
      let a0 = -Math.PI / 2;
      vals.forEach((v, i) => {
        if (!v) return;
        const sweep = (v / total) * Math.PI * 2;
        const color = this._sliceColor(i);
        if (v === total) {
          out += '<circle cx="' + cx + '" cy="' + cy + '" r="' + r + '" fill="' + color + '"/>';
          if (donut) out += '<circle cx="' + cx + '" cy="' + cy + '" r="' + r * 0.58 + '" fill="var(--nile-chart-hole, #fff)"/>';
        } else {
          out += '<path d="' + arc(cx, cy, r, donut ? r * 0.58 : 0, a0, a0 + sweep) + '" fill="' + color + '" stroke="var(--nile-chart-gap, #fff)" stroke-width="1.5"/>';
        }
        a0 += sweep;
      });
      return out;
    }
    // ---- pointer ------------------------------------------------------------------------------------------------------
    _at(e) {
      const r = this._plot.getBoundingClientRect();
      return { x: e.clientX - r.left, y: e.clientY - r.top, w: r.width };
    }
    _index(p) {
      const g = this._geo;
      if (!g) return -1;
      if (g.type === "pie" || g.type === "donut") {
        const dx = p.x - g.cx, dy = p.y - g.cy;
        const dist = Math.sqrt(dx * dx + dy * dy);
        if (dist > g.r || dist < g.inner || !g.total) return -1;
        let a = Math.atan2(dy, dx) + Math.PI / 2;
        if (a < 0) a += Math.PI * 2;
        let acc = 0;
        for (let i = 0; i < g.vals.length; i++) {
          acc += (g.vals[i] / g.total) * Math.PI * 2;
          if (a <= acc && g.vals[i]) return i;
        }
        return -1;
      }
      if (p.x < g.left || p.x > g.left + g.w || p.y < g.top || p.y > g.top + g.h || !g.n) return -1;
      return Math.min(g.n - 1, Math.floor(((p.x - g.left) / g.w) * g.n));
    }
    _rows(i) {
      const d = this._data;
      const g = this._geo;
      if (g.type === "pie" || g.type === "donut") {
        const v = g.vals[i];
        return { title: d.labels[i], rows: [{ name: this._pct(v, g.total), value: v, color: this._sliceColor(i) }] };
      }
      return { title: d.labels[i], rows: g.live.map((k) => ({ name: d.series[k].name, value: d.series[k].values[i] || 0, color: this._color(d.series[k], k) })) };
    }
    _pct(v, total) {
      return Math.round((v / (total || 1)) * 1000) / 10 + "%";
    }
    _hover(e) {
      const p = this._at(e);
      const i = this._index(p);
      if (i < 0) {
        this._unhover();
        return;
      }
      const info = this._rows(i);
      this._tip.innerHTML = "";
      const head = document.createElement("div");
      head.style.fontWeight = "600";
      head.textContent = info.title;
      this._tip.appendChild(head);
      info.rows.forEach((row) => {
        const line = document.createElement("div");
        const sw = document.createElement("span");
        sw.className = "nile-chart-swatch";
        sw.style.background = row.color;
        sw.style.marginRight = "6px";
        line.appendChild(sw);
        line.appendChild(document.createTextNode((row.name ? row.name + ": " : "") + this._f(row.value)));
        this._tip.appendChild(line);
      });
      this._tip.style.display = "block";
      const tw = this._tip.offsetWidth || 120;
      this._tip.style.left = Math.max(0, Math.min(p.x + 14, p.w - tw - 4)) + "px";
      this._tip.style.top = Math.max(0, p.y - 10 - (this._tip.offsetHeight || 40)) + "px";
      const g = this._geo;
      if (g.type === "pie" || g.type === "donut") this._cursor.style.display = "none";
      else {
        const band = g.w / g.n;
        this._cursor.style.display = "block";
        this._cursor.style.left = g.left + band * i + "px";
        this._cursor.style.top = g.top + "px";
        this._cursor.style.width = band + "px";
        this._cursor.style.height = g.h + "px";
      }
    }
    _unhover() {
      this._tip.style.display = "none";
      this._cursor.style.display = "none";
    }
    _click(e) {
      const i = this._index(this._at(e));
      if (i < 0) return;
      const info = this._rows(i);
      emit(this, "chartclick", { index: i, label: info.title, series: info.rows.map((r) => ({ name: r.name, value: r.value })) }, false);
    }
  }
  customElements.define("nile-chart", NileChart);
}

/** An SVG slice (ring segment when `r0 > 0`) from angle a0 to a1 (radians). */
function arc(cx, cy, r, r0, a0, a1) {
  const p = (rad, a) => [cx + rad * Math.cos(a), cy + rad * Math.sin(a)];
  const large = a1 - a0 > Math.PI ? 1 : 0;
  const [x0, y0] = p(r, a0), [x1, y1] = p(r, a1);
  if (!r0) return "M" + cx + " " + cy + "L" + x0 + " " + y0 + "A" + r + " " + r + " 0 " + large + " 1 " + x1 + " " + y1 + "Z";
  const [x2, y2] = p(r0, a1), [x3, y3] = p(r0, a0);
  return "M" + x0 + " " + y0 + "A" + r + " " + r + " 0 " + large + " 1 " + x1 + " " + y1 + "L" + x2 + " " + y2 + "A" + r0 + " " + r0 + " 0 " + large + " 0 " + x3 + " " + y3 + "Z";
}

/** A smooth path through points (Catmull-Rom as cubic Beziers). */
function curve(pts) {
  if (pts.length < 3) return "M" + pts.map((p) => p[0] + " " + p[1]).join("L");
  let d = "M" + pts[0][0] + " " + pts[0][1];
  for (let i = 0; i < pts.length - 1; i++) {
    const p0 = pts[i - 1] || pts[i], p1 = pts[i], p2 = pts[i + 1], p3 = pts[i + 2] || p2;
    d += "C" + (p1[0] + (p2[0] - p0[0]) / 6) + " " + (p1[1] + (p2[1] - p0[1]) / 6) + " " + (p2[0] - (p3[0] - p1[0]) / 6) + " " + (p2[1] - (p3[1] - p1[1]) / 6) + " " + p2[0] + " " + p2[1];
  }
  return d;
}

defineChart();
