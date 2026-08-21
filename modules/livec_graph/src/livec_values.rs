use serde::{Deserialize, Serialize};

use crate::StrId;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    I64(i64),
    F64(f64),
    Bool(bool),
    Str(StrId),
    Bytes(Vec<u8>),
    Null,
    ArrayFloat(Vec<f32>),
}

impl Eq for Value {}

impl std::hash::Hash for Value {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        use Value::*;
        match self {
            I64(v) => {
                0u8.hash(state);
                v.hash(state);
            }
            F64(v) => {
                1u8.hash(state);
                v.to_bits().hash(state);
            }
            Bool(v) => {
                2u8.hash(state);
                v.hash(state);
            }
            Str(id) => {
                3u8.hash(state);
                id.hash(state);
            }
            Bytes(b) => {
                4u8.hash(state);
                b.hash(state);
            }
            ArrayFloat(vs) => {
                5u8.hash(state);
                for f in vs {
                    f.to_bits().hash(state);
                }
            }
            Null => {
                5u8.hash(state);
            }
        }
    }
}
