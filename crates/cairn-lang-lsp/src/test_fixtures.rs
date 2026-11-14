//! Documents the unit tests open, and the targets they pin.

use cairn_lang_core::Edition;

use crate::pin::Pin;

/// A document with a finding on every line.
pub(crate) const DUPLICATE: &str = "\
struct s size=4x4 size=5x5
  walls height=4 height=5
  door id=front
  door id=front
";

/// A document neither `check` nor lowering has anything to say about.
pub(crate) const CLEAN: &str = "\
theme t:
  slot wall -> @stone

struct s size=2x2
  floor mat_slot=wall
  walls height=4 mat_slot=wall
";

/// A wall bound to Bedrock's spelling of the oak sign, which Java's alias table answers.
pub(crate) const SIGN: &str = "\
theme t:
  slot wall -> @standing_sign
  slot floor -> @oak_planks
struct s size=3x3
  floor mat_slot=floor
  walls mat_slot=wall height=2
";

/// A wall of a block Java 1.21.4 added, behind the floor declared for it.
pub(crate) const PALE_OAK: &str = "\
@requires version>=1.21.4
theme t:
  slot wall -> @pale_oak_planks
struct s size=3x3
  walls mat_slot=wall height=2
";

/// A part placed with the Java variant of a theme that has a Bedrock one.
pub(crate) const VARIANTS: &str = "\
theme t_java:
  slot wall -> @oak_planks
theme t_bedrock:
  slot wall -> @oak_planks
def hut size=3x3:
  walls mat_slot=wall height=2
site v:
  place id=a use=hut theme=t_java at=origin
";

/// The pin of a Java target.
pub(crate) fn java(version: &str) -> Pin {
    Pin::Target {
        edition: Edition::Java,
        version: version.to_owned(),
    }
}
