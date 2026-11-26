//! What a registry table says of a row's blocks, in every state alike or looked up from the values
//! of some of their properties.

use std::collections::HashMap;

use indexmap::IndexMap;

/// What holds of a row's blocks.
#[derive(Debug, Clone)]
pub(super) enum Held<T> {
    /// The same in every state.
    Every(T),
    /// Looked up from the values of `reads`, comma-joined in their order.
    Lookup {
        reads: Vec<String>,
        map: HashMap<String, T>,
    },
}

impl<T: Copy> Held<T> {
    /// What holds where `value` gives each property's value, or `None` when it gives none for a
    /// property the lookup reads or the map holds no key for the values it gives.
    pub(super) fn at<'v>(&self, value: impl Fn(&str) -> Option<&'v str>) -> Option<T> {
        match self {
            Self::Every(held) => Some(*held),
            Self::Lookup { reads, map } => {
                let values: Vec<&str> =
                    reads.iter().map(|key| value(key)).collect::<Option<_>>()?;
                map.get(&values.join(",")).copied()
            }
        }
    }
}

impl<T> Held<T> {
    /// Each property the lookup reads, paired with every value one of its keys names.
    pub(super) fn named(&self) -> impl Iterator<Item = (&str, &str)> {
        let (reads, map): (&[String], Option<&HashMap<String, T>>) = match self {
            Self::Every(_) => (&[], None),
            Self::Lookup { reads, map } => (reads, Some(map)),
        };
        map.into_iter()
            .flat_map(HashMap::keys)
            .flat_map(move |key| reads.iter().map(String::as_str).zip(key.split(',')))
    }

    /// The first state, as `(property, value)` pairs in the order the lookup reads them, that
    /// `domain` gives the properties it reads and the map holds no key for; `None` when it holds
    /// every one, or `domain` gives a property it reads no value.
    pub(super) fn unmapped<'d>(
        &self,
        domain: &'d IndexMap<String, Vec<String>>,
    ) -> Option<Vec<(&str, &'d str)>> {
        let Self::Lookup { reads, map } = self else {
            return None;
        };
        let held: Vec<&[String]> = reads
            .iter()
            .map(|key| {
                domain
                    .get(key)
                    .map(Vec::as_slice)
                    .filter(|values| !values.is_empty())
            })
            .collect::<Option<_>>()?;
        let mut at = vec![0_usize; held.len()];
        loop {
            let values: Vec<&'d str> = held
                .iter()
                .zip(&at)
                .map(|(values, &i)| values[i].as_str())
                .collect();
            if !map.contains_key(&values.join(",")) {
                return Some(reads.iter().map(String::as_str).zip(values).collect());
            }
            let mut place = held.len();
            loop {
                place = place.checked_sub(1)?;
                at[place] += 1;
                if at[place] < held[place].len() {
                    break;
                }
                at[place] = 0;
            }
        }
    }
}

/// The lookup a row writes for `field`, `reads` and `map`, each value parsed by `parse`; `every`
/// says what a row writes instead to hold one value in every state.
///
/// # Errors
///
/// What is wrong, when the lookup reads no property or maps no value, a key does not give one
/// value for each property it reads, or `parse` refuses a value.
pub(super) fn lookup<V, T>(
    field: &str,
    reads: &[String],
    map: &IndexMap<String, V>,
    every: &str,
    parse: impl Fn(&V) -> Result<T, String>,
) -> Result<Held<T>, String> {
    if reads.is_empty() || map.is_empty() {
        return Err(format!(
            "the `{field}` lookup reads no property or maps no value; {every}"
        ));
    }
    let mut parsed = HashMap::with_capacity(map.len());
    for (key, value) in map {
        if key.split(',').count() != reads.len() {
            return Err(format!(
                "the `{field}` key `{key}` holds no value for each of {}",
                reads.join(", ")
            ));
        }
        parsed.insert(key.clone(), parse(value)?);
    }
    Ok(Held::Lookup {
        reads: reads.to_vec(),
        map: parsed,
    })
}
