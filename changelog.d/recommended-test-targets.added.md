- *(cli)* `cairn info` prints a `recommended test targets` row, the seventh of eight, before
  `verified target`. Per edition it lists the oldest and newest buildable versions, and the pair
  either side of each semantic-sensitivity boundary that falls between them (`Java: 1.20.4, 1.21,
  26.3` for a chest, whose item format changes at 1.20.5). `--format json` carries the row as
  `recommended_test_targets`, one `{edition, versions}` per edition.
- *(core)* `VersionAxes` gains `recommended_test_targets`, which `compute_axes` fills with one
  `RecommendedTestTargets` per edition.
