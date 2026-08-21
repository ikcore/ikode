//! Deterministic keyword/graph retrieval and impact analysis.

use super::*;

impl Indexer {
    pub fn search(&self, query: &str, k: usize) -> Vec<&ChunkRecord> {
        let terms: Vec<String> = query
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .filter(|s| s.len() > 1)
            .map(|s| s.to_string())
            .collect();
        if terms.is_empty() {
            return Vec::new();
        }

        let mut scored: Vec<(i64, &ChunkRecord)> = Vec::new();
        for file in self.files.values() {
            for ch in &file.chunks {
                let name_l = ch.name.to_lowercase();
                let path_l = ch.path.to_lowercase();
                let code_l = ch.code.to_lowercase();
                let mut score = 0i64;
                for t in &terms {
                    if name_l.contains(t) {
                        score += 8;
                    }
                    if path_l.contains(t) {
                        score += 3;
                    }
                    let occurrences = code_l.matches(t.as_str()).count() as i64;
                    score += occurrences.min(5);
                }
                if score > 0 {
                    scored.push((score, ch));
                }
            }
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.chunk_id.cmp(&b.1.chunk_id)));
        scored.into_iter().take(k).map(|(_, c)| c).collect()
    }

    /// Resolve a chunk by its `chunk_id` (exact, `path::name`) or — failing an exact
    /// id match — by `name`. An exact `chunk_id` is unambiguous and short-circuits to a
    /// single result; a bare name may match several chunks, so the caller gets the full
    /// list and decides (one match → edit it; many → ask for a full chunk_id). Returns
    /// clones so the caller can drop the index borrow before mutating.
    pub fn resolve_chunks(&self, id_or_name: &str) -> Vec<ChunkRecord> {
        let want = id_or_name.replace('\\', "/");
        for f in self.files.values() {
            for ch in &f.chunks {
                if ch.chunk_id == want {
                    return vec![ch.clone()];
                }
            }
        }
        let mut out = Vec::new();
        for f in self.files.values() {
            for ch in &f.chunks {
                if ch.name == id_or_name {
                    out.push(ch.clone());
                }
            }
        }
        out.sort_by(|a, b| a.chunk_id.cmp(&b.chunk_id));
        out
    }

    pub fn outline(&self, path: &str) -> Option<&FileIndex> {
        let norm = path.replace('\\', "/");
        self.files.get(&norm).or_else(|| {
            self.files
                .values()
                .find(|f| f.path == norm || f.path.ends_with(&norm))
        })
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn language_breakdown(&self) -> Vec<(String, usize)> {
        let mut map: HashMap<String, usize> = HashMap::new();
        for f in self.files.values() {
            *map.entry(f.language.clone()).or_insert(0) += 1;
        }
        let mut v: Vec<(String, usize)> = map.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        v
    }

    /// Build heuristic relationship edges across the whole index and mirror them
    /// into the graph. Pure-Rust, zero inference:
    /// - `CALLS` (caller -> callee): a chunk *invokes* another chunk's callable
    ///   symbol (the identifier appears in call position, `name(`). The precise
    ///   call graph.
    /// - `REFERENCES` (referrer -> referent): a chunk *mentions* another chunk's
    ///   symbol without calling it (a type in a signature, a field, a same-named
    ///   symbol). The looser dependency graph.
    /// - `TESTS` (test chunk -> target): a chunk in test code that references X.
    /// - `IMPLEMENTS`/`FOR_TYPE`/`HAS_IMPL`: parsed from `impl Trait for Type` headers.
    /// - `IMPLEMENTS`/`EXTENDS`: class/interface inheritance across languages.
    /// - `SATISFIES`: an impl/class method -> the trait/interface method it satisfies.
    /// - `TypeParam` nodes + `HAS_TYPE_PARAM`/`BOUND_BY`: generic params and bounds.
    ///
    /// Returns the number of distinct `CALLS` + `REFERENCES` edges submitted (the
    /// call / dependency graph).
    pub fn link_references(&self) -> usize {
        let graph = match &self.graph {
            Some(g) => g,
            None => return 0,
        };

        // Edges that already exist for the two summarisable kinds. We skip
        // re-`Merge`-ing these (a Merge replaces an edge's props, which would wipe a
        // stored relationship `summary`). Changed chunks have their edges deleted by
        // the re-index cascade, so they fall out of this set and are recreated fresh;
        // unchanged edges are preserved. This also makes edge-linking incremental.
        let existing_calls = self.existing_edge_pairs("CALLS");
        let existing_refs = self.existing_edge_pairs("REFERENCES");
        let existing_sat = self.existing_edge_pairs("SATISFIES");

        // Symbol table: identifier name -> [(chunkId, kind)].
        let mut symbols: HashMap<&str, Vec<(&str, &str)>> = HashMap::new();
        for file in self.files.values() {
            for ch in &file.chunks {
                if ch.name.len() < 3
                    || !is_symbol_name(&ch.name)
                    || REF_STOPWORDS.contains(&ch.name.as_str())
                {
                    continue;
                }
                symbols
                    .entry(ch.name.as_str())
                    .or_default()
                    .push((ch.chunk_id.as_str(), ch.kind.as_str()));
            }
        }
        if symbols.is_empty() {
            return 0;
        }

        // Resolve a symbol name to the chunkId+kind of a definition matching one of
        // the allowed kinds (e.g. the Trait named `Display`, the Struct named `Wrapper`).
        let resolve_kind = |name: &str, kinds: &[&str]| -> Option<(String, String)> {
            let defs = symbols.get(name)?;
            defs.iter()
                .find(|(_, k)| kinds.contains(k))
                .map(|(id, k)| (id.to_string(), k.to_string()))
        };
        // Resolve a name to the first definition of any kind (for inheritance,
        // where the target may be a Class, Interface, Struct, etc.).
        let resolve_any = |name: &str| -> Option<(String, String)> {
            symbols
                .get(name)
                .and_then(|d| d.first())
                .map(|(id, k)| (id.to_string(), k.to_string()))
        };

        // Methods grouped by their parent container's chunkId, for SATISFIES matching.
        let mut container_methods: HashMap<&str, Vec<(&str, &str)>> = HashMap::new();
        for file in self.files.values() {
            for ch in &file.chunks {
                if ch.kind == "Method" {
                    if let Some(pid) = ch.parent_id.as_deref() {
                        container_methods
                            .entry(pid)
                            .or_default()
                            .push((ch.name.as_str(), ch.chunk_id.as_str()));
                    }
                }
            }
        }

        let mut nodes: Vec<NodeAction> = Vec::new();
        let mut edges: Vec<EdgeAction> = Vec::new();
        let mut ref_count = 0usize;
        let mut seen_call: HashSet<(String, String)> = HashSet::new();
        let mut seen_ref: HashSet<(String, String)> = HashSet::new();
        let mut seen_test: HashSet<(String, String)> = HashSet::new();
        let mut seen_impl: HashSet<(String, String, String)> = HashSet::new();
        // (container chunkId, trait/interface chunkId) pairs to match methods for SATISFIES.
        let mut implements_pairs: Vec<(String, String)> = Vec::new();

        for file in self.files.values() {
            for ch in &file.chunks {
                // --- Type-system edges from impl headers (impl Trait for Type). ---
                if ch.kind == "ImplBlock" {
                    if let Some((trait_name, type_name)) = parse_impl_header(&ch.code) {
                        if let Some((type_id, type_kind)) = resolve_kind(&type_name, TYPE_KINDS) {
                            if seen_impl.insert((
                                ch.chunk_id.clone(),
                                "FOR_TYPE".to_string(),
                                type_id.clone(),
                            )) {
                                edges.push(chunk_edge(
                                    &ch.kind,
                                    &ch.chunk_id,
                                    &type_kind,
                                    &type_id,
                                    "FOR_TYPE",
                                ));
                                edges.push(chunk_edge(
                                    &type_kind,
                                    &type_id,
                                    &ch.kind,
                                    &ch.chunk_id,
                                    "HAS_IMPL",
                                ));
                            }
                        }
                        if let Some(tn) = trait_name {
                            if let Some((trait_id, trait_kind)) = resolve_kind(&tn, TRAIT_KINDS) {
                                if seen_impl.insert((
                                    ch.chunk_id.clone(),
                                    "IMPLEMENTS".to_string(),
                                    trait_id.clone(),
                                )) {
                                    edges.push(chunk_edge(
                                        &ch.kind,
                                        &ch.chunk_id,
                                        &trait_kind,
                                        &trait_id,
                                        "IMPLEMENTS",
                                    ));
                                    implements_pairs.push((ch.chunk_id.clone(), trait_id.clone()));
                                }
                            }
                        }
                    }
                }

                // --- Inheritance edges for class/interface/struct chunks across
                //     languages (EXTENDS for a base type, IMPLEMENTS for a contract). ---
                if matches!(ch.kind.as_str(), "Class" | "Interface" | "Struct" | "Enum") {
                    for base in parse_supertypes(&file.language, &ch.code) {
                        if let Some((base_id, base_kind)) = resolve_any(&base) {
                            if base_id == ch.chunk_id {
                                continue;
                            }
                            let edge = if TRAIT_KINDS.contains(&base_kind.as_str()) {
                                "IMPLEMENTS"
                            } else {
                                "EXTENDS"
                            };
                            if seen_impl.insert((
                                ch.chunk_id.clone(),
                                edge.to_string(),
                                base_id.clone(),
                            )) {
                                edges.push(chunk_edge(
                                    &ch.kind,
                                    &ch.chunk_id,
                                    &base_kind,
                                    &base_id,
                                    edge,
                                ));
                                if edge == "IMPLEMENTS" {
                                    implements_pairs.push((ch.chunk_id.clone(), base_id.clone()));
                                }
                            }
                        }
                    }
                }

                // --- Generic type parameters: TypeParam nodes + HAS_TYPE_PARAM,
                //     and BOUND_BY when a bound resolves to a known trait/interface. ---
                if GENERIC_KINDS.contains(&ch.kind.as_str()) {
                    for (param, bounds) in parse_generics(&ch.code) {
                        let tp_id = format!("{}::<{}>", ch.chunk_id, param);
                        nodes.push(type_param_node(&tp_id, &param, &ch.path));
                        edges.push(chunk_edge(
                            &ch.kind,
                            &ch.chunk_id,
                            "TypeParam",
                            &tp_id,
                            "HAS_TYPE_PARAM",
                        ));
                        for b in bounds {
                            if let Some((trait_id, trait_kind)) = resolve_kind(&b, TRAIT_KINDS) {
                                edges.push(chunk_edge(
                                    "TypeParam",
                                    &tp_id,
                                    &trait_kind,
                                    &trait_id,
                                    "BOUND_BY",
                                ));
                            }
                        }
                    }
                }

                // --- Call / reference + test edges from token matching. ---
                let is_test = is_test_chunk(&ch.path, &ch.name);
                // Identifiers used in call position (`name(`) — promote these to
                // `CALLS` when they resolve to a callable def; everything else stays
                // a looser `REFERENCES` mention.
                let called = called_identifiers(&ch.code);
                let mut tokens: HashSet<&str> = HashSet::new();
                for tok in ch
                    .code
                    .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                    .filter(|s| s.len() >= 3)
                {
                    tokens.insert(tok);
                }
                for tok in tokens {
                    if tok == ch.name.as_str() {
                        continue;
                    }
                    let defs = match symbols.get(tok) {
                        Some(d) => d,
                        None => continue,
                    };
                    let in_call_pos = called.contains(tok);
                    for &(def_id, def_kind) in defs {
                        if def_id == ch.chunk_id.as_str() {
                            continue;
                        }
                        let pair = (ch.chunk_id.clone(), def_id.to_string());
                        // A callable symbol used in call position is a true CALLS
                        // edge; any other match is a plain REFERENCES mention. Each
                        // (caller, def) pair has a single name token, so it lands in
                        // exactly one bucket — no duplicate edges.
                        if in_call_pos && is_callable_kind(def_kind) {
                            if seen_call.insert(pair.clone()) && !existing_calls.contains(&pair) {
                                edges.push(chunk_edge(
                                    &ch.kind,
                                    &ch.chunk_id,
                                    def_kind,
                                    def_id,
                                    "CALLS",
                                ));
                                ref_count += 1;
                            }
                        } else if seen_ref.insert(pair.clone()) && !existing_refs.contains(&pair) {
                            edges.push(chunk_edge(
                                &ch.kind,
                                &ch.chunk_id,
                                def_kind,
                                def_id,
                                "REFERENCES",
                            ));
                            ref_count += 1;
                        }
                        // A test chunk that references a symbol also TESTS it.
                        if is_test && seen_test.insert((ch.chunk_id.clone(), def_id.to_string())) {
                            edges.push(chunk_edge(
                                &ch.kind,
                                &ch.chunk_id,
                                def_kind,
                                def_id,
                                "TESTS",
                            ));
                        }
                    }
                }
            }
        }

        // --- SATISFIES: an impl/class method satisfies the trait/interface method
        //     of the same name, wherever an IMPLEMENTS relationship was established. ---
        let mut seen_sat: HashSet<(String, String)> = HashSet::new();
        for (impl_id, trait_id) in &implements_pairs {
            let impl_ms = match container_methods.get(impl_id.as_str()) {
                Some(v) => v,
                None => continue,
            };
            let trait_ms = match container_methods.get(trait_id.as_str()) {
                Some(v) => v,
                None => continue,
            };
            for (mname, mid) in impl_ms {
                if let Some((_, tid)) = trait_ms.iter().find(|(tn, _)| tn == mname) {
                    if seen_sat.insert((mid.to_string(), tid.to_string()))
                        && !existing_sat.contains(&(mid.to_string(), tid.to_string()))
                    {
                        edges.push(chunk_edge("Method", mid, "Method", tid, "SATISFIES"));
                    }
                }
            }
        }

        if edges.is_empty() && nodes.is_empty() {
            return 0;
        }
        let txn = GraphTxn {
            idempotency_key: None,
            timestamp_ms: now_ms(),
            nodes: if nodes.is_empty() { None } else { Some(nodes) },
            edges: Some(edges),
            indexes: None,
        };
        match graph.submit_txn(txn) {
            // `links` in IndexStats counts CALLS + REFERENCES (the call / dependency
            // graph); TESTS/IMPLEMENTS/FOR_TYPE/HAS_IMPL are extra structure via /graph.
            Ok(_) => ref_count,
            Err(_) => 0,
        }
    }

    /// Graph-native impact/blast-radius traversal over both dependency edge kinds
    /// ([`DepEdges::All`]). `callers = true` walks incoming `CALLS` + `REFERENCES`
    /// edges (who calls / depends on this); `false` walks outgoing (what this calls /
    /// depends on). Returns `(symbol_found, hits)` ordered by increasing depth.
    pub fn impact(&self, symbol: &str, depth: usize, callers: bool) -> (bool, Vec<GraphHit>) {
        self.impact_with(symbol, depth, callers, DepEdges::All)
    }

    /// Like [`Self::impact`] but restricts the traversal to a subset of dependency
    /// edges — `DepEdges::Calls` for the precise call graph (true invocations only),
    /// `DepEdges::References` for incidental mentions only.
    pub fn impact_with(
        &self,
        symbol: &str,
        depth: usize,
        callers: bool,
        edges: DepEdges,
    ) -> (bool, Vec<GraphHit>) {
        let mut found = false;
        let mut hits: Vec<GraphHit> = Vec::new();
        let graph = match &self.graph {
            Some(g) => g,
            None => return (found, hits),
        };

        graph.with_read(|g| {
            let chunkid_key = match g.prop_keys.get_id("chunk_id") {
                Some(k) => k,
                None => return,
            };
            let name_key = g.prop_keys.get_id("name");
            let q = match g.strings.get_id(symbol) {
                Some(id) => id,
                None => return,
            };

            // Resolve start nodes whose chunkId or name equals the query.
            let mut starts: Vec<NodeId> = Vec::new();
            for (i, nr) in g.nodes.iter().enumerate() {
                if !nr.alive {
                    continue;
                }
                let nid = i as NodeId;
                let cid_match =
                    matches!(g.get_node_prop(nid, chunkid_key), Some(LValue::Str(s)) if *s == q);
                let name_match = name_key.is_some_and(
                    |nk| matches!(g.get_node_prop(nid, nk), Some(LValue::Str(s)) if *s == q),
                );
                if cid_match || name_match {
                    starts.push(nid);
                }
            }
            if starts.is_empty() {
                return;
            }
            found = true;

            // Dependents/dependencies flow along the selected edge kinds — the
            // precise call graph (`CALLS`), the looser mention graph (`REFERENCES`),
            // or both.
            let dep_labels: Vec<_> = edges
                .labels()
                .iter()
                .filter_map(|l| g.label_names.get_id(l))
                .collect();
            if dep_labels.is_empty() {
                return; // nodes exist but no edges of the requested kind(s) yet
            }

            let mut visited: HashSet<NodeId> = starts.iter().copied().collect();
            let mut queue: VecDeque<(NodeId, usize)> =
                starts.iter().map(|&n| (n, 0usize)).collect();
            let mut reached: Vec<(NodeId, usize)> = Vec::new();
            while let Some((n, d)) = queue.pop_front() {
                if d > 0 {
                    reached.push((n, d));
                }
                if d >= depth {
                    continue;
                }
                let mut neighbors: Vec<NodeId> = Vec::new();
                for &lbl in &dep_labels {
                    if callers {
                        neighbors.extend(g.neighbors_in(n, Some(lbl)));
                    } else {
                        neighbors.extend(g.neighbors_out(n, Some(lbl)));
                    }
                }
                for m in neighbors {
                    if visited.insert(m) {
                        queue.push_back((m, d + 1));
                    }
                }
            }

            for (nid, d) in reached {
                hits.push(GraphHit {
                    chunk_id: node_str(g, nid, chunkid_key).unwrap_or_default(),
                    kind: node_kind(g, nid),
                    name: name_key
                        .and_then(|nk| node_str(g, nid, nk))
                        .unwrap_or_default(),
                    path: g
                        .prop_keys
                        .get_id("file_path")
                        .and_then(|pk| node_str(g, nid, pk))
                        .unwrap_or_default(),
                    line_start: g
                        .prop_keys
                        .get_id("line_start")
                        .map_or(0, |k| node_i64(g, nid, k)),
                    line_end: g
                        .prop_keys
                        .get_id("line_end")
                        .map_or(0, |k| node_i64(g, nid, k)),
                    depth: d,
                });
            }
        });

        hits.sort_by(|a, b| a.depth.cmp(&b.depth).then(a.chunk_id.cmp(&b.chunk_id)));
        (found, hits)
    }

    /// Run a structured [`GraphQuerySpec`] over the graph: resolve a start node
    /// set, apply each `traverse` step (labelled BFS, in/out, depth-bounded) so the
    /// reached set feeds the next step, then filter by `where` and project to
    /// [`QueryRow`]s sorted by chunk_id and capped at `limit` (default 100, max
    /// 1000). Pure traversal, no inference. Returns `Err` for a query too broad to
    /// run safely (no anchor and no filter) or when persistence is disabled.
    pub fn graph_query(&self, spec: &GraphQuerySpec) -> std::result::Result<Vec<QueryRow>, String> {
        let graph = self
            .graph
            .as_ref()
            .ok_or_else(|| "graph persistence is disabled".to_string())?;
        let limit = spec.limit.unwrap_or(100).clamp(1, 1000);

        let start = spec.start.clone().unwrap_or_default();
        let has_anchor = start.symbol.is_some() || start.chunk_id.is_some() || start.kind.is_some();
        let filt = spec.filter.clone().unwrap_or_default();
        let has_filter = !filt.kind.is_empty()
            || filt.path_prefix.is_some()
            || filt.visibility.is_some()
            || filt.name.is_some();
        if !spec.traverse.is_empty() && !has_anchor {
            return Err("`traverse` requires a `start` anchor (symbol/chunk_id/kind)".to_string());
        }
        if !has_anchor && !has_filter {
            return Err(
                "query too broad: specify a `start` anchor or a `where` filter".to_string(),
            );
        }

        let mut rows: Vec<QueryRow> = Vec::new();
        graph.with_read(|g| {
            let chunkid_key = g.prop_keys.get_id("chunk_id");
            let name_key = g.prop_keys.get_id("name");
            let fp_key = g.prop_keys.get_id("file_path");
            let path_key = g.prop_keys.get_id("path");
            let vis_key = g.prop_keys.get_id("visibility");
            let ls_key = g.prop_keys.get_id("line_start");
            let le_key = g.prop_keys.get_id("line_end");

            // 1. Resolve the start frontier.
            let mut frontier: Vec<NodeId> = Vec::new();
            if start.symbol.is_none() && start.chunk_id.is_none() {
                // No node anchor: scan everything; the `where` filter narrows it.
                for (i, nr) in g.nodes.iter().enumerate() {
                    if nr.alive {
                        frontier.push(i as NodeId);
                    }
                }
            } else {
                for (i, nr) in g.nodes.iter().enumerate() {
                    if !nr.alive {
                        continue;
                    }
                    let nid = i as NodeId;
                    let cid = chunkid_key.and_then(|k| node_str(g, nid, k));
                    let hit = if let Some(id) = start.chunk_id.as_deref() {
                        cid.as_deref() == Some(id)
                    } else if let Some(sym) = start.symbol.as_deref() {
                        cid.as_deref() == Some(sym)
                            || name_key.and_then(|k| node_str(g, nid, k)).as_deref() == Some(sym)
                    } else {
                        false
                    };
                    if hit {
                        frontier.push(nid);
                    }
                }
            }
            if let Some(k) = start.kind.as_deref() {
                frontier.retain(|&nid| node_kind(g, nid).eq_ignore_ascii_case(k));
            }

            // 2. Apply traversal steps; each step's reached set feeds the next.
            for step in &spec.traverse {
                let lbl = match g
                    .label_names
                    .get_id(step.edge.trim().to_ascii_uppercase().as_str())
                {
                    Some(l) => l,
                    None => {
                        frontier.clear();
                        break;
                    }
                };
                let out = !step
                    .dir
                    .as_deref()
                    .is_some_and(|d| d.eq_ignore_ascii_case("in"));
                let depth = step.depth.unwrap_or(1).clamp(1, 10);
                let mut visited: HashSet<NodeId> = frontier.iter().copied().collect();
                let mut queue: VecDeque<(NodeId, usize)> =
                    frontier.iter().map(|&n| (n, 0usize)).collect();
                let mut reached: Vec<NodeId> = Vec::new();
                while let Some((n, d)) = queue.pop_front() {
                    if d > 0 {
                        reached.push(n);
                    }
                    if d >= depth {
                        continue;
                    }
                    let nbrs: Vec<NodeId> = if out {
                        g.neighbors_out(n, Some(lbl)).collect()
                    } else {
                        g.neighbors_in(n, Some(lbl)).collect()
                    };
                    for m in nbrs {
                        if visited.insert(m) {
                            queue.push_back((m, d + 1));
                        }
                    }
                }
                frontier = reached;
            }

            // 3. Filter and project.
            let want_kinds: Vec<String> =
                filt.kind.iter().map(|k| k.to_ascii_lowercase()).collect();
            let mut seen: HashSet<NodeId> = HashSet::new();
            for &nid in &frontier {
                if !seen.insert(nid) {
                    continue;
                }
                let kind = node_kind(g, nid);
                if !want_kinds.is_empty()
                    && !want_kinds.iter().any(|k| k == &kind.to_ascii_lowercase())
                {
                    continue;
                }
                let path = fp_key
                    .and_then(|k| node_str(g, nid, k))
                    .or_else(|| path_key.and_then(|k| node_str(g, nid, k)))
                    .unwrap_or_default();
                if let Some(pre) = filt.path_prefix.as_deref() {
                    if !path.starts_with(pre) {
                        continue;
                    }
                }
                if let Some(v) = filt.visibility.as_deref() {
                    let vis = vis_key
                        .and_then(|k| node_str(g, nid, k))
                        .unwrap_or_default();
                    if !vis.eq_ignore_ascii_case(v) {
                        continue;
                    }
                }
                let name = name_key
                    .and_then(|k| node_str(g, nid, k))
                    .unwrap_or_default();
                if let Some(want) = filt.name.as_deref() {
                    if !name.eq_ignore_ascii_case(want) && !name.contains(want) {
                        continue;
                    }
                }
                let chunk_id = chunkid_key
                    .and_then(|k| node_str(g, nid, k))
                    .or_else(|| path_key.and_then(|k| node_str(g, nid, k)))
                    .unwrap_or_default();
                rows.push(QueryRow {
                    chunk_id,
                    kind,
                    name,
                    path,
                    line_start: ls_key.map_or(0, |k| node_i64(g, nid, k)),
                    line_end: le_key.map_or(0, |k| node_i64(g, nid, k)),
                });
            }
        });

        rows.sort_by(|a, b| a.chunk_id.cmp(&b.chunk_id).then(a.path.cmp(&b.path)));
        rows.truncate(limit);
        Ok(rows)
    }

    /// Collect test chunks that guard any of `ids` (symbol names or chunkIds) by
    /// reverse-traversing incoming `TESTS` edges. Pure traversal, no inference.
    pub fn guarding_tests(&self, ids: &[String]) -> Vec<GraphHit> {
        let mut hits: Vec<GraphHit> = Vec::new();
        let graph = match &self.graph {
            Some(g) => g,
            None => return hits,
        };
        graph.with_read(|g| {
            let chunkid_key = match g.prop_keys.get_id("chunk_id") {
                Some(k) => k,
                None => return,
            };
            let name_key = g.prop_keys.get_id("name");
            let tests_label = match g.label_names.get_id("TESTS") {
                Some(l) => l,
                None => return, // no TESTS edges in the graph yet
            };
            let want: HashSet<_> = ids.iter().filter_map(|s| g.strings.get_id(s)).collect();
            if want.is_empty() {
                return;
            }
            let mut starts: Vec<NodeId> = Vec::new();
            for (i, nr) in g.nodes.iter().enumerate() {
                if !nr.alive {
                    continue;
                }
                let nid = i as NodeId;
                let cid = matches!(g.get_node_prop(nid, chunkid_key), Some(LValue::Str(s)) if want.contains(s));
                let nm = name_key.is_some_and(|nk| {
                    matches!(g.get_node_prop(nid, nk), Some(LValue::Str(s)) if want.contains(s))
                });
                if cid || nm {
                    starts.push(nid);
                }
            }
            let mut seen: HashSet<NodeId> = HashSet::new();
            for &s in &starts {
                for t in g.neighbors_in(s, Some(tests_label)).collect::<Vec<_>>() {
                    if seen.insert(t) {
                        hits.push(GraphHit {
                            chunk_id: node_str(g, t, chunkid_key).unwrap_or_default(),
                            kind: node_kind(g, t),
                            name: name_key.and_then(|nk| node_str(g, t, nk)).unwrap_or_default(),
                            path: g
                                .prop_keys
                                .get_id("file_path")
                                .and_then(|pk| node_str(g, t, pk))
                                .unwrap_or_default(),
                            line_start: g.prop_keys.get_id("line_start").map_or(0, |k| node_i64(g, t, k)),
                            line_end: g.prop_keys.get_id("line_end").map_or(0, |k| node_i64(g, t, k)),
                            depth: 0,
                        });
                    }
                }
            }
        });
        hits.sort_by(|a, b| a.chunk_id.cmp(&b.chunk_id));
        hits
    }

    /// Retrieval + connectivity: the `ask_codebase` core. Fuses keyword search
    /// (the candidate chunks) with the graph neighbourhood (how those chunks are
    /// wired to each other), so the agent gets the relevant code *and* the wiring
    /// between it. Pure-Rust, zero inference at the tool boundary — the calling
    /// agent reasons over the structured result.
    pub fn ask(&self, question: &str, k: usize) -> AskResult {
        let candidates = self.search(question, k);
        let summaries = self.chunk_prop_map("summary");
        let chunks: Vec<AskChunk> = candidates
            .iter()
            .map(|ch| self.to_ask_chunk(ch, &summaries))
            .collect();
        let connectivity = self.connectivity_for(&chunks);
        let referenced_by = self.referenced_by(&chunks, IMPACT_CAP);
        AskResult {
            chunks,
            connectivity,
            referenced_by,
        }
    }

    /// Build an `AskChunk` view of a record, attaching its stored summary (if any).
    pub(super) fn to_ask_chunk(
        &self,
        ch: &ChunkRecord,
        summaries: &HashMap<String, String>,
    ) -> AskChunk {
        AskChunk {
            chunk_id: ch.chunk_id.clone(),
            kind: ch.kind.clone(),
            name: ch.name.clone(),
            path: ch.path.clone(),
            line_start: ch.line_start,
            line_end: ch.line_end,
            snippet: ch.code.lines().take(6).collect::<Vec<_>>().join("\n"),
            summary: summaries.get(&ch.chunk_id).cloned(),
        }
    }

    /// The subgraph of edges whose *both* endpoints are among `chunks` — i.e. how
    /// the retrieved chunks are wired to each other. Shared by keyword and semantic
    /// `ask`.
    pub(super) fn connectivity_for(&self, chunks: &[AskChunk]) -> Vec<ConnEdge> {
        let mut connectivity = Vec::new();
        if chunks.is_empty() {
            return connectivity;
        }
        let graph = match &self.graph {
            Some(g) => g,
            None => return connectivity,
        };
        let wanted: HashSet<&str> = chunks.iter().map(|c| c.chunk_id.as_str()).collect();
        graph.with_read(|g| {
            let chunkid_key = match g.prop_keys.get_id("chunk_id") {
                Some(k) => k,
                None => return,
            };
            let summary_key = g.prop_keys.get_id("summary");
            let want_ids: HashSet<_> = wanted.iter().filter_map(|c| g.strings.get_id(c)).collect();
            let mut node_chunk: HashMap<NodeId, String> = HashMap::new();
            for (i, nr) in g.nodes.iter().enumerate() {
                if !nr.alive {
                    continue;
                }
                let nid = i as NodeId;
                if let Some(LValue::Str(s)) = g.get_node_prop(nid, chunkid_key) {
                    if want_ids.contains(s) {
                        if let Some(name) = g.strings.resolve(*s) {
                            node_chunk.insert(nid, name.to_string());
                        }
                    }
                }
            }
            for e in g.alive_edges() {
                if let (Some(from), Some(to)) = (node_chunk.get(&e.src), node_chunk.get(&e.dst)) {
                    let kind = g
                        .label_names
                        .resolve(e.type_id)
                        .unwrap_or("EDGE")
                        .to_string();
                    let summary = summary_key.and_then(|sk| match g.get_edge_prop(e.id, sk) {
                        Some(LValue::Str(s)) => g.strings.resolve(*s).map(|x| x.to_string()),
                        _ => None,
                    });
                    connectivity.push(ConnEdge {
                        from: from.clone(),
                        to: to.clone(),
                        kind,
                        summary,
                    });
                }
            }
        });
        connectivity.sort_by(|a, b| {
            a.from
                .cmp(&b.from)
                .then(a.to.cmp(&b.to))
                .then(a.kind.cmp(&b.kind))
        });
        connectivity
    }

    /// The *impact surface* of the retrieved chunks: incoming dependency edges
    /// ([`IMPACT_EDGE_KINDS`]) whose source lies **outside** the set — i.e. who
    /// references / implements / tests this code and would be affected by a change to
    /// it. Distinct from [`connectivity_for`], which only links the set to itself.
    /// Capped at `cap` edges (0 = unlimited). Pure graph traversal, no inference.
    pub(super) fn referenced_by(&self, chunks: &[AskChunk], cap: usize) -> Vec<ConnEdge> {
        let mut out = Vec::new();
        if chunks.is_empty() {
            return out;
        }
        let graph = match &self.graph {
            Some(g) => g,
            None => return out,
        };
        let wanted: HashSet<&str> = chunks.iter().map(|c| c.chunk_id.as_str()).collect();
        graph.with_read(|g| {
            let chunkid_key = match g.prop_keys.get_id("chunk_id") {
                Some(k) => k,
                None => return,
            };
            let summary_key = g.prop_keys.get_id("summary");
            let want_str_ids: HashSet<_> =
                wanted.iter().filter_map(|c| g.strings.get_id(c)).collect();
            // The retrieved chunk nodes — the edge *targets* whose dependents we want.
            let mut target_nodes: HashMap<NodeId, String> = HashMap::new();
            for (i, nr) in g.nodes.iter().enumerate() {
                if !nr.alive {
                    continue;
                }
                let nid = i as NodeId;
                if let Some(LValue::Str(s)) = g.get_node_prop(nid, chunkid_key) {
                    if want_str_ids.contains(s) {
                        if let Some(name) = g.strings.resolve(*s) {
                            target_nodes.insert(nid, name.to_string());
                        }
                    }
                }
            }
            for e in g.alive_edges() {
                let to = match target_nodes.get(&e.dst) {
                    Some(t) => t,
                    None => continue,
                };
                let kind = g.label_names.resolve(e.type_id).unwrap_or("EDGE");
                if !IMPACT_EDGE_KINDS.contains(&kind) {
                    continue;
                }
                // The dependent (edge source). Skip edges from within the set — those
                // are already shown by `connectivity_for`.
                let from = match g.get_node_prop(e.src, chunkid_key) {
                    Some(LValue::Str(s)) => match g.strings.resolve(*s) {
                        Some(n) => n.to_string(),
                        None => continue,
                    },
                    _ => continue,
                };
                if wanted.contains(from.as_str()) {
                    continue;
                }
                let summary = summary_key.and_then(|sk| match g.get_edge_prop(e.id, sk) {
                    Some(LValue::Str(s)) => g.strings.resolve(*s).map(|x| x.to_string()),
                    _ => None,
                });
                out.push(ConnEdge {
                    from,
                    to: to.clone(),
                    kind: kind.to_string(),
                    summary,
                });
            }
        });
        out.sort_by(|a, b| {
            a.to.cmp(&b.to)
                .then(a.from.cmp(&b.from))
                .then(a.kind.cmp(&b.kind))
        });
        out.dedup_by(|a, b| a.from == b.from && a.to == b.to && a.kind == b.kind);
        if cap != 0 && out.len() > cap {
            out.truncate(cap);
        }
        out
    }

    /// Read a string prop (`key`) off every chunk node into a `chunk_id -> value`
    /// map. Used to fold stored `summary` / `embed_hash` props back into queries
    /// and to skip already-enriched chunks.
    pub(super) fn chunk_prop_map(&self, key: &str) -> HashMap<String, String> {
        let mut out = HashMap::new();
        let graph = match &self.graph {
            Some(g) => g,
            None => return out,
        };
        graph.with_read(|g| {
            let cid_key = match g.prop_keys.get_id("chunk_id") {
                Some(k) => k,
                None => return,
            };
            let val_key = match g.prop_keys.get_id(key) {
                Some(k) => k,
                None => return,
            };
            for (i, nr) in g.nodes.iter().enumerate() {
                if !nr.alive {
                    continue;
                }
                let nid = i as NodeId;
                let v = match g.get_node_prop(nid, val_key) {
                    Some(LValue::Str(s)) => g.strings.resolve(*s).map(|x| x.to_string()),
                    _ => None,
                };
                if let (Some(LValue::Str(c)), Some(val)) = (g.get_node_prop(nid, cid_key), v) {
                    if let Some(cid) = g.strings.resolve(*c) {
                        out.insert(cid.to_string(), val);
                    }
                }
            }
        });
        out
    }

    /// All currently-live edges of label `edge_label`, as `(src_chunk_id, dst_chunk_id)`
    /// pairs. Used by `link_references` to avoid re-`Merge`-ing an edge that already
    /// exists — a re-Merge would *replace* (clear) its props, wiping any stored
    /// relationship `summary`. Endpoints without a `chunk_id` (e.g. File/Directory)
    /// are skipped.
    pub(super) fn existing_edge_pairs(&self, edge_label: &str) -> HashSet<(String, String)> {
        let mut out = HashSet::new();
        let graph = match &self.graph {
            Some(g) => g,
            None => return out,
        };
        graph.with_read(|g| {
            let cid_key = match g.prop_keys.get_id("chunk_id") {
                Some(k) => k,
                None => return,
            };
            let want = match g.label_names.get_id(edge_label) {
                Some(l) => l,
                None => return,
            };
            let cid = |nid: NodeId| -> Option<String> {
                match g.get_node_prop(nid, cid_key) {
                    Some(LValue::Str(s)) => g.strings.resolve(*s).map(|x| x.to_string()),
                    _ => None,
                }
            };
            for e in g.alive_edges() {
                if e.type_id != want {
                    continue;
                }
                if let (Some(s), Some(d)) = (cid(e.src), cid(e.dst)) {
                    out.insert((s, d));
                }
            }
        });
        out
    }

    /// Read a string prop (`key`) off every `Directory` node into a `path -> value`
    /// map — the directory-tree analogue of `chunk_prop_map` (directories are keyed
    /// by `path`, not `chunk_id`). Used to fold stored directory `summary`s back and
    /// to skip already-summarised directories.
    pub(super) fn dir_prop_map(&self, key: &str) -> HashMap<String, String> {
        let mut out = HashMap::new();
        let graph = match &self.graph {
            Some(g) => g,
            None => return out,
        };
        graph.with_read(|g| {
            let path_key = match g.prop_keys.get_id("path") {
                Some(k) => k,
                None => return,
            };
            let val_key = match g.prop_keys.get_id(key) {
                Some(k) => k,
                None => return,
            };
            for (i, nr) in g.nodes.iter().enumerate() {
                if !nr.alive {
                    continue;
                }
                let nid = i as NodeId;
                if node_kind(g, nid) != "Directory" {
                    continue;
                }
                let v = match g.get_node_prop(nid, val_key) {
                    Some(LValue::Str(s)) => g.strings.resolve(*s).map(|x| x.to_string()),
                    _ => None,
                };
                if let (Some(LValue::Str(p)), Some(val)) = (g.get_node_prop(nid, path_key), v) {
                    if let Some(path) = g.strings.resolve(*p) {
                        out.insert(path.to_string(), val);
                    }
                }
            }
        });
        out
    }

    /// The stored `summary` of the directory at project-relative `path` (root = `"."`),
    /// if `summarize_directories` has run. Exposed mainly for tests / callers that want
    /// the rollup without a graph query.
    pub fn directory_summary(&self, path: &str) -> Option<String> {
        self.dir_prop_map("summary").remove(path)
    }

    /// The stored relationship `summary` of the `edge_kind` edge between the chunks
    /// `from_cid` and `to_cid`, if `summarize_relationships` has described it.
    pub fn relationship_summary(
        &self,
        from_cid: &str,
        to_cid: &str,
        edge_kind: &str,
    ) -> Option<String> {
        let graph = self.graph.as_ref()?;
        let mut found = None;
        graph.with_read(|g| {
            let cid_key = match g.prop_keys.get_id("chunk_id") {
                Some(k) => k,
                None => return,
            };
            let summary_key = match g.prop_keys.get_id("summary") {
                Some(k) => k,
                None => return,
            };
            let want = match g.label_names.get_id(edge_kind) {
                Some(l) => l,
                None => return,
            };
            let cid = |nid: NodeId| -> Option<String> {
                match g.get_node_prop(nid, cid_key) {
                    Some(LValue::Str(s)) => g.strings.resolve(*s).map(|x| x.to_string()),
                    _ => None,
                }
            };
            for e in g.alive_edges() {
                if e.type_id != want {
                    continue;
                }
                if cid(e.src).as_deref() == Some(from_cid) && cid(e.dst).as_deref() == Some(to_cid)
                {
                    if let Some(LValue::Str(s)) = g.get_edge_prop(e.id, summary_key) {
                        found = g.strings.resolve(*s).map(|x| x.to_string());
                    }
                    return;
                }
            }
        });
        found
    }
}
