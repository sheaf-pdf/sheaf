//! A change to a field that can be empty: keep it, clear it, or set it.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A change to an optional field. In JSON, leaving the field out keeps it,
/// `null` clears it, and a value sets it; a patch's fields are
/// `#[serde(default, skip_serializing_if = "Patch::is_keep")]`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub enum Patch<T> {
    #[default]
    Keep,
    Clear,
    Set(T),
}

impl<T> Patch<T> {
    pub fn is_keep(&self) -> bool {
        matches!(self, Patch::Keep)
    }

    /// The field `to`, changed.
    pub fn apply(self, to: &mut Option<T>) {
        match self {
            Patch::Keep => {}
            Patch::Clear => *to = None,
            Patch::Set(v) => *to = Some(v),
        }
    }
}

impl<T: Serialize> Serialize for Patch<T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Patch::Keep => Err(serde::ser::Error::custom(
                "a patch keeps a field by leaving it out",
            )),
            Patch::Clear => s.serialize_none(),
            Patch::Set(v) => s.serialize_some(v),
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Patch<T> {
    /// Called only for a field that's there: one left out is the default,
    /// [`Patch::Keep`].
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Patch<T>, D::Error> {
        Ok(match Option::<T>::deserialize(d)? {
            None => Patch::Clear,
            Some(v) => Patch::Set(v),
        })
    }
}

impl<T: JsonSchema> JsonSchema for Patch<T> {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        Option::<T>::schema_name()
    }

    fn json_schema(g: &mut SchemaGenerator) -> Schema {
        Option::<T>::json_schema(g)
    }
}
