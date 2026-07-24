# Scry

Scry is a tiny S-expression rule language implemented in Solveig. It is a
mini-Prolog-style example: facts and rules live in `knowledge.scry`, and the
program runs an interactive query prompt.

Run it from the repository root:

```bash
cargo run -- --project-root examples/scry
```

Or feed the sample session:

```bash
cargo run -- --project-root examples/scry < examples/scry/sample-input.txt
```

## Language

Facts are predicate calls wrapped in `fact`:

```lisp
(fact (parent alice bob))
(fact (likes alice logic))
```

Rules have a head and a goal body:

```lisp
(rule (grandparent ?x ?z)
  (and
    (parent ?x ?y)
    (parent ?y ?z)))
```

Variables start with `?`. Atoms are plain words. Predicate arguments are atoms
or variables; Scry deliberately omits nested terms to keep the example compact.

Goals can be:

```lisp
(parent alice ?who)
(and (parent ?x ?y) (parent ?y ?z))
(or (parent ?x ?z) (ancestor ?x ?z))
```

At the prompt:

```text
scry> (parent alice bob)
true
scry> (grandparent alice ?who)
?who = clara
scry> (ancestor alice ?who)
?who = bob
?who = faye
?who = clara
?who = dima
```

Commands:

```text
:help   show prompt help
:rules  print loaded facts and rules
:quit   exit
```

## Solveig Features Exercised

- multi-module Solveig projects
- ADTs for S-expressions, terms, goals, and clauses
- recursive parsing from `String` through `[Char]`
- monadic IO with `do`
- recursive rule solving
- list-backed nondeterministic search
- `Maybe` for parse failure
- user-defined substitutions and unification
- `Show`, string operations, and file IO
