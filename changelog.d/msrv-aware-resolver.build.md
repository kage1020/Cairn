- The workspace resolves dependencies with Cargo's MSRV-aware resolver (`resolver = "3"`), so
  `cargo update` no longer picks a dependency whose own `rust-version` is above Cairn's 1.95. The
  committed `Cargo.lock` is unchanged.
