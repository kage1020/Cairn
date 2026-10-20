- *(formats)* A loaded `RegistryPack` can no longer be changed, so a caller cannot reorder
  `data_versions` or replace its `latest` after the loader validated them.
  `RegistryPack::targetable` returns the targetable rows in table order, which the loader already
  holds to ascending `data_version`, instead of sorting them again.
