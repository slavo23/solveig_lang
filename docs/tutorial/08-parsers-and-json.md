# 8. Parsers and JSON

`std.parser` is a generic parser-combinator library. It is generic in three
directions:

```solveig
Parser input customError value
```

- `input` is the cursor representation;
- `customError` is the grammar's domain-specific error type;
- `value` is the successful result.

The supplied adapters cover text, bytes, lists of tokens, and feedable token
streams. A custom checkpointable input can use the same combinators by defining
`InputOps input token`.

## A first text parser

This parser accepts one or more lowercase ASCII letters:

```solveig
import std.parser(*)
import std.text(*)

isLower : Char -> Bool
isLower character = and (character >= 'a') (character <= 'z')

word : Parser TextInput () String
word =
    mapResult
        std.text.charsToString
        (some textInputOps (satisfy textInputOps "lowercase letter" isLower))
```

Run it against complete text with the text facade:

```solveig
reply : ParseReply TextInput () String
reply = parseText word "solveig"
```

`parseText`, `parseBytes`, and `parseTokenList` are complete-input facades for
the supplied cursor types. `runComplete` is the general form for a custom
`InputOps`; `runParser` permits a successful prefix and returns the remaining
cursor.

## Understand replies

Parsing produces one of three reply shapes:

- `Parsed consumption value rest` succeeded;
- `Rejected consumption rest error` failed;
- `Awaiting consumption suspended continuation` needs another input chunk.

`Consumption` is `EmptyInput` or `ConsumedInput`. Choice may try its right
branch only when the left branch failed without consuming or committing. This
rule prevents surprising backtracking after a parser has clearly selected a
grammar branch.

Use `foldReply` when application code does not need to pattern match reply
constructors directly:

```solveig
describeReply : ParseReply TextInput () String -> String
describeReply replyValue =
    foldReply
        (fun _ value _ => "parsed: " <> value)
        (fun _ _ _ => "rejected")
        (fun _ _ _ => "more input required")
        replyValue
```

Errors retain a position, unexpected token, deterministic expected set,
grammar contexts, caller-defined failures, and an optional structural reason.

## Combine parsers

The core vocabulary is:

- `satisfy`, `token`, and `tokens` consume input;
- `mapResult` transforms a result;
- `applyParser` and `andThen` compose dependent or independent steps;
- `keepLeft` and `keepRight` discard delimiter results;
- `choice` selects an alternative;
- `many`, `some`, `optional`, and `between` express repetition and structure;
- `label` or `<?>` improves the expected item;
- `customFailure` attaches a domain error;
- `defer` defines non-left-recursive recursive grammars.

`Parser input customError` has `Functor`, `Applicative`, and `Monad` instances,
so `<$>`, `<*>`, `<*`, `*>`, `>>=`, and `do` work. Prefer Applicative structure
when later parser choices do not depend on earlier values; use Monad when they
do.

## Backtracking deliberately

`attempt ops parser` changes a consumed failure into a retryable failure while
preserving input supplied during incremental parsing. Use it around the
smallest ambiguous prefix. Applying it to a large grammar can hide the point at
which the input became invalid and can repeat expensive work.

`cut parser` commits after a prefix that uniquely identifies a branch. A
committed failure is not eligible for the next `choice`. This improves both
diagnostics and predictability.

Left-recursive grammars still need to be rewritten, usually as an atom followed
by repeated operator-and-operand pairs.

## Token and byte inputs

For a compiler front end, feed a list of tokens:

```solveig
tokenCursor = listInput tokens
tokenOps = listInputOps showToken
```

For binary formats, use `byteInput` and `byteInputOps`; each token is an integer
byte. Text input tracks Unicode characters and line/column positions. The input
adapter owns checkpoint restoration, so parsers do not assume that input is a
`String` or even that it arrived all at once.

For an effectful scoped source, adapt each pull to a token chunk and call
`parseStream`:

```solveig
import std.stream

wordFromStream : Parser (StreamInput Char) () String
wordFromStream =
    let ops = streamInputOps show in
    mapResult charsToString
        (some ops (satisfy ops "lowercase letter" isLower))

parseChunks : forall scope e.
    std.stream.Stream scope e [Char] ->
    IO e (Either (ParseError ()) String)
parseChunks chunks =
    parseStream show wordFromStream chunks
```

The source is pulled only when the parser is awaiting input. Upstream failures
remain the stream's `IO e`; a grammar rejection is the inner `Left`. This shape
fits rank-2 file and process stream callbacks without translating a resource
error into a parse error. `StreamInput` retains all supplied tokens until parsing
ends. `attempt` and `cut` control grammar work, not storage reclamation; this is
incremental input, not a bounded-memory parser.

## Parse JSON

`std.json` parses RFC 8259 bytes and returns the generic parser reply:

```solveig
import std.bytes(*)
import std.json(*)

reply =
    parseJson
        defaultJsonLimits
        (stringToBytes "{\"ready\":true,\"items\":[1,2,3]}")
```

JSON objects use `std.map.Map String Json`; arrays use `std.vector.Vector Json`.
Object ordering is deterministic. Duplicate keys are rejected. `JsonNumber`
retains a validated decimal spelling instead of forcing every number through a
`Float`.

Unlike ordinary `readFile` and `fetch`, JSON parsing necessarily receives a
policy because the parser itself is consuming potentially adversarial nested
data. `defaultJsonLimits` is the normal choice: 16 MiB input, nesting depth 128,
1 MiB per string, one million items in one container, and 1024 bytes in one
number token. Construct `JsonLimits` explicitly only when the domain needs a
different boundary.

## Decode directly

When the full `Json` tree is unnecessary, a decoder can construct the requested
representation directly:

```solveig
import std.json(*)
import std.vector(*)

namesDecoder : Decoder () (std.vector.Vector String)
namesDecoder = arrayDecoder stringDecoder
```

Available direct building blocks include `stringDecoder`, `boolDecoder`,
`numberDecoder`, `nullDecoder`, `arrayDecoder`, `objectDecoder`, and
`nullableDecoder`. Direct decoding avoids allocating an intermediate tree for
the decoded values.

Heterogeneous objects use applicative `FieldDecoder` values:

```solveig
data User = User String Bool

userDecoder : Decoder () User
userDecoder =
    recordDecoder
        (User <$> requiredField "name" stringDecoder <*>
            optionalFieldWithDefault "active" True boolDecoder)
```

`requiredField`, `optionalField`, and `optionalFieldWithDefault` distinguish a
missing field from a present `null` or malformed value. Nested failures render
paths such as `$["users"][2]["name"]`. Unknown object fields are ignored.
`taggedDecoder tagName variants` chooses a record-shaped variant by a required
string tag and reports missing, mistyped, and unknown tags at the tag path.

The first parameter of `Decoder e a` is the application's validation error.
Use `()` when there is none, or add a typed check:

```solveig
data NameError = EmptyName

nonemptyName : String -> Either NameError String
nonemptyName "" = Left EmptyName
nonemptyName name = Right name

nameDecoder : Decoder NameError String
nameDecoder = validateDecoder nonemptyName stringDecoder
```

`decodeJson` keeps syntax positions and expected tokens in `ParseError`.
Its custom errors are `DecodeError e`: `JsonDecodingError` wraps JSON failures;
`CustomDecodeError path e` preserves validation data without requiring `Show e`.
Fields, arrays, and objects preserve that path in both direct and value decoding.
`mapDecoderError` changes only the application's error type. Validation runs
after successful decoding and never silently chooses a default. For human
output, provide `Show NameError`; `show` then handles the complete parse error.

Encoding is symmetric and explicit. `Encoder` has string, Boolean, exact
number, null, array, object, nullable, and raw-JSON encoders.
`requiredFieldEncoder` and `optionalFieldEncoder` build `recordEncoder` values;
optional `Nothing` is omitted rather than silently encoded as null.
`variantEncoder` plus `taggedEncoder` encodes tagged sums from mutually
exclusive projections. `encodeJson limits encoder value` validates and renders
in deterministic object-key order, while `encodeJsonValue` stops at `Json`.
Primitive and homogeneous decoders remain direct; heterogeneous records and
tagged variants construct one bounded tree at that boundary. `decodeJsonValue`
instead traverses an existing tree, without reapplying input-byte limits.

`jsonNumberToInt` handles fractions and exponents from the exact decimal
spelling. It returns `Just` only when the mathematical value is integral and
inside the signed `Int` range; it never rounds through `Float`.

Use `renderJson` for default rendering limits and `renderJsonWith` for an
application-specific policy. Smart constructors validate number spellings;
rendering also revalidates a directly constructed `JsonNumber`.

## Parser design checklist

- Does the grammar require complete input or accept a prefix?
- Which errors are structural, and which belong in `customError`?
- Where does a prefix uniquely select a branch?
- Is `attempt` limited to a genuinely ambiguous prefix?
- Can a repeated parser succeed without consuming input? If so, rewrite it.
- Does a recursive grammar make progress before recursion?
- Are resource limits appropriate for the input's trust level?

Next: [Packages, CLI, and editor](09-packages-and-tooling.md).
