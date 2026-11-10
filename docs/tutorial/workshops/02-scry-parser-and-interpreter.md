# Workshop: a parser and interpreter

[`examples/scry`](../../../examples/scry/) is a complete small language written
in Solveig. It reads rules from a file and answers interactive logic queries.
Run the checked sample:

```sh
cargo run -- --project-root examples/scry < examples/scry/sample-input.txt
```

The repository integration test asserts the complete prompt and answer stream.

## Follow the representation pipeline

```text
String
  └─ tokenize ─▶ [Tok]
       └─ parseSExp ─▶ SExp
            └─ goalFromSExp / clauseFromSExp ─▶ Goal / Clause
                 └─ solveQuery ─▶ [[Binding]]
                      └─ renderResults ─▶ String
```

Each arrow has a smaller input language than the previous one. The tokenizer
knows only whitespace and parentheses. The S-expression parser knows nesting.
The domain decoder knows facts, rules, predicates, variables, conjunction, and
disjunction. The solver never sees raw characters.

## Model the language before implementing it

[`scry.ast`](../../../examples/scry/src/scry/ast.sv) defines:

```solveig
data Term = Atom String | Var String
data Pred = Pred String [Term]
data Goal = GoalTrue | GoalPred Pred | GoalAnd [Goal] | GoalOr [Goal]
data Clause = Clause Pred Goal
```

The design deliberately omits nested function terms. That keeps unification
finite and makes the missing occurs-check harmless for this language. Extending
`Term` with recursive structures would require revisiting that correctness
argument rather than only adding a parser branch.

## Parse in two stages

The hand-written parser in
[`scry.parser`](../../../examples/scry/src/scry/parser.sv) first builds
`SAtom` and `SList`. `parseList` consumes until `TRParen`; EOF before the
terminator is `ParseError`. `parseAllTokens` rejects trailing malformed
input.

The second stage returns `Maybe` domain values. For example,
`termFromSExp` accepts atoms, classifying names beginning with `?` as
variables, and rejects lists. `clauseFromSExp` accepts exactly the two public
forms:

```text
(fact predicate)
(rule head goal)
```

This parser is intentionally compact and loses detailed positions. Use
`std.parser` when a public language needs source positions, expected-token
sets, controlled backtracking, incremental input, or domain parse errors.

## Understand unification

[`scry.solver`](../../../examples/scry/src/scry/solver.sv) represents a
substitution as `[Binding]`. `walk` follows variable-to-term bindings until
it reaches an atom or unbound variable. `unifyTerm` then has three cases:

- two equal atoms keep the substitution;
- a variable binds to the other term;
- two different atoms fail with `Nothing`.

`unifyArgs` also checks arity by requiring both lists to end together.
`unifyPred` first checks predicate name, then arguments.

Rules are freshened before every use. Without `freshClause depth`, variables
from separate rule applications could collide because their source spellings
are equal. The depth suffix gives each application a distinct internal name.

## Search is ordinary list computation

`solveGoal` maps the goal constructors to search behavior:

- `GoalTrue` returns the current binding once;
- `GoalPred` tries every clause;
- `GoalAnd` threads every surviving substitution through the next goal;
- `GoalOr` appends solutions from alternatives.

This produces a strict list of all solutions. It is readable, but it can retain
large search results and diverge on recursive rule sets. A production logic
engine would need a demand-driven result stream, search strategy, depth or work
budget, and perhaps cycle detection. The current example makes none of those
claims.

## Keep IO at the edge

`main.sv` reads `knowledge.scry`, parses it once, and enters `repl`.
Commands are handled before query parsing. `handleQuery` is the small effect
boundary: parse a line, solve pure data, render, print.

This makes parser and solver functions independently testable. The integration
test supplies stdin rather than automating a terminal.

## Experiments

1. Add a new fact to `knowledge.scry` and predict answer order.
2. Add malformed parentheses and trace the exact stage that returns `Nothing`.
3. Add a new `Goal` constructor and let exhaustiveness diagnostics enumerate
   parser, solver, pretty-printer, and variable-collection work.
4. Replace the hand parser with `std.parser` while preserving the domain AST.
5. Design a depth budget that returns a typed “search exhausted” outcome.

## Review checklist

- Does each representation contain only facts known at that stage?
- Are syntax failure and “no logical solution” different values?
- Are rule variables fresh on every application?
- Is search order part of observable behavior?
- What bounds parsing, recursion, and retained solutions?
