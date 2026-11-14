- *(cli)* `cairn info` prints a sixth row, `recommended test targets`. Per edition it lists the
  oldest and newest buildable versions, and the pair either side of each semantic-sensitivity
  boundary that falls between them (`Java: 1.20.4, 1.21, 26.3`). `--format json` carries the row as
  `recommended_test_targets`, one `{edition, versions}` per edition. Every row's value now starts
  two columns further right, so the values stay aligned under the longer label.
- *(core)* `VersionAxes` gains `recommended_test_targets`, which `compute_axes` fills with one
  `RecommendedTestTargets` per edition.
