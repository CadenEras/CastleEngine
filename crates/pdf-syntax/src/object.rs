//! PDF objects (ISO 32000-2:2020, 7.3).
//!
//! Direct objects own their decoded bytes. A stream keeps only the location of its
//! encoded data in the input: nothing is copied or decoded at this layer.

use std::collections::BTreeMap;
use std::ops::Range;

/// Identifier of an indirect object: positive object number and generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectId {
    pub number: u32,
    pub generation: u16,
}

impl ObjectId {
    /// Checks the ranges allowed by ISO 32000-2 (7.3.10): object number 1 or
    /// more, generation from 0 to 65 535.
    pub fn new(number: i64, generation: i64) -> Option<Self> {
        let number = u32::try_from(number).ok().filter(|n| *n > 0)?;
        let generation = u16::try_from(generation).ok()?;
        Some(Self { number, generation })
    }
}

/// A direct object. Strings and names hold decoded bytes; PDF does not fix
/// their encoding at this layer.
#[derive(Debug, Clone, PartialEq)]
pub enum Object {
    Null,
    Boolean(bool),
    Integer(i64),
    Real(f64),
    /// Literal or hexadecimal string, decoded.
    String(Vec<u8>),
    /// Name without the slash, `#xx` escapes resolved.
    Name(Vec<u8>),
    Array(Vec<Object>),
    Dictionary(Dictionary),
    /// `n g R`, not resolved: resolution needs the cross-reference table.
    Reference(ObjectId),
}

impl Object {
    pub fn as_integer(&self) -> Option<i64> {
        match self {
            Self::Integer(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_name(&self) -> Option<&[u8]> {
        match self {
            Self::Name(name) => Some(name),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Object]> {
        match self {
            Self::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_dictionary(&self) -> Option<&Dictionary> {
        match self {
            Self::Dictionary(dictionary) => Some(dictionary),
            _ => None,
        }
    }

    pub fn as_reference(&self) -> Option<ObjectId> {
        match self {
            Self::Reference(id) => Some(*id),
            _ => None,
        }
    }
}

/// Dictionary with unique keys, sorted by key bytes. Source order is not kept:
/// it carries no meaning in PDF. An entry whose value is `null` is stored as
/// written; ISO 32000-2 treats it as absent, which `get` callers must honour.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Dictionary {
    entries: BTreeMap<Vec<u8>, Object>,
}

impl Dictionary {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, key: &[u8]) -> Option<&Object> {
        self.entries.get(key)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&[u8], &Object)> {
        self.entries.iter().map(|(k, v)| (k.as_slice(), v))
    }

    /// Inserts an entry; returns the previous value for this key, if any.
    pub fn insert(&mut self, key: Vec<u8>, value: Object) -> Option<Object> {
        self.entries.insert(key, value)
    }

    /// Removes and returns an entry, so that a value can be moved out without
    /// copying it.
    pub fn remove(&mut self, key: &[u8]) -> Option<Object> {
        self.entries.remove(key)
    }
}

/// A stream object: its dictionary and the byte range of its encoded data in
/// the input, `/Length` bytes long and checked against the input size.
#[derive(Debug, Clone, PartialEq)]
pub struct Stream {
    pub dictionary: Dictionary,
    pub data: Range<u64>,
}

/// Body of an indirect object.
#[derive(Debug, Clone, PartialEq)]
pub enum IndirectValue {
    Object(Object),
    Stream(Stream),
}

/// `n g obj ... endobj` as read at a given offset.
#[derive(Debug, Clone, PartialEq)]
pub struct IndirectObject {
    pub id: ObjectId,
    pub value: IndirectValue,
}
