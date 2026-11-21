/** An isometric drawing of one block array, cut away above a chosen layer. */

import type { BlockArray, BlockState } from "./protocol";

/** What to draw: the array, how many layers from the bottom, and a quarter-turn count. */
export interface ViewState {
  array: BlockArray;
  layers: number;
  turns: number;
}

const COS30 = Math.cos(Math.PI / 6);
const PAD = 12;

/** Blocks the light passes through, drawn faint and hiding nothing behind them. */
const SEE_THROUGH = /glass|pane|leaves|bars|fence|door|trapdoor|torch|lantern|button|lever|rail|redstone_wire|repeater|comparator|sign|carpet|flower|sapling|ladder|chain/;

/** A base colour for an id, by the first material word it holds. */
const MATERIALS: [RegExp, string][] = [
  [/redstone/, "#b3261e"],
  [/glass|pane/, "#a7d3e8"],
  [/cobblestone|stone_brick|andesite|stone/, "#8a8a8a"],
  [/deepslate|blackstone|basalt/, "#4b4b52"],
  [/dark_oak/, "#4a3220"],
  [/spruce/, "#6b4a2b"],
  [/birch/, "#c8b77a"],
  [/jungle/, "#a0703f"],
  [/acacia/, "#b15e33"],
  [/mangrove/, "#773631"],
  [/cherry/, "#e3b1ac"],
  [/pale_oak/, "#e3dbd3"],
  [/bamboo/, "#c9b45a"],
  [/crimson/, "#7b3a4f"],
  [/warped/, "#2b6a64"],
  [/oak|planks|log|wood/, "#a2834f"],
  [/brick/, "#965a46"],
  [/sandstone|sand/, "#d8cb94"],
  [/terracotta/, "#9e6246"],
  [/quartz/, "#e9e3d9"],
  [/prismarine/, "#5aa59a"],
  [/copper/, "#c06c4f"],
  [/iron/, "#d8d8d8"],
  [/gold/, "#e8c547"],
  [/grass|moss|leaves/, "#5c8f3a"],
  [/dirt|mud/, "#7a5638"],
  [/water/, "#3f76e4"],
  [/lava/, "#e3661c"],
  [/white/, "#e9ecec"],
  [/black/, "#1f1f23"],
  [/red/, "#a12722"],
  [/blue/, "#35399d"],
  [/green/, "#546d1b"],
  [/yellow/, "#f8c527"],
];

/** Colours for an id no material word matches, picked by a hash of the id. */
const SPARE = ["#8f6f9e", "#6f8f9e", "#9e8f6f", "#6f9e7c", "#9e6f7c", "#7c6f9e", "#9e9a6f"];

/** The colour a palette entry is drawn in, as `#rrggbb`. */
export function colourOf(state: BlockState): string {
  const name = state.id.replace(/^minecraft:/, "");
  for (const [pattern, colour] of MATERIALS) if (pattern.test(name)) return colour;
  let hash = 0;
  for (const char of name) hash = (hash * 31 + char.charCodeAt(0)) | 0;
  return SPARE[Math.abs(hash) % SPARE.length];
}

/** Draw `state` into `canvas`, sized to the canvas's CSS box. */
export function draw(canvas: HTMLCanvasElement, state: ViewState, background: string): void {
  const { array, layers, turns } = state;
  const { x: X, y: Y, z: Z } = array.dims;
  const quarter = ((turns % 4) + 4) % 4;
  // Seen from the turned frame: `vx` runs right-down, `vz` left-down, `y` up.
  const VX = quarter % 2 ? Z : X;
  const VZ = quarter % 2 ? X : Z;
  const cell = (vx: number, y: number, vz: number): number => {
    let x = vx;
    let z = vz;
    if (quarter === 1) [x, z] = [vz, Z - 1 - vx];
    else if (quarter === 2) [x, z] = [X - 1 - vx, Z - 1 - vz];
    else if (quarter === 3) [x, z] = [X - 1 - vz, vx];
    return array.voxels[(y * Z + z) * X + x];
  };

  const ratio = window.devicePixelRatio || 1;
  const width = canvas.clientWidth;
  const height = canvas.clientHeight;
  canvas.width = Math.round(width * ratio);
  canvas.height = Math.round(height * ratio);
  const context = canvas.getContext("2d");
  if (!context) return;
  context.setTransform(ratio, 0, 0, ratio, 0, 0);
  context.fillStyle = background;
  context.fillRect(0, 0, width, height);
  if (!X || !Y || !Z) return;

  const shown = Math.max(1, Math.min(layers, Y));
  const size = Math.max(
    1,
    Math.min(
      (width - 2 * PAD) / ((VX + VZ) * COS30),
      (height - 2 * PAD) / ((VX + VZ) / 2 + shown),
    ),
  );
  const drawnWidth = (VX + VZ) * COS30 * size;
  const drawnHeight = ((VX + VZ) / 2 + shown) * size;
  const originX = (width - drawnWidth) / 2 + VZ * COS30 * size;
  const originY = (height - drawnHeight) / 2 + shown * size;
  const at = (vx: number, y: number, vz: number): [number, number] => [
    originX + (vx - vz) * COS30 * size,
    originY + ((vx + vz) / 2 - y) * size,
  ];

  const seeThrough = array.palette.map((entry) => SEE_THROUGH.test(entry.id));
  const colours = array.palette.map(colourOf);
  const solid = (vx: number, y: number, vz: number): boolean => {
    if (vx >= VX || vz >= VZ || y >= shown) return false;
    const index = cell(vx, y, vz);
    return index !== 0 && !seeThrough[index];
  };
  const face = (corners: [number, number, number][], fill: string, alpha: number): void => {
    context.beginPath();
    corners.forEach(([vx, y, vz], i) => {
      const [px, py] = at(vx, y, vz);
      if (i === 0) context.moveTo(px, py);
      else context.lineTo(px, py);
    });
    context.closePath();
    context.globalAlpha = alpha;
    context.fillStyle = fill;
    context.fill();
    context.globalAlpha = Math.min(1, alpha + 0.2);
    context.strokeStyle = "rgba(0, 0, 0, 0.25)";
    context.lineWidth = Math.min(1, size / 8);
    context.stroke();
  };

  // Painter's order: a cell with a larger `vx + y + vz` is nearer the viewer.
  for (let sum = 0; sum <= VX + shown + VZ - 3; sum += 1) {
    for (let y = Math.max(0, sum - VX - VZ + 2); y <= Math.min(shown - 1, sum); y += 1) {
      for (let vx = Math.max(0, sum - y - VZ + 1); vx <= Math.min(VX - 1, sum - y); vx += 1) {
        const vz = sum - y - vx;
        const index = cell(vx, y, vz);
        if (index === 0) continue;
        const colour = colours[index];
        const alpha = seeThrough[index] ? 0.45 : 1;
        if (!solid(vx, y + 1, vz)) {
          face(
            [
              [vx, y + 1, vz],
              [vx + 1, y + 1, vz],
              [vx + 1, y + 1, vz + 1],
              [vx, y + 1, vz + 1],
            ],
            colour,
            alpha,
          );
        }
        if (!solid(vx + 1, y, vz)) {
          face(
            [
              [vx + 1, y, vz],
              [vx + 1, y + 1, vz],
              [vx + 1, y + 1, vz + 1],
              [vx + 1, y, vz + 1],
            ],
            shade(colour, 0.8),
            alpha,
          );
        }
        if (!solid(vx, y, vz + 1)) {
          face(
            [
              [vx, y, vz + 1],
              [vx + 1, y, vz + 1],
              [vx + 1, y + 1, vz + 1],
              [vx, y + 1, vz + 1],
            ],
            shade(colour, 0.62),
            alpha,
          );
        }
      }
    }
  }
  context.globalAlpha = 1;
}

/** `colour`, a `#rrggbb`, with each channel scaled by `factor`. */
function shade(colour: string, factor: number): string {
  const channel = (at: number): number =>
    Math.round(Number.parseInt(colour.slice(at, at + 2), 16) * factor);
  return `rgb(${channel(1)} ${channel(3)} ${channel(5)})`;
}

/** How many cells hold each palette entry. */
export function counts(array: BlockArray): number[] {
  const tally = new Array<number>(array.palette.length).fill(0);
  for (const index of array.voxels) tally[index] += 1;
  return tally;
}
