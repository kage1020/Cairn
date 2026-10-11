//! Documents the unit tests open.

/// A document with a finding on every line: two `size=` on one line, two
/// `height=` on the next, and one door id declared twice.
pub(crate) const DUPLICATE: &str = "\
struct s size=4x4 size=5x5
  walls height=4 height=5
  door id=front
  door id=front
";

/// A document `check` has nothing to say about.
pub(crate) const CLEAN: &str = "\
struct s size=2x2
  floor mat_slot=wall
  walls height=4 mat_slot=wall
";
