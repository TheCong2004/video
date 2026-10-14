const fs = require('fs');
const path = require('path');

const w = 2560, h = 1440, step = 44;
let lines = [];
lines.push('<svg xmlns="http://www.w3.org/2000/svg" width="2560" height="1440" viewBox="0 0 2560 1440">');
lines.push('  <defs>');
lines.push('    <radialGradient id="amb1" cx="20%" cy="10%" r="40%">');
lines.push('      <stop offset="0%" stop-color="#7654FF" stop-opacity="0.12"/>');
lines.push('      <stop offset="100%" stop-color="#080D1B" stop-opacity="0"/>');
lines.push('    </radialGradient>');
lines.push('    <radialGradient id="amb2" cx="80%" cy="25%" r="45%">');
lines.push('      <stop offset="0%" stop-color="#3568FF" stop-opacity="0.10"/>');
lines.push('      <stop offset="100%" stop-color="#080D1B" stop-opacity="0"/>');
lines.push('    </radialGradient>');
lines.push('    <radialGradient id="amb3" cx="50%" cy="85%" r="50%">');
lines.push('      <stop offset="0%" stop-color="#39C8FF" stop-opacity="0.08"/>');
lines.push('      <stop offset="100%" stop-color="#080D1B" stop-opacity="0"/>');
lines.push('    </radialGradient>');
lines.push('  </defs>');
lines.push('  <rect width="100%" height="100%" fill="#080D1B"/>');
lines.push('  <rect width="100%" height="100%" fill="url(#amb1)"/>');
lines.push('  <rect width="100%" height="100%" fill="url(#amb2)"/>');
lines.push('  <rect width="100%" height="100%" fill="url(#amb3)"/>');
lines.push('  <g stroke-linecap="round">');

for (let x = 0; x <= w; x += step) {
    if (x % (step * 5) === 0) {
        lines.push(`    <line x1="${x}" y1="0" x2="${x}" y2="${h}" stroke="#33466B" stroke-width="1" stroke-opacity="0.14"/>`);
    } else {
        lines.push(`    <line x1="${x}" y1="0" x2="${x}" y2="${h}" stroke="#26365F" stroke-width="1" stroke-opacity="0.06"/>`);
    }
}

for (let y = 0; y <= h; y += step) {
    if (y % (step * 5) === 0) {
        lines.push(`    <line x1="0" y1="${y}" x2="${w}" y2="${y}" stroke="#33466B" stroke-width="1" stroke-opacity="0.14"/>`);
    } else {
        lines.push(`    <line x1="0" y1="${y}" x2="${w}" y2="${y}" stroke="#26365F" stroke-width="1" stroke-opacity="0.06"/>`);
    }
}

for (let x = 0; x <= w; x += step * 5) {
    for (let y = 0; y <= h; y += step * 5) {
        lines.push(`    <circle cx="${x}" cy="${y}" r="1.5" fill="#39C8FF" fill-opacity="0.18"/>`);
    }
}

lines.push('  </g>');
lines.push('</svg>');

const outPath = path.join(__dirname, '..', 'src', 'crates', 'concat', 'ui', 'assets', 'neon-grid-bg.svg');
fs.writeFileSync(outPath, lines.join('\n'));
console.log('Successfully written neon-grid-bg.svg:', fs.statSync(outPath).size, 'bytes');
