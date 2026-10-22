//! Execution-feature coverage used as fuzzing feedback.
//!
//! **This is not control-flow-graph coverage.** The interpreter reports
//! *semantic features* of each run: which statement kinds executed, which
//! operator produced which class of result (zero / positive / negative /
//! error), which branch direction was taken, how many times loops iterated
//! (log buckets), which operator was nested inside which, and which runtime
//! error ended the run. An input that shows a
//! feature never seen before is kept in the corpus. No compiler
//! instrumentation, basic blocks or CFG edges are involved; see
//! `docs/COVERAGE.md` for how real edge coverage could be added.

use std::collections::BTreeSet;

/// One observable execution feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Feature {
    /// A statement kind executed (0 let, 1 assign, 2 print, 5 block).
    Stmt(u32),
    /// Binary operator id × result class (0 zero/false, 1 positive/true, 2 negative, 3 error).
    Op(u32, u32),
    /// A binary operator evaluated with a binary operator as its left (0) or
    /// right (1) operand: `(parent op, side, child op)`.
    OpPair(u32, u32, u32),
    /// `if` took the true (1) or false (0) branch.
    Branch(u32),
    /// A `while` loop finished after a bucketed number of iterations.
    Loop(u32),
    /// A value of a given class was printed.
    Printed(u32),
    /// The run ended with a given runtime error.
    Error(u32),
}

impl Feature {
    /// Stable numeric encoding (used for hashing and reports).
    pub fn code(self) -> u32 {
        match self {
            Feature::Stmt(k) => k,
            Feature::Op(op, class) => 100 + op * 4 + class,
            Feature::OpPair(p, side, c) => 1000 + p * 100 + side * 50 + c,
            Feature::Branch(b) => 200 + b,
            Feature::Loop(b) => 300 + b,
            Feature::Printed(c) => 400 + c,
            Feature::Error(e) => 500 + e,
        }
    }
}

/// The set of features one or more runs exhibited.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Coverage {
    features: BTreeSet<Feature>,
}

impl Coverage {
    /// Record a feature.
    pub fn hit(&mut self, f: Feature) {
        self.features.insert(f);
    }

    /// Number of distinct features.
    pub fn len(&self) -> usize {
        self.features.len()
    }

    /// True if no feature was recorded.
    pub fn is_empty(&self) -> bool {
        self.features.is_empty()
    }

    /// Add everything from `other`; returns how many features were new.
    pub fn merge(&mut self, other: &Coverage) -> usize {
        let before = self.features.len();
        self.features.extend(other.features.iter().copied());
        self.features.len() - before
    }

    /// Would merging `other` add anything?
    pub fn has_new(&self, other: &Coverage) -> bool {
        other.features.iter().any(|f| !self.features.contains(f))
    }

    /// Sorted feature codes.
    pub fn codes(&self) -> Vec<u32> {
        self.features.iter().map(|f| f.code()).collect()
    }
}
