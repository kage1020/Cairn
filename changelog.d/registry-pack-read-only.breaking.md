- *(formats)* `RegistryPack`'s fields `manifest`, `data_versions`, `materials`, `blocks`,
  `aliases`, `bytes_hash` and `source` are private. Each is read through the method of the same
  name, which borrows it: `RegistryPack::manifest()`, `data_versions()`, `materials()`,
  `blocks()`, `aliases()`, `bytes_hash()` and `source()`. Nothing replaces writing to a field.
