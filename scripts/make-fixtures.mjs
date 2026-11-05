#!/usr/bin/env node
// Generates the synthetic public fixtures deterministically.
// `node scripts/make-fixtures.mjs` writes them; `--check` only verifies that
// the committed files are byte-identical to what this script produces.
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const outDir = path.join(root, 'tests', 'corpus', 'public');

// Builds a classic-xref PDF from object bodies (object n = bodies[n - 1]).
// Offsets are computed on the exact bytes, so the xref table is valid.
export function buildPdf(bodies, version = '1.4') {
  const chunks = [Buffer.from(`%PDF-${version}\n%\xE2\xE3\xCF\xD3\n`, 'latin1')];
  let length = chunks[0].length;
  const offsets = [];
  bodies.forEach((body, index) => {
    offsets.push(length);
    const chunk = Buffer.from(`${index + 1} 0 obj\n${body}\nendobj\n`, 'latin1');
    chunks.push(chunk);
    length += chunk.length;
  });
  const xrefOffset = length;
  const entries = offsets.map(o => `${String(o).padStart(10, '0')} 00000 n \n`).join('');
  chunks.push(Buffer.from(
    `xref\n0 ${bodies.length + 1}\n0000000000 65535 f \n${entries}`
    + `trailer\n<< /Size ${bodies.length + 1} /Root 1 0 R >>\nstartxref\n${xrefOffset}\n%%EOF\n`,
    'latin1'));
  return Buffer.concat(chunks);
}

function stream(content) {
  return `<< /Length ${Buffer.byteLength(content, 'latin1')} >>\nstream\n${content}\nendstream`;
}

export const fixtures = {
  // One A4 page, one line of text in a standard 14 font, uncompressed.
  'minimal.pdf': buildPdf([
    '<< /Type /Catalog /Pages 2 0 R >>',
    '<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
    '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] '
      + '/Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>',
    stream('BT /F1 24 Tf 72 770 Td (CastleEngine minimal fixture) Tj ET'),
    '<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>',
  ]),
};

export const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const check = process.argv.includes('--check');
  let failed = false;
  mkdirSync(outDir, { recursive: true });
  for (const [name, bytes] of Object.entries(fixtures)) {
    const file = path.join(outDir, name);
    if (check) {
      let current = null;
      try { current = readFileSync(file); } catch { /* reported below */ }
      if (!current?.equals(bytes)) {
        console.error(`${name}: differs from generator output`);
        failed = true;
        continue;
      }
    } else {
      writeFileSync(file, bytes);
    }
    console.log(`${name}  ${bytes.length} bytes  sha256 ${sha256(bytes)}`);
  }
  process.exit(failed ? 1 : 0);
}
