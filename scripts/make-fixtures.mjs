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

// Appends an incremental update (ISO 32000-2, 7.5.6): the given objects
// (number -> body), one xref subsection per object, and a trailer whose /Prev
// points at the previous section.
export function appendUpdate(base, objects, size) {
  const prev = Number(/startxref\n(\d+)\n%%EOF\n$/.exec(base.toString('latin1'))[1]);
  const chunks = [base];
  let length = base.length;
  let entries = '';
  for (const [number, body] of Object.entries(objects)) {
    const chunk = Buffer.from(`${number} 0 obj\n${body}\nendobj\n`, 'latin1');
    entries += `${number} 1\n${String(length).padStart(10, '0')} 00000 n \n`;
    chunks.push(chunk);
    length += chunk.length;
  }
  chunks.push(Buffer.from(
    `xref\n${entries}trailer\n<< /Size ${size} /Root 1 0 R /Prev ${prev} >>\n`
    + `startxref\n${length}\n%%EOF\n`, 'latin1'));
  return Buffer.concat(chunks);
}

function stream(content, extra = '') {
  return `<< /Length ${Buffer.byteLength(content, 'latin1')}${extra} >>\nstream\n${content}\nendstream`;
}

// Deterministic zlib (RFC 1950/1951) with one fixed-Huffman block of literals
// only: real Huffman-coded Deflate whose bytes never depend on the Node or
// zlib version that runs this script. Returns a latin1 string.
export function zlibFixedHuffman(text) {
  const input = Buffer.from(text, 'latin1');
  const out = [0x78, 0x01];
  let bitBuffer = 0;
  let bitCount = 0;
  const writeBits = (value, count) => {
    for (let i = 0; i < count; i += 1) {
      bitBuffer |= ((value >> i) & 1) << bitCount;
      bitCount += 1;
      if (bitCount === 8) {
        out.push(bitBuffer);
        bitBuffer = 0;
        bitCount = 0;
      }
    }
  };
  // Huffman codes are packed most significant bit first.
  const writeCode = (code, length) => {
    for (let i = length - 1; i >= 0; i -= 1) writeBits((code >> i) & 1, 1);
  };
  const literal = symbol => {
    if (symbol < 144) writeCode(0x30 + symbol, 8);
    else if (symbol < 256) writeCode(0x190 + symbol - 144, 9);
    else writeCode(symbol - 256, 7);
  };
  writeBits(1, 1); // BFINAL
  writeBits(1, 2); // BTYPE = 01, fixed Huffman codes
  for (const byte of input) literal(byte);
  literal(256); // end of block
  if (bitCount > 0) out.push(bitBuffer);
  let a = 1;
  let b = 0;
  for (const byte of input) {
    a = (a + byte) % 65521;
    b = (b + a) % 65521;
  }
  const adler = ((b << 16) | a) >>> 0;
  out.push((adler >>> 24) & 0xff, (adler >>> 16) & 0xff, (adler >>> 8) & 0xff, adler & 0xff);
  return Buffer.from(out).toString('latin1');
}

// PNG "Up" prediction (filter type 2) of rows of `columns` bytes.
function pngUp(bytes, columns) {
  const out = [];
  for (let row = 0; row * columns < bytes.length; row += 1) {
    out.push(2);
    for (let i = 0; i < columns; i += 1) {
      const above = row === 0 ? 0 : bytes[(row - 1) * columns + i];
      out.push((bytes[row * columns + i] - above) & 0xff);
    }
  }
  return Buffer.from(out).toString('latin1');
}

// Builds a PDF 1.5 file with a cross-reference stream (ISO 32000-2, 7.5.8).
// `objects` maps every object number to `{ body }` (written directly) or
// `{ compressed: true, body }` (placed in the object stream `objstm`). The
// object stream and the cross-reference stream are Flate-encoded; the
// cross-reference data also uses the PNG Up predictor, as producers often do.
// With `hybrid`, a classic table lists the direct objects and a /XRefStm
// stream lists only the compressed ones (7.5.8.4).
export function buildXrefStreamPdf(objects, objstm, { hybrid = false } = {}) {
  const chunks = [Buffer.from('%PDF-1.5\n%\xE2\xE3\xCF\xD3\n', 'latin1')];
  let length = chunks[0].length;
  const entries = new Map([[0, [0, 0, 65535]]]);
  const write = (number, body) => {
    entries.set(number, [1, length, 0]);
    const chunk = Buffer.from(`${number} 0 obj\n${body}\nendobj\n`, 'latin1');
    chunks.push(chunk);
    length += chunk.length;
  };
  const numbers = Object.keys(objects).map(Number).sort((a, b) => a - b);
  let header = '';
  let body = '';
  let index = 0;
  for (const number of numbers) {
    const object = objects[number];
    if (object.compressed) {
      header += `${number} ${body.length} `;
      body += `${object.body}\n`;
      entries.set(number, [2, objstm, index]);
      index += 1;
    } else {
      write(number, object.body);
    }
  }
  write(objstm, stream(zlibFixedHuffman(header + body),
    ` /Type /ObjStm /N ${index} /First ${header.length} /Filter /FlateDecode`));
  const xrefNumber = Math.max(...entries.keys()) + 1;
  const size = xrefNumber + 1;
  const xrefOffset = length;
  entries.set(xrefNumber, [1, xrefOffset, 0]);
  // W [1 3 1]: type, then 3-byte field 2, then 1-byte field 3.
  const listed = hybrid
    ? [...entries.keys()].filter(n => entries.get(n)[0] === 2).sort((a, b) => a - b)
    : Array.from({ length: size }, (_, n) => n);
  const records = [];
  for (const number of listed) {
    const [type, field2, field3] = entries.get(number) ?? [0, 0, 0];
    records.push(type, (field2 >> 16) & 0xff, (field2 >> 8) & 0xff, field2 & 0xff, field3 & 0xff);
  }
  const index2 = hybrid ? ` /Index [${listed.map(n => `${n} 1`).join(' ')}]` : '';
  const trailerKeys = hybrid ? '' : ' /Root 1 0 R';
  write(xrefNumber, stream(zlibFixedHuffman(pngUp(Buffer.from(records), 5)),
    ` /Type /XRef /Size ${size} /W [1 3 1]${index2}${trailerKeys}`
    + ' /Filter /FlateDecode /DecodeParms << /Predictor 12 /Columns 5 >>'));
  if (!hybrid) {
    chunks.push(Buffer.from(`startxref\n${xrefOffset}\n%%EOF\n`, 'latin1'));
    return Buffer.concat(chunks);
  }
  // Classic table covering every object: compressed objects are listed as
  // free, as producers do for readers without cross-reference streams.
  let table = `xref\n0 ${size}\n`;
  for (let number = 0; number < size; number += 1) {
    const [type, field2, field3] = entries.get(number) ?? [0, 0, 0];
    table += type === 1
      ? `${String(field2).padStart(10, '0')} ${String(field3).padStart(5, '0')} n \n`
      : `0000000000 ${number === 0 ? '65535' : '00000'} f \n`;
  }
  const tableOffset = length;
  chunks.push(Buffer.from(`${table}trailer\n<< /Size ${size} /Root 1 0 R /XRefStm ${xrefOffset} >>\n`
    + `startxref\n${tableOffset}\n%%EOF\n`, 'latin1'));
  return Buffer.concat(chunks);
}

// The one-page document of `onePage`, with catalog, pages and font marked for
// an object stream; the page and its content stream stay uncompressed.
function compressedPage(text) {
  const [catalog, pages, page, content, font] = onePage(text);
  return {
    1: { compressed: true, body: catalog },
    2: { compressed: true, body: pages },
    3: { body: page },
    4: { body: content },
    5: { compressed: true, body: font },
  };
}

function onePage(text) {
  return [
    '<< /Type /Catalog /Pages 2 0 R >>',
    '<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
    '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] '
      + '/Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>',
    stream(`BT /F1 24 Tf 72 770 Td (${text}) Tj ET`),
    '<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>',
  ];
}

export const fixtures = {
  // One A4 page, one line of text in a standard 14 font, uncompressed.
  'minimal.pdf': buildPdf(onePage('CastlePDF minimal fixture')),
  // Three pages in a nested tree: MediaBox and Resources inherited from the
  // root, Rotate from an intermediate node, one page with its own MediaBox.
  'page-tree.pdf': buildPdf([
    '<< /Type /Catalog /Pages 2 0 R >>',
    '<< /Type /Pages /Kids [3 0 R 6 0 R] /Count 3 /MediaBox [0 0 595 842] '
      + '/Resources << /Font << /F1 7 0 R >> >> >>',
    '<< /Type /Pages /Parent 2 0 R /Kids [4 0 R 5 0 R] /Count 2 /Rotate 90 >>',
    '<< /Type /Page /Parent 3 0 R /Contents 8 0 R >>',
    '<< /Type /Page /Parent 3 0 R /MediaBox [0 0 842 595] /Contents 9 0 R >>',
    '<< /Type /Page /Parent 2 0 R /Contents 10 0 R >>',
    '<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>',
    stream('BT /F1 24 Tf 72 770 Td (Page 1) Tj ET'),
    stream('BT /F1 24 Tf 72 520 Td (Page 2) Tj ET'),
    stream('BT /F1 24 Tf 72 770 Td (Page 3) Tj ET'),
  ]),
  // One page whose content stream is Flate-compressed; a /Filter given by an
  // indirect reference; a data stream with a PNG predictor (4 columns).
  'flate.pdf': buildPdf([
    '<< /Type /Catalog /Pages 2 0 R >>',
    '<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
    '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] '
      + '/Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>',
    stream(zlibFixedHuffman('BT /F1 24 Tf 72 770 Td (CastlePDF Flate fixture) Tj ET'),
      ' /Filter 6 0 R'),
    '<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>',
    '/FlateDecode',
    stream(zlibFixedHuffman(pngUp(Buffer.from([1, 2, 3, 4, 2, 4, 6, 8, 3, 6, 9, 12]), 4)),
      ' /Filter /FlateDecode /DecodeParms << /Predictor 12 /Columns 4 >>'),
  ]),
  // PDF 1.5 layout: catalog, pages and font compressed in object stream 6;
  // cross-reference stream (Flate + PNG predictor) instead of a table.
  'objstm.pdf': buildXrefStreamPdf(compressedPage('CastlePDF object stream fixture'), 6),
  // The same objects in a hybrid file: classic table plus /XRefStm stream.
  'hybrid.pdf': buildXrefStreamPdf(compressedPage('CastlePDF hybrid fixture'), 6,
    { hybrid: true }),
  // The same page, then an incremental update replacing its content stream.
  'incremental.pdf': appendUpdate(
    buildPdf(onePage('CastlePDF original revision')),
    { 4: stream('BT /F1 24 Tf 72 770 Td (CastlePDF updated revision) Tj ET') },
    6),
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
