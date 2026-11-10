# 2. Transform log data with pure functions

Log Audit has to answer a simple question: how many lines begin with `I`,
`W`, or `E`? The useful lesson is not log syntax. It is how to isolate a
deterministic transformation from files, threads, and printing.

**Skip ahead:** readers comfortable with curried functions, immutable values,
algebraic lists, and tail recursion can go directly to
[the complete scan](#trace-the-complete-scan).

## Values and signatures

The application gives every major boundary a signature:

```solveig
analyzeLog : String -> LogCounts
renderReport : AuditSummary -> String
reportLine : String -> Int -> String
```

`->` associates to the right. The last signature means:

```text
String -> (Int -> String)
```

Calling `reportLine "warnings: "` returns a function waiting for an `Int`.
Ordinary calls therefore need no commas or call parentheses:

```solveig
reportLine "warnings: " warnings
```

Function application binds more tightly than infix operators. Parentheses are
needed when an expression, rather than one atom, is an argument:

```solveig
show (warnings + errors)
```

## Primitive data used here

Log Audit uses:

- `Int` for checked signed 64-bit counts;
- `BigInt` for arbitrary-precision integers;
- `Bool` for `True` and `False`;
- `Char` for one Unicode scalar value such as `'W'` or `'\n'`;
- `String` for text;
- `()` for a successful action with no interesting value.

Solveig also has `Float` for floating-point work. Use `Int` for ordinary
bounded counts, `BigInt` where integer size is not known in advance, and
`Float` where the domain is approximate. The numeric reference, conversions,
division semantics, and instances are listed in
[the Prelude guide](../prelude.md).

Numeric literals are resolved from context. Unconstrained whole literals
default to `Int`, while an out-of-range literal defaults to `BigInt`.
`Int` arithmetic is checked: overflow and integral division by zero are
runtime defects rather than wrapping results.

## Definitions are equations

A top-level definition has a name, arguments, `=`, and an expression:

```solveig
analyzeLog : String -> LogCounts
analyzeLog contents = scanLog (stringToChars contents) True 0 0 0 0
```

Arguments are immutable bindings. “Updating a counter” means calling the next
recursive step with a new value. The old value remains valid; no variable is
assigned in place.

Multiple equations may pattern-match arguments:

```solveig
messageSuffix : String -> String
messageSuffix message = if std.text.isEmpty message then "" else ": " <> message
```

Log Audit mostly uses `case` because the scan has several pieces of state.
Both forms compile through the same pattern machinery.

## Expressions, not statements

`if` has both branches because it returns a value:

```solveig
severityName : Char -> String
severityName marker =
    if marker == 'I'
        then "info"
        else if marker == 'W' then "warning" else "other"
```

This isolated illustration is pure. Both branches must have compatible types.
Omitting `else`, or returning an `Int` from only one branch, is a type error.

`let` introduces local values:

```solveig
let nextEvents = events + 1
in scanLog rest False nextEvents infos warnings errors
```

Inside a `do` block, the layout form omits `in`:

```solveig
let reportPath = requiredPath "output/report.txt"
mapError fsToIO (std.fs.writeFile reportPath report)
```

The first form is one pure expression. The second is `do` notation syntax.

## Lists expose their shape

`stringToChars contents` produces `[Char]`. Lists are immutable and
singly-linked. Their two shapes are:

```solveig
[]
c :: rest
```

`[]` is empty. `c :: rest` has first element `c` and remaining list
`rest`. The scanner consumes one constructor at a time:

```solveig
case chars of
    [] => LogCounts events infos warnings errors
    | c :: rest => ...
```

The compiler checks pattern coverage. If a function handles `c :: rest` but
not `[]`, it reports a non-exhaustive match rather than assuming an impossible
input.

List literals are convenient for finite configuration:

```solveig
["api", "billing", "search"]
```

For library transformations, prefer `map`, `filter`, and folds where they
state the operation directly. Use explicit recursion when, as in `scanLog`,
several state values advance together and the termination argument is obvious.

## Trace the complete scan

The checked implementation is
[`scanLog`](../../examples/log_audit/src/main.sv). Its state is:

- unconsumed characters;
- whether the next character begins a line;
- total events;
- info, warning, and error counts.

At end of input it constructs the result. At the start of a line it classifies
the first character. Elsewhere it only watches for the next newline. One branch
from the real function is:

```solveig
if c == 'W'
    then scanLog rest False (events + 1) infos (warnings + 1) errors
    else if c == 'E'
            then
                scanLog
                    rest
                    False
                    (events + 1)
                    infos
                    warnings
                    (errors + 1)
            else ...
```

The recursive call is in tail position: after it returns, this function has no
more work. Solveig's runtime executes this application using the language's
normal call machinery; the important source-level property is bounded state,
not a claim that every recursion pattern is automatically converted into a
loop.

Try tracing `"W cache miss\nI recovered\n"`:

1. `W` at line start increments events and warnings.
2. Characters through `\n` leave counters unchanged.
3. The newline makes the next position a line start.
4. `I` increments events and info.
5. End of input returns `LogCounts 2 1 1 0`.

Blank lines do not count as events. An unrecognized nonblank first character
counts as an event but not as one of the three named severities. These are
application semantics visible in the branches, not behavior supplied by a
library.

## Build text deliberately

The application uses both `appendString` and the `(<>)` operator. They are
equivalent ways to concatenate strings:

```solveig
reportLine label value = appendString label (appendString (show value) "\n")
```

A shorter equivalent isolated definition is:

```solveig
reportLine label value = label <> show value <> "\n"
```

`show` is explicit. Solveig does not interpolate arbitrary values into
strings or coerce them at concatenation sites.

Use `($)` to remove a final pair of parentheses:

```solveig
printLine $ show result
```

Use `(.)` when the intermediate value is genuinely irrelevant:

```solveig
printResult : AuditSummary -> Task ()
printResult = printLine . renderReport
```

The second definition is an isolated equivalent, not part of Log Audit. Do not
force point-free style across branching or error handling; names are cheaper
than hidden control flow.

## Tuples for temporary grouping

Tuples group a fixed number of values without defining a domain type:

```solveig
("warnings", 3)
(elapsed, result)
```

Pattern matching takes them apart:

```solveig
case measured of
    (elapsed, result) => ...
```

Solveig supports tuple arities two through eight. Log counts are not a tuple
because their positions have domain meaning; the next chapter gives that state
a constructor.

## A useful deliberate error

Change the base case to:

```solveig
[] => (events, warnings)
```

The branch now returns a tuple, while the other path recursively returns
`LogCounts`. The compiler reports that the alternatives do not agree. The
repair is not a cast: both branches must describe the same result model.

## Checkpoint

Run Log Audit:

```sh
cargo run -- --project-root examples/log_audit
```

Then inspect the five input logs and predict the six report numbers. Verify the
prediction against the output. This checks that you understand the pure scan,
not only how to invoke the executable.

## Exercises

1. In a scratch copy, recognize a `D` marker as an event without adding a new
   field. Which count changes?
2. Write a pure `totalProblems : LogCounts -> Int` after reading the
   constructor syntax in the next chapter.
3. Explain why `String -> Int -> String` permits partial application.
4. Replace one nested `appendString` expression with `(<>)`, format the
   package, and compare readability.

## Recap

- Function application uses spaces and is curried.
- Bindings are immutable; recursion passes new state.
- `if`, `case`, and `let` are expressions.
- Lists are built from `[]` and `(::)`; patterns expose those constructors.
- Domain values deserve named types; tuples are best for temporary grouping.
- `show`, `(.)`, and `($)` are explicit tools, not implicit magic.

Next: [Model valid states with data and patterns](03-data-and-patterns.md).
