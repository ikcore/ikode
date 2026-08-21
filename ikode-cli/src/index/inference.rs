//! Embeddings, semantic retrieval, and model-backed enrichment passes.

use super::*;

impl Indexer {
    /// Load the project-local enrichment exclusions for each pass so edits to
    /// `.ikignore` take effect without restarting or rebuilding the index.
    fn enrichment_ignore(&self) -> Result<IkIgnore> {
        self.discover_ikignore().map_err(|error| {
            anyhow::anyhow!(
                "discovering .ikignore files below {} failed: {error}",
                self.root.display()
            )
        })
    }

    /// Map each chunk's `chunk_id` to its outgoing *semantic* relationships as sorted,
    /// de-duplicated `(edge_label, target_name)` pairs (only [`EMBED_RELATION_EDGES`]).
    /// Read in a single graph pass and used to compose the Keywords / Relationships
    /// fields of an [`embedding_document`]. Pure traversal, no inference.
    fn outgoing_relations(&self) -> HashMap<String, Vec<(String, String)>> {
        let mut out: HashMap<String, Vec<(String, String)>> = HashMap::new();
        let graph = match &self.graph {
            Some(g) => g,
            None => return out,
        };
        graph.with_read(|g| {
            let cid_key = match g.prop_keys.get_id("chunk_id") {
                Some(k) => k,
                None => return,
            };
            let name_key = match g.prop_keys.get_id("name") {
                Some(k) => k,
                None => return,
            };
            for e in g.alive_edges() {
                let kind = match g.label_names.resolve(e.type_id) {
                    Some(k) if EMBED_RELATION_EDGES.contains(&k) => k,
                    _ => continue,
                };
                let src = match node_str(g, e.src, cid_key) {
                    Some(s) => s,
                    None => continue,
                };
                let target = match node_str(g, e.dst, name_key) {
                    Some(n) => n,
                    None => continue,
                };
                out.entry(src).or_default().push((kind.to_string(), target));
            }
        });
        for rels in out.values_mut() {
            rels.sort();
            rels.dedup();
        }
        out
    }

    /// **Inference layer — embeddings (semantic index).** Embed every indexed chunk
    /// with `client`/`model` and store the vector as an `embedding` (`ArrayFloat`)
    /// prop on its graph node, enabling `semantic_search`.
    ///
    /// The embedded text is a composed [`embedding_document`] — Context / File /
    /// Summary / Signature / Comments / Keywords / Relationships / Content — not raw
    /// code, so a chunk's natural-language summary, its comments, and its graph
    /// relationships all steer where it lands in the vector space. The document hash
    /// is the embedding's deterministic key: a chunk is skipped only when that hash
    /// is unchanged *and* the stored vector came from the current model. Because the
    /// document folds in the summary and relationship edges, running `/embed` *after*
    /// `/summarize` (or via `/enrich`) yields richer vectors — and changing either
    /// re-triggers a cheap, incremental re-embed of just the affected chunks.
    pub async fn embed_index(&self, client: &dyn GaiseClient, model: &str) -> Result<EnrichStats> {
        self.embed_index_reporting(client, model, &mut |_| {}).await
    }

    /// As [`embed_index`], but invokes `on_item` with each chunk's id as it is embedded
    /// and stored, so callers can stream per-chunk progress to the console.
    pub async fn embed_index_reporting(
        &self,
        client: &dyn GaiseClient,
        model: &str,
        on_item: &mut dyn FnMut(&str),
    ) -> Result<EnrichStats> {
        let mut stats = EnrichStats::default();
        if self.graph.is_none() {
            return Ok(stats);
        }
        let ignore = self.enrichment_ignore()?;
        let have = self.chunk_prop_map("embed_hash");
        let have_model = self.chunk_prop_map("embed_model");
        let have_task = self.chunk_prop_map("embed_task");
        // Only models whose vectors change with the task need the stored `embed_task`
        // to match; for the rest (OpenAI, Titan, …) a vector embedded before tasks
        // existed is identical to one embedded as `document`, so keep it.
        let task_sensitive = embedding_task_sensitive(model);
        let task = EMBED_DOCUMENT_TASK.as_str();
        let summaries = self.chunk_prop_map("summary");
        let relations = self.outgoing_relations();
        let no_rel: Vec<(String, String)> = Vec::new();
        // (chunk_id, kind, hash, text) for chunks needing a fresh embedding.
        let mut todo: Vec<(String, String, String, String)> = Vec::new();
        for file in self.files.values() {
            if ignore.is_ignored(&file.path) {
                stats.ignored += file.chunks.len();
                continue;
            }
            // chunk_id -> (kind, name) within this file, for parent context.
            let kind_name: HashMap<&str, (&str, &str)> = file
                .chunks
                .iter()
                .map(|c| (c.chunk_id.as_str(), (c.kind.as_str(), c.name.as_str())))
                .collect();
            for ch in &file.chunks {
                let parent = ch
                    .parent_id
                    .as_deref()
                    .and_then(|pid| kind_name.get(pid))
                    .map(|&(k, n)| (k, n));
                let rels = relations.get(&ch.chunk_id).unwrap_or(&no_rel);
                let text = embedding_document(
                    ch,
                    &file.language,
                    parent,
                    summaries.get(&ch.chunk_id).map(String::as_str),
                    rels,
                );
                let hash = format!("{:x}", hash_bytes(text.as_bytes()));
                // Skip only when the body is unchanged AND the stored vector was
                // produced by the *current* model AND (for task-sensitive models) under
                // the current embedding task. Vectors from a different embedding model
                // aren't comparable (different space, often different dimension), so
                // switching models must re-embed — `embed_hash` alone (body-only) would
                // wrongly skip everything. Likewise a prefix/taskType model embeds the
                // same text differently per task, so a vector stored without (or under
                // another) `embed_task` would no longer match query-side vectors.
                let hash_ok = have.get(&ch.chunk_id).map(|h| h.as_str()) == Some(hash.as_str());
                let model_ok = have_model.get(&ch.chunk_id).map(|m| m.as_str()) == Some(model);
                let task_ok = !task_sensitive
                    || have_task.get(&ch.chunk_id).map(|t| t.as_str()) == Some(task);
                if hash_ok && model_ok && task_ok {
                    stats.skipped += 1;
                    continue;
                }
                todo.push((ch.chunk_id.clone(), ch.kind.clone(), hash, text));
            }
        }
        if todo.is_empty() {
            return Ok(stats);
        }

        // Embed in small batches (EMBED_BATCH) and persist after each, rather than
        // one request for the whole corpus. A single all-in-one request can exceed the
        // provider's per-request token cap, which fails wholesale and stores nothing —
        // so a large repo could never finish embedding, and every re-run would rebuild
        // the same oversized request and fail again. `todo` already contains only
        // chunks whose `embed_hash`/`embed_model` are stale (current ones were skipped
        // above), so batching just slices that out-of-date set. Concurrent/parallel
        // batching — N requests in flight — remains a separate future investigation.
        let graph = self.graph.as_ref().unwrap();
        for batch in todo.chunks(EMBED_BATCH) {
            let req = embedding_request(
                model,
                EMBED_DOCUMENT_TASK,
                OneOrMany::Many(batch.iter().map(|(_, _, _, t)| t.clone()).collect()),
            );
            let resp = client
                .embeddings(&req)
                .await
                .map_err(|e| anyhow::anyhow!("embeddings call failed: {e}"))?;
            if resp.output.len() != batch.len() {
                return Err(anyhow::anyhow!(
                    "embedding count mismatch: got {}, expected {}",
                    resp.output.len(),
                    batch.len()
                ));
            }

            let mut nodes = Vec::with_capacity(batch.len());
            for ((cid, kind, hash, _text), embedding) in batch.iter().zip(resp.output.into_iter()) {
                on_item(cid);
                nodes.push(NodeAction::Merge {
                    ident: NodeIdent::ByIndex {
                        label: label(kind),
                        key: PropKey::Name("chunk_id".to_string()),
                        value: GValue::Str(cid.clone()),
                    },
                    label: label(kind),
                    props: Props(vec![
                        prop("embedding", GValue::ArrayFloat(embedding)),
                        prop("embed_hash", GValue::Str(hash.clone())),
                        prop("embed_model", GValue::Str(model.to_string())),
                        prop("embed_task", GValue::Str(task.to_string())),
                    ]),
                    alias: None,
                });
            }
            // Persist this batch immediately so progress survives a later-batch failure
            // or a Ctrl+C (the next run skips what's already embedded via embed_hash).
            let txn = GraphTxn {
                idempotency_key: None,
                timestamp_ms: now_ms(),
                nodes: Some(nodes),
                edges: None,
                indexes: None,
            };
            graph
                .submit_txn(txn)
                .map_err(|e| anyhow::anyhow!("storing embeddings failed: {e}"))?;
            stats.embedded += batch.len();
        }
        Ok(stats)
    }

    /// True if any chunk node currently carries an `embedding` prop (so callers can
    /// decide whether semantic search is available without embedding a query first).
    pub fn has_embeddings(&self) -> bool {
        let graph = match &self.graph {
            Some(g) => g,
            None => return false,
        };
        let mut any = false;
        graph.with_read(|g| {
            if let Some(emb_key) = g.prop_keys.get_id("embedding") {
                any = g.alive_nodes().enumerate().any(|(i, _)| {
                    matches!(
                        g.get_node_prop(i as NodeId, emb_key),
                        Some(LValue::ArrayFloat(_))
                    )
                });
            }
        });
        any
    }

    /// Rank indexed chunks by cosine similarity of their stored `embedding` to
    /// `query_vec`, returning the top `k` as `AskChunk`s (meta + snippet pulled from
    /// the in-memory body, summary from the graph). Chunks without an embedding are
    /// ignored. Pure-Rust, sync — embed the query separately (see `ask_semantic`).
    pub fn semantic_search(&self, query_vec: &[f32], k: usize) -> Vec<AskChunk> {
        let graph = match &self.graph {
            Some(g) => g,
            None => return Vec::new(),
        };
        let by_id: HashMap<&str, &ChunkRecord> = self
            .files
            .values()
            .flat_map(|f| f.chunks.iter().map(|c| (c.chunk_id.as_str(), c)))
            .collect();

        let mut scored: Vec<(String, f32)> = Vec::new();
        graph.with_read(|g| {
            let cid_key = match g.prop_keys.get_id("chunk_id") {
                Some(k) => k,
                None => return,
            };
            let emb_key = match g.prop_keys.get_id("embedding") {
                Some(k) => k,
                None => return,
            };
            for (i, nr) in g.nodes.iter().enumerate() {
                if !nr.alive {
                    continue;
                }
                let nid = i as NodeId;
                let emb = match g.get_node_prop(nid, emb_key) {
                    Some(LValue::ArrayFloat(v)) => v,
                    _ => continue,
                };
                // Defend against mixed embedding spaces (e.g. a half-finished model
                // switch): a vector of a different dimension than the query can't be
                // meaningfully compared, so skip it rather than score noise.
                if emb.len() != query_vec.len() {
                    continue;
                }
                if let Some(LValue::Str(s)) = g.get_node_prop(nid, cid_key) {
                    if let Some(cid) = g.strings.resolve(*s) {
                        let score = LivecGraph::cosine_similarity(emb, query_vec);
                        scored.push((cid.to_string(), score));
                    }
                }
            }
        });
        scored.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.0.cmp(&b.0))
        });
        let summaries = self.chunk_prop_map("summary");
        scored
            .into_iter()
            .take(k)
            .filter_map(|(cid, _)| {
                by_id
                    .get(cid.as_str())
                    .map(|ch| self.to_ask_chunk(ch, &summaries))
            })
            .collect()
    }

    /// Embed `text` with `client`/`model` as a code-retrieval *query* (the
    /// asymmetric counterpart of the `document` task the index was embedded under)
    /// and return the single resulting vector.
    async fn embed_query(
        &self,
        client: &dyn GaiseClient,
        model: &str,
        text: &str,
    ) -> Result<Vec<f32>> {
        let req = query_embedding_request(model, text);
        let resp = client
            .embeddings(&req)
            .await
            .map_err(|e| anyhow::anyhow!("query embedding failed: {e}"))?;
        resp.output
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("embedding response was empty"))
    }

    /// Semantic variant of `ask`: embed the question, rank chunks by vector
    /// similarity, then attach the same connectivity subgraph as keyword `ask`.
    pub async fn ask_semantic(
        &self,
        client: &dyn GaiseClient,
        model: &str,
        question: &str,
        k: usize,
    ) -> Result<AskResult> {
        let qvec = self.embed_query(client, model, question).await?;
        let chunks = self.semantic_search(&qvec, k);
        let connectivity = self.connectivity_for(&chunks);
        let referenced_by = self.referenced_by(&chunks, IMPACT_CAP);
        Ok(AskResult {
            chunks,
            connectivity,
            referenced_by,
        })
    }

    /// The retrieval strategy callers should use by default: semantic (`ask_semantic`)
    /// when an embedding index exists, falling back to keyword `ask` when it doesn't —
    /// or if the embedding call fails (so a flaky/unavailable embedder degrades to
    /// keyword rather than erroring). Shared by the `/ask` command and the
    /// `ask_codebase` tool so both behave identically.
    pub async fn ask_auto(
        &self,
        client: &dyn GaiseClient,
        model: &str,
        question: &str,
        k: usize,
    ) -> AskResult {
        if self.has_embeddings() {
            match self.ask_semantic(client, model, question, k).await {
                Ok(r) => return r,
                Err(_) => return self.ask(question, k),
            }
        }
        self.ask(question, k)
    }

    /// **Inference-backed `/ask`.** Embed the question, rank chunks by cosine, then feed
    /// them to the chat model in small batches ([`ASK_BATCH`]) — answering from the
    /// first cumulative set that suffices, and only widening to the next set when the
    /// model replies `INSUFFICIENT`. This keeps token cost minimal: the model sees a
    /// handful of chunk *summaries*, never the codebase, and stops the moment it can
    /// answer. Embedding the query or an answer call failing propagates as `Err`, so the
    /// caller can fall back to keyword/graph [`Indexer::ask`] (the "traditional way").
    /// When all [`ASK_MAX_PASSES`] sets are exhausted without sufficiency, returns
    /// `answer: None` carrying the best chunks for the caller to show raw.
    pub async fn ask_inferred(
        &self,
        client: &dyn GaiseClient,
        embed_model: &str,
        chat_model: &str,
        question: &str,
    ) -> Result<AskAnswer> {
        let qvec = self.embed_query(client, embed_model, question).await?;
        let pool = self.semantic_search(&qvec, ASK_BATCH * ASK_MAX_PASSES);
        if pool.is_empty() {
            return Ok(AskAnswer::default());
        }

        // Widen the window one batch at a time: pass N sees the top N*ASK_BATCH chunks,
        // so an answer needing an early + a late chunk can still be synthesised, while a
        // query answerable from the first few stops after one cheap call.
        let mut considered: Vec<AskChunk> = Vec::new();
        let mut passes = 0usize;
        for batch in pool.chunks(ASK_BATCH) {
            passes += 1;
            considered.extend(batch.iter().cloned());
            let payload = Self::ask_payload(question, &considered);
            let req = GaiseInstructRequest {
                model: chat_model.to_string(),
                input: prompts::sys_user(prompts::ASK_ANSWER, payload),
                ..Default::default()
            };
            let resp = client
                .instruct(&req)
                .await
                .map_err(|e| anyhow::anyhow!("ask inference failed: {e}"))?;
            let msg = match resp.output {
                OneOrMany::One(m) => m,
                OneOrMany::Many(mut v) => match v.pop() {
                    Some(m) => m,
                    None => continue,
                },
            };
            let answer = first_message_text(&msg.content)
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            // Empty or the INSUFFICIENT sentinel → fetch the next set and retry.
            if answer.is_empty() || answer.eq_ignore_ascii_case("INSUFFICIENT") {
                continue;
            }
            let connectivity = self.connectivity_for(&considered);
            let referenced_by = self.referenced_by(&considered, IMPACT_CAP);
            return Ok(AskAnswer {
                answer: Some(answer),
                chunks: considered,
                connectivity,
                referenced_by,
                passes,
            });
        }

        // Exhausted every candidate set without a sufficient answer: hand back the best
        // chunks raw so the user still has leads ("best chunks + a note").
        let connectivity = self.connectivity_for(&considered);
        let referenced_by = self.referenced_by(&considered, IMPACT_CAP);
        Ok(AskAnswer {
            answer: None,
            chunks: considered,
            connectivity,
            referenced_by,
            passes,
        })
    }

    /// Render retrieved chunks into the user payload for one `/ask` inference pass.
    /// Each chunk leads with its `path:line` reference (the citation target), then its
    /// stored one-line summary plus signature when summarised — full body omitted to
    /// keep the prompt tiny — or its full snippet as a fallback when un-summarised, so
    /// the model is never blind. The question is appended last.
    fn ask_payload(question: &str, chunks: &[AskChunk]) -> String {
        let mut p = String::from("Code chunks:\n");
        for c in chunks {
            p.push_str(&format!(
                "\n[{}:{}-{}] {} {}\n",
                c.path, c.line_start, c.line_end, c.kind, c.name
            ));
            let sig = c.snippet.lines().map(str::trim).find(|l| !l.is_empty());
            match &c.summary {
                Some(s) if !s.trim().is_empty() => {
                    p.push_str(&format!("  Summary: {}\n", s.trim()));
                    if let Some(sig) = sig {
                        p.push_str(&format!("  Signature: {}\n", sig));
                    }
                }
                // Un-summarised: include the full snippet so the model still has the code.
                _ => {
                    for line in c.snippet.lines() {
                        p.push_str(&format!("  {}\n", line));
                    }
                }
            }
        }
        p.push_str(&format!("\nQuestion: {}\n", question));
        p
    }

    /// **Inference layer — chunk summaries.** Generate a one-line natural-language
    /// summary for each chunk via `client`/`model` and store it as a `summary` prop.
    /// `max` caps the number of inference calls this pass (0 = unlimited); chunks
    /// already summarised against the same body (`summary_hash`) are skipped.
    ///
    /// This convenience form summarises **every** chunk one-at-a-time
    /// ([`SummarizeOptions::unfiltered`]); callers that want the default policy
    /// (skip tests/trivial, run concurrently) use [`Indexer::summarize_index_with`].
    pub async fn summarize_index(
        &self,
        client: &dyn GaiseClient,
        model: &str,
        max: usize,
    ) -> Result<EnrichStats> {
        self.summarize_index_with(
            client,
            model,
            max,
            SummarizeOptions::unfiltered(),
            &mut |_, _| {},
        )
        .await
    }

    /// As [`summarize_index`] but takes an explicit [`SummarizeOptions`] policy and
    /// invokes `on_item` with each chunk's id as it is summarised (so callers can
    /// stream per-chunk progress). Target selection is fully algorithmic — policy
    /// exclusions (tests, trivial chunks) and the `summary_hash` gate are applied
    /// *before* any model call, and the surviving set is summarised with up to
    /// `opts.concurrency` calls in flight. Results are flushed to the WAL every
    /// `SUMMARIZE_FLUSH`, so a mid-pass cancel or error keeps the work already done.
    pub async fn summarize_index_with(
        &self,
        client: &dyn GaiseClient,
        model: &str,
        max: usize,
        opts: SummarizeOptions,
        on_item: &mut dyn FnMut(&str, SummaryProgress),
    ) -> Result<EnrichStats> {
        use futures_util::StreamExt;

        let mut stats = EnrichStats::default();
        if self.graph.is_none() {
            return Ok(stats);
        }
        let ignore = self.enrichment_ignore()?;
        let have = self.chunk_prop_map("summary_hash");
        // Deterministic order so `max` truncation is stable across runs.
        let mut targets: Vec<&ChunkRecord> =
            self.files.values().flat_map(|f| f.chunks.iter()).collect();
        targets.sort_by(|a, b| a.chunk_id.cmp(&b.chunk_id));

        // Algorithmic pre-filter (no inference): apply the policy + hash gate + cap,
        // producing the owned work-list of chunks that actually need a summary call.
        let mut pending: Vec<(String, String, String, String)> = Vec::new();
        for ch in targets {
            if ignore.is_ignored(&ch.path) {
                stats.ignored += 1;
                continue;
            }
            if !opts.include_tests && is_test_chunk(&ch.path, &ch.name) {
                stats.skipped += 1;
                continue;
            }
            // Trivial-size filter on the *code body* (not the identity-prefixed
            // enrichment text), but never for callables — a tiny function reused
            // across the codebase is worth knowing about; a 3-line arg struct is not.
            if opts.min_body_lines > 0
                && !is_callable_kind(&ch.kind)
                && non_blank_lines(&ch.indented_code()) < opts.min_body_lines
            {
                stats.skipped += 1;
                continue;
            }
            let text = enrichment_text(ch);
            let hash = format!("{:x}", hash_bytes(text.as_bytes()));
            if have.get(&ch.chunk_id).map(|h| h.as_str()) == Some(hash.as_str()) {
                // Already-current chunk: report it so the console shows it was checked
                // and skipped, not silently absent.
                on_item(&ch.chunk_id, SummaryProgress::Skipped);
                stats.skipped += 1;
                continue;
            }
            if max != 0 && pending.len() >= max {
                break;
            }
            let payload = format!("{} named `{}`:\n\n{}", ch.kind, ch.name, text);
            pending.push((ch.chunk_id.clone(), ch.kind.clone(), hash, payload));
        }

        let graph = self.graph.as_ref().unwrap();
        let mut nodes: Vec<NodeAction> = Vec::new();
        let flush = |batch: Vec<NodeAction>| -> Result<()> {
            if batch.is_empty() {
                return Ok(());
            }
            let txn = GraphTxn {
                idempotency_key: None,
                timestamp_ms: now_ms(),
                nodes: Some(batch),
                edges: None,
                indexes: None,
            };
            graph
                .submit_txn(txn)
                .map(|_| ())
                .map_err(|e| anyhow::anyhow!("storing summaries failed: {e}"))
        };

        // Issue the surviving summary calls with bounded concurrency. Each future is
        // independent and self-contained (owned request); only `client` is shared.
        let concurrency = opts.concurrency.max(1);
        let mut in_flight = futures_util::stream::iter(pending.into_iter().map(
            |(chunk_id, kind, hash, payload)| {
                let req = GaiseInstructRequest {
                    model: model.to_string(),
                    input: prompts::sys_user(prompts::CHUNK_SUMMARY, payload),
                    ..Default::default()
                };
                async move {
                    let resp = client.instruct(&req).await;
                    (chunk_id, kind, hash, resp)
                }
            },
        ))
        .buffer_unordered(concurrency);

        while let Some((chunk_id, kind, hash, resp)) = in_flight.next().await {
            let resp = match resp {
                Ok(r) => r,
                Err(e) => {
                    // Persist whatever completed before the failure, then surface it.
                    flush(std::mem::take(&mut nodes))?;
                    return Err(anyhow::anyhow!("summary call failed: {e}"));
                }
            };
            let msg = match resp.output {
                OneOrMany::One(m) => m,
                OneOrMany::Many(mut v) => match v.pop() {
                    Some(m) => m,
                    None => continue,
                },
            };
            let summary = match first_message_text(&msg.content) {
                Some(s) => s.trim().to_string(),
                None => continue,
            };
            if summary.is_empty() {
                continue;
            }
            nodes.push(NodeAction::Merge {
                ident: NodeIdent::ByIndex {
                    label: label(&kind),
                    key: PropKey::Name("chunk_id".to_string()),
                    value: GValue::Str(chunk_id.clone()),
                },
                label: label(&kind),
                props: Props(vec![
                    prop("summary", GValue::Str(summary)),
                    prop("summary_hash", GValue::Str(hash)),
                ]),
                alias: None,
            });
            on_item(&chunk_id, SummaryProgress::Summarised);
            stats.summarized += 1;
            // Flush periodically so a Ctrl+C mid-pass keeps already-computed summaries.
            if nodes.len() >= SUMMARIZE_FLUSH {
                flush(std::mem::take(&mut nodes))?;
            }
        }
        flush(nodes)?;
        Ok(stats)
    }

    /// **Inference layer — per-edge relationship summaries.** Describe each edge of
    /// kind `edge_kind` (typically `REFERENCES` — the call graph — or `SATISFIES`) in
    /// one sentence via `client`/`model`, storing it as a `summary` prop *on the edge*.
    /// `max` caps inference calls this pass (0 = unlimited); an edge whose two endpoint
    /// bodies are unchanged since it was last described (matching `rel_hash`) is
    /// skipped. `link_references` deliberately does not re-`Merge` existing edges of
    /// these kinds, so a stored summary survives re-indexing. Returns the counts
    /// (`summarized` = edges described).
    pub async fn summarize_relationships(
        &self,
        client: &dyn GaiseClient,
        model: &str,
        edge_kind: &str,
        max: usize,
    ) -> Result<EnrichStats> {
        self.summarize_relationships_reporting(client, model, edge_kind, max, &mut |_| {})
            .await
    }

    /// As [`summarize_relationships`], but invokes `on_item` (with `src → dst`) per edge
    /// described, and flushes to the WAL every [`SUMMARIZE_FLUSH`] edges so a Ctrl+C
    /// mid-pass keeps work already done.
    pub async fn summarize_relationships_reporting(
        &self,
        client: &dyn GaiseClient,
        model: &str,
        edge_kind: &str,
        max: usize,
        on_item: &mut dyn FnMut(&str),
    ) -> Result<EnrichStats> {
        let mut stats = EnrichStats::default();
        if self.graph.is_none() {
            return Ok(stats);
        }
        let by_id: HashMap<&str, &ChunkRecord> = self
            .files
            .values()
            .flat_map(|f| f.chunks.iter().map(|c| (c.chunk_id.as_str(), c)))
            .collect();

        // (src_cid, dst_cid, existing rel_hash) for every edge of this kind.
        let mut edges_info: Vec<(String, String, Option<String>)> = Vec::new();
        let graph = self.graph.as_ref().unwrap();
        graph.with_read(|g| {
            let cid_key = match g.prop_keys.get_id("chunk_id") {
                Some(k) => k,
                None => return,
            };
            let hash_key = g.prop_keys.get_id("rel_hash");
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
                if let (Some(s), Some(d)) = (cid(e.src), cid(e.dst)) {
                    let h = hash_key.and_then(|hk| match g.get_edge_prop(e.id, hk) {
                        Some(LValue::Str(s)) => g.strings.resolve(*s).map(|x| x.to_string()),
                        _ => None,
                    });
                    edges_info.push((s, d, h));
                }
            }
        });
        // Deterministic order so `max` truncation is stable across runs.
        edges_info.sort();

        let mut actions: Vec<EdgeAction> = Vec::new();
        for (src, dst, existing_hash) in edges_info {
            let (sc, dc) = match (by_id.get(src.as_str()), by_id.get(dst.as_str())) {
                (Some(s), Some(d)) => (*s, *d),
                // An endpoint not in the in-memory index (not re-indexed this session):
                // skip rather than describe with missing context.
                _ => continue,
            };
            let hash = format!(
                "{:x}",
                hash_bytes(
                    format!("{}\u{0}{}", enrichment_text(sc), enrichment_text(dc)).as_bytes()
                )
            );
            if existing_hash.as_deref() == Some(hash.as_str()) {
                stats.skipped += 1;
                continue;
            }
            if max != 0 && stats.summarized >= max {
                break;
            }
            let payload = format!(
                "Relationship type: {edge}\n\nSOURCE — {} `{}` in {}:\n{}\n\nTARGET — {} `{}` in {}:\n{}",
                sc.kind, sc.name, sc.path, enrichment_text(sc),
                dc.kind, dc.name, dc.path, enrichment_text(dc),
                edge = edge_kind,
            );
            let req = GaiseInstructRequest {
                model: model.to_string(),
                input: prompts::sys_user(prompts::RELATIONSHIP_SUMMARY, payload),
                ..Default::default()
            };
            let resp = client
                .instruct(&req)
                .await
                .map_err(|e| anyhow::anyhow!("relationship summary call failed: {e}"))?;
            let msg = match resp.output {
                OneOrMany::One(m) => m,
                OneOrMany::Many(mut v) => match v.pop() {
                    Some(m) => m,
                    None => continue,
                },
            };
            let summary = match first_message_text(&msg.content) {
                Some(s) => s.trim().to_string(),
                None => continue,
            };
            if summary.is_empty() {
                continue;
            }
            actions.push(chunk_edge_with_props(
                &sc.kind,
                &src,
                &dc.kind,
                &dst,
                edge_kind,
                vec![
                    prop("summary", GValue::Str(summary)),
                    prop("rel_hash", GValue::Str(hash)),
                ],
            ));
            on_item(&format!("{} → {}", src, dst));
            stats.summarized += 1;
            // Flush periodically so a Ctrl+C mid-pass keeps already-described edges.
            if actions.len() >= SUMMARIZE_FLUSH {
                let batch = std::mem::take(&mut actions);
                let txn = GraphTxn {
                    idempotency_key: None,
                    timestamp_ms: now_ms(),
                    nodes: None,
                    edges: Some(batch),
                    indexes: None,
                };
                graph
                    .submit_txn(txn)
                    .map_err(|e| anyhow::anyhow!("storing relationship summaries failed: {e}"))?;
            }
        }
        if !actions.is_empty() {
            let txn = GraphTxn {
                idempotency_key: None,
                timestamp_ms: now_ms(),
                nodes: None,
                edges: Some(actions),
                indexes: None,
            };
            graph
                .submit_txn(txn)
                .map_err(|e| anyhow::anyhow!("storing relationship summaries failed: {e}"))?;
        }
        Ok(stats)
    }

    /// **Inference layer — directory rollups.** For each `Directory` node, compose a
    /// one-line summary of what that directory holds from its descendant chunks'
    /// summaries (falling back to their `kind name` when a chunk is un-summarised),
    /// via `client`/`model`, storing it as a `summary` prop on the Directory node.
    /// `max` caps inference calls (0 = unlimited); a directory whose child set is
    /// unchanged since it was last summarised (matching `summary_hash`) is skipped.
    /// Best run after `summarize_index` so the rollup composes from real summaries.
    /// Returns the counts (`summarized` = directories described).
    pub async fn summarize_directories(
        &self,
        client: &dyn GaiseClient,
        model: &str,
        max: usize,
    ) -> Result<EnrichStats> {
        self.summarize_directories_reporting(client, model, max, &mut |_| {})
            .await
    }

    /// As [`summarize_directories`], but invokes `on_item` with each directory path as it
    /// is summarised, and flushes to the WAL every [`SUMMARIZE_FLUSH`] directories so a
    /// Ctrl+C mid-pass keeps work already done.
    pub async fn summarize_directories_reporting(
        &self,
        client: &dyn GaiseClient,
        model: &str,
        max: usize,
        on_item: &mut dyn FnMut(&str),
    ) -> Result<EnrichStats> {
        let mut stats = EnrichStats::default();
        if self.graph.is_none() {
            return Ok(stats);
        }
        let chunk_summaries = self.chunk_prop_map("summary");
        let have = self.dir_prop_map("summary_hash");
        // Deterministic order so `max` truncation is stable.
        let mut dirs: Vec<String> = self.graph_dir_paths().into_iter().collect();
        dirs.sort();

        // Bound the child list fed to the model (and into the hash) so a huge
        // directory can't blow up the prompt; entries are sorted for stability.
        const MAX_DIR_ENTRIES: usize = 40;

        let graph = self.graph.as_ref().unwrap();
        let mut nodes: Vec<NodeAction> = Vec::new();
        for dir in dirs {
            // Files under this directory (root "." holds everything).
            let prefix = if dir == "." {
                String::new()
            } else {
                format!("{}/", dir)
            };
            let mut entries: Vec<String> = Vec::new();
            for file in self.files.values() {
                if dir != "." && !file.path.starts_with(&prefix) {
                    continue;
                }
                for ch in &file.chunks {
                    // Only roll up top-level definitions to keep the picture coarse.
                    if ch.parent_id.is_some() {
                        continue;
                    }
                    let entry = match chunk_summaries.get(&ch.chunk_id) {
                        Some(s) => format!("{} {} — {}", ch.kind, ch.name, s),
                        None => format!("{} {}", ch.kind, ch.name),
                    };
                    entries.push(entry);
                }
            }
            if entries.is_empty() {
                continue;
            }
            entries.sort();
            entries.dedup();
            let shown = entries.len().min(MAX_DIR_ENTRIES);
            let listing = entries[..shown].join("\n");
            let hash = format!("{:x}", hash_bytes(listing.as_bytes()));
            if have.get(&dir).map(|h| h.as_str()) == Some(hash.as_str()) {
                stats.skipped += 1;
                continue;
            }
            if max != 0 && stats.summarized >= max {
                break;
            }
            let name = if dir == "." {
                "the project root".to_string()
            } else {
                dir.clone()
            };
            let payload = format!("Directory: {}\n\nTop-level elements:\n{}", name, listing);
            let req = GaiseInstructRequest {
                model: model.to_string(),
                input: prompts::sys_user(prompts::DIRECTORY_SUMMARY, payload),
                ..Default::default()
            };
            let resp = client
                .instruct(&req)
                .await
                .map_err(|e| anyhow::anyhow!("directory summary call failed: {e}"))?;
            let msg = match resp.output {
                OneOrMany::One(m) => m,
                OneOrMany::Many(mut v) => match v.pop() {
                    Some(m) => m,
                    None => continue,
                },
            };
            let summary = match first_message_text(&msg.content) {
                Some(s) => s.trim().to_string(),
                None => continue,
            };
            if summary.is_empty() {
                continue;
            }
            nodes.push(NodeAction::Merge {
                ident: NodeIdent::ByIndex {
                    label: label("Directory"),
                    key: PropKey::Name("path".to_string()),
                    value: GValue::Str(dir.clone()),
                },
                label: label("Directory"),
                props: Props(vec![
                    prop("summary", GValue::Str(summary)),
                    prop("summary_hash", GValue::Str(hash)),
                ]),
                alias: None,
            });
            on_item(&dir);
            stats.summarized += 1;
            // Flush periodically so a Ctrl+C mid-pass keeps already-summarised dirs.
            if nodes.len() >= SUMMARIZE_FLUSH {
                let batch = std::mem::take(&mut nodes);
                let txn = GraphTxn {
                    idempotency_key: None,
                    timestamp_ms: now_ms(),
                    nodes: Some(batch),
                    edges: None,
                    indexes: None,
                };
                graph
                    .submit_txn(txn)
                    .map_err(|e| anyhow::anyhow!("storing directory summaries failed: {e}"))?;
            }
        }
        if !nodes.is_empty() {
            let txn = GraphTxn {
                idempotency_key: None,
                timestamp_ms: now_ms(),
                nodes: Some(nodes),
                edges: None,
                indexes: None,
            };
            graph
                .submit_txn(txn)
                .map_err(|e| anyhow::anyhow!("storing directory summaries failed: {e}"))?;
        }
        Ok(stats)
    }

    /// **Inference layer — relationship/architecture overview.** A single inference
    /// call that turns the graph's shape (node/edge label counts and the most
    /// connected definitions) into a prose overview of how the codebase fits
    /// together — the relationship-level companion to per-chunk summaries. Returns
    /// the generated prose.
    pub async fn summarize_architecture(
        &self,
        client: &dyn GaiseClient,
        model: &str,
    ) -> Result<String> {
        let stats = self
            .graph_stats()
            .ok_or_else(|| anyhow::anyhow!("graph persistence is disabled"))?;
        let nodes: Vec<String> = stats
            .node_labels
            .iter()
            .map(|(l, c)| format!("{l}: {c}"))
            .collect();
        let edges: Vec<String> = stats
            .edge_labels
            .iter()
            .map(|(l, c)| format!("{l}: {c}"))
            .collect();
        let langs: Vec<String> = self
            .language_breakdown()
            .iter()
            .map(|(l, c)| format!("{l} ({c})"))
            .collect();
        let payload = format!(
            "Languages: {}\nNode counts by kind:\n  {}\nRelationship (edge) counts:\n  {}",
            langs.join(", "),
            nodes.join("\n  "),
            edges.join("\n  "),
        );
        let req = GaiseInstructRequest {
            model: model.to_string(),
            input: prompts::sys_user(prompts::ARCHITECTURE, payload),
            ..Default::default()
        };
        let resp = client
            .instruct(&req)
            .await
            .map_err(|e| anyhow::anyhow!("architecture summary failed: {e}"))?;
        let msg = match resp.output {
            OneOrMany::One(m) => m,
            OneOrMany::Many(mut v) => v.pop().ok_or_else(|| anyhow::anyhow!("empty response"))?,
        };
        first_message_text(&msg.content)
            .map(|s| s.trim().to_string())
            .ok_or_else(|| anyhow::anyhow!("no text in architecture summary"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use gaise_core::contracts::{
        GaiseEmbeddingTask, GaiseEmbeddingsResponse, GaiseInstructResponse,
        GaiseInstructStreamResponse,
    };
    use std::error::Error;
    use std::pin::Pin;
    use std::sync::Mutex;

    /// Records the task of every embeddings request and answers with a fixed-size
    /// vector, so the tests can assert what the index sends without a model.
    struct RecordingClient {
        tasks: Mutex<Vec<Option<GaiseEmbeddingTask>>>,
    }

    impl RecordingClient {
        fn new() -> Self {
            RecordingClient {
                tasks: Mutex::new(Vec::new()),
            }
        }
        fn tasks(&self) -> Vec<Option<GaiseEmbeddingTask>> {
            self.tasks.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl GaiseClient for RecordingClient {
        async fn instruct_stream(
            &self,
            _request: &GaiseInstructRequest,
        ) -> Result<
            Pin<
                Box<
                    dyn futures_util::Stream<
                            Item = Result<
                                GaiseInstructStreamResponse,
                                Box<dyn Error + Send + Sync>,
                            >,
                        > + Send,
                >,
            >,
            Box<dyn Error + Send + Sync>,
        > {
            Err("not exercised".into())
        }

        async fn instruct(
            &self,
            _request: &GaiseInstructRequest,
        ) -> Result<GaiseInstructResponse, Box<dyn Error + Send + Sync>> {
            Err("not exercised".into())
        }

        async fn embeddings(
            &self,
            request: &GaiseEmbeddingsRequest,
        ) -> Result<GaiseEmbeddingsResponse, Box<dyn Error + Send + Sync>> {
            self.tasks.lock().unwrap().push(request.task.clone());
            let n = match &request.input {
                OneOrMany::One(_) => 1,
                OneOrMany::Many(v) => v.len(),
            };
            Ok(GaiseEmbeddingsResponse {
                external_id: None,
                output: vec![vec![1.0, 0.0]; n],
                usage: None,
            })
        }
    }

    fn indexed(dir: &Path) -> Indexer {
        let mut idx = Indexer::new(dir.to_path_buf());
        let p = dir.join("a.rs");
        std::fs::write(&p, "pub fn alpha_fn() { compute(); }\n").unwrap();
        let spec = lang::detect_language(&p).unwrap();
        idx.index_file(&p, &spec).unwrap();
        idx
    }

    /// Overwrite every chunk's stored `embed_task`, simulating vectors written by
    /// an older build (no task) or under a different convention.
    fn set_embed_task(idx: &Indexer, value: &str) {
        let graph = idx.graph.as_ref().unwrap();
        let nodes = idx
            .files
            .values()
            .flat_map(|f| f.chunks.iter())
            .map(|ch| NodeAction::Merge {
                ident: NodeIdent::ByIndex {
                    label: label(&ch.kind),
                    key: PropKey::Name("chunk_id".to_string()),
                    value: GValue::Str(ch.chunk_id.clone()),
                },
                label: label(&ch.kind),
                props: Props(vec![prop("embed_task", GValue::Str(value.to_string()))]),
                alias: None,
            })
            .collect();
        graph
            .submit_txn(GraphTxn {
                idempotency_key: None,
                timestamp_ms: now_ms(),
                nodes: Some(nodes),
                edges: None,
                indexes: None,
            })
            .unwrap();
    }

    #[test]
    fn task_sensitivity_follows_the_registry_profile() {
        // Prefix / taskType models embed documents and queries differently.
        assert!(embedding_task_sensitive("ollama::nomic-embed-text:latest"));
        assert!(embedding_task_sensitive("gemini::gemini-embedding-001"));
        // OpenAI has no task concept: the same text always yields the same vector.
        assert!(!embedding_task_sensitive("openai::text-embedding-3-small"));
        // Unknown models and malformed ids err on the side of re-embedding.
        assert!(embedding_task_sensitive("ollama::some-new-embedder"));
        assert!(embedding_task_sensitive("no-provider-prefix"));
    }

    #[tokio::test]
    async fn index_embeds_as_document_and_queries_as_code_query() {
        let dir = tempfile::tempdir().unwrap();
        let idx = indexed(dir.path());
        let client = RecordingClient::new();
        let model = "openai::text-embedding-3-small";

        assert_eq!(idx.embed_index(&client, model).await.unwrap().embedded, 1);
        assert_eq!(
            idx.chunk_prop_map("embed_task").values().next().map(String::as_str),
            Some("document")
        );
        idx.embed_query(&client, model, "where is alpha computed?")
            .await
            .unwrap();
        assert_eq!(
            client.tasks(),
            vec![
                Some(GaiseEmbeddingTask::Document),
                Some(GaiseEmbeddingTask::CodeQuery)
            ]
        );
    }

    #[tokio::test]
    async fn stale_task_reembeds_only_for_task_sensitive_models() {
        let dir = tempfile::tempdir().unwrap();
        let idx = indexed(dir.path());
        let client = RecordingClient::new();

        // A model whose vectors ignore the task keeps pre-task vectors.
        let openai = "openai::text-embedding-3-small";
        assert_eq!(idx.embed_index(&client, openai).await.unwrap().embedded, 1);
        set_embed_task(&idx, "legacy");
        let again = idx.embed_index(&client, openai).await.unwrap();
        assert_eq!((again.embedded, again.skipped), (0, 1));

        // A prefix model must re-embed: `search_document: …` vectors differ from
        // bare-text ones, so the stored vector no longer matches query vectors.
        let nomic = "ollama::nomic-embed-text:latest";
        assert_eq!(idx.embed_index(&client, nomic).await.unwrap().embedded, 1);
        assert_eq!(idx.embed_index(&client, nomic).await.unwrap().skipped, 1);
        set_embed_task(&idx, "legacy");
        let again = idx.embed_index(&client, nomic).await.unwrap();
        assert_eq!((again.embedded, again.skipped), (1, 0));
        assert_eq!(
            idx.chunk_prop_map("embed_task").values().next().map(String::as_str),
            Some("document")
        );
    }
}
