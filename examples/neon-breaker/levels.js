// Validate the complete campaign before it can replace a run.
export function parseLevels(text) {
  if (text.length > 16384) throw new Error('Level files must be at most 16 KB.');
  let data;
  try { data = JSON.parse(text); } catch (_) { throw new Error('The file is not valid JSON.'); }
  if (!data || data.version !== 1 || !Array.isArray(data.sectors) || data.sectors.length !== 3)
    throw new Error('Expected version 1 and exactly three sectors.');
  const colors = ['#a78bfa', '#818cf8', '#60a5fa', '#22d3ee', '#2dd4bf', '#fbbf24'];
  return data.sectors.map(rows => {
    if (!Array.isArray(rows) || rows.length !== 6 || rows.some(row => typeof row !== 'string' || !/^[.12WMS]{12}$/.test(row)))
      throw new Error('Each sector needs six rows of twelve cells: . 1 2 W M S.');
    const bricks = [];
    rows.forEach((line, row) => [...line].forEach((cell, col) => {
      if (cell === '.') return;
      const hp = cell === '2' ? 2 : 1;
      bricks.push({ id: row * 12 + col, x: 44 + col * 73, y: 95 + row * 34, w: 68, h: 24,
        hp, maxHp: hp, color: colors[row], power: ({ W: 'wide', M: 'multi', S: 'shield' })[cell] || null });
    }));
    if (!bricks.length) throw new Error('Every sector must contain at least one brick.');
    return bricks;
  });
}
