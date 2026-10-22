//! Deliberately introduced bugs, used to prove the fuzzer works.
//!
//! Each bug is **off by default** and only active when explicitly enabled
//! (`--plant <name>` on the CLI, or [`Bugs::with`] in code). They exist so the
//! acceptance tests can show, end to end, that a specific class of defect is
//! found from a fixed seed and minimized to a small reproducer.

use std::fmt;
use std::str::FromStr;

/// One planted bug.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PlantedBug {
    /// Optimizer rewrites `a - (b - c)` to `(a - b) - c` (should be `a - b + c`).
    /// Expected finding: invariant violation, oracle `optimizer-equivalence`.
    OptSubAssoc,
    /// Printer drops the parentheses around a right-hand `-`/`+` operand of `-`,
    /// so `a - (b - c)` prints as `a - b - c`.
    /// Expected finding: invariant violation, oracle `roundtrip-ast`.
    PrintSubParens,
    /// Type checker accepts `==`/`!=` between an Int and a Bool.
    /// Expected finding: invariant violation, oracle `type-soundness`.
    TypeckEqMixed,
    /// Interpreter indexes a table with a negative remainder and panics.
    /// Expected finding: crash (index out of bounds).
    InterpRemPanic,
}

impl PlantedBug {
    /// Every planted bug.
    pub const ALL: [PlantedBug; 4] = [
        PlantedBug::OptSubAssoc,
        PlantedBug::PrintSubParens,
        PlantedBug::TypeckEqMixed,
        PlantedBug::InterpRemPanic,
    ];

    /// CLI name.
    pub fn name(self) -> &'static str {
        match self {
            PlantedBug::OptSubAssoc => "opt-sub-assoc",
            PlantedBug::PrintSubParens => "print-sub-parens",
            PlantedBug::TypeckEqMixed => "typeck-eq-mixed",
            PlantedBug::InterpRemPanic => "interp-rem-panic",
        }
    }
}

impl fmt::Display for PlantedBug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for PlantedBug {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        PlantedBug::ALL.into_iter().find(|b| b.name() == s).ok_or_else(|| {
            let names: Vec<_> = PlantedBug::ALL.iter().map(|b| b.name()).collect();
            format!("unknown planted bug `{s}`; expected one of: {}", names.join(", "))
        })
    }
}

/// The set of enabled planted bugs (empty = the correct implementation).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Bugs(u8);

impl Bugs {
    /// No planted bugs.
    pub const NONE: Bugs = Bugs(0);

    fn bit(b: PlantedBug) -> u8 {
        match b {
            PlantedBug::OptSubAssoc => 1,
            PlantedBug::PrintSubParens => 2,
            PlantedBug::TypeckEqMixed => 4,
            PlantedBug::InterpRemPanic => 8,
        }
    }

    /// This set plus `b`.
    #[must_use]
    pub fn with(self, b: PlantedBug) -> Bugs {
        Bugs(self.0 | Self::bit(b))
    }

    /// Is `b` enabled?
    pub fn has(self, b: PlantedBug) -> bool {
        self.0 & Self::bit(b) != 0
    }

    /// Enabled bugs, in a fixed order.
    pub fn list(self) -> Vec<PlantedBug> {
        PlantedBug::ALL.into_iter().filter(|b| self.has(*b)).collect()
    }
}

impl FromIterator<PlantedBug> for Bugs {
    fn from_iter<I: IntoIterator<Item = PlantedBug>>(iter: I) -> Self {
        iter.into_iter().fold(Bugs::NONE, Bugs::with)
    }
}
