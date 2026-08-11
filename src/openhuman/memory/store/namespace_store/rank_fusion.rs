//! Reciprocal Rank Fusion (RRF) for hybrid memory recall.
//!
//! The recall path scores each candidate on several arms — graph relevance,
//! vector (cosine) similarity, lexical/keyword overlap, and episodic/event FTS
//! rank — that live on **incommensurable scales**. Blending them with a fixed
//! linear weight (the historical `GRAPH_WEIGHT`/`VECTOR_WEIGHT`/`KEYWORD_WEIGHT`
//! sum in [`super::query`]) makes the ranking sensitive to those magic constants
//! and to each arm's score distribution, and it throws away the real FTS rank of
//! episodic/event hits (replacing it with a positional `1 - idx/len`).
//!
//! RRF (Cormack, Clarke & Buettcher, SIGIR 2009 — "Reciprocal Rank Fusion
//! outperforms Condorcet and individual Rank Learning Methods") fuses the arms by
//! **rank**, not score: an item's fused score is `Σ 1/(k + rank)` over the arms it
//! appears in. Because it only reads each arm's ordering it is invariant to score
//! scale, and it rewards candidates that several arms agree on. This mirrors the
//! recipe the code-search path already uses (`codegraph::search`, `RRF_K = 60`).

use std::collections::HashMap;

/// RRF damping constant. A larger `k` flattens the contribution of top ranks,
/// making fusion rely more on breadth of agreement than on any single arm's #1.
/// `60` is the canonical value from the SIGIR 2009 paper and the value the
/// in-repo code-search fusion already uses.
pub(crate) const RRF_K: f64 = 60.0;

/// Fuse several ranked id lists (each best-first) into one fused score per id.
///
/// For every list an id appears in, adds `1 / (RRF_K + rank + 1)` where `rank` is
/// the id's 0-based position in that list. Returns `id -> fused score`; the
/// caller sorts by it (and may apply a freshness/priority tie-break). Ids absent
/// from every list simply do not appear in the result.
pub(crate) fn reciprocal_rank_fusion(rankings: &[Vec<String>]) -> HashMap<String, f64> {
    let mut fused: HashMap<String, f64> = HashMap::new();
    for ranking in rankings {
        for (rank, id) in ranking.iter().enumerate() {
            *fused.entry(id.clone()).or_insert(0.0) += 1.0 / (RRF_K + rank as f64 + 1.0);
        }
    }
    fused
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn single_ranking_orders_by_rank() {
        let f = reciprocal_rank_fusion(&[ids(&["a", "b", "c"])]);
        assert!(f["a"] > f["b"] && f["b"] > f["c"]);
        // rank-0 contribution is exactly 1/(k+1).
        assert!((f["a"] - 1.0 / (RRF_K + 1.0)).abs() < 1e-12);
    }

    #[test]
    fn cross_arm_agreement_beats_a_single_arm_leader() {
        // `b` and `c` each lead exactly one arm; `a` is second in BOTH. RRF
        // rewards breadth of agreement, so `a` overtakes the one-arm leaders —
        // the property a fixed-weight linear blend misses when the arms' score
        // scales differ.
        let arm1 = ids(&["b", "a"]);
        let arm2 = ids(&["c", "a"]);
        let f = reciprocal_rank_fusion(&[arm1, arm2]);
        let expected_a = 2.0 / (RRF_K + 2.0);
        let expected_b = 1.0 / (RRF_K + 1.0);
        assert!(
            f["a"] > f["b"],
            "cross-arm agreement (a={}) must beat a single-arm leader (b={})",
            f["a"],
            f["b"]
        );
        assert!((f["a"] - expected_a).abs() < 1e-12);
        assert!((f["b"] - expected_b).abs() < 1e-12);
        assert!((f["c"] - expected_b).abs() < 1e-12);
    }

    #[test]
    fn item_present_in_one_arm_still_scores() {
        let f = reciprocal_rank_fusion(&[ids(&["x"]), ids(&["y"])]);
        assert_eq!(f.len(), 2);
        // Both are rank-0 in their (only) arm, so they tie.
        assert!((f["x"] - f["y"]).abs() < 1e-12);
    }

    #[test]
    fn empty_inputs_yield_empty_output() {
        assert!(reciprocal_rank_fusion(&[]).is_empty());
        assert!(reciprocal_rank_fusion(&[Vec::new()]).is_empty());
    }

    #[test]
    fn duplicate_id_across_arms_accumulates() {
        // The same id at rank 0 in two arms scores twice the single-arm value.
        let f = reciprocal_rank_fusion(&[ids(&["a"]), ids(&["a"])]);
        assert!((f["a"] - 2.0 / (RRF_K + 1.0)).abs() < 1e-12);
    }
}
