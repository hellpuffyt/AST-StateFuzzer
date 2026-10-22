#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]
//! Unit-level tests of the Mini language implementation (the system under test).

use astfuzz::fuzz::generator::{GenConfig, generate};
use astfuzz::fuzz::rng::{ByteSource, Rng};
use astfuzz::lang::ast::{BinOp, Expr, Program, Stmt, UnOp};
use astfuzz::lang::interp::{Limits, RuntimeError, Value, run};
use astfuzz::lang::lexer::MAX_SOURCE_BYTES;
use astfuzz::lang::opt::optimize;
use astfuzz::lang::parser::{MAX_NESTING, parse};
use astfuzz::lang::printer::{print_expr, print_program};
use astfuzz::lang::typeck::check;
use proptest::prelude::*;

fn eval(src: &str) -> (Vec<Value>, Result<(), RuntimeError>) {
    let (o, _) = run(&parse(src).unwrap(), Limits::default());
    (o.output, o.result)
}

fn e(src: &str) -> Expr {
    match parse(&format!("print {src};")).unwrap().stmts.remove(0) {
        Stmt::Print(e) => e,
        s => panic!("unexpected {s:?}"),
    }
}

#[test]
fn precedence_and_associativity() {
    use Expr::{Binary as B, Int};
    let b = |x: Expr| Box::new(x);
    assert_eq!(e("1 - 2 - 3"), B(BinOp::Sub, b(B(BinOp::Sub, b(Int(1)), b(Int(2)))), b(Int(3))));
    assert_eq!(e("1 + 2 * 3"), B(BinOp::Add, b(Int(1)), b(B(BinOp::Mul, b(Int(2)), b(Int(3))))));
    assert_eq!(e("-2"), Expr::Unary(UnOp::Neg, b(Int(2))));
    assert_eq!(print_expr(&e("1 - (2 - 3)")), "1 - (2 - 3)");
    assert_eq!(print_expr(&e("(1 - 2) - 3")), "1 - 2 - 3");
    assert_eq!(print_expr(&e("((1))")), "1");
    assert_eq!(print_expr(&e("!(1 < 2) || true && false")), "!(1 < 2) || true && false");
}

#[test]
fn comparisons_do_not_chain() {
    let err = parse("print 1 < 2 < 3;").unwrap_err();
    assert!(err.message.contains("do not chain"), "{err}");
    assert!(parse("print (1 < 2) == true;").is_ok());
}

#[test]
fn parse_errors_are_actionable() {
    let err = parse("let x = ;").unwrap_err();
    assert_eq!(err.offset, 8);
    assert!(err.message.contains("expected an expression"), "{err}");
    assert!(parse("if true { print 1;").unwrap_err().message.contains("unterminated block"));
    assert!(parse("print 99999999999999999999;").unwrap_err().message.contains("does not fit"));
    assert!(parse("print 1 $ 2;").unwrap_err().message.contains("unexpected character"));
    assert!(parse("print é;").is_err());
}

#[test]
fn hostile_nesting_is_rejected_not_stack_overflowed() {
    let deep = format!("print {}1{};", "(".repeat(10_000), ")".repeat(10_000));
    assert!(parse(&deep).unwrap_err().message.contains("nesting"));
    let chain = format!("print 1{};", " + 1".repeat(10_000));
    assert!(parse(&chain).unwrap_err().message.contains("nesting"));
    let neg = format!("print {}1;", "-".repeat(10_000));
    assert!(parse(&neg).unwrap_err().message.contains("nesting"));
    let blocks = format!("{}{}", "{".repeat(10_000), "}".repeat(10_000));
    assert!(parse(&blocks).unwrap_err().message.contains("nesting"));
    // Right at the limit is fine.
    let ok = format!("print {}1{};", "(".repeat(MAX_NESTING - 2), ")".repeat(MAX_NESTING - 2));
    assert!(parse(&ok).is_ok());
}

#[test]
fn oversized_source_is_rejected() {
    let big = "#".repeat(MAX_SOURCE_BYTES + 1);
    assert!(parse(&big).unwrap_err().message.contains("larger than"));
}

#[test]
fn comments_and_whitespace_are_ignored() {
    let p = parse("# header\nlet x = 1; # trailing\n\n  print x ;").unwrap();
    assert_eq!(print_program(&p), "let x = 1;\nprint x;\n");
}

#[test]
fn interpreter_semantics() {
    assert_eq!(
        eval("print 7 / 2; print 7 % 3; print -7 / 2; print -7 % 3;").0,
        vec![Value::Int(3), Value::Int(1), Value::Int(-3), Value::Int(-1)]
    );
    assert_eq!(eval("print 9223372036854775807 + 1;").1, Err(RuntimeError::Overflow));
    assert_eq!(eval("print -9223372036854775807 - 1 - 1;").1, Err(RuntimeError::Overflow));
    assert_eq!(eval("print (-9223372036854775807 - 1) / -1;").1, Err(RuntimeError::Overflow));
    assert_eq!(eval("print 1 / 0;").1, Err(RuntimeError::DivByZero));
    assert_eq!(eval("print 1 % 0;").1, Err(RuntimeError::DivByZero));
    assert_eq!(eval("print 1 + true;").1, Err(RuntimeError::Type));
    assert_eq!(eval("print x;").1, Err(RuntimeError::Unbound));
    assert_eq!(eval("x = 1;").1, Err(RuntimeError::Unbound));
    // Output before an error is kept.
    assert_eq!(eval("print 1; print 1 / 0;"), (vec![Value::Int(1)], Err(RuntimeError::DivByZero)));
}

#[test]
fn short_circuit_skips_errors() {
    assert_eq!(eval("print false && 1 / 0 == 0;").1, Ok(()));
    assert_eq!(eval("print true || 1 / 0 == 0;").1, Ok(()));
    assert_eq!(eval("print true && 1 / 0 == 0;").1, Err(RuntimeError::DivByZero));
}

#[test]
fn scoping() {
    let src = "let x = 1; { let x = 2; x = 3; print x; } print x; if true { x = 5; } print x;";
    assert_eq!(eval(src).0, vec![Value::Int(3), Value::Int(1), Value::Int(5)]);
    assert_eq!(eval("if true { let y = 1; } print y;").1, Err(RuntimeError::Unbound));
    assert!(check(&parse("if true { let y = 1; } print y;").unwrap()).is_err());
}

#[test]
fn fuel_and_wall_clock_bound_infinite_loops() {
    let p = parse("while true { }").unwrap();
    let (o, _) = run(&p, Limits { fuel: 1_000, wall: None });
    assert_eq!(o.result, Err(RuntimeError::OutOfFuel));
    assert!(o.steps <= 1_001);
    let (o, _) = run(&p, Limits { fuel: u64::MAX, wall: Some(std::time::Duration::from_millis(20)) });
    assert_eq!(o.result, Err(RuntimeError::WallTimeout));
}

#[test]
fn type_checker_accepts_and_rejects() {
    assert!(check(&parse("let x = 1; x = x + 1; print x < 3 && true;").unwrap()).is_ok());
    for bad in [
        "print 1 + true;",
        "let x = 1; x = false;",
        "if 1 { }",
        "while 0 { }",
        "print 1 == true;",
        "print !1;",
        "print -true;",
        "y = 1;",
    ] {
        assert!(check(&parse(bad).unwrap()).is_err(), "{bad} should be rejected");
    }
}

#[test]
fn optimizer_folds_without_hiding_errors() {
    let p = parse("print 2 * 3 - 10; if 1 < 2 { print true && false; } while false { print 1; }").unwrap();
    assert_eq!(print_program(&optimize(&p)), "print -4;\n{\n    print false;\n}\n{\n}\n");
    // Errors are never folded away.
    for keep in ["print 1 / 0;", "print 9223372036854775807 + 1;", "print 1 == true;"] {
        let p = parse(keep).unwrap();
        assert_eq!(optimize(&p), p, "{keep} must not be folded");
    }
}

#[test]
fn generated_programs_round_trip_and_type_check_mostly() {
    let cfg = GenConfig { ill_typed_percent: 0, ..GenConfig::default() };
    for i in 0..500 {
        let p = generate(&mut Rng::fork(42, i), &cfg);
        assert_eq!(parse(&print_program(&p)).unwrap(), p, "round trip failed for program {i}");
        assert!(check(&p).is_ok(), "ill_typed_percent=0 must give well-typed programs (program {i})");
    }
}

#[test]
fn empty_program_is_valid() {
    assert_eq!(parse("").unwrap(), Program::default());
    assert_eq!(eval("").1, Ok(()));
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    /// Any byte string drives the generator to a finite program that
    /// round-trips through the printer and parser.
    #[test]
    fn byte_driven_generation_round_trips(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
        let p = generate(&mut ByteSource::new(&bytes), &GenConfig::default());
        prop_assert_eq!(parse(&print_program(&p)).unwrap(), p);
    }

    /// The parser never panics on arbitrary text.
    #[test]
    fn parser_never_panics(s in ".{0,200}") {
        let _ = parse(&s);
    }
}

#[test]
fn deepest_accepted_program_survives_every_pass() {
    use astfuzz::fuzz::exec::{ExecConfig, check};
    // Close to the parser's limit in both dimensions: nested blocks and a long
    // operator chain inside them. Every recursive pass (printer, parser,
    // type checker, interpreter, optimizer) must handle it on a test thread's
    // default stack.
    let depth = 90;
    let chain = format!("print 1{};", " - 1".repeat(90));
    let src = format!("{}{chain}{}", "{ ".repeat(depth), " }".repeat(depth));
    let p = parse(&src).unwrap();
    let v = check(&p, &ExecConfig::default());
    assert!(v.failure.is_none(), "{:?}", v.failure);
    let negs = format!("print {}1;", "-".repeat(150));
    let v = check(&parse(&negs).unwrap(), &ExecConfig::default());
    assert!(v.failure.is_none(), "{:?}", v.failure);
}
