- *(formats)* The `.nbt` and `.litematic` writers buffer the bytes they stream before the gzip
  encoder compresses them, rather than compressing a few bytes at a time, so a 256×64×256 structure
  writes about four times faster as either.
