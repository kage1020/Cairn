/**
 * What `compile` answers for three sources, as `cairn-lang-wasm` serialises it: `satisfies` holds
 * it to the types in `protocol.ts`, and a bindings test in `cairn-lang-wasm` holds it to the Rust
 * output and prints the new array when the two part.
 */

import type { Compiled } from "./protocol";

export const golden = [
  {
    "diagnostics": [
      {
        "code": "E_VERSION_CAP",
        "col": 1,
        "data": {
          "edition": "java",
          "floor": "version>=1.21",
          "kind": "version_cap",
          "target": "1.20.4",
          "valid": [
            "1.21",
            "1.21.4",
            "26.3"
          ]
        },
        "end_col": 24,
        "end_line": 1,
        "line": 1,
        "notes": [
          {
            "message": "valid java targets: 1.21, 1.21.4, 26.3"
          },
          {
            "message": "fix: --target 1.21, or lower the `@requires` floor"
          }
        ],
        "primary": "this file requires version>=1.21 (target 1.20.4)",
        "severity": "error"
      }
    ],
    "ir": null,
    "ok": false,
    "run_level": []
  },
  {
    "diagnostics": [],
    "ir": {
      "members": {
        "struct::s": [
          {
            "address": "floor[0]",
            "geometry": {
              "actual_bbox": {
                "max": [1, 0, 1],
                "min": [0, 0, 0]
              },
              "anchor": [0, 0, 0],
              "declared_bbox": {
                "max": [1, 0, 1],
                "min": [0, 0, 0]
              }
            },
            "resolved_state": [
              {
                "runs": [
                  {
                    "from": [0, 0, 0],
                    "len": 2
                  },
                  {
                    "from": [0, 0, 1],
                    "len": 2
                  }
                ],
                "state": {
                  "id": "minecraft:stonebrick"
                }
              }
            ],
            "role": "floor"
          }
        ]
      },
      "structures": {
        "struct::s": {
          "dims": {
            "x": 2,
            "y": 1,
            "z": 2
          },
          "palette": [
            {
              "id": "minecraft:air"
            },
            {
              "id": "minecraft:stonebrick"
            }
          ],
          "source_scope": "struct::s",
          "voxels": [1, 1, 1, 1]
        }
      }
    },
    "ok": false,
    "run_level": [
      {
        "code": "E_REQUIRES_CONFLICT",
        "col": 1,
        "line": 1,
        "notes": [
          {
            "col": 16,
            "line": 3,
            "message": "`minecraft:stonebrick` is last declared at bedrock 1.21.0"
          },
          {
            "message": "fix: lower the `@requires` floor to 1.21.0 or below, or replace `minecraft:stonebrick` with a block a bedrock target at or above 1.21.40 declares"
          }
        ],
        "primary": "this file requires bedrock version>=1.21.40, but no bedrock target it admits declares `minecraft:stonebrick`, which this build paints.",
        "severity": "error"
      }
    ]
  },
  {
    "diagnostics": [],
    "ir": {
      "members": {
        "struct::s": [
          {
            "address": "floor[0]",
            "geometry": {
              "actual_bbox": {
                "max": [0, 0, 0],
                "min": [0, 0, 0]
              },
              "anchor": [0, 0, 0],
              "declared_bbox": {
                "max": [0, 0, 0],
                "min": [0, 0, 0]
              }
            },
            "resolved_state": [
              {
                "runs": [
                  {
                    "from": [0, 0, 0],
                    "len": 1
                  }
                ],
                "state": {
                  "id": "minecraft:oak_stairs",
                  "properties": {
                    "facing": "south",
                    "shape": "outer_left"
                  }
                }
              }
            ],
            "role": "floor"
          }
        ]
      },
      "structures": {
        "struct::s": {
          "dims": {
            "x": 1,
            "y": 1,
            "z": 1
          },
          "palette": [
            {
              "id": "minecraft:air"
            },
            {
              "id": "minecraft:oak_stairs",
              "properties": {
                "facing": "south",
                "shape": "outer_left"
              }
            }
          ],
          "source_scope": "struct::s",
          "voxels": [1]
        }
      }
    },
    "ok": true,
    "run_level": [
      {
        "code": "W_INTENT_DEGRADED",
        "primary": "struct::s: minecraft:oak_stairs[facing=south,shape=outer_left]: this Bedrock version has no stair `shape` state; its stairs render straight, so corners show visual gaps",
        "severity": "warning"
      }
    ]
  }
] satisfies Omit<Compiled, "artifacts">[];
