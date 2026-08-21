//! Higher-dimensional graph layout for the `/visualize` **3D graph** view.
//!
//! Given the indexed chunks (nodes) and their relationship edges (REFERENCES,
//! IMPLEMENTS, …), we compute a high-dimensional embedding, **project it to 3D via
//! PCA** (the top three principal axes), and scale it for display. Two embedding
//! algorithms are available (see [`LayoutAlgo`]); both are pure-Rust, so the CLI gains
//! no native deps, and both are deterministic (seeded RNG, no `Date`/`rand` entropy) so
//! the same index always lays out the same way:
//!
//!   - [`LayoutAlgo::FastRp`] (**default**) — *Fast Random Projection*, the embedding
//!     Neo4j's Graph Data Science library uses. Seed each node with a sparse random
//!     vector, then propagate it over the degree-normalised adjacency a few hops,
//!     accumulating weighted contributions. Cost is **linear in edges** (`O(iters ·
//!     edges · dim)`), so it stays fast on big graphs and is robust to the many
//!     isolated / disconnected chunks a code graph has (they keep their random seed and
//!     scatter onto the surrounding sphere). This replaced the force model, which was
//!     `O(nodes²)` per tick and too slow to display.
//!   - [`LayoutAlgo::Force`] (**parked**) — the original force-directed model (the
//!     `livec_tesseract` physics: centre force → spherical shell, inverse-square repel,
//!     weighted link attraction). Kept for small graphs / comparison; opt in via
//!     [`LayoutParams::algo`].
//!
//! Everything here is pure and unit-tested; the browser only renders the result.

use std::collections::HashMap;

use serde::Serialize;

/// One node fed into the layout — a symbol, a file, or a directory. Lightweight (no
/// code body) so the layout layer is decoupled from `ChunkDetail`; the caller
/// assembles symbols + synthesised file/directory containers into a single list.
#[derive(Debug, Clone)]
pub struct LayoutNode {
    pub id: String,
    pub name: String,
    /// `"Directory"`, `"File"`, or a symbol kind (`Function`, `Struct`, …).
    pub kind: String,
    pub path: String,
    /// `[start, end]` for symbols; `[0, 0]` for containers.
    pub lines: [usize; 2],
}

/// One positioned node sent to the 3D view. `x`/`y`/`z` are the PCA-projected,
/// display-scaled coordinates; `degree` (incident relationship edges) drives node size.
#[derive(Debug, Clone, Serialize)]
pub struct GraphNode3D {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub path: String,
    pub lines: [usize; 2],
    pub degree: usize,
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// One relationship edge, as *indices* into [`GraphView::nodes`] (compact for the wire).
#[derive(Debug, Clone, Serialize)]
pub struct GraphEdge3D {
    pub source: usize,
    pub target: usize,
    pub label: String,
}

/// The full 3D graph payload for `/api/graph`.
#[derive(Debug, Clone, Serialize)]
pub struct GraphView {
    pub nodes: Vec<GraphNode3D>,
    pub edges: Vec<GraphEdge3D>,
    /// How many chunks were dropped because the node cap was hit (0 = nothing dropped).
    /// Surfaced to the user rather than silently truncating.
    pub truncated: usize,
    /// Dimensions the layout ran in before projecting to 3D (for the legend / debugging).
    pub dims: usize,
}

/// Knobs for the layout. Defaults mirror `livec_tesseract` (which produced good clouds)
/// except the attraction is linear in edge weight rather than `log2` — `log2(1) == 0`
/// would leave singly-connected nodes un-attracted, which we don't want.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutAlgo {
    /// Fast Random Projection (default) — linear in edges, the Neo4j-GDS approach.
    FastRp,
    /// Force-directed N-D physics (parked) — `O(nodes²)` per tick; small graphs only.
    /// Not wired to a default or CLI flag yet (opt in by setting [`LayoutParams::algo`]),
    /// hence intentionally unconstructed in normal builds.
    #[allow(dead_code)]
    Force,
}

#[derive(Debug, Clone)]
pub struct LayoutParams {
    /// Which embedding algorithm to run before the PCA→3D projection.
    pub algo: LayoutAlgo,
    /// Embedding dimensionality (FastRP projection width, or force-model dims).
    pub dims: usize,
    /// FastRP propagation weights, one per hop. `[0]` weights the raw random seed (so
    /// isolated nodes still get a position); later entries weight each propagation hop.
    pub fastrp_weights: Vec<f32>,
    // --- force model (parked) ---
    pub ticks: usize,
    pub centre_force: f32,
    pub repel_force: f32,
    pub attraction_force: f32,
    pub damping: f32,
    // --- shared ---
    /// Cap on laid-out nodes (highest-degree kept); 0 = no cap.
    pub max_nodes: usize,
    pub seed: u64,
    /// Half-extent the projected cloud is scaled to fit (display units).
    pub display_radius: f32,
}

impl Default for LayoutParams {
    fn default() -> Self {
        LayoutParams {
            algo: LayoutAlgo::FastRp,
            dims: 128,
            fastrp_weights: vec![0.3, 1.0, 1.0],
            ticks: 300,
            centre_force: 0.03,
            repel_force: 0.02,
            attraction_force: 0.02,
            damping: 0.9,
            max_nodes: 6000,
            seed: 0x5eed_1234_abcd_ef01,
            display_radius: 220.0,
        }
    }
}

/// Tiny deterministic PRNG (SplitMix64) — keeps the layout reproducible without pulling
/// `rand` or touching wall-clock/OS entropy (which the workflow sandbox forbids anyway).
struct SplitMix64(u64);

impl SplitMix64 {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform f32 in `[-1.0, 1.0)`.
    fn unit(&mut self) -> f32 {
        let v = (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32; // [0,1)
        v * 2.0 - 1.0
    }
}

/// Build the 3D graph view from the node list (symbols + file/directory containers) and
/// the edge list (`(src_id, dst_id, label)` — relationship *and* containment edges).
/// Pure and deterministic.
pub fn build_graph_view(
    input: &[LayoutNode],
    edges_in: &[(String, String, String)],
    p: &LayoutParams,
) -> GraphView {
    let by_id: HashMap<&str, &LayoutNode> = input.iter().map(|n| (n.id.as_str(), n)).collect();

    // --- 1. Degrees (unique undirected neighbours), so a node cap can keep the
    //        most-connected nodes and node size reflects real connectivity. A→B and
    //        B→A coalesce to one, matching the edge set built below.
    let mut seen_pairs: std::collections::HashSet<(&str, &str)> = std::collections::HashSet::new();
    let mut degree: HashMap<&str, usize> = HashMap::new();
    for (s, d, _) in edges_in {
        if by_id.contains_key(s.as_str()) && by_id.contains_key(d.as_str()) && s != d {
            let key = if s < d {
                (s.as_str(), d.as_str())
            } else {
                (d.as_str(), s.as_str())
            };
            if seen_pairs.insert(key) {
                *degree.entry(s.as_str()).or_default() += 1;
                *degree.entry(d.as_str()).or_default() += 1;
            }
        }
    }

    // Stable node ordering: by id, so layout/serialisation are reproducible.
    let mut ids: Vec<&str> = by_id.keys().copied().collect();
    ids.sort_unstable();

    // Apply the node cap by keeping the highest-degree nodes (tie-break by id for
    // determinism). Below the cap, keep everything.
    let truncated = if p.max_nodes > 0 && ids.len() > p.max_nodes {
        let dropped = ids.len() - p.max_nodes;
        ids.sort_by(|a, b| {
            degree
                .get(b)
                .copied()
                .unwrap_or(0)
                .cmp(&degree.get(a).copied().unwrap_or(0))
                .then_with(|| a.cmp(b))
        });
        ids.truncate(p.max_nodes);
        ids.sort_unstable(); // restore stable order for indexing
        dropped
    } else {
        0
    };

    let index: HashMap<&str, usize> = ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    let n = ids.len();

    // --- 2. Collapse edges to unique pairs with multiplicity as weight. ----------
    // Key is (min,max) index so A→B and B→A coalesce; the displayed label is the first
    // one seen for that pair.
    let mut pair_weight: HashMap<(usize, usize), f32> = HashMap::new();
    let mut pair_label: HashMap<(usize, usize), String> = HashMap::new();
    for (s, d, label) in edges_in {
        let (si, di) = match (index.get(s.as_str()), index.get(d.as_str())) {
            (Some(&a), Some(&b)) if a != b => (a, b),
            _ => continue,
        };
        let key = if si < di { (si, di) } else { (di, si) };
        *pair_weight.entry(key).or_insert(0.0) += 1.0;
        pair_label.entry(key).or_insert_with(|| label.clone());
    }

    // Adjacency (both directions, with weight), built from a *sorted* pair list:
    // `HashMap` iteration order is per-map randomised, and summing embeddings/forces in
    // a different order changes f32 rounding, which compounds. Sorting keeps the whole
    // layout bit-for-bit reproducible. Shared by both layout algorithms.
    let mut pairs: Vec<((usize, usize), f32)> = pair_weight.iter().map(|(&k, &w)| (k, w)).collect();
    pairs.sort_by(|x, y| x.0.cmp(&y.0));
    let mut adj: Vec<Vec<(usize, f32)>> = vec![Vec::new(); n];
    for &((a, b), w) in &pairs {
        adj[a].push((b, w));
        adj[b].push((a, w));
    }

    // --- 3. Embed in `dims` dimensions, then project to 3D via PCA and scale. ----
    let dims = p.dims.max(3);
    let pos = match p.algo {
        LayoutAlgo::FastRp => fastrp_embed(n, &adj, dims, &p.fastrp_weights, p.seed),
        LayoutAlgo::Force => force_embed(n, &adj, dims, p),
    };
    let proj = pca_project(&pos, dims, 3);
    // Connected structure fills the inner 72% so the isolated-node shell (below) sits
    // clearly outside it.
    let mut scaled = scale_to_radius(&proj, p.display_radius * 0.72);

    // Isolated nodes (no relationship edges) carry no structural signal, so PCA leaves
    // them bunched near the origin. Scatter them deterministically onto the surrounding
    // sphere instead — the "isolated symbols sit on the shell, connected ones cluster
    // inside" picture from the original brief.
    let isolated: Vec<usize> = (0..n)
        .filter(|&i| degree.get(ids[i]).copied().unwrap_or(0) == 0)
        .collect();
    for (k, &i) in isolated.iter().enumerate() {
        scaled[i] = point_on_sphere(k, isolated.len(), p.display_radius);
    }

    let nodes: Vec<GraphNode3D> = ids
        .iter()
        .enumerate()
        .map(|(i, &id)| {
            let d = by_id[id];
            let c = scaled.get(i).copied().unwrap_or([0.0, 0.0, 0.0]);
            GraphNode3D {
                id: d.id.clone(),
                name: d.name.clone(),
                kind: d.kind.clone(),
                path: d.path.clone(),
                lines: d.lines,
                degree: degree.get(id).copied().unwrap_or(0),
                x: c[0],
                y: c[1],
                z: c[2],
            }
        })
        .collect();

    let mut edges: Vec<GraphEdge3D> = pair_weight
        .keys()
        .map(|&(a, b)| GraphEdge3D {
            source: a,
            target: b,
            label: pair_label.get(&(a, b)).cloned().unwrap_or_default(),
        })
        .collect();
    edges.sort_by(|x, y| x.source.cmp(&y.source).then(x.target.cmp(&y.target)));

    GraphView {
        nodes,
        edges,
        truncated,
        dims,
    }
}

/// **FastRP** embedding (Fast Random Projection — the Neo4j-GDS approach).
///
/// Seed each node with a *very sparse* random vector (entries in `{-√s, 0, +√s}`), then
/// repeatedly propagate over the symmetric degree-normalised adjacency
/// `Â = D^-½ · W · D^-½`. Each hop's (L2-normalised) result is accumulated with the
/// matching `weights[hop]`; `weights[0]` weights the raw seed so isolated nodes keep a
/// position. Cost is `O(iters · edges · dim)` — linear in edges, the reason this is fast
/// enough to display where the force model wasn't.
fn fastrp_embed(
    n: usize,
    adj: &[Vec<(usize, f32)>],
    dim: usize,
    weights: &[f32],
    seed: u64,
) -> Vec<Vec<f32>> {
    if n == 0 {
        return Vec::new();
    }
    // Sparse random projection. s = 3 → two-thirds of entries are zero (Achlioptas).
    let s = 3.0f32;
    let scale = s.sqrt();
    let mut rng = SplitMix64(seed);
    let mut x: Vec<Vec<f32>> = (0..n)
        .map(|_| {
            (0..dim)
                .map(|_| {
                    let r = (rng.next_u64() >> 40) as f32 / (1u64 << 24) as f32; // [0,1)
                    if r < 0.5 / s {
                        -scale
                    } else if r < 1.0 / s {
                        scale
                    } else {
                        0.0
                    }
                })
                .collect()
        })
        .collect();

    // Weighted degree for the symmetric normalisation.
    let deg: Vec<f32> = adj
        .iter()
        .map(|nb| nb.iter().map(|(_, w)| *w).sum())
        .collect();

    let mut emb = vec![vec![0.0f32; dim]; n];
    add_weighted(&mut emb, &x, weights.first().copied().unwrap_or(0.0));

    for &w_hop in weights.iter().skip(1) {
        let mut xn = vec![vec![0.0f32; dim]; n];
        for i in 0..n {
            if deg[i] <= 0.0 {
                continue;
            }
            let di = deg[i].sqrt();
            for &(j, w) in &adj[i] {
                if deg[j] <= 0.0 {
                    continue;
                }
                let c = w / (di * deg[j].sqrt());
                let (row_i, row_j) = (&mut xn[i], &x[j]);
                for d in 0..dim {
                    row_i[d] += c * row_j[d];
                }
            }
        }
        l2_normalize_rows(&mut xn);
        add_weighted(&mut emb, &xn, w_hop);
        x = xn;
    }
    emb
}

/// `acc[i] += w * src[i]` over every coordinate.
fn add_weighted(acc: &mut [Vec<f32>], src: &[Vec<f32>], w: f32) {
    if w == 0.0 {
        return;
    }
    for (a, s) in acc.iter_mut().zip(src) {
        for (av, sv) in a.iter_mut().zip(s) {
            *av += w * sv;
        }
    }
}

/// L2-normalise each row in place (rows that are ~zero are left as zero).
fn l2_normalize_rows(rows: &mut [Vec<f32>]) {
    for row in rows.iter_mut() {
        let norm = row.iter().map(|v| v * v).sum::<f32>().sqrt();
        if norm > 1e-12 {
            for v in row.iter_mut() {
                *v /= norm;
            }
        }
    }
}

/// The **parked** force-directed embedding: the `livec_tesseract` physics in `dims`
/// dimensions (centre force + inverse-square repulsion + weighted link attraction,
/// integrated with damping). `O(nodes²)` per tick — fine for small graphs, too slow for
/// large ones, which is why FastRP is the default.
#[allow(clippy::needless_range_loop)] // index math reads clearest for the physics kernels
fn force_embed(
    n: usize,
    adj: &[Vec<(usize, f32)>],
    dims: usize,
    p: &LayoutParams,
) -> Vec<Vec<f32>> {
    let mut rng = SplitMix64(p.seed);
    let mut pos: Vec<Vec<f32>> = (0..n)
        .map(|_| (0..dims).map(|_| rng.unit()).collect())
        .collect();
    let mut vel: Vec<Vec<f32>> = vec![vec![0.0; dims]; n];

    for _ in 0..p.ticks {
        let mut force = vec![vec![0.0f32; dims]; n];

        // Centre force: pull toward origin → spherical containment.
        for i in 0..n {
            for (f, &x) in force[i].iter_mut().zip(&pos[i]) {
                *f -= x * p.centre_force;
            }
        }

        // Inverse-square repulsion between every pair (O(n²); the node cap bounds it).
        for i in 0..n {
            for j in (i + 1)..n {
                let mut d2 = 0.0f32;
                for (&pi, &pj) in pos[i].iter().zip(&pos[j]) {
                    let diff = pi - pj;
                    d2 += diff * diff;
                }
                if d2 > 1e-9 {
                    let rep = p.repel_force / d2;
                    for d in 0..dims {
                        let f = rep * (pos[i][d] - pos[j][d]);
                        force[i][d] += f;
                        force[j][d] -= f;
                    }
                }
            }
        }

        // Link attraction: connected nodes pull together (linear in edge weight).
        for i in 0..n {
            for &(j, w) in &adj[i] {
                for d in 0..dims {
                    force[i][d] += p.attraction_force * (pos[j][d] - pos[i][d]) * w;
                }
            }
        }

        // Integrate with damping.
        for i in 0..n {
            for d in 0..dims {
                vel[i][d] = (vel[i][d] + force[i][d]) * p.damping;
                pos[i][d] += vel[i][d];
            }
        }
    }
    pos
}

/// Deterministic point `k` of `count` spread evenly over a sphere of the given radius
/// (Fibonacci / golden-spiral lattice). Used to scatter isolated nodes onto the shell.
fn point_on_sphere(k: usize, count: usize, radius: f32) -> [f32; 3] {
    if count == 0 {
        return [0.0, 0.0, 0.0];
    }
    let i = k as f32 + 0.5;
    let y = 1.0 - 2.0 * i / count as f32; // -1 .. 1
    let r = (1.0 - y * y).max(0.0).sqrt();
    let golden = std::f32::consts::PI * (1.0 + 5.0f32.sqrt()); // golden angle
    let theta = golden * i;
    [
        radius * r * theta.cos(),
        radius * y,
        radius * r * theta.sin(),
    ]
}

/// Project `n` points of dimension `dims` onto their top `out` principal axes.
/// Centres the data, then finds the leading eigenvectors of the covariance by power
/// iteration with deflation (covariance is `dims × dims`, tiny — no linear-algebra dep
/// needed). Returns `n` vectors of length `out`.
// Index-based loops read clearest for the triangular covariance fill and matrix–vector
// products here (and matching pairs of `cov`/`centred`/`axes` rows), so range loops are
// intentional.
#[allow(clippy::needless_range_loop)]
fn pca_project(pos: &[Vec<f32>], dims: usize, out: usize) -> Vec<[f32; 3]> {
    let n = pos.len();
    let out = out.min(3);
    if n == 0 {
        return Vec::new();
    }
    // Mean.
    let mut mean = vec![0.0f64; dims];
    for p in pos {
        for d in 0..dims {
            mean[d] += p[d] as f64;
        }
    }
    for m in &mut mean {
        *m /= n as f64;
    }
    // Covariance (dims × dims).
    let mut cov = vec![vec![0.0f64; dims]; dims];
    for p in pos {
        let centred: Vec<f64> = (0..dims).map(|d| p[d] as f64 - mean[d]).collect();
        for a in 0..dims {
            for b in a..dims {
                cov[a][b] += centred[a] * centred[b];
            }
        }
    }
    for a in 0..dims {
        for b in a..dims {
            cov[a][b] /= n as f64;
            cov[b][a] = cov[a][b];
        }
    }

    // Top `out` eigenvectors via power iteration + deflation.
    let mut axes: Vec<Vec<f64>> = Vec::with_capacity(out);
    let mut rng = SplitMix64(0xC0FF_EE12_3456_789A);
    for _ in 0..out {
        let mut v: Vec<f64> = (0..dims).map(|_| rng.unit() as f64 + 0.5).collect();
        normalize(&mut v);
        for _ in 0..64 {
            // w = cov · v
            let mut w = vec![0.0f64; dims];
            for a in 0..dims {
                for b in 0..dims {
                    w[a] += cov[a][b] * v[b];
                }
            }
            // Deflate against axes already found (keep orthogonal).
            for ax in &axes {
                let dotp: f64 = (0..dims).map(|d| w[d] * ax[d]).sum();
                for d in 0..dims {
                    w[d] -= dotp * ax[d];
                }
            }
            if !normalize(&mut w) {
                break;
            }
            v = w;
        }
        axes.push(v);
    }
    // Pad with zero axes if dims < out (shouldn't happen: dims >= 3).
    while axes.len() < 3 {
        axes.push(vec![0.0f64; dims]);
    }

    pos.iter()
        .map(|p| {
            let centred: Vec<f64> = (0..dims).map(|d| p[d] as f64 - mean[d]).collect();
            let mut c = [0.0f32; 3];
            for (k, item) in c.iter_mut().enumerate() {
                let dotp: f64 = (0..dims).map(|d| centred[d] * axes[k][d]).sum();
                *item = dotp as f32;
            }
            c
        })
        .collect()
}

/// In-place L2 normalise; returns false if the vector is ~zero (can't normalise).
fn normalize(v: &mut [f64]) -> bool {
    let norm = v.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm < 1e-12 {
        return false;
    }
    for x in v.iter_mut() {
        *x /= norm;
    }
    true
}

/// Scale projected points so the largest coordinate magnitude maps to `radius`,
/// keeping the cloud centred and aspect-true.
fn scale_to_radius(points: &[[f32; 3]], radius: f32) -> Vec<[f32; 3]> {
    let mut max_abs = 0.0f32;
    for p in points {
        for &c in p {
            max_abs = max_abs.max(c.abs());
        }
    }
    if max_abs < 1e-9 {
        return points.to_vec();
    }
    let k = radius / max_abs;
    points
        .iter()
        .map(|p| [p[0] * k, p[1] * k, p[2] * k])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, name: &str) -> LayoutNode {
        LayoutNode {
            id: id.to_string(),
            name: name.to_string(),
            kind: "Function".to_string(),
            path: "src/lib.rs".to_string(),
            lines: [1, 9],
        }
    }

    fn nodes(ids: &[(&str, &str)]) -> Vec<LayoutNode> {
        ids.iter().map(|(id, n)| node(id, n)).collect()
    }

    /// Two tightly-connected clusters (a triangle and a triangle) joined by a single
    /// bridge edge end up with each triangle's members closer to each other than to the
    /// far triangle — i.e. "connections pull closer" survives the projection.
    #[test]
    fn connected_nodes_cluster_closer_than_unrelated() {
        let cs = nodes(&[
            ("a", "a"),
            ("b", "b"),
            ("c", "c"), // cluster 1
            ("x", "x"),
            ("y", "y"),
            ("z", "z"), // cluster 2
        ]);
        let r = |s: &str, d: &str| (s.to_string(), d.to_string(), "REFERENCES".to_string());
        let rels = vec![
            r("a", "b"),
            r("b", "c"),
            r("c", "a"), // triangle 1
            r("x", "y"),
            r("y", "z"),
            r("z", "x"), // triangle 2
            r("c", "x"), // thin bridge
        ];
        let view = build_graph_view(&cs, &rels, &LayoutParams::default());
        assert_eq!(view.nodes.len(), 6);
        assert_eq!(view.edges.len(), 7);

        let pos: HashMap<&str, [f32; 3]> = view
            .nodes
            .iter()
            .map(|n| (n.id.as_str(), [n.x, n.y, n.z]))
            .collect();
        let dist = |p: &str, q: &str| {
            let (a, b) = (pos[p], pos[q]);
            ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
        };
        // Within-cluster distance is smaller than the cross-cluster distance.
        let within = dist("a", "b").max(dist("b", "c")).max(dist("a", "c"));
        let across = dist("a", "x").min(dist("a", "y")).min(dist("a", "z"));
        assert!(
            within < across,
            "within {within} should be < across {across}"
        );
    }

    /// Degrees and edge-index mapping are correct; A↔B duplicate directions coalesce.
    #[test]
    fn degrees_and_edges_are_computed() {
        let cs = nodes(&[("a", "a"), ("b", "b"), ("c", "c")]);
        let rels = vec![
            ("a".into(), "b".into(), "REFERENCES".into()),
            ("b".into(), "a".into(), "REFERENCES".into()), // same pair, coalesces
            ("b".into(), "c".into(), "IMPLEMENTS".into()),
            ("a".into(), "missing".into(), "REFERENCES".into()), // dangling, ignored
        ];
        let view = build_graph_view(&cs, &rels, &LayoutParams::default());
        assert_eq!(view.edges.len(), 2); // a-b and b-c
        let by_name: HashMap<&str, &GraphNode3D> =
            view.nodes.iter().map(|n| (n.name.as_str(), n)).collect();
        assert_eq!(by_name["a"].degree, 1);
        assert_eq!(by_name["b"].degree, 2);
        assert_eq!(by_name["c"].degree, 1);
    }

    /// The node cap keeps the highest-degree nodes and reports how many were dropped.
    #[test]
    fn node_cap_keeps_hubs_and_reports_truncation() {
        let cs = nodes(&[("hub", "hub"), ("a", "a"), ("b", "b"), ("c", "c")]);
        // hub connects to everyone (degree 3); a,b,c have degree 1.
        let rels = vec![
            ("hub".into(), "a".into(), "REFERENCES".into()),
            ("hub".into(), "b".into(), "REFERENCES".into()),
            ("hub".into(), "c".into(), "REFERENCES".into()),
        ];
        let p = LayoutParams {
            max_nodes: 2,
            ..Default::default()
        };
        let view = build_graph_view(&cs, &rels, &p);
        assert_eq!(view.nodes.len(), 2);
        assert_eq!(view.truncated, 2);
        assert!(
            view.nodes.iter().any(|n| n.name == "hub"),
            "the hub must survive the cap"
        );
    }

    /// Determinism: identical inputs lay out identically (seeded RNG, no wall-clock).
    #[test]
    fn layout_is_deterministic() {
        let cs = nodes(&[("a", "a"), ("b", "b"), ("c", "c")]);
        let rels = vec![
            ("a".into(), "b".into(), "REFERENCES".into()),
            ("b".into(), "c".into(), "REFERENCES".into()),
        ];
        let v1 = build_graph_view(&cs, &rels, &LayoutParams::default());
        let v2 = build_graph_view(&cs, &rels, &LayoutParams::default());
        for (n1, n2) in v1.nodes.iter().zip(&v2.nodes) {
            assert_eq!(n1.id, n2.id);
            assert!(
                (n1.x - n2.x).abs() < 1e-6
                    && (n1.y - n2.y).abs() < 1e-6
                    && (n1.z - n2.z).abs() < 1e-6
            );
        }
    }

    /// An empty graph is handled without panic.
    #[test]
    fn empty_graph_is_safe() {
        let view = build_graph_view(&[], &[], &LayoutParams::default());
        assert!(view.nodes.is_empty() && view.edges.is_empty());
    }

    /// Isolated nodes (no relationship edges) are scattered onto the display sphere, not
    /// bunched at the origin — every one sits ~`display_radius` from the centre.
    #[test]
    fn isolated_nodes_land_on_the_sphere() {
        let cs = nodes(&[("a", "a"), ("b", "b"), ("c", "c"), ("d", "d")]);
        let p = LayoutParams::default();
        let view = build_graph_view(&cs, &[], &p); // no edges → all isolated
        assert_eq!(view.nodes.len(), 4);
        for n in &view.nodes {
            let r = (n.x * n.x + n.y * n.y + n.z * n.z).sqrt();
            assert!(
                (r - p.display_radius).abs() < 1.0,
                "isolated node off the shell: r={r}"
            );
        }
    }

    /// The parked force-directed algorithm still lays the graph out and keeps connected
    /// nodes closer than unrelated ones (also constructs `LayoutAlgo::Force`).
    #[test]
    fn force_algo_still_clusters() {
        let cs = nodes(&[
            ("a", "a"),
            ("b", "b"),
            ("c", "c"),
            ("x", "x"),
            ("y", "y"),
            ("z", "z"),
        ]);
        let r = |s: &str, d: &str| (s.to_string(), d.to_string(), "REFERENCES".to_string());
        let rels = vec![
            r("a", "b"),
            r("b", "c"),
            r("c", "a"),
            r("x", "y"),
            r("y", "z"),
            r("z", "x"),
            r("c", "x"),
        ];
        let p = LayoutParams {
            algo: LayoutAlgo::Force,
            ..LayoutParams::default()
        };
        let view = build_graph_view(&cs, &rels, &p);
        assert_eq!(view.nodes.len(), 6);
        let pos: HashMap<&str, [f32; 3]> = view
            .nodes
            .iter()
            .map(|n| (n.id.as_str(), [n.x, n.y, n.z]))
            .collect();
        let dist = |p: &str, q: &str| {
            let (a, b) = (pos[p], pos[q]);
            ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
        };
        let within = dist("a", "b").max(dist("b", "c")).max(dist("a", "c"));
        let across = dist("a", "x").min(dist("a", "y")).min(dist("a", "z"));
        assert!(
            within < across,
            "force: within {within} should be < across {across}"
        );
    }
}
