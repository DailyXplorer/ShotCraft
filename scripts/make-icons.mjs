// Regenerates the raster icons from the SVG sources in assets/logo/.
//
//   npm install playwright && npx playwright install chromium
//   node scripts/make-icons.mjs
//
// Writes assets/window-icon.png (256 px), assets/menubar.png (36 px, template image),
// assets/shotcraft.ico (the Windows exe icon, 16 to 256 px) and, on macOS, assets/AppIcon.icns
// through `iconutil`. Set PLAYWRIGHT_MODULE to import Playwright from another location.
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const root = path.join(path.dirname(fileURLToPath(import.meta.url)), '..');
const svg = (name) => fs.readFileSync(path.join(root, 'assets/logo', name), 'utf8');

const browser = await chromium.launch();
const page = await browser.newPage({ deviceScaleFactor: 1 });
async function render(source, size, out) {
  const uri = 'data:image/svg+xml;base64,' + Buffer.from(source).toString('base64');
  await page.setViewportSize({ width: size, height: size });
  await page.setContent(`<body style="margin:0;background:transparent"><img src="${uri}" width="${size}" height="${size}" style="display:block"></body>`);
  return page.screenshot({ path: out, omitBackground: true });
}

// An .ico file holding each image as a PNG, which Windows reads since Vista.
function ico(images) {
  const header = Buffer.alloc(6 + 16 * images.length);
  header.writeUInt16LE(1, 2); // type: icon
  header.writeUInt16LE(images.length, 4);
  let offset = header.length;
  images.forEach(({ size, png }, i) => {
    const entry = 6 + 16 * i;
    header.writeUInt8(size % 256, entry); // width and height, 0 stands for 256
    header.writeUInt8(size % 256, entry + 1);
    header.writeUInt16LE(1, entry + 4); // colour planes
    header.writeUInt16LE(32, entry + 6); // bits per pixel
    header.writeUInt32LE(png.length, entry + 8);
    header.writeUInt32LE(offset, entry + 12);
    offset += png.length;
  });
  return Buffer.concat([header, ...images.map((image) => image.png)]);
}

const app = svg('app-icon.svg');
const iconset = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'shotcraft-icon-')), 'AppIcon.iconset');
fs.mkdirSync(iconset);
for (const base of [16, 32, 128, 256, 512]) {
  await render(app, base, path.join(iconset, `icon_${base}x${base}.png`));
  await render(app, base * 2, path.join(iconset, `icon_${base}x${base}@2x.png`));
}
await render(app, 256, path.join(root, 'assets/window-icon.png'));
await render(svg('menubar.svg'), 36, path.join(root, 'assets/menubar.png'));
const images = [];
for (const size of [16, 24, 32, 48, 64, 128, 256]) images.push({ size, png: await render(app, size) });
fs.writeFileSync(path.join(root, 'assets/shotcraft.ico'), ico(images));
await browser.close();

if (process.platform === 'darwin') {
  execFileSync('iconutil', ['-c', 'icns', iconset, '-o', path.join(root, 'assets/AppIcon.icns')]);
  fs.rmSync(path.dirname(iconset), { recursive: true });
} else {
  console.log(`Run on a Mac: iconutil -c icns ${iconset} -o assets/AppIcon.icns`);
}
