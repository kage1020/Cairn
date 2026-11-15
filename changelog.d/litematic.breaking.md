- *(core)* A `site` named after a device Windows reserves (`CON`, `NUL`, `COM1`, …) is
  `E_INVALID_SITE_NAME`, since `--structure-format litematic` writes the site under its own name.
  `check` refuses it whatever format a build writes, so a source with such a site no longer checks.
- *(formats)* `OutputExt` gains `Litematic`, so an exhaustive match on it no longer compiles.
- *(formats)* `prepare_structure` and `prepare_mcstructure`, and the writers built on them, refuse a
  block array with a block entity on no voxel of its grid, which they wrote without the entity
  before. Lowering never builds one.
