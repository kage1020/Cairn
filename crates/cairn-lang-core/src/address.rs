//! The address a member of a `struct` or `def` body takes from where it stands
//! (`spec/syntax` "IDs, classes, addresses").

use std::fmt;

use serde::{Serialize, Serializer};

/// Where a member stands in its body, by meaning: `window@front[0]`, `level[1].window@front[0]`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct MemberAddress {
    segments: Vec<AddressSegment>,
}

/// One step of a [`MemberAddress`]: a member among the members of one parent.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AddressSegment {
    /// The member's keyword.
    pub role: String,
    /// The wall the member stands on or against, as its line writes it.
    pub side: Option<String>,
    /// Its place among its parent's members of the same role and side, from 0.
    pub index: u32,
}

impl MemberAddress {
    /// The steps from the body down to the member, outermost first; never empty on a member.
    #[must_use]
    pub fn segments(&self) -> &[AddressSegment] {
        &self.segments
    }

    /// This address with `segment` appended, the address of a member nested under this one.
    #[must_use]
    pub fn child(&self, segment: AddressSegment) -> Self {
        let mut segments = self.segments.clone();
        segments.push(segment);
        Self { segments }
    }
}

impl fmt::Display for AddressSegment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.role)?;
        if let Some(side) = &self.side {
            write!(f, "@{side}")?;
        }
        write!(f, "[{}]", self.index)
    }
}

impl fmt::Display for MemberAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, segment) in self.segments.iter().enumerate() {
            if i > 0 {
                f.write_str(".")?;
            }
            write!(f, "{segment}")?;
        }
        Ok(())
    }
}

impl Serialize for MemberAddress {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}
