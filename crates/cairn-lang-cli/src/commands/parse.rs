//! `cairn parse`: the AST of a source.

use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::process::ExitCode;

use cairn_lang_core::ast::{Item, Module, Statement};
use cairn_lang_core::lower;
use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::ser::{Serialize, SerializeMap, Serializer};
use serde_json::Number;

use crate::cli::ParseFormat;
use crate::pipeline::load_module;
use crate::report::print_json;

pub(crate) fn run_parse(file: &Path, format: ParseFormat) -> ExitCode {
    let (_, module) = match load_module(file, format.failure_report()) {
        Ok(loaded) => loaded,
        Err(code) => return code,
    };
    match format {
        ParseFormat::Json => match document(&module) {
            Ok(document) => match print_json("AST", &document) {
                Ok(()) => ExitCode::SUCCESS,
                Err(code) => code,
            },
            Err(err) => {
                eprintln!("error: failed to serialise AST as JSON: {err}");
                ExitCode::from(1)
            }
        },
        ParseFormat::Debug => {
            outln!("{module:#?}");
            ExitCode::SUCCESS
        }
    }
}

/// The JSON of `module`, each member line of a `struct` or `def` body carrying the address the
/// Intent IR gives it.
fn document(module: &Module) -> serde_json::Result<Json> {
    let text = serde_json::to_string(module)?;
    let mut reader = serde_json::Deserializer::from_str(&text);
    reader.disable_recursion_limit();
    let mut document = Json::deserialize(&mut reader)?;
    reader.end()?;
    let ir = lower(module);
    let addresses: HashMap<usize, String> = ir
        .addressed_members()
        .into_iter()
        .filter_map(|(_, _, member)| {
            Some((member.span.start, member.address.as_ref()?.to_string()))
        })
        .collect();
    let Some(items) = document.get_mut("items").and_then(Json::as_array_mut) else {
        return Ok(document);
    };
    for (item, json) in module.items.iter().zip(items) {
        if let Item::Struct { body, .. } | Item::Def { body, .. } = item {
            add_to_body(body, json.get_mut("body"), &addresses);
        }
    }
    Ok(document)
}

/// Puts on each line of `json`, the JSON array of `body`, the address `addresses` holds for it.
fn add_to_body(body: &[Statement], json: Option<&mut Json>, addresses: &HashMap<usize, String>) {
    let Some(lines) = json.and_then(Json::as_array_mut) else {
        return;
    };
    for (statement, line) in body.iter().zip(lines) {
        let children = match statement {
            Statement::Generic { span, children, .. } => {
                if let (Some(address), Json::Object(fields)) =
                    (addresses.get(&span.start), &mut *line)
                {
                    fields.push(("address".to_owned(), Json::String(address.clone())));
                }
                children
            }
            Statement::EditionGuard { children, .. } => children,
            _ => continue,
        };
        add_to_body(children, line.get_mut("children"), addresses);
    }
}

/// A JSON value whose objects keep their keys in the order they were read.
enum Json {
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    fn get_mut(&mut self, key: &str) -> Option<&mut Self> {
        let Self::Object(fields) = self else {
            return None;
        };
        fields.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    fn as_array_mut(&mut self) -> Option<&mut Vec<Self>> {
        match self {
            Self::Array(values) => Some(values),
            _ => None,
        }
    }
}

impl Serialize for Json {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Null => serializer.serialize_unit(),
            Self::Bool(b) => serializer.serialize_bool(*b),
            Self::Number(n) => n.serialize(serializer),
            Self::String(s) => serializer.serialize_str(s),
            Self::Array(values) => serializer.collect_seq(values),
            Self::Object(fields) => {
                let mut map = serializer.serialize_map(Some(fields.len()))?;
                for (key, value) in fields {
                    map.serialize_entry(key, value)?;
                }
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for Json {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(JsonVisitor)
    }
}

struct JsonVisitor;

impl<'de> Visitor<'de> for JsonVisitor {
    type Value = Json;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON value")
    }

    fn visit_unit<E: de::Error>(self) -> Result<Json, E> {
        Ok(Json::Null)
    }

    fn visit_bool<E: de::Error>(self, b: bool) -> Result<Json, E> {
        Ok(Json::Bool(b))
    }

    fn visit_i64<E: de::Error>(self, n: i64) -> Result<Json, E> {
        Ok(Json::Number(n.into()))
    }

    fn visit_u64<E: de::Error>(self, n: u64) -> Result<Json, E> {
        Ok(Json::Number(n.into()))
    }

    fn visit_f64<E: de::Error>(self, n: f64) -> Result<Json, E> {
        Number::from_f64(n)
            .map(Json::Number)
            .ok_or_else(|| E::custom("a JSON number is finite"))
    }

    fn visit_str<E: de::Error>(self, s: &str) -> Result<Json, E> {
        Ok(Json::String(s.to_owned()))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Json, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = seq.next_element()? {
            values.push(value);
        }
        Ok(Json::Array(values))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Json, A::Error> {
        let mut fields = Vec::new();
        while let Some(entry) = map.next_entry()? {
            fields.push(entry);
        }
        Ok(Json::Object(fields))
    }
}
