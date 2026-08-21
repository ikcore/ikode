use crate::{GSize, StrId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct StringInterner {
    /// Rebuilt from `id_to_value` after snapshot recovery. Omitting this reverse
    /// lookup avoids storing every interned string twice.
    #[serde(skip)]
    value_to_id: HashMap<String, StrId>,
    id_to_value: Vec<String>,
    ref_counts: Vec<GSize>,
}

impl StringInterner {
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn rebuild_lookup(&mut self) {
        self.value_to_id = self
            .id_to_value
            .iter()
            .enumerate()
            .map(|(id, value)| (value.clone(), id as StrId))
            .collect();
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.id_to_value.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.id_to_value.is_empty()
    }

    pub fn intern<S: Into<String>>(&mut self, s: S) -> StrId {
        let s = s.into();
        if let Some(&id) = self.value_to_id.get(&s) {
            return id;
        }
        let id = self.id_to_value.len() as StrId;
        self.id_to_value.push(s.clone());
        self.value_to_id.insert(s, id);
        self.ref_counts.push(0);
        id
    }

    #[inline]
    pub fn resolve(&self, id: StrId) -> Option<&str> {
        self.id_to_value.get(id as usize).map(|s| s.as_str())
    }

    #[inline]
    pub fn inc_ref(&mut self, id: StrId) {
        if let Some(rc) = self.ref_counts.get_mut(id as usize) {
            *rc = rc.saturating_add(1);
        }
    }

    #[inline]
    pub fn dec_ref(&mut self, id: StrId) {
        if let Some(rc) = self.ref_counts.get_mut(id as usize) {
            *rc = rc.saturating_sub(1);
        }
    }

    #[inline]
    pub fn ref_count(&self, id: StrId) -> Option<u32> {
        self.ref_counts.get(id as usize).copied()
    }

    #[inline]
    pub fn get_id(&self, s: &str) -> Option<StrId> {
        self.value_to_id.get(s).copied()
    }

    #[inline]
    pub fn resolve_name_id(&self, s: &str) -> GSize {
        self.value_to_id.get(s).copied().unwrap_or(GSize::MAX)
    }
}
