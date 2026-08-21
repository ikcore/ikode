use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::{livec_crypto, LValue, LivecGraph};
use crate::{EdgeId, GraphError, LabelId, NodeId, PropKeyId};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TxnResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nodes: Option<Vec<NodeId>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edges: Option<Vec<EdgeId>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PropKey {
    #[serde(rename = "id", alias = "Id", alias = "ID")]
    Id(u32),
    #[serde(rename = "name", alias = "Name", alias = "prop_name")]
    Name(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LabelRef {
    #[serde(with = "label_single")]
    Single(LabelSingle),
    Many(Vec<LabelRef>),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LabelSingle {
    #[serde(rename = "id", alias = "Id", alias = "ID")]
    Id(u32),
    #[serde(rename = "name", alias = "Name", alias = "label_name")]
    Name(String),
}

mod label_single {
    use super::LabelSingle;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(ls: &LabelSingle, s: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        #[serde(untagged)]
        enum Obj<'a> {
            Id {
                #[serde(rename = "id")]
                id: u32,
            },
            Name {
                #[serde(rename = "name")]
                name: &'a str,
            },
        }
        match ls {
            LabelSingle::Id(id) => Obj::Id { id: *id }.serialize(s),
            LabelSingle::Name(n) => Obj::Name { name: n }.serialize(s),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<LabelSingle, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Obj {
            Id {
                #[serde(rename = "id")]
                id: u32,
            },
            Name {
                #[serde(rename = "name")]
                name: String,
            },
        }
        let o: Obj = Obj::deserialize(d)?;
        Ok(match o {
            Obj::Id { id } => LabelSingle::Id(id),
            Obj::Name { name } => LabelSingle::Name(name),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Value {
    #[serde(rename = "str", alias = "Str")]
    Str(String),
    #[serde(rename = "i64", alias = "I64")]
    I64(i64),
    #[serde(rename = "f64", alias = "F64")]
    F64(f64),
    #[serde(rename = "bool", alias = "Bool")]
    Bool(bool),
    #[serde(rename = "bytes", alias = "Bytes")]
    Bytes(Vec<u8>),
    #[serde(rename = "array_float", alias = "ArrayFloat")]
    ArrayFloat(Vec<f32>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Props(pub Vec<(PropKey, Option<Value>)>);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NodeRef {
    #[serde(rename = "by_xid", alias = "ByXid", alias = "xid")]
    ByXid(String),
    #[serde(rename = "by_id", alias = "ById", alias = "id")]
    ById(u32),
    #[serde(rename = "by_index", alias = "ByIndex", alias = "index")]
    ByIndex {
        label: LabelRef,
        key: PropKey,
        value: Value,
    },
    #[serde(rename = "by_alias", alias = "ByAlias", alias = "alias")]
    ByAlias(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NodeIdent {
    #[serde(rename = "by_id", alias = "ById", alias = "id")]
    ById(u32),
    #[serde(rename = "by_index", alias = "ByIndex")]
    ByIndex {
        label: LabelRef,
        key: PropKey,
        value: Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EdgeIdent {
    #[serde(rename = "by_xid", alias = "ByXid", alias = "xid")]
    ByXid(String),
    #[serde(rename = "by_id", alias = "ById", alias = "id")]
    ById(u32),
    #[serde(rename = "by_endpoints", alias = "ByEndpoints")]
    ByEndpoints {
        from: NodeRef,
        to: NodeRef,
        label: LabelRef,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NodeAction {
    #[serde(rename = "create", alias = "Create")]
    Create {
        label: LabelRef,
        props: Props,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        alias: Option<String>,
    },
    #[serde(rename = "merge", alias = "Merge")]
    Merge {
        ident: NodeIdent,
        label: LabelRef,
        props: Props,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        alias: Option<String>,
    },
    #[serde(rename = "update", alias = "Update")]
    Update {
        ident: NodeIdent,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<LabelRef>,
        props: Props,
    },
    #[serde(rename = "delete", alias = "Delete")]
    Delete { ident: NodeIdent },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EdgeAction {
    #[serde(rename = "create", alias = "Create")]
    Create {
        from: NodeRef,
        to: NodeRef,
        label: LabelRef,
        props: Props,
    },
    #[serde(rename = "merge", alias = "Merge")]
    Merge { ident: EdgeIdent, props: Props },
    #[serde(rename = "update", alias = "Update")]
    Update { ident: EdgeIdent, props: Props },
    #[serde(rename = "delete", alias = "Delete")]
    Delete { ident: EdgeIdent },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IndexScope {
    #[serde(rename = "global", alias = "Global")]
    Global,
    #[serde(rename = "label", alias = "Label")]
    Label(LabelRef),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum IndexType {
    #[serde(rename = "non_unique", alias = "NonUnique")]
    NonUnique,
    #[serde(rename = "unique", alias = "Unique")]
    Unique,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IndexTarget {
    #[serde(rename = "node", alias = "Node")]
    Node(PropKey),
    #[serde(rename = "edge", alias = "Edge")]
    Edge(PropKey),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IndexAction {
    #[serde(rename = "create_index", alias = "CreateIndex")]
    CreateIndex {
        scope: IndexScope,
        target: IndexTarget,
        idx_type: IndexType,
    },
    #[serde(rename = "drop_index", alias = "DropIndex")]
    DropIndex {
        scope: IndexScope,
        target: IndexTarget,
    },
    #[serde(rename = "re_index", alias = "Reindex")]
    Reindex {
        scope: IndexScope,
        target: IndexTarget,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphTxn {
    pub idempotency_key: Option<String>,
    pub timestamp_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nodes: Option<Vec<NodeAction>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edges: Option<Vec<EdgeAction>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indexes: Option<Vec<IndexAction>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GraphEvent {
    #[serde(rename = "txn", alias = "Txn")]
    Txn(GraphTxn),
    #[serde(rename = "snapshot", alias = "Snapshot")]
    Snapshot { graph_version: u64, data: Vec<u8> },
}

// ====== Adapter over LivecGraph ======

#[derive(Default, Serialize, Deserialize)]
struct NodeIndexDefs {
    // JSON objects cannot represent tuple keys, so snapshot this map as entries.
    #[serde(with = "node_index_defs_serde")]
    defs: HashMap<(Option<LabelId>, PropKeyId), IndexType>,
}

mod node_index_defs_serde {
    use super::{IndexType, LabelId, PropKeyId};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::HashMap;

    type Entry = (Option<LabelId>, PropKeyId, IndexType);
    type Definitions = HashMap<(Option<LabelId>, PropKeyId), IndexType>;

    pub fn serialize<S: Serializer>(defs: &Definitions, serializer: S) -> Result<S::Ok, S::Error> {
        defs.iter()
            .map(|(&(scope, key), &kind)| (scope, key, kind))
            .collect::<Vec<Entry>>()
            .serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Definitions, D::Error> {
        Ok(Vec::<Entry>::deserialize(deserializer)?
            .into_iter()
            .map(|(scope, key, kind)| ((scope, key), kind))
            .collect())
    }
}

#[derive(Default, Serialize, Deserialize)]
pub struct GraphAdapter {
    pub livec: LivecGraph,
    node_indexes: NodeIndexDefs,
    pub applied_ids: HashSet<String>,
    pub key: u64,
}

impl GraphAdapter {
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn finish_snapshot_restore(&mut self) {
        self.livec.label_names.rebuild_lookup();
        self.livec.prop_keys.rebuild_lookup();
        self.livec.strings.rebuild_lookup();
        self.livec.rebuild_eq_index();
    }

    /// Resolve a single label (Id or Name) into a LabelId.
    fn resolve_label_single(&mut self, ls: &LabelSingle) -> LabelId {
        match ls {
            LabelSingle::Id(id) => *id as LabelId,
            LabelSingle::Name(s) => self.livec.label_id(s.clone()),
        }
    }

    /// Resolve any LabelRef into 1..N LabelIds.
    fn resolve_labels(&mut self, lr: &LabelRef) -> Vec<LabelId> {
        match lr {
            LabelRef::Single(ls) => vec![self.resolve_label_single(ls)],
            LabelRef::Many(list) => {
                let mut out = Vec::with_capacity(list.len().max(1));
                for item in list {
                    match item {
                        LabelRef::Single(ls) => out.push(self.resolve_label_single(ls)),
                        LabelRef::Many(_) => {
                            // flatten nested Many
                            for id in self.resolve_labels(item) {
                                out.push(id);
                            }
                        }
                    }
                }
                out
            }
        }
    }

    fn primary_label_id(&mut self, lr: &LabelRef) -> Option<LabelId> {
        match lr {
            LabelRef::Single(ls) => Some(self.resolve_label_single(ls)),
            LabelRef::Many(list) => list.first().map(|first| match first {
                LabelRef::Single(ls) => self.resolve_label_single(ls),
                LabelRef::Many(_) => {
                    let ids = self.resolve_labels(lr);
                    ids.first().copied().unwrap_or(u32::MAX)
                }
            }),
        }
    }

    fn resolve_prop(&mut self, pk: &PropKey) -> PropKeyId {
        match pk {
            PropKey::Id(id) => *id as PropKeyId,
            PropKey::Name(s) => self.livec.key_id(s.clone()),
        }
    }

    fn convert_to_livec_value(&mut self, v: &Value) -> LValue {
        match v {
            Value::Str(s) => {
                let sid = self.livec.strings.intern(s.clone());
                self.livec.strings.inc_ref(sid);
                LValue::Str(sid)
            }
            Value::I64(i) => LValue::I64(*i),
            Value::F64(f) => LValue::F64(*f),
            Value::Bool(b) => LValue::Bool(*b),
            Value::Bytes(b) => LValue::Bytes(b.clone()),
            Value::ArrayFloat(b) => LValue::ArrayFloat(b.clone()),
        }
    }

    // Merge semantics for node props, enforcing optional unique-index constraints.
    fn set_node_prop_merge(
        &mut self,
        nid: NodeId,
        key: PropKeyId,
        vopt: &Option<Value>,
        primary_label_hint: Option<LabelId>,
    ) -> Result<(), GraphError> {
        // Unique index check (if any)
        if let Some((_scope, idx_type)) =
            self.node_indexes
                .defs
                .iter()
                .find_map(|((scope_lbl, k), it)| {
                    if *k == key {
                        Some((scope_lbl, it))
                    } else {
                        None
                    }
                })
        {
            if *idx_type == IndexType::Unique {
                if let Some(Value::Str(s)) = vopt {
                    // Check equality index if present
                    if self.livec.is_key_indexed(key) {
                        let lbl = primary_label_hint.unwrap_or_else(|| {
                            // best effort: use first label of this node if hint absent
                            self.livec.node_labels(nid).into_iter().next().unwrap_or(0)
                        });
                        let lv = self.convert_to_livec_value(&Value::Str(s.clone()));
                        let hits = self.livec.find_nodes_eq(lbl, key, &lv);
                        if hits.iter().any(|&other| other != nid) {
                            return Err(GraphError::UniqueViolation(
                                "label(key) unique violation".to_string(),
                            ));
                        }
                    }
                }
            }
        }

        match vopt {
            Some(v) => {
                let lv = self.convert_to_livec_value(v);
                self.livec.set_node_prop(nid, key, lv);
            }
            None => {
                // tombstone => set Null
                self.livec.set_node_prop(nid, key, LValue::Null);
            }
        }
        Ok(())
    }

    // ---- Node ref resolution ----

    fn resolve_node_ref(&mut self, nr: &NodeRef) -> Result<NodeId, GraphError> {
        match nr {
            NodeRef::ByXid(xid) => {
                if let Some((_txn_id, node_id, ty)) =
                    livec_crypto::fast_id_codec::decrypt_string(xid, self.key)
                {
                    if ty == 0 {
                        return Ok(node_id as NodeId);
                    }
                }
                Err(GraphError::NotFound(format!("node not found xid: {}", xid)))
            }
            NodeRef::ById(id) => Ok(*id as NodeId),
            NodeRef::ByIndex { label, key, value } => {
                let lid = self
                    .primary_label_id(label)
                    .ok_or_else(|| GraphError::Conflict("ByIndex label cannot be empty".into()))?;
                let k = self.resolve_prop(key);
                if !self.livec.is_key_indexed(k) {
                    return Err(GraphError::MissingIndex(
                        "index required for ByIndex".into(),
                    ));
                }
                let lv = self.convert_to_livec_value(value);
                if let Some(nid) = {
                    let hits = self.livec.find_nodes_eq(lid, k, &lv);
                    hits.into_iter().next()
                } {
                    Ok(nid)
                } else {
                    Err(GraphError::NotFound("node not found (ByIndex)".into()))
                }
            }
            NodeRef::ByAlias(_) => {
                // The alias-aware path is resolve_node_ref_with_aliases(); this fallback is only
                // here to keep signature parity. If we reach here, it's a bug in the caller.
                Err(GraphError::NotFound(
                    "alias not available in this context".into(),
                ))
            }
        }
    }

    fn resolve_node_ref_with_aliases(
        &mut self,
        nr: &NodeRef,
        aliases: &HashMap<String, NodeId>,
    ) -> Result<NodeId, GraphError> {
        match nr {
            NodeRef::ByAlias(a) => aliases
                .get(a)
                .copied()
                .ok_or_else(|| GraphError::NotFound(format!("alias not bound: {}", a))),
            _ => self.resolve_node_ref(nr),
        }
    }

    /// Returns (nid, optional label_hint_for_updates)
    fn resolve_node_ident_for_update(
        &mut self,
        ident: &NodeIdent,
        label_hint: Option<LabelRef>,
    ) -> Result<(NodeId, Option<LabelId>), GraphError> {
        match ident {
            NodeIdent::ById(id) => {
                let nid = *id as NodeId;
                if (nid as usize) >= self.livec.nodes.len() || !self.livec.nodes[nid as usize].alive
                {
                    Err(GraphError::NotFound(format!("node {} not found", id)))
                } else {
                    let lid = label_hint.and_then(|lr| self.primary_label_id(&lr));
                    if let Some(req) = lid {
                        if !self.livec.node_has_label(nid, req) {
                            return Err(GraphError::NotFound(
                                "node with required label not found".into(),
                            ));
                        }
                    }
                    Ok((nid, lid))
                }
            }
            NodeIdent::ByIndex { label, key, value } => {
                let lid = self
                    .primary_label_id(label)
                    .ok_or_else(|| GraphError::Conflict("ByIndex label cannot be empty".into()))?;
                let k = self.resolve_prop(key);
                if !self.livec.is_key_indexed(k) {
                    return Err(GraphError::MissingIndex(
                        "index required for ByIndex".into(),
                    ));
                }
                let lv = self.convert_to_livec_value(value);
                if let Some(nid) = {
                    let hits = self.livec.find_nodes_eq(lid, k, &lv);
                    hits.into_iter().next()
                } {
                    // Assert label_hint if present
                    if let Some(lh) = label_hint.and_then(|lr| self.primary_label_id(&lr)) {
                        if !self.livec.node_has_label(nid, lh) {
                            return Err(GraphError::NotFound(
                                "node with required label not found".into(),
                            ));
                        }
                    }
                    Ok((nid, Some(lid)))
                } else {
                    Err(GraphError::NotFound("node not found (ByIndex)".into()))
                }
            }
        }
    }

    fn resolve_node_ident_for_merge_upsert(
        &mut self,
        ident: &NodeIdent,
        default_labels: &LabelRef,
    ) -> Result<(NodeId, LabelId, bool), GraphError> {
        let default_primary = self
            .primary_label_id(default_labels)
            .ok_or_else(|| GraphError::Conflict("default labels cannot be empty".into()))?;
        match ident {
            NodeIdent::ById(id) => {
                let nid = *id as NodeId;
                // Update-only for ById: must exist with the primary default label
                if self
                    .livec
                    .get_node_by_id_and_label(nid, default_primary)
                    .is_some()
                {
                    Ok((nid, default_primary, false))
                } else {
                    Err(GraphError::NotFound(format!(
                        "node id {} not found for merge",
                        id
                    )))
                }
            }
            NodeIdent::ByIndex { label, key, value } => {
                let lid = self
                    .primary_label_id(label)
                    .ok_or_else(|| GraphError::Conflict("ByIndex label cannot be empty".into()))?;
                let k = self.resolve_prop(key);
                if !self.livec.is_key_indexed(k) {
                    return Err(GraphError::MissingIndex(
                        "index required for Merge ByIndex".into(),
                    ));
                }
                let lv = self.convert_to_livec_value(value);
                if let Some(nid) = {
                    let hits = self.livec.find_nodes_eq(lid, k, &lv);
                    hits.into_iter().next()
                } {
                    Ok((nid, lid, false))
                } else {
                    // create with ALL provided default labels, seed ident key=value
                    let all_lids = self.resolve_labels(default_labels);
                    if all_lids.is_empty() {
                        return Err(GraphError::Conflict(
                            "cannot create node without labels".into(),
                        ));
                    }
                    let nid = self.livec.add_node(&all_lids);
                    self.livec.set_node_prop(nid, k, lv);
                    Ok((nid, default_primary, true))
                }
            }
        }
    }

    fn find_edge_by_endpoints(&self, from: NodeId, to: NodeId, type_id: LabelId) -> Option<EdgeId> {
        let ids = self.livec.edge_ids_between(from, to, Some(type_id));
        ids.into_iter().next()
    }

    fn resolve_edge_ident_for_update(&mut self, ident: &EdgeIdent) -> Result<EdgeId, GraphError> {
        match ident {
            EdgeIdent::ByXid(xid) => {
                if let Some((_txn_id, edge_id, ty)) =
                    livec_crypto::fast_id_codec::decrypt_string(xid, self.key)
                {
                    if ty == 1 {
                        return Ok(edge_id as EdgeId);
                    }
                }
                Err(GraphError::NotFound(format!("edge not found xid: {}", xid)))
            }
            EdgeIdent::ById(id) => {
                let eid = *id as EdgeId;
                if (eid as usize) >= self.livec.edges.len() || !self.livec.edges[eid as usize].alive
                {
                    Err(GraphError::NotFound(format!("edge {} not found", id)))
                } else {
                    Ok(eid)
                }
            }
            EdgeIdent::ByEndpoints { from, to, label } => {
                let from_id = self.resolve_node_ref(from)?;
                let to_id = self.resolve_node_ref(to)?;
                let lid = self
                    .primary_label_id(label)
                    .ok_or_else(|| GraphError::Conflict("edge label cannot be empty".into()))?;
                self.find_edge_by_endpoints(from_id, to_id, lid)
                    .ok_or_else(|| GraphError::NotFound("edge (by endpoints) not found".into()))
            }
        }
    }

    fn resolve_edge_ident_for_merge_upsert(
        &mut self,
        ident: &EdgeIdent,
    ) -> Result<(EdgeId, bool), GraphError> {
        match ident {
            EdgeIdent::ByXid(xid) => {
                if let Some((_txn_id, edge_id, ty)) =
                    livec_crypto::fast_id_codec::decrypt_string(xid, self.key)
                {
                    if (edge_id as usize) >= self.livec.edges.len()
                        || !self.livec.edges[edge_id as usize].alive
                    {
                        return Err(GraphError::NotFound(format!(
                            "edge {} not found for merge",
                            edge_id
                        )));
                    }
                    if ty == 1 {
                        return Ok((edge_id as EdgeId, false));
                    }
                }
                Err(GraphError::NotFound(format!("edge not found xid: {}", xid)))
            }
            EdgeIdent::ById(id) => {
                let eid = *id as EdgeId;
                if (eid as usize) >= self.livec.edges.len() || !self.livec.edges[eid as usize].alive
                {
                    Err(GraphError::NotFound(format!(
                        "edge {} not found for merge",
                        id
                    )))
                } else {
                    Ok((eid, false))
                }
            }
            EdgeIdent::ByEndpoints { from, to, label } => {
                let from_id = self.resolve_node_ref(from)?;
                let to_id = self.resolve_node_ref(to)?;
                let lid = self
                    .primary_label_id(label)
                    .ok_or_else(|| GraphError::Conflict("edge label cannot be empty".into()))?;
                if let Some(eid) = self.find_edge_by_endpoints(from_id, to_id, lid) {
                    Ok((eid, false))
                } else {
                    let new_id = self.livec.add_edge(from_id, to_id, lid);
                    Ok((new_id, true))
                }
            }
        }
    }

    fn resolve_edge_ident_for_merge_upsert_with_aliases(
        &mut self,
        ident: &EdgeIdent,
        aliases: &HashMap<String, NodeId>,
    ) -> Result<(EdgeId, bool), GraphError> {
        match ident {
            EdgeIdent::ByXid(_) => self.resolve_edge_ident_for_merge_upsert(ident),
            EdgeIdent::ById(_) => self.resolve_edge_ident_for_merge_upsert(ident),
            EdgeIdent::ByEndpoints { from, to, label } => {
                let from_id = self.resolve_node_ref_with_aliases(from, aliases)?;
                let to_id = self.resolve_node_ref_with_aliases(to, aliases)?;
                let lid = self
                    .primary_label_id(label)
                    .ok_or_else(|| GraphError::Conflict("edge label cannot be empty".into()))?;
                if let Some(eid) = self.find_edge_by_endpoints(from_id, to_id, lid) {
                    Ok((eid, false))
                } else {
                    let new_id = self.livec.add_edge(from_id, to_id, lid);
                    Ok((new_id, true))
                }
            }
        }
    }

    fn resolve_edge_ident_for_update_with_aliases(
        &mut self,
        ident: &EdgeIdent,
        aliases: &HashMap<String, NodeId>,
    ) -> Result<EdgeId, GraphError> {
        match ident {
            EdgeIdent::ByXid(_) => self.resolve_edge_ident_for_update(ident),
            EdgeIdent::ById(_) => self.resolve_edge_ident_for_update(ident),
            EdgeIdent::ByEndpoints { from, to, label } => {
                let from_id = self.resolve_node_ref_with_aliases(from, aliases)?;
                let to_id = self.resolve_node_ref_with_aliases(to, aliases)?;
                let lid = self
                    .primary_label_id(label)
                    .ok_or_else(|| GraphError::Conflict("edge label cannot be empty".into()))?;
                self.find_edge_by_endpoints(from_id, to_id, lid)
                    .ok_or_else(|| GraphError::NotFound("edge (by endpoints) not found".into()))
            }
        }
    }

    fn apply_index_action(&mut self, act: &IndexAction) -> Result<(), GraphError> {
        let (scope_label, (target_is_node, key_id), idx_type) = match act {
            IndexAction::CreateIndex {
                scope,
                target,
                idx_type,
            } => {
                let scope_label = match scope {
                    IndexScope::Global => None,
                    IndexScope::Label(lr) => self.primary_label_id(lr),
                };
                let (is_node, key_id) = match target {
                    IndexTarget::Node(pk) => (true, self.resolve_prop(pk)),
                    IndexTarget::Edge(_pk) => (false, 0), // edge indexes not wired into executor in current adapter
                };
                (scope_label, (is_node, key_id), Some(*idx_type))
            }
            IndexAction::DropIndex { scope, target } => {
                let scope_label = match scope {
                    IndexScope::Global => None,
                    IndexScope::Label(lr) => self.primary_label_id(lr),
                };
                let (is_node, key_id) = match target {
                    IndexTarget::Node(pk) => (true, self.resolve_prop(pk)),
                    IndexTarget::Edge(_pk) => (false, 0),
                };
                (scope_label, (is_node, key_id), None)
            }
            IndexAction::Reindex { scope, target } => {
                let scope_label = match scope {
                    IndexScope::Global => None,
                    IndexScope::Label(lr) => self.primary_label_id(lr),
                };
                let (is_node, key_id) = match target {
                    IndexTarget::Node(pk) => (true, self.resolve_prop(pk)),
                    IndexTarget::Edge(_pk) => (false, 0),
                };
                let prev = self
                    .node_indexes
                    .defs
                    .get(&(scope_label, key_id))
                    .copied()
                    .unwrap_or(IndexType::NonUnique);
                (scope_label, (is_node, key_id), Some(prev))
            }
        };

        if target_is_node {
            match idx_type {
                Some(t) => {
                    self.node_indexes.defs.insert((scope_label, key_id), t);
                    self.livec.index_key(key_id);
                }
                None => {
                    self.node_indexes.defs.remove(&(scope_label, key_id));
                    self.livec.unindex_key(key_id);
                }
            }
        }
        Ok(())
    }

    fn update_edge_set_or_tombstone(&mut self, eid: EdgeId, props: &Props) {
        for (pk, vopt) in &props.0 {
            let key = self.resolve_prop(pk);
            match vopt {
                Some(v) => {
                    let lv = self.convert_to_livec_value(v);
                    self.livec.set_edge_prop(eid, key, lv);
                }
                None => {
                    // tombstone on edges as well
                    self.livec.set_edge_prop(eid, key, LValue::Null);
                }
            }
        }
    }

    fn replace_edge_props(&mut self, eid: EdgeId, props: &Props) -> Result<(), GraphError> {
        if (eid as usize) >= self.livec.edges.len() || !self.livec.edges[eid as usize].alive {
            return Err(GraphError::NotFound(format!("edge {} not found", eid)));
        }
        if let Some(idx) = self.livec.edges[eid as usize].prop_ref {
            let rec = &mut self.livec.props[idx as usize];
            rec.entries.clear();
            rec.entries.shrink_to_fit();
        }
        self.update_edge_set_or_tombstone(eid, props);
        Ok(())
    }

    pub fn apply_txn(&mut self, txn: &GraphTxn) -> Result<TxnResponse, GraphError> {
        if let Some(k) = &txn.idempotency_key {
            if self.applied_ids.contains(k) {
                return Err(GraphError::AlreadyApplied(format!(
                    "transaction already applied: {}",
                    k
                )));
            }
        }
        for act in txn.indexes.as_ref().into_iter().flatten() {
            self.apply_index_action(act)?;
        }
        let mut aliases: HashMap<String, NodeId> = HashMap::new();

        let mut mutated_nodes = vec![];
        let mut mutated_edges = vec![];

        for act in txn.nodes.as_ref().into_iter().flatten() {
            match act {
                NodeAction::Create {
                    label,
                    props,
                    alias,
                } => {
                    let lids = self.resolve_labels(label);
                    if lids.is_empty() {
                        return Err(GraphError::Conflict(
                            "cannot create node without labels".into(),
                        ));
                    }
                    let primary = lids[0];
                    let nid = self.livec.add_node(&lids);
                    mutated_nodes.push(nid);
                    for (pk, vopt) in &props.0 {
                        let key = self.resolve_prop(pk);
                        self.set_node_prop_merge(nid, key, vopt, Some(primary))?;
                    }
                    if let Some(a) = alias {
                        if aliases.contains_key(a) {
                            return Err(GraphError::Conflict(format!(
                                "alias already bound: {}",
                                a
                            )));
                        }
                        aliases.insert(a.clone(), nid);
                    }
                }

                NodeAction::Merge {
                    ident,
                    label,
                    props,
                    alias,
                } => {
                    let (nid, label_used, _created) =
                        self.resolve_node_ident_for_merge_upsert(ident, label)?;
                    for (pk, vopt) in &props.0 {
                        let key = self.resolve_prop(pk);
                        self.set_node_prop_merge(nid, key, vopt, Some(label_used))?;
                    }
                    if let Some(a) = alias {
                        if aliases.contains_key(a) {
                            return Err(GraphError::Conflict(format!(
                                "alias already bound: {}",
                                a
                            )));
                        }
                        aliases.insert(a.clone(), nid);
                    }
                    mutated_nodes.push(nid);
                }

                NodeAction::Update {
                    ident,
                    label,
                    props,
                } => {
                    let (nid, label_hint) =
                        self.resolve_node_ident_for_update(ident, label.clone())?;
                    for (pk, vopt) in &props.0 {
                        let key = self.resolve_prop(pk);
                        self.set_node_prop_merge(nid, key, vopt, label_hint)?;
                    }
                    mutated_nodes.push(nid);
                }

                NodeAction::Delete { ident } => {
                    let (nid, _) = self.resolve_node_ident_for_update(ident, None)?;
                    if (nid as usize) >= self.livec.nodes.len()
                        || !self.livec.nodes[nid as usize].alive
                    {
                        return Err(GraphError::NotFound("node not found for delete".into()));
                    }
                    self.livec.delete_node(nid);
                }
            }
        }
        for act in txn.edges.as_ref().into_iter().flatten() {
            match act {
                EdgeAction::Create {
                    from,
                    to,
                    label,
                    props,
                } => {
                    let from_id = self.resolve_node_ref_with_aliases(from, &aliases)?;
                    let to_id = self.resolve_node_ref_with_aliases(to, &aliases)?;
                    let lid = self
                        .primary_label_id(label)
                        .ok_or_else(|| GraphError::Conflict("edge label cannot be empty".into()))?;
                    let eid = self.livec.add_edge(from_id, to_id, lid);
                    // apply props with tombstoning behavior
                    for (pk, vopt) in &props.0 {
                        let key = self.resolve_prop(pk);
                        match vopt {
                            Some(v) => {
                                let lv = self.convert_to_livec_value(v);
                                self.livec.set_edge_prop(eid, key, lv);
                            }
                            None => self.livec.set_edge_prop(eid, key, LValue::Null),
                        }
                    }
                    mutated_edges.push(eid);
                }

                EdgeAction::Merge { ident, props } => {
                    let (eid, created) =
                        self.resolve_edge_ident_for_merge_upsert_with_aliases(ident, &aliases)?;
                    if created {
                        self.update_edge_set_or_tombstone(eid, props);
                    } else {
                        self.replace_edge_props(eid, props)?;
                    }
                    mutated_edges.push(eid);
                }

                EdgeAction::Update { ident, props } => {
                    let eid = self.resolve_edge_ident_for_update_with_aliases(ident, &aliases)?;
                    self.update_edge_set_or_tombstone(eid, props);
                    mutated_edges.push(eid);
                }

                EdgeAction::Delete { ident } => {
                    let eid = self.resolve_edge_ident_for_update_with_aliases(ident, &aliases)?;
                    self.livec.delete_edge(eid);
                }
            }
        }

        let mut response = TxnResponse {
            status: Some("OK".into()),
            ..Default::default()
        };

        if !mutated_nodes.is_empty() {
            response.nodes = Some(mutated_nodes);
        }
        if !mutated_edges.is_empty() {
            response.edges = Some(mutated_edges);
        }
        if let Some(k) = &txn.idempotency_key {
            self.applied_ids.insert(k.clone());
        }
        Ok(response)
    }

    pub fn with_read<F: FnOnce(&LivecGraph)>(&self, f: F) {
        f(&self.livec);
    }
}
