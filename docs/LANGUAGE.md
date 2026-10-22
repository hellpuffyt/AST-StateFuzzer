# The Mini language

Mini is the fuzzer's initial target: small enough to specify completely, rich enough to contain real bugs.

## Syntax

See the EBNF in `src/lang/parser.rs`. Summary:

```text
let x = 1 + 2 * 3;        # declaration (shadowing allowed)
x = x - 1;                # assignment to the innermost binding
print x < 10 && !false;   # output
if x == 6 { print 1; } else { print 0; }
while x < 10 { x = x + 1; }
{ let y = x; print y; }   # nested scope
```

Comments start with `#`. Integer literals are non-negative decimal numbers up to `i64::MAX`; `-5` is unary minus
applied to `5`. Comparisons (`< <= == !=`) do not chain: `1 < 2 < 3` is a parse error.

Precedence, loosest first: `||`, `&&`, comparisons, `+ -`, `* / %`, unary `- !`. Binary operators are
left-associative.

## Types

`Int` (64-bit signed) and `Bool`. Arithmetic and `< <=` take Ints; `&& || !` take Bools; `== !=` take two values of
the same type. Conditions must be Bool. A variable keeps the type of its declaration.

## Semantics

- Arithmetic is **checked**: overflow raises `Overflow`; `/` and `%` by zero raise `DivByZero`.
  `/` truncates toward zero; `%` has the sign of the dividend (as in Rust).
- `&&` and `||` short-circuit: the right operand is not evaluated when the left decides.
- Every `if` branch, `while` body (each iteration) and `{ }` block opens a new scope; `let` inside it is not visible
  after it.
- `print` appends the value to the output, which is kept even if a later statement fails.
- Dynamic errors: `Type` (operator applied to the wrong type), `Unbound` (undeclared variable).
- Budget errors: `OutOfFuel` (one unit per statement, expression node and loop iteration; default 20 000) and
  `WallTimeout` (checked every 1 024 steps; default 2 s).

## Guarantees checked by the fuzzer

1. `parse(print(p)) == p` for every AST that respects the non-negative-literal invariant.
2. A program accepted by `typeck::check` never fails with `Type` or `Unbound` at run time.
3. `opt::optimize(p)` prints the same values and ends with the same result as `p`.
4. No pass panics on any input.
