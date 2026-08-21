use crate::LValue as Value;
use crate::{livec_crypto, EdgeId, GSize, LabelId, NodeId, PropKeyId, StringInterner};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PropEntry {
    pub key: PropKeyId,
    pub value: Value,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PropRecord {
    pub entries: Vec<PropEntry>,
}

impl PropRecord {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
    pub fn get(&self, key: PropKeyId) -> Option<&Value> {
        self.entries.iter().find(|e| e.key == key).map(|e| &e.value)
    }
    pub fn set(&mut self, key: PropKeyId, value: Value) -> Option<Value> {
        if let Some(e) = self.entries.iter_mut().find(|e| e.key == key) {
            let old = e.value.clone();
            e.value = value;
            Some(old)
        } else {
            self.entries.push(PropEntry { key, value });
            None
        }
    }
    pub fn remove(&mut self, key: PropKeyId) -> Option<Value> {
        if let Some(i) = self.entries.iter().position(|e| e.key == key) {
            Some(self.entries.swap_remove(i).value)
        } else {
            None
        }
    }
}

// ====== Nodes and Edges with intrusive adjacency ======
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeRec {
    pub id: NodeId,
    pub first_out: Option<EdgeId>,
    pub first_in: Option<EdgeId>,
    pub labels: Vec<LabelId>,    // sorted unique
    pub prop_ref: Option<GSize>, // index into PropStore
    pub alive: bool,
    pub txn_id: GSize,
}

impl NodeRec {
    pub fn get_xid(&self, key: u64) -> String {
        livec_crypto::fast_id_codec::encrypt_string(self.txn_id as u64, self.id as u64, 0, key)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EdgeRec {
    pub id: EdgeId,
    pub src: NodeId,
    pub dst: NodeId,
    pub type_id: LabelId, // relationship type
    pub next_out: Option<EdgeId>,
    pub prev_out: Option<EdgeId>,
    pub next_in: Option<EdgeId>,
    pub prev_in: Option<EdgeId>,
    pub prop_ref: Option<GSize>,
    pub alive: bool,
    pub txn_id: GSize,
}

impl EdgeRec {
    pub fn get_xid(&self, key: u64) -> String {
        livec_crypto::fast_id_codec::encrypt_string(self.txn_id as u64, self.id as u64, 0, key)
    }
}

// ====== Eq index for (label, key) -> value -> NodeId set ======
#[derive(Default, Debug)]
pub struct EqIndex {
    map: HashMap<(LabelId, PropKeyId), HashMap<Value, HashSet<NodeId>>>,
}

impl EqIndex {
    pub fn add(&mut self, label: LabelId, key: PropKeyId, value: Value, node: NodeId) {
        self.map
            .entry((label, key))
            .or_default()
            .entry(value)
            .or_default()
            .insert(node);
    }
    pub fn remove(&mut self, label: LabelId, key: PropKeyId, value: &Value, node: NodeId) {
        if let Some(val_map) = self.map.get_mut(&(label, key)) {
            if let Some(set) = val_map.get_mut(value) {
                set.remove(&node);
                if set.is_empty() {
                    val_map.remove(value);
                }
            }
        }
    }
    pub fn lookup(
        &self,
        label: LabelId,
        key: PropKeyId,
        value: &Value,
    ) -> Option<&HashSet<NodeId>> {
        self.map.get(&(label, key))?.get(value)
    }
}

// ====== Graph storage ======
#[derive(Default, Debug, Serialize, Deserialize)]
pub struct LivecGraph {
    pub nodes: Vec<NodeRec>,
    pub edges: Vec<EdgeRec>,
    pub props: Vec<PropRecord>,
    pub label_names: StringInterner,
    pub prop_keys: StringInterner,
    pub strings: StringInterner,
    #[serde(skip)]
    pub eq_index: EqIndex,
    pub free_nodes: Vec<NodeId>,
    pub free_edges: Vec<EdgeId>,
    pub indexed_keys: HashSet<PropKeyId>,
    pub insertions: GSize,
    pub key: u64,
}

impl LivecGraph {
    pub fn new() -> Self {
        Self {
            indexed_keys: HashSet::new(),
            ..Self::default()
        }
    }

    /// Rebuild the derived equality index after loading a snapshot.
    ///
    /// The index is deliberately omitted from snapshots: it can be reconstructed
    /// from live node properties and would otherwise duplicate a large amount of
    /// data on disk.
    pub(crate) fn rebuild_eq_index(&mut self) {
        self.eq_index = EqIndex::default();
        for (node_id, node) in self.nodes.iter().enumerate() {
            if !node.alive {
                continue;
            }
            let Some(prop_ref) = node.prop_ref else {
                continue;
            };
            let Some(record) = self.props.get(prop_ref as usize) else {
                continue;
            };
            for entry in &record.entries {
                if !self.indexed_keys.contains(&entry.key) || matches!(entry.value, Value::Null) {
                    continue;
                }
                for &label in &node.labels {
                    self.eq_index
                        .add(label, entry.key, entry.value.clone(), node_id as NodeId);
                }
            }
        }
    }
    pub fn label_id<S: Into<String>>(&mut self, name: S) -> LabelId {
        self.label_names.intern(name)
    }
    pub fn key_id<S: Into<String>>(&mut self, key: S) -> PropKeyId {
        self.prop_keys.intern(key)
    }

    #[inline]
    pub fn index_key(&mut self, key: PropKeyId) {
        self.indexed_keys.insert(key);
    }

    #[inline]
    pub fn unindex_key(&mut self, key: PropKeyId) {
        self.indexed_keys.remove(&key);
    }

    #[inline]
    pub fn is_key_indexed(&self, key: PropKeyId) -> bool {
        self.indexed_keys.contains(&key)
    }

    #[inline]
    fn inc_value_refs(&mut self, v: &Value) {
        if let Value::Str(id) = v {
            self.strings.inc_ref(*id);
        }
    }

    #[inline]
    fn dec_value_refs(&mut self, v: &Value) {
        if let Value::Str(id) = v {
            self.strings.dec_ref(*id);
        }
    }

    #[inline]
    fn get_txn(&mut self) -> GSize {
        let txn_id = self.insertions;
        self.insertions += 1;
        txn_id
    }

    pub fn add_node(&mut self, labels: &[LabelId]) -> NodeId {
        let mut ls = labels.to_vec();
        ls.sort_unstable();
        ls.dedup();
        let txn_id = self.get_txn();
        self.insertions += 1;

        if let Some(id) = self.free_nodes.pop() {
            let slot = &mut self.nodes[id as usize];
            slot.first_out = None;
            slot.first_in = None;
            slot.labels = ls;
            slot.prop_ref = None;
            slot.alive = true;
            return id;
        }
        let id = self.nodes.len() as NodeId;
        self.nodes.push(NodeRec {
            id,
            first_out: None,
            first_in: None,
            labels: ls,
            prop_ref: None,
            alive: true,
            txn_id,
        });
        id
    }
    pub fn add_edge(&mut self, src: NodeId, dst: NodeId, type_id: LabelId) -> EdgeId {
        assert!(
            self.nodes
                .get(src as usize)
                .map(|n| n.alive)
                .unwrap_or(false),
            "src not alive"
        );
        assert!(
            self.nodes
                .get(dst as usize)
                .map(|n| n.alive)
                .unwrap_or(false),
            "dst not alive"
        );

        let txn_id = self.get_txn();
        self.insertions += 1;

        let src_head = self.nodes[src as usize].first_out;
        let dst_head = self.nodes[dst as usize].first_in;

        let id = if let Some(id) = self.free_edges.pop() {
            let e = &mut self.edges[id as usize];
            e.id = id;
            e.src = src;
            e.dst = dst;
            e.type_id = type_id;
            e.next_out = src_head;
            e.prev_out = None;
            e.next_in = dst_head;
            e.prev_in = None;
            e.prop_ref = None;
            e.alive = true;
            id
        } else {
            let id = self.edges.len() as EdgeId;
            self.edges.push(EdgeRec {
                id,
                src,
                dst,
                type_id,
                next_out: src_head,
                prev_out: None,
                next_in: dst_head,
                prev_in: None,
                prop_ref: None,
                alive: true,
                txn_id,
            });
            id
        };
        if let Some(h) = src_head {
            self.edges[h as usize].prev_out = Some(id);
        }
        if let Some(h) = dst_head {
            self.edges[h as usize].prev_in = Some(id);
        }

        self.nodes[src as usize].first_out = Some(id);
        self.nodes[dst as usize].first_in = Some(id);
        id
    }

    fn detach_node(&mut self, n: NodeId) {
        let (labels, prop_ref_opt) = {
            let nd = &self.nodes[n as usize];
            (nd.labels.clone(), nd.prop_ref)
        };
        if let Some(pidx) = prop_ref_opt {
            let entries = std::mem::take(&mut self.props[pidx as usize].entries);
            for PropEntry { key, value } in entries {
                if self.is_key_indexed(key) && !matches!(value, Value::Null) {
                    for &lbl in &labels {
                        self.eq_index.remove(lbl, key, &value, n);
                    }
                }
                self.dec_value_refs(&value);
            }
            self.props[pidx as usize].entries.shrink_to_fit();
            self.nodes[n as usize].prop_ref = None;
        }
    }

    fn detach_edge(&mut self, e: EdgeId) {
        if let Some(pidx) = self.edges[e as usize].prop_ref {
            let entries = std::mem::take(&mut self.props[pidx as usize].entries);
            for PropEntry { value, .. } in entries {
                self.dec_value_refs(&value);
            }
            self.props[pidx as usize].entries.shrink_to_fit();
            self.edges[e as usize].prop_ref = None;
        }
    }

    pub fn delete_edge(&mut self, e: EdgeId) {
        if e as usize >= self.edges.len() {
            return;
        }
        if !self.edges[e as usize].alive {
            return;
        }
        let (src, dst, prev_out, next_out, prev_in, next_in) = {
            let edge = &self.edges[e as usize];
            (
                edge.src as usize,
                edge.dst as usize,
                edge.prev_out,
                edge.next_out,
                edge.prev_in,
                edge.next_in,
            )
        };
        match (prev_out, next_out) {
            (None, Some(nxt)) => {
                self.nodes[src].first_out = Some(nxt);
                self.edges[nxt as usize].prev_out = None;
            }
            (None, None) => {
                self.nodes[src].first_out = None;
            }
            (Some(prv), Some(nxt)) => {
                self.edges[prv as usize].next_out = Some(nxt);
                self.edges[nxt as usize].prev_out = Some(prv);
            }
            (Some(prv), None) => {
                self.edges[prv as usize].next_out = None;
            }
        }
        match (prev_in, next_in) {
            (None, Some(nxt)) => {
                self.nodes[dst].first_in = Some(nxt);
                self.edges[nxt as usize].prev_in = None;
            }
            (None, None) => {
                self.nodes[dst].first_in = None;
            }
            (Some(prv), Some(nxt)) => {
                self.edges[prv as usize].next_in = Some(nxt);
                self.edges[nxt as usize].prev_in = Some(prv);
            }
            (Some(prv), None) => {
                self.edges[prv as usize].next_in = None;
            }
        }
        self.detach_edge(e);
        self.edges[e as usize].alive = false;
        self.free_edges.push(e);
    }

    pub fn delete_node(&mut self, n: NodeId) {
        if n as usize >= self.nodes.len() {
            return;
        }
        if !self.nodes[n as usize].alive {
            return;
        }

        let mut to_del = Vec::new();
        let mut e = self.nodes[n as usize].first_out;
        while let Some(id) = e {
            let ed = &self.edges[id as usize];
            e = ed.next_out;
            if ed.alive {
                to_del.push(id);
            }
        }
        let mut e2 = self.nodes[n as usize].first_in;
        while let Some(id) = e2 {
            let ed = &self.edges[id as usize];
            e2 = ed.next_in;
            if ed.alive {
                to_del.push(id);
            }
        }

        for id in to_del {
            self.delete_edge(id);
        }
        self.detach_node(n);
        self.nodes[n as usize].first_out = None;
        self.nodes[n as usize].first_in = None;
        self.nodes[n as usize].alive = false;
        self.free_nodes.push(n);
    }

    pub fn set_node_prop(&mut self, n: NodeId, key: PropKeyId, value: Value) {
        let (labels, mut prop_idx_opt) = {
            let node = &self.nodes[n as usize];
            (node.labels.clone(), node.prop_ref)
        };
        if prop_idx_opt.is_none() {
            let idx = self.props.len() as u32;
            self.props.push(PropRecord::new());
            self.nodes[n as usize].prop_ref = Some(idx);
            prop_idx_opt = Some(idx);
        }
        let prop_idx = prop_idx_opt.unwrap();
        let old = {
            let rec = &mut self.props[prop_idx as usize];
            rec.set(key, value.clone())
        };
        self.inc_value_refs(&value);
        if let Some(ref oldv) = old {
            self.dec_value_refs(oldv);
        }
        if self.is_key_indexed(key) {
            if let Some(oldv) = &old {
                if !matches!(oldv, Value::Null) {
                    for &lbl in &labels {
                        self.eq_index.remove(lbl, key, oldv, n);
                    }
                }
            }
            if !matches!(value, Value::Null) {
                for &lbl in &labels {
                    self.eq_index.add(lbl, key, value.clone(), n);
                }
            }
        }
    }

    pub fn remove_node_prop(&mut self, n: NodeId, key: PropKeyId) -> Option<Value> {
        let (labels, prop_idx) = {
            let node = self.nodes.get(n as usize)?;
            if !node.alive {
                return None;
            }
            (node.labels.clone(), node.prop_ref?)
        };
        let old = self.props[prop_idx as usize].get(key).cloned()?;

        if self.is_key_indexed(key) && !matches!(old, Value::Null) {
            for &lbl in &labels {
                self.eq_index.remove(lbl, key, &old, n);
            }
        }
        let _ = self.props[prop_idx as usize].remove(key);
        self.dec_value_refs(&old);
        Some(old)
    }

    pub fn get_node_prop(&self, n: NodeId, key: PropKeyId) -> Option<&Value> {
        let node = self.nodes.get(n as usize)?;
        let pidx = node.prop_ref? as usize;
        self.props.get(pidx)?.get(key)
    }

    pub fn set_edge_prop(&mut self, e: EdgeId, key: PropKeyId, value: Value) {
        let prop_idx = match self.edges[e as usize].prop_ref {
            Some(idx) => idx,
            None => {
                let idx = self.props.len() as u32;
                self.props.push(PropRecord::new());
                self.edges[e as usize].prop_ref = Some(idx);
                idx
            }
        };
        let old = self.props[prop_idx as usize].set(key, value.clone());
        self.inc_value_refs(&value);
        if let Some(ref oldv) = old {
            self.dec_value_refs(oldv);
        }
    }

    pub fn get_edge_prop(&self, e: EdgeId, key: PropKeyId) -> Option<&Value> {
        let edge = self.edges.get(e as usize)?;
        self.props.get(edge.prop_ref? as usize)?.get(key)
    }

    pub fn node_labels(&self, n: NodeId) -> Vec<LabelId> {
        self.nodes
            .get(n as usize)
            .map(|nd| nd.labels.clone())
            .unwrap_or_default()
    }

    pub fn all_node_props(&self, n: NodeId) -> Vec<(PropKeyId, Value)> {
        if let Some(node) = self.nodes.get(n as usize) {
            if let Some(pidx) = node.prop_ref {
                return self.props[pidx as usize]
                    .entries
                    .iter()
                    .map(|e| (e.key, e.value.clone()))
                    .collect();
            }
        }
        Vec::new()
    }

    pub fn all_edge_props(&self, e: EdgeId) -> Vec<(PropKeyId, Value)> {
        if let Some(edge) = self.edges.get(e as usize) {
            if let Some(pidx) = edge.prop_ref {
                return self.props[pidx as usize]
                    .entries
                    .iter()
                    .map(|pe| (pe.key, pe.value.clone()))
                    .collect();
            }
        }
        Vec::new()
    }

    #[inline]
    pub fn get_edge_info(&self, e: EdgeId) -> (NodeId, NodeId, LabelId) {
        let ed = &self.edges[e as usize];
        (ed.src, ed.dst, ed.type_id)
    }

    pub fn neighbors_out<'a>(
        &'a self,
        n: NodeId,
        type_filter: Option<LabelId>,
    ) -> impl Iterator<Item = NodeId> + 'a {
        struct It<'a> {
            g: &'a LivecGraph,
            cur: Option<EdgeId>,
            filt: Option<LabelId>,
        }
        impl<'a> Iterator for It<'a> {
            type Item = NodeId;
            fn next(&mut self) -> Option<Self::Item> {
                while let Some(eid) = self.cur {
                    let e = &self.g.edges[eid as usize];
                    self.cur = e.next_out;
                    if e.alive && self.g.nodes[e.dst as usize].alive {
                        if let Some(t) = self.filt {
                            if e.type_id != t {
                                continue;
                            }
                        }
                        return Some(e.dst);
                    }
                }
                None
            }
        }
        let cur = self.nodes.get(n as usize).and_then(|nd| nd.first_out);
        It {
            g: self,
            cur,
            filt: type_filter,
        }
    }

    pub fn neighbors_in<'a>(
        &'a self,
        n: NodeId,
        type_filter: Option<LabelId>,
    ) -> impl Iterator<Item = NodeId> + 'a {
        struct It<'a> {
            g: &'a LivecGraph,
            cur: Option<EdgeId>,
            filt: Option<LabelId>,
        }
        impl<'a> Iterator for It<'a> {
            type Item = NodeId;
            fn next(&mut self) -> Option<Self::Item> {
                while let Some(eid) = self.cur {
                    let e = &self.g.edges[eid as usize];
                    self.cur = e.next_in;
                    if e.alive && self.g.nodes[e.src as usize].alive {
                        if let Some(t) = self.filt {
                            if e.type_id != t {
                                continue;
                            }
                        }
                        return Some(e.src);
                    }
                }
                None
            }
        }
        let cur = self.nodes.get(n as usize).and_then(|nd| nd.first_in);
        It {
            g: self,
            cur,
            filt: type_filter,
        }
    }

    pub fn edge_ids_between(
        &self,
        src: NodeId,
        dst: NodeId,
        type_filter: Option<LabelId>,
    ) -> Vec<EdgeId> {
        let mut out = Vec::new();
        let mut e = self.nodes.get(src as usize).and_then(|n| n.first_out);
        while let Some(eid) = e {
            let ed = &self.edges[eid as usize];
            e = ed.next_out;
            if ed.alive && ed.dst == dst {
                if let Some(t) = type_filter {
                    if ed.type_id != t {
                        continue;
                    }
                }
                out.push(eid);
            }
        }
        out
    }

    pub fn edge_ids_in(&self, n: NodeId, type_filter: Option<LabelId>) -> Vec<EdgeId> {
        let mut out = Vec::new();
        let mut e = self.nodes.get(n as usize).and_then(|nd| nd.first_in);
        while let Some(eid) = e {
            let ed = &self.edges[eid as usize];
            e = ed.next_in;
            if ed.alive {
                if let Some(t) = type_filter {
                    if ed.type_id != t {
                        continue;
                    }
                }
                out.push(eid);
            }
        }
        out
    }

    pub fn edge_ids_out(&self, n: NodeId, type_filter: Option<LabelId>) -> Vec<EdgeId> {
        let mut out = Vec::new();
        let mut e = self.nodes.get(n as usize).and_then(|nd| nd.first_out);
        while let Some(eid) = e {
            let ed = &self.edges[eid as usize];
            e = ed.next_out;
            if ed.alive {
                if let Some(t) = type_filter {
                    if ed.type_id != t {
                        continue;
                    }
                }
                out.push(eid);
            }
        }
        out
    }

    pub fn delete_edge_by_id(&mut self, eid: EdgeId) -> bool {
        if (eid as usize) < self.edges.len() && self.edges[eid as usize].alive {
            self.delete_edge(eid);
            true
        } else {
            false
        }
    }

    pub fn delete_edges_between(
        &mut self,
        src: NodeId,
        dst: NodeId,
        type_filter: Option<LabelId>,
    ) -> usize {
        let ids = self.edge_ids_between(src, dst, type_filter);
        let n = ids.len();
        for eid in ids {
            self.delete_edge(eid);
        }
        n
    }

    pub fn out_edge_ids(&self, n: NodeId, type_filter: Option<LabelId>) -> Vec<EdgeId> {
        let mut out = Vec::new();
        let mut e = self.nodes.get(n as usize).and_then(|nd| nd.first_out);
        while let Some(eid) = e {
            let ed = &self.edges[eid as usize];
            e = ed.next_out;
            if ed.alive {
                if let Some(t) = type_filter {
                    if ed.type_id != t {
                        continue;
                    }
                }
                out.push(eid);
            }
        }
        out
    }

    pub fn in_edge_ids(&self, n: NodeId, type_filter: Option<LabelId>) -> Vec<EdgeId> {
        let mut out = Vec::new();
        let mut e = self.nodes.get(n as usize).and_then(|nd| nd.first_in);
        while let Some(eid) = e {
            let ed = &self.edges[eid as usize];
            e = ed.next_in;
            if ed.alive {
                if let Some(t) = type_filter {
                    if ed.type_id != t {
                        continue;
                    }
                }
                out.push(eid);
            }
        }
        out
    }

    #[inline]
    pub fn find_nodes_eq(&self, label: LabelId, key: PropKeyId, value: &Value) -> Vec<NodeId> {
        if let Some(set) = self.eq_index.lookup(label, key, value) {
            set.iter()
                .copied()
                .filter(|&n| self.nodes[n as usize].alive)
                .collect()
        } else {
            Vec::new()
        }
    }

    #[inline]
    pub fn node_has_label(&self, id: NodeId, label: LabelId) -> bool {
        match self.nodes.get(id as usize) {
            Some(n) if n.alive => n.labels.binary_search(&label).is_ok(),
            _ => false,
        }
    }

    #[inline]
    pub fn get_node_by_id_and_label(&self, id: NodeId, label: LabelId) -> Option<&NodeRec> {
        let n = self.nodes.get(id as usize)?;
        if n.alive && n.labels.binary_search(&label).is_ok() {
            Some(n)
        } else {
            None
        }
    }

    pub fn count_labels(&self) -> HashMap<LabelId, u32> {
        let mut tally: HashMap<LabelId, u32> = HashMap::new();
        for node in &self.nodes {
            if !node.alive {
                continue;
            }
            for &label in &node.labels {
                *tally.entry(label).or_insert(0) += 1;
            }
        }
        tally
    }

    pub fn count_edges(&self) -> HashMap<LabelId, u32> {
        let mut tally: HashMap<LabelId, u32> = HashMap::new();
        for edge in &self.edges {
            if !edge.alive {
                continue;
            }
            *tally.entry(edge.type_id).or_insert(0) += 1;
        }
        tally
    }

    pub fn get_label_name_by_id_unsafe(&self, id: LabelId) -> String {
        self.label_names.resolve(id).unwrap().to_string()
    }

    #[inline]
    pub fn all_nodes(&self) -> &[NodeRec] {
        &self.nodes
    }
    #[inline]
    pub fn all_edges(&self) -> &[EdgeRec] {
        &self.edges
    }
    #[inline]
    pub fn alive_nodes(&self) -> impl Iterator<Item = &NodeRec> {
        self.nodes.iter().filter(|n| n.alive)
    }
    #[inline]
    pub fn alive_edges(&self) -> impl Iterator<Item = &EdgeRec> {
        self.edges.iter().filter(|e| e.alive)
    }

    pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        let len = a.len().min(b.len());
        if len == 0 {
            return 0.0;
        }

        let mut dot = 0.0f32;
        let mut na = 0.0f32;
        let mut nb = 0.0f32;

        for i in 0..len {
            let x = a[i];
            let y = b[i];
            dot += x * y;
            na += x * x;
            nb += y * y;
        }

        if na == 0.0 || nb == 0.0 {
            0.0
        } else {
            dot / (na.sqrt() * nb.sqrt())
        }
    }
    pub fn order_nodes_by_cosine_desc(
        &self,
        nodes: &[NodeId],
        embedding_key: PropKeyId,
        query_vec: &[f32],
    ) -> Vec<(NodeId, f32)> {
        use crate::livec_graph::Value as LValue;

        let mut scored: Vec<(NodeId, f32)> = nodes
            .iter()
            .filter_map(|&nid| {
                self.get_node_prop(nid, embedding_key).and_then(|v| {
                    if let LValue::ArrayFloat(ref emb) = v {
                        Some((nid, Self::cosine_similarity(emb, query_vec)))
                    } else {
                        None
                    }
                })
            })
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored
    }

    #[inline]
    pub fn get_edge_type(&self, eid: EdgeId) -> LabelId {
        self.edges
            .get(eid as usize)
            .map(|e| e.type_id)
            .unwrap_or(LabelId::MAX)
    }

    #[inline]
    pub fn label_name(&self, label_id: LabelId) -> Option<&str> {
        self.label_names.resolve(label_id)
    }

    #[inline]
    pub fn key_name(&self, key: PropKeyId) -> Option<String> {
        self.prop_keys.resolve(key).map(|s| s.to_string())
    }

    #[inline]
    pub fn key_id_by_name(&self, name: &str) -> Option<PropKeyId> {
        self.prop_keys.get_id(name)
    }

    #[inline]
    pub fn all_labels(&self) -> Vec<LabelId> {
        let n = self.label_names.len() as LabelId;
        (0..n).collect()
    }

    #[inline]
    pub fn label_id_by_name(&self, name: &str) -> Option<LabelId> {
        self.label_names.get_id(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(mut v: Vec<u32>) -> Vec<u32> {
        v.sort();
        v
    }

    #[test]
    fn reuse_node_ids() {
        let mut g = LivecGraph::new();
        let person = g.label_id("Person");
        let a = g.add_node(&[person]);
        let b = g.add_node(&[person]);
        assert_eq!(a, 0);
        assert_eq!(b, 1);
        g.delete_node(a);
        let c = g.add_node(&[person]);
        assert_eq!(c, a, "should reuse freed node id");
    }

    #[test]
    fn reuse_edge_ids_and_adjacency() {
        let mut g = LivecGraph::new();
        let person = g.label_id("Person");
        let follows = g.label_id("FOLLOWS");
        let n1 = g.add_node(&[person]);
        let n2 = g.add_node(&[person]);
        let e1 = g.add_edge(n1, n2, follows);
        assert_eq!(e1, 0);
        assert_eq!(
            g.neighbors_out(n1, Some(follows)).collect::<Vec<_>>(),
            vec![n2]
        );
        g.delete_edge(e1);
        assert!(g.neighbors_out(n1, Some(follows)).next().is_none());
        let e2 = g.add_edge(n1, n2, follows);
        assert_eq!(e2, e1);
        assert_eq!(
            g.neighbors_out(n1, Some(follows)).collect::<Vec<_>>(),
            vec![n2]
        );
    }

    #[test]
    fn props_and_indexes_cleanup_on_delete() {
        let mut g = LivecGraph::new();
        let person = g.label_id("Person");
        let name_k = g.key_id("name");
        let alice = g.add_node(&[person]);
        let s_alice = g.strings.intern("Alice");
        g.index_key(name_k);
        g.set_node_prop(alice, name_k, Value::Str(s_alice));
        assert_eq!(
            g.find_nodes_eq(person, name_k, &Value::Str(s_alice)),
            vec![alice]
        );
        g.delete_node(alice);
        assert!(g
            .find_nodes_eq(person, name_k, &Value::Str(s_alice))
            .is_empty());
        assert_eq!(g.strings.ref_count(s_alice), Some(0));
    }

    #[test]
    fn edge_ids_between_and_delete_by_id() {
        let mut g = LivecGraph::new();
        let person = g.label_id("Person");
        let follows = g.label_id("FOLLOWS");
        let likes = g.label_id("LIKES");
        let a = g.add_node(&[person]);
        let b = g.add_node(&[person]);
        let e1 = g.add_edge(a, b, follows);
        let e2 = g.add_edge(a, b, follows);
        let e3 = g.add_edge(a, b, likes);

        let all_ab = sorted(g.edge_ids_between(a, b, None));
        assert_eq!(all_ab, sorted(vec![e1, e2, e3]));

        let only_follows = sorted(g.edge_ids_between(a, b, Some(follows)));
        assert_eq!(only_follows, sorted(vec![e1, e2]));

        assert!(g.delete_edge_by_id(e1));
        assert!(!g.delete_edge_by_id(e1));
        let remaining = sorted(g.edge_ids_between(a, b, None));
        assert_eq!(remaining, sorted(vec![e2, e3]));
    }

    #[test]
    fn out_in_edge_ids_helpers() {
        let mut g = LivecGraph::new();
        let person = g.label_id("Person");
        let follows = g.label_id("FOLLOWS");
        let likes = g.label_id("LIKES");
        let a = g.add_node(&[person]);
        let b = g.add_node(&[person]);
        let c = g.add_node(&[person]);

        let e1 = g.add_edge(a, b, follows);
        let e2 = g.add_edge(a, c, likes);
        let e3 = g.add_edge(b, a, likes);

        assert_eq!(sorted(g.out_edge_ids(a, None)), sorted(vec![e1, e2]));
        assert_eq!(g.out_edge_ids(a, Some(follows)), vec![e1]);
        assert_eq!(sorted(g.in_edge_ids(a, None)), sorted(vec![e3]));
        assert_eq!(g.in_edge_ids(a, Some(likes)), vec![e3]);
    }

    #[test]
    fn optional_property_indexing_and_cleanup() {
        let mut g = LivecGraph::new();
        let person = g.label_id("Person");
        let id_k = g.key_id("id");
        let name_k = g.key_id("name");
        let alice = g.add_node(&[person]);
        let s_alice = g.strings.intern("Alice");
        let s_id = g.strings.intern("ID-123");

        g.set_node_prop(alice, id_k, Value::Str(s_id));
        g.set_node_prop(alice, name_k, Value::Str(s_alice));
        assert!(g.find_nodes_eq(person, id_k, &Value::Str(s_id)).is_empty());
        assert!(g
            .find_nodes_eq(person, name_k, &Value::Str(s_alice))
            .is_empty());

        g.index_key(id_k);
        g.set_node_prop(alice, id_k, Value::Str(s_id));
        g.set_node_prop(alice, name_k, Value::Str(s_alice));

        assert_eq!(
            g.find_nodes_eq(person, id_k, &Value::Str(s_id)),
            vec![alice]
        );
        assert!(g
            .find_nodes_eq(person, name_k, &Value::Str(s_alice))
            .is_empty());

        g.delete_node(alice);
        assert!(g.find_nodes_eq(person, id_k, &Value::Str(s_id)).is_empty());
    }

    #[test]
    fn bytes_property_can_be_indexed() {
        let mut g = LivecGraph::new();
        let blob_k = g.key_id("blob");
        let label = g.label_id("Item");
        let n = g.add_node(&[label]);

        let data = vec![0xde, 0xad, 0xbe, 0xef];
        g.set_node_prop(n, blob_k, Value::Bytes(data.clone()));
        assert!(g
            .find_nodes_eq(label, blob_k, &Value::Bytes(data.clone()))
            .is_empty());

        g.index_key(blob_k);
        g.set_node_prop(n, blob_k, Value::Bytes(data.clone()));

        let hits = g.find_nodes_eq(label, blob_k, &Value::Bytes(data.clone()));
        assert_eq!(hits, vec![n]);

        g.delete_node(n);
        assert!(g
            .find_nodes_eq(label, blob_k, &Value::Bytes(data.clone()))
            .is_empty());
    }

    #[test]
    fn refcount_increments_and_decrements_on_node_prop_set_and_replace() {
        let mut g = LivecGraph::new();
        let label = g.label_id("Person");
        let name_k = g.key_id("name");
        let n = g.add_node(&[label]);

        let s_alice = g.strings.intern("Alice");
        let s_bob = g.strings.intern("Bob");

        assert_eq!(g.strings.ref_count(s_alice), Some(0));
        assert_eq!(g.strings.ref_count(s_bob), Some(0));

        g.set_node_prop(n, name_k, Value::Str(s_alice));
        assert_eq!(g.strings.ref_count(s_alice), Some(1));
        assert_eq!(g.strings.ref_count(s_bob), Some(0));

        g.set_node_prop(n, name_k, Value::Str(s_bob));
        assert_eq!(
            g.strings.ref_count(s_alice),
            Some(0),
            "old value decremented"
        );
        assert_eq!(g.strings.ref_count(s_bob), Some(1), "new value incremented");
    }

    #[test]
    fn refcount_decrements_on_remove_node_prop() {
        let mut g = LivecGraph::new();
        let label = g.label_id("Person");
        let name_k = g.key_id("name");
        let n = g.add_node(&[label]);

        let s_alice = g.strings.intern("Alice");
        g.set_node_prop(n, name_k, Value::Str(s_alice));
        assert_eq!(g.strings.ref_count(s_alice), Some(1));

        let old = g.remove_node_prop(n, name_k);
        assert!(matches!(old, Some(Value::Str(id)) if id == s_alice));
        assert_eq!(g.strings.ref_count(s_alice), Some(0));
    }

    #[test]
    fn refcount_decrements_on_node_delete() {
        let mut g = LivecGraph::new();
        let label = g.label_id("Person");
        let name_k = g.key_id("name");
        let n = g.add_node(&[label]);
        let s_alice = g.strings.intern("Alice");

        g.set_node_prop(n, name_k, Value::Str(s_alice));
        assert_eq!(g.strings.ref_count(s_alice), Some(1));
        g.delete_node(n);
        assert_eq!(g.strings.ref_count(s_alice), Some(0));
    }

    #[test]
    fn refcount_edge_props_inc_dec_and_cleanup_on_delete() {
        let mut g = LivecGraph::new();
        let person = g.label_id("Person");
        let rel = g.label_id("LIKES");
        let note_k = g.key_id("note");

        let a = g.add_node(&[person]);
        let b = g.add_node(&[person]);
        let e = g.add_edge(a, b, rel);

        let s_foo = g.strings.intern("foo");
        let s_bar = g.strings.intern("bar");

        g.set_edge_prop(e, note_k, Value::Str(s_foo));
        assert_eq!(g.strings.ref_count(s_foo), Some(1));

        g.set_edge_prop(e, note_k, Value::Str(s_bar));
        assert_eq!(g.strings.ref_count(s_foo), Some(0));
        assert_eq!(g.strings.ref_count(s_bar), Some(1));

        g.delete_edge(e);
        assert_eq!(g.strings.ref_count(s_bar), Some(0));
    }

    #[test]
    fn get_node_by_id_and_label_filters_correctly() {
        let mut g = LivecGraph::new();
        let person = g.label_id("Person");
        let item = g.label_id("Item");
        let a = g.add_node(&[person]);
        assert!(g.get_node_by_id_and_label(a, person).is_some());
        assert!(g.get_node_by_id_and_label(a, item).is_none());
        g.delete_node(a);
        assert!(g.get_node_by_id_and_label(a, person).is_none());
    }

    #[test]
    fn count_labels_counts_only_alive_nodes() {
        let mut g = LivecGraph::new();
        let person = g.label_id("Person");
        let item = g.label_id("Item");

        let _a = g.add_node(&[person]);
        let b = g.add_node(&[person, item]);
        let _c = g.add_node(&[item]);

        g.delete_node(b);
        let tally = g.count_labels();

        assert_eq!(tally.get(&person).copied(), Some(1));
        assert_eq!(tally.get(&item).copied(), Some(1));
        assert_eq!(tally.len(), 2);
    }

    #[test]
    fn count_edges_counts_only_alive_edges() {
        let mut g = LivecGraph::new();
        let person = g.label_id("Person");
        let follows = g.label_id("FOLLOWS");
        let likes = g.label_id("LIKES");

        let n1 = g.add_node(&[person]);
        let n2 = g.add_node(&[person]);
        let n3 = g.add_node(&[person]);

        let _e1 = g.add_edge(n1, n2, follows);
        let e2 = g.add_edge(n2, n3, follows);
        let _e3 = g.add_edge(n1, n3, likes);

        let tally = g.count_edges();
        assert_eq!(tally.get(&follows).copied(), Some(2));
        assert_eq!(tally.get(&likes).copied(), Some(1));

        g.delete_edge(e2);
        let tally2 = g.count_edges();
        assert_eq!(tally2.get(&follows).copied(), Some(1));
        assert_eq!(tally2.get(&likes).copied(), Some(1));
    }

    #[test]
    fn get_label_name_by_id_unsafe_roundtrips_name() {
        let mut g = LivecGraph::new();
        let person = g.label_id("Person");
        let item = g.label_id("Item");

        let person_name = g.get_label_name_by_id_unsafe(person);
        let item_name = g.get_label_name_by_id_unsafe(item);

        assert_eq!(person_name, "Person");
        assert_eq!(item_name, "Item");
    }

    fn approx_eq(a: f32, b: f32, eps: f32) {
        let diff = (a - b).abs();
        assert!(diff <= eps, "expected {a} ≈ {b} (|diff|={diff} > {eps})");
    }

    #[test]
    fn cosine_similarity_basic_cases() {
        let v1 = [1.0_f32, 2.0, 3.0];
        let v2 = [1.0_f32, 2.0, 3.0];
        let s = LivecGraph::cosine_similarity(&v1, &v2);
        approx_eq(s, 1.0, 1e-5);

        let v3 = [1.0_f32, 0.0];
        let v4 = [-1.0_f32, 0.0];
        let s = LivecGraph::cosine_similarity(&v3, &v4);
        approx_eq(s, -1.0, 1e-5);

        let v5 = [1.0_f32, 0.0];
        let v6 = [0.0_f32, 1.0];
        let s = LivecGraph::cosine_similarity(&v5, &v6);
        approx_eq(s, 0.0, 1e-5);

        let z = [0.0_f32, 0.0];
        let nz = [1.0_f32, 2.0];
        let s1 = LivecGraph::cosine_similarity(&z, &nz);
        let s2 = LivecGraph::cosine_similarity(&nz, &z);
        approx_eq(s1, 0.0, 1e-5);
        approx_eq(s2, 0.0, 1e-5);

        let short = [1.0_f32, 0.0];
        let long = [1.0_f32, 0.0, 999.0];
        let s = LivecGraph::cosine_similarity(&short, &long);

        approx_eq(s, 1.0, 1e-5);
    }

    #[test]
    fn order_nodes_by_cosine_desc_sorts_correctly() {
        let mut g = LivecGraph::new();

        let item_label = g.label_id("Item");
        let emb_key = g.key_id("embedding");

        let n1 = g.add_node(&[item_label]);
        let n2 = g.add_node(&[item_label]);
        let n3 = g.add_node(&[item_label]);

        let query = vec![1.0_f32, 0.0];

        // Embeddings:
        // n1 -> [1,0]   cosine ~ 1.0
        // n3 -> [1,1]   cosine ~ 0.707
        // n2 -> [0,1]   cosine ~ 0.0
        g.set_node_prop(n1, emb_key, Value::ArrayFloat(vec![1.0_f32, 0.0]));
        g.set_node_prop(n2, emb_key, Value::ArrayFloat(vec![0.0_f32, 1.0]));
        g.set_node_prop(n3, emb_key, Value::ArrayFloat(vec![1.0_f32, 1.0]));

        let nodes = vec![n1, n2, n3];
        let scored = g.order_nodes_by_cosine_desc(&nodes, emb_key, &query);

        assert_eq!(scored.len(), 3);

        // Order should be: n1 (best), n3 (middle), n2 (worst)
        assert_eq!(scored[0].0, n1);
        assert_eq!(scored[1].0, n3);
        assert_eq!(scored[2].0, n2);

        // Scores should be non-increasing
        assert!(scored[0].1 >= scored[1].1);
        assert!(scored[1].1 >= scored[2].1);

        // Sanity check approximate values
        approx_eq(scored[0].1, 1.0, 1e-5); // [1,0] vs [1,0]
        approx_eq(scored[1].1, 1.0 / 2f32.sqrt(), 1e-5); // [1,0] vs [1,1]
        approx_eq(scored[2].1, 0.0, 1e-5); // [1,0] vs [0,1]
    }

    #[test]
    fn order_nodes_by_cosine_desc_ignores_nodes_without_embedding() {
        let mut g = LivecGraph::new();

        let item_label = g.label_id("Item");
        let emb_key = g.key_id("embedding");

        // n1 has embedding, n2 does not
        let n1 = g.add_node(&[item_label]);
        let n2 = g.add_node(&[item_label]);

        g.set_node_prop(n1, emb_key, Value::ArrayFloat(vec![1.0_f32, 0.0]));
        let query = vec![1.0_f32, 0.0];
        let nodes = vec![n1, n2];

        let scored = g.order_nodes_by_cosine_desc(&nodes, emb_key, &query);

        assert_eq!(scored.len(), 1);
        assert_eq!(scored[0].0, n1);
    }
}
