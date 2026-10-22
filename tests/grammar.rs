#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]
//! Generated and mutated programs satisfy Mini's grammar and the generator's
//! documented budgets, for arbitrary seeds and arbitrary generator settings.
//! The grammar is the one `lang::parser` implements (EBNF in its module docs):
//! a program is grammatical iff its printed text parses back to the same AST.

use astfuzz::fuzz::generator::{GenConfig, generate};
use astfuzz::fuzz::mutate::mutate;
use astfuzz::fuzz::rng::Rng;
use astfuzz::lang::ast::{Expr, Program, Stmt};
use astfuzz::lang::interp::{Limits, RuntimeError, run};
use astfuzz::lang::parser::parse;
use astfuzz::lang::printer::print_program;
use astfuzz::lang::typeck::check;
use proptest::prelude::*;

fn config() -> impl Strategy<Value = GenConfig> {
    (0u32..8, 0u32..5, 0u32..10, 0u32..=100, 0u32..=100, proptest::array::uniform6(0u32..50)).prop_map(
        |(e, b, s, ill, unb, w)| GenConfig {
            max_expr_depth: e,
            max_block_depth: b,
            max_stmts: s,
            ill_typed_percent: ill,
            unbounded_loop_percent: unb,
            stmt_weights: w,
        },
    )
}

/// Largest statements-per-block, block nesting and expression height, and
/// whether every integer literal is non-negative.
struct Shape {
    max_block_len: usize,
    max_nesting: usize,
    max_expr_height: usize,
    literals_non_negative: bool,
}

fn shape(p: &Program) -> Shape {
    fn expr(e: &Expr, s: &mut Shape) {
        s.max_expr_height = s.max_expr_height.max(e.depth());
        if let Expr::Int(n) = e {
            s.literals_non_negative &= *n >= 0;
        }
        for c in e.children() {
            expr(c, s);
        }
    }
    fn block(stmts: &[Stmt], nesting: usize, s: &mut Shape) {
        s.max_block_len = s.max_block_len.max(stmts.len());
        s.max_nesting = s.max_nesting.max(nesting);
        for st in stmts {
            for e in st.exprs() {
                expr(e, s);
            }
            for b in st.blocks() {
                block(b, nesting + 1, s);
            }
        }
    }
    let mut s = Shape { max_block_len: 0, max_nesting: 0, max_expr_height: 0, literals_non_negative: true };
    block(&p.stmts, 0, &mut s);
    s
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]

    /// Every generated program, for any seed and any settings, prints to text
    /// the parser accepts and that parses back to the identical AST.
    #[test]
    fn generated_programs_satisfy_the_grammar(seed in any::<u64>(), it in any::<u64>(), cfg in config()) {
        let p = generate(&mut Rng::fork(seed, it), &cfg);
        let text = print_program(&p);
        let back = parse(&text);
        prop_assert!(back.is_ok(), "{:?}\n{}", back, text);
        prop_assert_eq!(back.unwrap(), p);
    }

    /// The generator's budgets are hard limits, with the documented
    /// allowance for the bounded-loop pattern: `let i = 0; while i < K { ..; i = i + 1; }`
    /// emits two statements per draw and appends one to the loop body, and its
    /// condition and increment have height 2.
    #[test]
    fn generated_programs_respect_the_budgets(seed in any::<u64>(), cfg in config()) {
        let p = generate(&mut Rng::new(seed), &cfg);
        let s = shape(&p);
        let stmts = cfg.max_stmts.max(1) as usize;
        prop_assert!(s.max_block_len <= 2 * stmts + 1, "block of {} statements, max_stmts {}", s.max_block_len, stmts);
        prop_assert!(s.max_nesting <= cfg.max_block_depth as usize, "nesting {} > {}", s.max_nesting, cfg.max_block_depth);
        prop_assert!(s.max_expr_height <= (cfg.max_expr_depth as usize).max(2));
        prop_assert!(s.literals_non_negative, "the grammar has no negative literals");
    }

    /// Generated programs only reference variables in scope, whatever share of
    /// ill-typed operands is requested: ill-typed mode breaks types, never
    /// scoping or syntax.
    #[test]
    fn generated_programs_never_reference_unbound_variables(seed in any::<u64>(), cfg in config()) {
        let p = generate(&mut Rng::new(seed), &cfg);
        let (o, _) = run(&p, Limits { fuel: 5_000, wall: None });
        prop_assert_ne!(o.result, Err(RuntimeError::Unbound));
        if cfg.ill_typed_percent == 0 {
            prop_assert!(check(&p).is_ok(), "ill_typed_percent = 0 must give well-typed programs");
        }
    }

    /// Mutation preserves grammaticality for any seed (stacked mutations).
    #[test]
    fn mutated_programs_satisfy_the_grammar(seed in any::<u64>(), cfg in config(), rounds in 1usize..8) {
        let mut rng = Rng::new(seed);
        let mut p = generate(&mut rng, &cfg);
        let donor = generate(&mut rng, &cfg);
        for _ in 0..rounds {
            mutate(&mut p, &donor, &mut rng, &cfg);
            let back = parse(&print_program(&p));
            prop_assert!(back.is_ok(), "{:?}", back);
            prop_assert_eq!(back.unwrap(), p.clone());
        }
    }
}

/// The documented invalid-input mode is *ill-typed* programs (not syntax
/// errors: those would die in the parser, which is the point of a
/// structure-aware fuzzer). At 100 % it must produce many programs the type
/// checker rejects, all still grammatical.
#[test]
fn ill_typed_mode_produces_type_errors_but_valid_syntax() {
    let cfg = GenConfig { ill_typed_percent: 100, ..GenConfig::default() };
    let mut rejected = 0;
    for i in 0..400 {
        let p = generate(&mut Rng::fork(9, i), &cfg);
        assert_eq!(parse(&print_program(&p)).unwrap(), p);
        if check(&p).is_err() {
            rejected += 1;
        }
    }
    assert!(rejected > 200, "only {rejected}/400 ill-typed programs were rejected by the type checker");
    let default_rejected =
        (0..400).filter(|&i| check(&generate(&mut Rng::fork(9, i), &GenConfig::default())).is_err()).count();
    assert!(default_rejected < rejected, "the default share ({default_rejected}) must be lower than 100 %");
}
