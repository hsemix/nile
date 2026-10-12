// Loaded with <script src>. Reads layout results and writes them into the page.
function describe(el) {
  const r = el.getBoundingClientRect();
  const cs = getComputedStyle(el);
  return `${el.offsetWidth}×${el.offsetHeight} at (${Math.round(r.left)}, ${Math.round(r.top)}), ` +
         `padding ${cs.paddingLeft}, color ${cs.color}`;
}

for (const box of document.querySelectorAll('.box')) {
  box.querySelector('.info').textContent = describe(box);
}

// Reads see writes immediately: grow the box, then measure again.
const grow = document.getElementById('grow');
const before = grow.offsetHeight;
grow.style.height = '90px';
grow.querySelector('.info').textContent = `height ${before} → ${grow.offsetHeight} after setting style.height`;

// An inline element's rect is the union of its text runs.
const em = document.getElementById('inline');
document.getElementById('inline-info').textContent =
  `inline <em> is ${Math.round(em.getBoundingClientRect().width)}px wide`;

console.log('viewport', innerWidth, 'x', innerHeight, '| content boxes:', document.querySelectorAll('.box').length);
