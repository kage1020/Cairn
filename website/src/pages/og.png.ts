import type { APIRoute } from "astro";
import sharp from "sharp";

import hero from "../assets/hero.dark.svg?raw";

// The 1.91:1 that Open Graph and X show a link preview's image at without cropping it.
const width = 1200;
const height = 630;

/** The site's link-preview image: the hero's pyramid on the dark theme's background. */
export const GET: APIRoute = async () => {
  // No text: it would render in whichever fonts the machine building the site has.
  const pyramid = await sharp(Buffer.from(hero), { density: 72 * 2.6 }).png().toBuffer();
  const png = await sharp({ create: { width, height, channels: 4, background: "#0a1a30" } })
    .composite([{ input: pyramid, gravity: "center" }])
    .png()
    .toBuffer();
  return new Response(new Uint8Array(png), { headers: { "Content-Type": "image/png" } });
};
