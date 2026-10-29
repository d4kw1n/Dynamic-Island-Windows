import { useEffect, useState } from "react";

function hue(r: number, g: number, b: number): number {
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const delta = max - min;
  if (delta === 0) return 0;
  let h: number;
  if (max === r) h = ((g - b) / delta) % 6;
  else if (max === g) h = (b - r) / delta + 2;
  else h = (r - g) / delta + 4;
  h *= 60;
  return h < 0 ? h + 360 : h;
}

/**
 * Samples album art on a tiny canvas and derives a vivid accent colour for
 * the ambient glow behind the media panel.
 */
export function useArtGlow(url: string | undefined): string {
  const [glow, setGlow] = useState("#ff375f");

  useEffect(() => {
    if (!url) return;
    let alive = true;
    const image = new Image();
    image.onload = () => {
      if (!alive) return;
      try {
        const canvas = document.createElement("canvas");
        canvas.width = 8;
        canvas.height = 8;
        const context = canvas.getContext("2d", { willReadFrequently: true });
        if (!context) return;
        context.drawImage(image, 0, 0, 8, 8);
        const data = context.getImageData(0, 0, 8, 8).data;

        let r = 0;
        let g = 0;
        let b = 0;
        let count = 0;
        for (let i = 0; i < data.length; i += 4) {
          if (data[i + 3] < 120) continue;
          r += data[i];
          g += data[i + 1];
          b += data[i + 2];
          count += 1;
        }
        if (!count) return;
        r = Math.round(r / count);
        g = Math.round(g / count);
        b = Math.round(b / count);

        const max = Math.max(r, g, b) / 255;
        const min = Math.min(r, g, b) / 255;
        const lightness = (max + min) / 2;
        const denominator = 1 - Math.abs(2 * lightness - 1);
        const saturation =
          denominator === 0 ? 0 : (max - min) / denominator;

        setGlow(
          `hsl(${Math.round(hue(r, g, b))}, ${Math.max(
            58,
            Math.round(saturation * 100),
          )}%, 56%)`,
        );
      } catch {
        // Canvas can fail for exotic image sources; keep the default accent.
      }
    };
    image.src = url;
    return () => {
      alive = false;
    };
  }, [url]);

  return glow;
}
