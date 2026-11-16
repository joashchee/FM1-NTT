// Draws the home-screen icons (web/public/icon-*.png): a sine wave over a row of keys, in the text-mode palette,
// scaled with nearest-neighbour. Original art. Run with `node scripts/build-icons.ts`. No dependencies: the PNG is encoded by hand.
import { writeFileSync } from 'node:fs';
import { deflateSync } from 'node:zlib';

const BG = [0x00, 0x00, 0x00], PAL: Record<string, number[]> = { c: [0x55, 0xff, 0xff], w: [0xff, 0xff, 0xff], k: [0x55, 0x55, 0x55], y: [0xff, 0xff, 0x55] };
const ART = [
  '................',
  '................',
  '..cc........cc..',
  '.c..c......c..c.',
  '.c..c......c..c.',
  'c....c....c....c',
  '......c..c......',
  '......c..c......',
  '.......cc.......',
  '................',
  'wwkwwkwwwkwwkwww',
  'wwkwwkwwwkwwkwww',
  'wwkwwkwwwkwwkwww',
  'wwwwwwwwwwwwwwww',
  'wwwwwwwwwwwwwwww',
  '................',
];

function png(size: number, inset: number): Buffer {
  const cell = Math.floor((size * (1 - inset * 2)) / 16), off = Math.floor((size - cell * 16) / 2);
  const rows: Buffer[] = [];
  for (let y = 0; y < size; y++) {
    const row = Buffer.alloc(1 + size * 3);
    for (let x = 0; x < size; x++) {
      const cx = Math.floor((x - off) / cell), cy = Math.floor((y - off) / cell);
      const c = cx >= 0 && cx < 16 && cy >= 0 && cy < 16 ? PAL[ART[cy][cx]] ?? BG : BG;
      row.set(c, 1 + x * 3);
    }
    rows.push(row);
  }
  const crcTable = Array.from({ length: 256 }, (_, n) => { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; return c >>> 0; });
  const crc = (b: Buffer) => { let c = 0xffffffff; for (const v of b) c = crcTable[(c ^ v) & 255] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; };
  const chunk = (type: string, data: Buffer) => {
    const head = Buffer.alloc(8), body = Buffer.concat([Buffer.from(type), data]), tail = Buffer.alloc(4);
    head.writeUInt32BE(data.length); head.write(type, 4); tail.writeUInt32BE(crc(body));
    return Buffer.concat([head, data, tail]);
  };
  const ihdr = Buffer.alloc(13); ihdr.writeUInt32BE(size); ihdr.writeUInt32BE(size, 4); ihdr[8] = 8; ihdr[9] = 2;
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk('IHDR', ihdr), chunk('IDAT', deflateSync(Buffer.concat(rows))), chunk('IEND', Buffer.alloc(0))]);
}

// The maskable icon keeps the art inside the central 80% safe zone; the others fill more of the square.
const out = new URL('../web/public/', import.meta.url).pathname;
writeFileSync(out + 'icon-192.png', png(192, 0.12));
writeFileSync(out + 'icon-512.png', png(512, 0.12));
writeFileSync(out + 'icon-maskable-512.png', png(512, 0.2));
writeFileSync(out + 'apple-touch-icon.png', png(180, 0.12));
