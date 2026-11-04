// Regenerates the raster icons from the SVG sources in assets/logo/.
//
//   npm install playwright && npx playwright install chromium
//   node scripts/make-icons.mjs
//
// Writes assets/window-icon.png (256 px), assets/menubar.png (36 px, template image) and, on
// macOS, assets/AppIcon.icns through `iconutil`. Set PLAYWRIGHT_MODULE to import Playwright
// from another location.
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
  await page.screenshot({ path: out, omitBackground: true });
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
await browser.close();

if (process.platform === 'darwin') {
  execFileSync('iconutil', ['-c', 'icns', iconset, '-o', path.join(root, 'assets/AppIcon.icns')]);
  fs.rmSync(path.dirname(iconset), { recursive: true });
} else {
  console.log(`Run on a Mac: iconutil -c icns ${iconset} -o assets/AppIcon.icns`);
}
