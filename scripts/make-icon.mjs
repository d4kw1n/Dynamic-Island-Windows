// Generates a 1024x1024 source icon (PNG) with no external dependencies.
// Run: node scripts/make-icon.mjs  ->  icon-source.png
import { deflateSync } from "node:zlib";
import { writeFileSync } from "node:fs";

const OUT = 1024;
const SS = 2; // supersampling factor
const S = OUT * SS;

const clamp = (v, a, b) => (v < a ? a : v > b ? b : v);
const mix = (a, b, t) => a + (b - a) * t;
const smooth = (d) => clamp(0.5 - d, 0, 1);

function sdRoundRect(x, y, cx, cy, w, h, r) {
  const dx = Math.abs(x - cx) - w / 2 + r;
  const dy = Math.abs(y - cy) - h / 2 + r;
  return Math.min(Math.max(dx, dy), 0) + Math.hypot(Math.max(dx, 0), Math.max(dy, 0)) - r;
}

function sdCircle(x, y, cx, cy, r) {
  return Math.hypot(x - cx, y - cy) - r;
}

function over(dst, src, alpha) {
  const a = clamp(alpha, 0, 1);
  const outA = a + dst[3] * (1 - a);
  if (outA <= 0) return [0, 0, 0, 0];
  for (let i = 0; i < 3; i++) {
    dst[i] = (src[i] * a + dst[i] * dst[3] * (1 - a)) / outA;
  }
  dst[3] = outA;
  return dst;
}

function render() {
  const buf = new Float32Array(S * S * 4);
  const half = S / 2;
  const scale = S / 1024;

  for (let y = 0; y < S; y++) {
    for (let x = 0; x < S; x++) {
      const px = (x + 0.5) / scale;
      const py = (y + 0.5) / scale;
      let px4 = [0, 0, 0, 0];

      // App tile: dark rounded square, subtle vertical gradient.
      const tile = smooth(sdRoundRect(px, py, 512, 512, 944, 944, 224));
      if (tile > 0) {
        const t = py / 1024;
        const col = [
          mix(24, 7, t),
          mix(26, 8, t),
          mix(32, 10, t),
        ];
        px4 = over(px4, col, tile);
      }

      // Top sheen on the tile.
      const sheen = smooth(sdRoundRect(px, py, 512, 512, 944, 944, 224)) *
        clamp(1 - py / 430, 0, 1) * 0.12;
      if (sheen > 0) px4 = over(px4, [255, 255, 255], sheen);

      // Island pill with a hairline border.
      const pillOuter = smooth(sdRoundRect(px, py, 512, 512, 800, 320, 160));
      const pillInner = smooth(sdRoundRect(px, py, 512, 512, 800 - 7, 320 - 7, 158));
      if (pillOuter > 0) px4 = over(px4, [188, 196, 210], pillOuter * 0.16);
      if (pillInner > 0) {
        const t = (py - 352) / 320;
        px4 = over(px4, [mix(10, 4, t), mix(11, 5, t), mix(13, 6, t)], pillInner);
      }

      // Soft glow behind the orb.
      const gd = Math.hypot(px - 470, py - 512);
      const glow = clamp(1 - gd / 300, 0, 1) ** 2 * 0.32 * pillInner;
      if (glow > 0) px4 = over(px4, [190, 90, 255], glow);

      // Gradient orb.
      const od = sdCircle(px, py, 470, 512, 96);
      const orb = smooth(od);
      if (orb > 0) {
        const ang = Math.atan2(py - 512, px - 470);
        const t = (ang + Math.PI) / (Math.PI * 2);
        const c1 = [255, 86, 128];
        const c2 = [176, 84, 255];
        const c3 = [64, 156, 255];
        const col = t < 0.5
          ? [mix(c1[0], c2[0], t * 2), mix(c1[1], c2[1], t * 2), mix(c1[2], c2[2], t * 2)]
          : [mix(c2[0], c3[0], (t - 0.5) * 2), mix(c2[1], c3[1], (t - 0.5) * 2), mix(c2[2], c3[2], (t - 0.5) * 2)];
        // radial lighten toward top-left
        const light = clamp(1 - Math.hypot(px - 438, py - 478) / 190, 0, 1) * 0.35;
        px4 = over(px4, [mix(col[0], 255, light), mix(col[1], 255, light), mix(col[2], 255, light)], orb);
      }

      // Camera dot.
      const dot = smooth(sdCircle(px, py, 672, 512, 27));
      if (dot > 0) px4 = over(px4, [240, 245, 255], dot * 0.92);
      const dotInner = smooth(sdCircle(px, py, 672, 512, 12));
      if (dotInner > 0) px4 = over(px4, [18, 24, 34], dotInner * 0.9);

      const i = (y * S + x) * 4;
      buf[i] = px4[0];
      buf[i + 1] = px4[1];
      buf[i + 2] = px4[2];
      buf[i + 3] = px4[3] * 255;
    }
  }

  // Downsample.
  const out = Buffer.alloc(OUT * OUT * 4);
  for (let y = 0; y < OUT; y++) {
    for (let x = 0; x < OUT; x++) {
      let r = 0, g = 0, b = 0, a = 0;
      for (let sy = 0; sy < SS; sy++) {
        for (let sx = 0; sx < SS; sx++) {
          const i = (((y * SS + sy) * S) + (x * SS + sx)) * 4;
          r += buf[i]; g += buf[i + 1]; b += buf[i + 2]; a += buf[i + 3];
        }
      }
      const n = SS * SS;
      const o = (y * OUT + x) * 4;
      out[o] = Math.round(r / n);
      out[o + 1] = Math.round(g / n);
      out[o + 2] = Math.round(b / n);
      out[o + 3] = Math.round(a / n);
    }
  }
  return out;
}

// Minimal PNG encoder (RGBA8).
const CRC_TABLE = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c >>> 0;
  }
  return t;
})();

function crc32(buf) {
  let c = 0xffffffff;
  for (const byte of buf) c = CRC_TABLE[(c ^ byte) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length, 0);
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body), 0);
  return Buffer.concat([len, body, crc]);
}

function encodePng(rgba, w, h) {
  const sig = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0);
  ihdr.writeUInt32BE(h, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // RGBA
  const raw = Buffer.alloc((w * 4 + 1) * h);
  for (let y = 0; y < h; y++) {
    raw[y * (w * 4 + 1)] = 0;
    rgba.copy(raw, y * (w * 4 + 1) + 1, y * w * 4, (y + 1) * w * 4);
  }
  const idat = deflateSync(raw, { level: 9 });
  return Buffer.concat([sig, chunk("IHDR", ihdr), chunk("IDAT", idat), chunk("IEND", Buffer.alloc(0))]);
}

const rgba = render();
writeFileSync("icon-source.png", encodePng(rgba, OUT, OUT));
console.log("icon-source.png written (1024x1024)");
