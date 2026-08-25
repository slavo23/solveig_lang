export (
    Position, positionOffset, positionLine, positionColumn, Consumption(..),
    InputStep(..), InputOps(..), nextInput, inputPosition, restoreInput,
    describeInputToken, ParseError, errorPosition, errorUnexpected, errorExpected,
    errorContexts, errorCustom, errorReason, ParseReply(..), Parser, foldReply, runParser,
    resume, runComplete, succeed, mapResult, andThen, applyParser, pair, keepRight, keepLeft,
    choice, attempt, cut, label, (<?>), context, expected, customFailure, defer,
    anyToken, satisfy, token, tokens, tokensExact, endOfInput, manyFold, many, some,
    skipMany, optional, between, separatedBy, ListInput, listInput, listInputOps,
    TextInput, textInput, textInputOps, ByteInput, byteInput, byteInputOps, StreamInput,
    streamInput, streamInputOps, feedStream, finishStream, supply, finishReply,
    renderError
)

import std.bytes(*)
import std.list(*)
import std.set(*)
import std.text(*)
import std.vector(*)

infix 0 <?>

-- | A parser position with a zero-based token offset and one-based line and column.
-- |
-- | Custom inputs may use line 1 and offset-plus-one as the column when their
-- | tokens do not originate in text.
data Position = Position Int Int Int

-- | Returns the zero-based token offset of a position.
positionOffset : Position -> Int
positionOffset (Position offset _ _) = offset

-- | Returns the one-based line of a position.
positionLine : Position -> Int
positionLine (Position _ line _) = line

-- | Returns the one-based column of a position.
positionColumn : Position -> Int
positionColumn (Position _ _ column) = column

-- | Records whether a parser consumed input or deliberately committed.
-- |
-- | Choice may try its right branch only after an EmptyInput failure.
data Consumption = EmptyInput | ConsumedInput

-- | Combines consumption from two sequential parser steps.
mergeConsumption : Consumption -> Consumption -> Consumption
mergeConsumption EmptyInput next = next
mergeConsumption ConsumedInput _ = ConsumedInput

-- | Describes one observation made by an input adapter.
data InputStep input token =
    -- | Supplies one token and the cursor after that token.
    InputToken token input
  | -- | Reports that a complete input has ended.
    InputEnd
  | -- | Reports that a partial input needs another externally supplied chunk.
    InputAwait

-- | Supplies the operations that let a parser consume an arbitrary input type.
-- |
-- | The restore function receives an old checkpoint and the newest cursor. It
-- | must retain newly supplied chunks while moving back to the checkpoint.
-- | Destructive inputs therefore need a buffering adapter when backtracking is
-- | possible.
data InputOps input token = InputOps
    (input -> InputStep input token)
    (input -> Position)
    (input -> input -> input)
    (token -> String)

-- | Reads the next token observation through an input adapter.
nextInput : forall input token. InputOps input token -> input -> InputStep input token
nextInput (InputOps next _ _ _) input = next input

-- | Returns the current position through an input adapter.
inputPosition : forall input token. InputOps input token -> input -> Position
inputPosition (InputOps _ position _ _) input = position input

-- | Restores a checkpoint against the newest version of an input source.
restoreInput : forall input token. InputOps input token -> input -> input -> input
restoreInput (InputOps _ _ restore _) checkpoint newest = restore checkpoint newest

-- | Renders one input token for an unexpected-input diagnostic.
describeInputToken : forall input token. InputOps input token -> token -> String
describeInputToken (InputOps _ _ _ describe) token = describe token

-- | A structured positioned error extended with caller-defined failures.
-- |
-- | Expected items are stored in deterministic order. Contexts run from the
-- | nearest labeled grammar region outward. A structural reason describes a
-- | parser contract failure such as repeating a parser that accepted no input.
data ParseError custom = ParseError
    Position
    (Maybe String)
    (std.set.Set String)
    [String]
    [custom]
    (Maybe String)

-- | Returns the position at which a parse error was reported.
errorPosition : forall custom. ParseError custom -> Position
errorPosition (ParseError position _ _ _ _ _) = position

-- | Returns the unexpected token description when one was observed.
errorUnexpected : forall custom. ParseError custom -> Maybe String
errorUnexpected (ParseError _ unexpected _ _ _ _) = unexpected

-- | Returns the deterministic set of items expected at the error position.
errorExpected : forall custom. ParseError custom -> std.set.Set String
errorExpected (ParseError _ _ expected _ _ _) = expected

-- | Returns grammar contexts from the nearest region outward.
errorContexts : forall custom. ParseError custom -> [String]
errorContexts (ParseError _ _ _ contexts _ _) = contexts

-- | Returns caller-defined failures attached at the error position.
errorCustom : forall custom. ParseError custom -> [custom]
errorCustom (ParseError _ _ _ _ custom _) = custom

-- | Returns a structural parser reason when the grammar contract failed.
errorReason : forall custom. ParseError custom -> Maybe String
errorReason (ParseError _ _ _ _ _ reason) = reason

-- | Builds an empty structured error at the current input position.
emptyError : forall input token custom. InputOps input token -> input -> ParseError custom
emptyError ops input =
    ParseError (inputPosition ops input) Nothing std.set.empty [] [] Nothing

-- | Chooses the furthest error, merging deterministic details at equal offsets.
-- |
-- | This is the positioned error merge from Leijen and Meijer, Parsec (2001).
mergeErrors : forall custom. ParseError custom -> ParseError custom -> ParseError custom
mergeErrors left right =
    let leftOffset = positionOffset (errorPosition left) in
    let rightOffset = positionOffset (errorPosition right) in
    if leftOffset > rightOffset then left
    else if rightOffset > leftOffset then right
    else mergeErrorsAtSamePosition left right

-- | Merges two errors already known to occupy the same position.
mergeErrorsAtSamePosition : forall custom. ParseError custom -> ParseError custom -> ParseError custom
mergeErrorsAtSamePosition
    (ParseError position unexpectedLeft expectedLeft contextsLeft customLeft reasonLeft)
    (ParseError _ unexpectedRight expectedRight contextsRight customRight reasonRight) =
        ParseError
            position
            (preferMaybe unexpectedLeft unexpectedRight)
            (std.set.union expectedLeft expectedRight)
            (preferList contextsLeft contextsRight)
            (customLeft ++ customRight)
            (preferMaybe reasonLeft reasonRight)

-- | Keeps the first present optional value, otherwise returning the second.
preferMaybe : forall a. Maybe a -> Maybe a -> Maybe a
preferMaybe (Just value) _ = Just value
preferMaybe Nothing fallback = fallback

-- | Keeps the first non-empty list, otherwise returning the second.
preferList : forall a. [a] -> [a] -> [a]
preferList [] fallback = fallback
preferList values _ = values

-- | One parser result, including resumable partial-input suspension.
-- |
-- | Rejected retains the newest cursor so attempt and choice can restore while
-- | preserving chunks supplied after parsing began. Awaiting exposes the exact
-- | suspended cursor that must be extended before its continuation is resumed.
data ParseReply input custom value =
    -- | Reports a parsed value and the unconsumed cursor.
    Parsed Consumption value input
  | -- | Reports a structured failure and the cursor at that failure.
    Rejected Consumption input (ParseError custom)
  | -- | Requests more input and retains a pure resumption continuation.
    Awaiting Consumption input (input -> ParseReply input custom value)

-- | A compositional parser over an input type with caller-defined error values.
-- |
-- | The constructor exposes the minimal functional representation needed by
-- | user-defined primitives. Ordinary grammars should prefer the documented
-- | combinators so consumption and streaming behavior stay predictable.
data Parser input custom value = Parser (input -> ParseReply input custom value)

-- | Eliminates a parser reply through success, failure, and awaiting callbacks.
-- |
-- | This keeps ordinary clients independent of reply constructor patterns while
-- | still exposing the suspended cursor and continuation needed by custom
-- | streaming integrations.
foldReply : forall input custom value result.
    (Consumption -> value -> input -> result) ->
    (Consumption -> input -> ParseError custom -> result) ->
    (Consumption -> input -> (input -> ParseReply input custom value) -> result) ->
    ParseReply input custom value ->
    result
foldReply onSuccess onFailure onAwait (Parsed consumption value rest) =
    onSuccess consumption value rest
foldReply onSuccess onFailure onAwait (Rejected consumption rest error) =
    onFailure consumption rest error
foldReply onSuccess onFailure onAwait (Awaiting consumption suspended continue) =
    onAwait consumption suspended continue

-- | Runs a parser from a supplied input cursor without requiring end-of-input.
runParser : forall input custom value. Parser input custom value -> input -> ParseReply input custom value
runParser (Parser run) input = run input

-- | Resumes an awaiting reply with an extended version of its suspended input.
-- |
-- | A reply that has already succeeded or failed is returned unchanged.
resume : forall input custom value. ParseReply input custom value -> input -> ParseReply input custom value
resume (Awaiting _ _ continue) input = continue input
resume (Parsed consumption value rest) _ = Parsed consumption value rest
resume (Rejected consumption rest error) _ = Rejected consumption rest error

-- | Requires a parser to consume a complete input and then reach final end.
-- |
-- | A partial input still returns Awaiting until its adapter is marked finished.
-- | For text, a typical entry point is
-- | `runComplete textInputOps parser (textInput source)`.
runComplete : forall input token custom value. InputOps input token -> Parser input custom value -> input -> ParseReply input custom value
runComplete ops parser input = runParser (keepLeft parser (endOfInput ops)) input

-- | Produces a value without consuming input.
succeed : forall input custom value. value -> Parser input custom value
succeed value = Parser (fun input => Parsed EmptyInput value input)

-- | Transforms a successful parser result without changing input behavior.
mapResult : forall input custom a b. (a -> b) -> Parser input custom a -> Parser input custom b
mapResult transform parser = Parser (fun input => mapReply transform (runParser parser input))

-- | Transforms every eventual success of a parser reply, including after suspension.
mapReply : forall input custom a b. (a -> b) -> ParseReply input custom a -> ParseReply input custom b
mapReply transform (Parsed consumption value rest) =
    Parsed consumption (transform value) rest
mapReply transform (Rejected consumption rest error) =
    Rejected consumption rest error
mapReply transform (Awaiting consumption suspended continue) =
    Awaiting consumption suspended (fun input => mapReply transform (continue input))

-- | Sequences a parser with a value-dependent continuation.
-- |
-- | Bind permits dynamic grammars but hides future parser structure. Prefer
-- | mapping, application, and fixed sequencing when the grammar is static.
-- | Use `andThen identifier (fun name => parserForName name)` when the value
-- | parsed on the left genuinely determines the grammar on the right.
andThen : forall input custom a b. Parser input custom a -> (a -> Parser input custom b) -> Parser input custom b
andThen parser continuation =
    Parser (fun input => bindReply continuation (runParser parser input))

-- | Continues an eventual successful reply through a value-dependent parser.
bindReply : forall input custom a b. (a -> Parser input custom b) -> ParseReply input custom a -> ParseReply input custom b
bindReply continuation (Parsed consumption value rest) =
    addConsumption consumption (runParser (continuation value) rest)
bindReply continuation (Rejected consumption rest error) =
    Rejected consumption rest error
bindReply continuation (Awaiting consumption suspended continue) =
    Awaiting consumption suspended (fun input => bindReply continuation (continue input))

-- | Applies a parser-produced function to a parser-produced argument.
applyParser : forall input custom a b. Parser input custom (a -> b) -> Parser input custom a -> Parser input custom b
applyParser parserFunction parserArgument =
    andThen parserFunction (fun transform => mapResult transform parserArgument)

-- | Adds earlier consumption to every eventual outcome of a later parser.
addConsumption : forall input custom value. Consumption -> ParseReply input custom value -> ParseReply input custom value
addConsumption previous (Parsed current value rest) =
    Parsed (mergeConsumption previous current) value rest
addConsumption previous (Rejected current rest error) =
    Rejected (mergeConsumption previous current) rest error
addConsumption previous (Awaiting current suspended continue) =
    Awaiting
        (mergeConsumption previous current)
        suspended
        (fun input => addConsumption previous (continue input))

-- | Sequences two parsers and keeps both results in a pair.
pair : forall input custom a b. Parser input custom a -> Parser input custom b -> Parser input custom (a, b)
pair left right = andThen left (fun a => mapResult (fun b => (a, b)) right)

-- | Sequences two parsers and keeps only the result of the second.
keepRight : forall input custom a b. Parser input custom a -> Parser input custom b -> Parser input custom b
keepRight left right = andThen left (fun _ => right)

-- | Sequences two parsers and keeps only the result of the first.
keepLeft : forall input custom a b. Parser input custom a -> Parser input custom b -> Parser input custom a
keepLeft left right = andThen left (fun value => mapResult (fun _ => value) right)

-- | Tries the right parser only when the left parser fails without consuming.
-- |
-- | Empty failures at the same position merge their expected items. Wrap the
-- | left parser in attempt only when its consumed prefix is genuinely shared.
-- | For example, alternatives for `"ab"` and `"ac"` need attempt around the
-- | first token sequence; alternatives starting with different tokens do not.
choice : forall input token custom value. InputOps input token -> Parser input custom value -> Parser input custom value -> Parser input custom value
choice ops left right =
    Parser (fun input => chooseLeft ops input right (runParser left input))

-- | Resolves the left reply of a choice, preserving its starting checkpoint.
chooseLeft : forall input token custom value. InputOps input token -> input -> Parser input custom value -> ParseReply input custom value -> ParseReply input custom value
chooseLeft ops checkpoint right (Rejected EmptyInput newest firstError) =
    let restored = restoreInput ops checkpoint newest in
    chooseRight firstError (runParser right restored)
chooseLeft ops checkpoint right (Awaiting consumption suspended continue) =
    Awaiting consumption suspended
        (fun input => chooseLeft ops checkpoint right (continue input))
chooseLeft _ _ _ (Parsed consumption value rest) = Parsed consumption value rest
chooseLeft _ _ _ (Rejected ConsumedInput rest error) =
    Rejected ConsumedInput rest error

-- | Merges an empty right-branch failure with the preceding left failure.
chooseRight : forall input custom value. ParseError custom -> ParseReply input custom value -> ParseReply input custom value
chooseRight firstError (Rejected EmptyInput rest secondError) =
    Rejected EmptyInput rest (mergeErrors firstError secondError)
chooseRight firstError (Awaiting consumption suspended continue) =
    Awaiting consumption suspended (fun input => chooseRight firstError (continue input))
chooseRight _ (Parsed consumption value rest) = Parsed consumption value rest
chooseRight _ (Rejected ConsumedInput rest error) = Rejected ConsumedInput rest error

-- | Makes a failing parser restore its starting checkpoint and report no consumption.
-- |
-- | Success retains its real consumption. On a partial input, restoration uses
-- | the newest supplied source so chunks received while suspended are not lost.
-- | Keep attempt as narrow as possible: wrap only the ambiguous prefix branch,
-- | then let later failures remain committed and informative.
attempt : forall input token custom value. InputOps input token -> Parser input custom value -> Parser input custom value
attempt ops parser =
    Parser (fun input => attemptReply ops input (runParser parser input))

-- | Restores every eventual failure of an attempted parser.
attemptReply : forall input token custom value. InputOps input token -> input -> ParseReply input custom value -> ParseReply input custom value
attemptReply ops checkpoint (Rejected _ newest error) =
    Rejected EmptyInput (restoreInput ops checkpoint newest) error
attemptReply ops checkpoint (Awaiting consumption suspended continue) =
    Awaiting consumption suspended
        (fun input => attemptReply ops checkpoint (continue input))
attemptReply _ _ (Parsed consumption value rest) = Parsed consumption value rest

-- | Commits every eventual failure of a parser, preventing later alternatives.
-- |
-- | Cut does not alter successful consumption and is normally placed after a
-- | prefix that uniquely identifies a grammar branch.
-- | A declaration grammar can parse its keyword, then cut the declaration body
-- | so malformed declarations are not reinterpreted as another construct.
cut : forall input custom value. Parser input custom value -> Parser input custom value
cut parser = Parser (fun input => cutReply (runParser parser input))

-- | Marks every eventual failure reply as committed.
cutReply : forall input custom value. ParseReply input custom value -> ParseReply input custom value
cutReply (Rejected _ rest error) = Rejected ConsumedInput rest error
cutReply (Awaiting consumption suspended continue) =
    Awaiting consumption suspended (fun input => cutReply (continue input))
cutReply (Parsed consumption value rest) = Parsed consumption value rest

-- | Replaces the expected set when a parser fails without consuming input.
label : forall input custom value. String -> Parser input custom value -> Parser input custom value
label expected parser = Parser (fun input => labelReply expected (runParser parser input))

-- | Infix form of label, following the grammar-reading order `parser <?> name`.
-- |
-- | Like label, this changes only an empty failure. A consumed or cut failure
-- | keeps its more specific diagnostic. The operator has precedence zero so a
-- | complete parser expression normally appears on its left without brackets.
(<?>) : forall input custom value. Parser input custom value -> String -> Parser input custom value
(<?>) parser expectedName = label expectedName parser

-- | Labels every eventual empty failure while retaining committed diagnostics.
labelReply : forall input custom value. String -> ParseReply input custom value -> ParseReply input custom value
labelReply expected (Rejected EmptyInput rest (ParseError position unexpected _ contexts custom reason)) =
    Rejected EmptyInput rest
        (ParseError position unexpected (std.set.singleton expected) contexts custom reason)
labelReply expected (Awaiting consumption suspended continue) =
    Awaiting consumption suspended (fun input => labelReply expected (continue input))
labelReply _ (Parsed consumption value rest) = Parsed consumption value rest
labelReply _ (Rejected ConsumedInput rest error) = Rejected ConsumedInput rest error

-- | Adds a grammar context to every eventual failure.
context : forall input custom value. String -> Parser input custom value -> Parser input custom value
context name parser = Parser (fun input => contextReply name (runParser parser input))

-- | Adds context to a reply without changing its consumption behavior.
contextReply : forall input custom value. String -> ParseReply input custom value -> ParseReply input custom value
contextReply name (Rejected consumption rest (ParseError position unexpected expected contexts custom reason)) =
    Rejected consumption rest
        (ParseError position unexpected expected (name :: contexts) custom reason)
contextReply name (Awaiting consumption suspended continue) =
    Awaiting consumption suspended (fun input => contextReply name (continue input))
contextReply _ (Parsed consumption value rest) = Parsed consumption value rest

-- | Fails without consuming input and records one expected item.
expected : forall input token custom value. InputOps input token -> String -> Parser input custom value
expected ops item =
    Parser (fun input =>
        case emptyError ops input of
            ParseError position unexpected _ contexts custom reason =>
                Rejected EmptyInput input
                    (ParseError position unexpected (std.set.singleton item) contexts custom reason))

-- | Fails without consuming input and attaches a caller-defined error value.
-- |
-- | Use this after parsing enough input to detect a domain rule, such as a
-- | reserved identifier. Surrounding sequencing determines whether it commits.
customFailure : forall input token custom value. InputOps input token -> custom -> Parser input custom value
customFailure ops customValue =
    Parser (fun input =>
        Rejected EmptyInput input
            (ParseError
                (inputPosition ops input)
                Nothing
                std.set.empty
                []
                (customValue :: [])
                Nothing))

-- | Delays construction of a recursive parser until the parser is run.
-- |
-- | This is the recursion facility for Solveig's strict evaluation. Grammars
-- | must still remove direct left recursion because defer does not memoize.
-- | Recursive list or expression grammars normally place
-- | `defer (fun _ => recursiveParser ops)` at the recursive occurrence.
defer : forall input custom value. (() -> Parser input custom value) -> Parser input custom value
defer makeParser = Parser (fun input => runParser (makeParser ()) input)

-- | Parses any next token, consuming exactly one token on success.
-- |
-- | Final input fails empty. Partial input suspends rather than pretending that
-- | the stream has ended.
anyToken : forall input token custom. InputOps input token -> Parser input custom token
anyToken ops = satisfy ops "token" (fun _ => True)

-- | Parses the next token when it satisfies a predicate.
-- |
-- | A mismatching token is observed but not consumed, allowing ordinary choice
-- | to try another branch from the same cursor.
satisfy : forall input token custom. InputOps input token -> String -> (token -> Bool) -> Parser input custom token
satisfy ops expectation predicate =
    Parser (fun input => satisfyStep ops expectation predicate input (nextInput ops input))

-- | Converts one input observation into a satisfying-token parser reply.
satisfyStep : forall input token custom. InputOps input token -> String -> (token -> Bool) -> input -> InputStep input token -> ParseReply input custom token
satisfyStep ops expectation predicate input (InputToken token rest) =
    if predicate token then Parsed ConsumedInput token rest
    else
        case emptyError ops input of
            ParseError position _ _ contexts custom reason =>
                Rejected EmptyInput input
                    (ParseError
                        position
                        (Just (describeInputToken ops token))
                        (std.set.singleton expectation)
                        contexts
                        custom
                        reason)
satisfyStep ops expectation predicate input InputAwait =
    Awaiting EmptyInput input
        (fun extended => satisfyStep ops expectation predicate extended (nextInput ops extended))
satisfyStep ops expectation predicate input InputEnd =
    case emptyError ops input of
        ParseError position _ _ contexts custom reason =>
            Rejected EmptyInput input
                (ParseError position (Just "end of input") (std.set.singleton expectation) contexts custom reason)

-- | Parses one exact token, using the supplied name in diagnostics.
token : forall input token custom. Eq token => InputOps input token -> String -> token -> Parser input custom token
token ops name wanted = satisfy ops name (fun actual => actual == wanted)

-- | Parses an exact token sequence from left to right.
-- |
-- | A later mismatch is consumed because the shared prefix was consumed. Use
-- | attempt at the choice site when another branch shares that prefix.
tokens : forall input token custom. Eq token => InputOps input token -> String -> [token] -> Parser input custom [token]
tokens ops name wanted = label name (tokensExact ops wanted)

-- | Parses an exact token list without changing its expected label.
tokensExact : forall input token custom. Eq token => InputOps input token -> [token] -> Parser input custom [token]
tokensExact ops [] = succeed []
tokensExact ops (wanted :: rest) =
    andThen (token ops (describeInputToken ops wanted) wanted) (fun value =>
        mapResult (fun values => value :: values) (tokensExact ops rest))

-- | Succeeds only at final end-of-input without consuming anything.
-- |
-- | An unfinished partial input suspends until more input arrives or the input
-- | adapter is marked complete.
endOfInput : forall input token custom. InputOps input token -> Parser input custom ()
endOfInput ops =
    Parser (fun input => endStep ops input (nextInput ops input))

-- | Converts one input observation into an end-of-input reply.
endStep : forall input token custom. InputOps input token -> input -> InputStep input token -> ParseReply input custom ()
endStep ops input InputEnd = Parsed EmptyInput () input
endStep ops input InputAwait =
    Awaiting EmptyInput input (fun extended => endStep ops extended (nextInput ops extended))
endStep ops input (InputToken token _) =
    case emptyError ops input of
        ParseError position _ _ contexts custom reason =>
            Rejected EmptyInput input
                (ParseError
                    position
                    (Just (describeInputToken ops token))
                    (std.set.singleton "end of input")
                    contexts
                    custom
                    reason)

-- | Parses zero or more values and folds them without constructing a result list.
-- |
-- | An empty failure ends the repetition. A consumed failure propagates. A
-- | successful iteration that consumed nothing becomes a committed structural
-- | error instead of looping forever.
manyFold : forall input token custom value accumulator. InputOps input token -> Parser input custom value -> accumulator -> (accumulator -> value -> accumulator) -> Parser input custom accumulator
manyFold ops parser initial combine =
    Parser (fun input => manyFoldLoop ops parser combine EmptyInput initial input)

-- | Runs the next iteration of a folding repetition.
manyFoldLoop : forall input token custom value accumulator. InputOps input token -> Parser input custom value -> (accumulator -> value -> accumulator) -> Consumption -> accumulator -> input -> ParseReply input custom accumulator
manyFoldLoop ops parser combine consumed accumulator input =
    manyFoldReply ops parser combine consumed accumulator (runParser parser input)

-- | Resolves one eventual repetition result and continues when progress was made.
manyFoldReply : forall input token custom value accumulator. InputOps input token -> Parser input custom value -> (accumulator -> value -> accumulator) -> Consumption -> accumulator -> ParseReply input custom value -> ParseReply input custom accumulator
manyFoldReply ops parser combine consumed accumulator (Parsed EmptyInput _ rest) =
    case emptyError ops rest of
        ParseError position unexpected expectedItems contexts custom _ =>
            Rejected ConsumedInput rest
                (ParseError
                    position
                    unexpected
                    expectedItems
                    contexts
                    custom
                    (Just "repeated parser succeeded without consuming input"))
manyFoldReply ops parser combine consumed accumulator (Parsed ConsumedInput value rest) =
    manyFoldLoop ops parser combine ConsumedInput (combine accumulator value) rest
manyFoldReply _ _ _ consumed accumulator (Rejected EmptyInput rest _) =
    Parsed consumed accumulator rest
manyFoldReply _ _ _ consumed _ (Rejected ConsumedInput rest error) =
    Rejected (mergeConsumption consumed ConsumedInput) rest error
manyFoldReply ops parser combine consumed accumulator (Awaiting current suspended continue) =
    Awaiting
        (mergeConsumption consumed current)
        suspended
        (fun input => manyFoldReply ops parser combine consumed accumulator (continue input))

-- | Parses zero or more values into a source-ordered list.
many : forall input token custom value. InputOps input token -> Parser input custom value -> Parser input custom [value]
many ops parser =
    mapResult std.list.reverse
        (manyFold ops parser [] (fun values value => value :: values))

-- | Parses one or more values into a source-ordered list.
some : forall input token custom value. InputOps input token -> Parser input custom value -> Parser input custom [value]
some ops parser =
    andThen parser (fun first =>
        mapResult (fun rest => first :: rest) (many ops parser))

-- | Repeats a parser while discarding every successful result.
skipMany : forall input token custom value. InputOps input token -> Parser input custom value -> Parser input custom ()
skipMany ops parser = manyFold ops parser () (fun _ _ => ())

-- | Parses an optional value, returning Nothing only after an empty failure.
optional : forall input token custom value. InputOps input token -> Parser input custom value -> Parser input custom (Maybe value)
optional ops parser = choice ops (mapResult Just parser) (succeed Nothing)

-- | Parses a value surrounded by opening and closing parsers.
between : forall input custom open close value. Parser input custom open -> Parser input custom close -> Parser input custom value -> Parser input custom value
between opening closing parser = keepRight opening (keepLeft parser closing)

-- | Parses zero or more values separated by a separator parser.
-- |
-- | A consumed separator followed by a missing value is an error, so trailing
-- | separators are rejected unless the element grammar handles them explicitly.
separatedBy : forall input token custom value separator. InputOps input token -> Parser input custom value -> Parser input custom separator -> Parser input custom [value]
separatedBy ops parser separator =
    choice ops
        (andThen parser (fun first =>
            mapResult (fun rest => first :: rest)
                (many ops (keepRight separator parser))))
        (succeed [])

-- | A complete persistent cursor over a list of tokens.
data ListInput token = ListInput Int [token]

-- | Creates a complete list cursor at token offset zero.
listInput : forall token. [token] -> ListInput token
listInput values = ListInput 0 values

-- | Creates operations for complete lists using a caller-supplied token description.
listInputOps : forall token. (token -> String) -> InputOps (ListInput token) token
listInputOps describe = InputOps listNext listPosition listRestore describe

-- | Advances a complete list cursor by one token.
listNext : forall token. ListInput token -> InputStep (ListInput token) token
listNext (ListInput _ []) = InputEnd
listNext (ListInput offset (value :: rest)) =
    InputToken value (ListInput (offset + 1) rest)

-- | Reports a list cursor position as a one-line token sequence.
listPosition : forall token. ListInput token -> Position
listPosition (ListInput offset _) = Position offset 1 (offset + 1)

-- | Restores a complete list checkpoint, whose retained suffix is authoritative.
listRestore : forall token. ListInput token -> ListInput token -> ListInput token
listRestore checkpoint _ = checkpoint

-- | A complete Unicode-scalar cursor over immutable text.
data TextInput = TextInput Int Int Int [Char]

-- | Creates a complete text cursor at line 1, column 1.
-- |
-- | Offset counts Unicode scalar values; line and column are also scalar based.
textInput : String -> TextInput
textInput text = TextInput 0 1 1 (std.text.stringToChars text)

-- | Supplies Unicode text operations with quoted character diagnostics.
textInputOps : InputOps TextInput Char
textInputOps = InputOps textNext textPosition textRestore show

-- | Advances a text cursor while updating scalar-based line and column values.
textNext : TextInput -> InputStep TextInput Char
textNext (TextInput _ _ _ []) = InputEnd
textNext (TextInput offset line column (character :: rest)) =
    if character == '\n' then
        InputToken character (TextInput (offset + 1) (line + 1) 1 rest)
    else
        InputToken character (TextInput (offset + 1) line (column + 1) rest)

-- | Returns the current text position.
textPosition : TextInput -> Position
textPosition (TextInput offset line column _) = Position offset line column

-- | Restores a complete text checkpoint, whose retained suffix is authoritative.
textRestore : TextInput -> TextInput -> TextInput
textRestore checkpoint _ = checkpoint

-- | A complete indexed cursor over immutable bytes.
data ByteInput = ByteInput Bytes Int

-- | Creates a complete byte cursor at offset zero.
byteInput : Bytes -> ByteInput
byteInput bytes = ByteInput bytes 0

-- | Supplies byte input operations with decimal byte diagnostics.
byteInputOps : InputOps ByteInput Int
byteInputOps = InputOps byteNext bytePosition byteRestore show

-- | Advances a byte cursor by one byte when the offset exists.
byteNext : ByteInput -> InputStep ByteInput Int
byteNext (ByteInput bytes offset) =
    case std.bytes.bytesIndex bytes offset of
        Nothing => InputEnd
        | Just byte => InputToken byte (ByteInput bytes (offset + 1))

-- | Reports a byte cursor position as a one-line byte sequence.
bytePosition : ByteInput -> Position
bytePosition (ByteInput _ offset) = Position offset 1 (offset + 1)

-- | Restores a byte checkpoint while retaining its immutable source bytes.
byteRestore : ByteInput -> ByteInput -> ByteInput
byteRestore checkpoint _ = checkpoint

-- | A persistent feedable token cursor for pure incremental parsing.
-- |
-- | The vector retains all supplied tokens so attempt can restore checkpoints
-- | across chunk boundaries. finishStream changes an empty cursor from Await to
-- | final End without performing IO inside the parser.
data StreamInput token = StreamInput (std.vector.Vector token) Int Bool

-- | Creates an unfinished stream containing no tokens.
streamInput : forall token. StreamInput token
streamInput = StreamInput std.vector.empty 0 False

-- | Creates operations for a persistent feedable token stream.
streamInputOps : forall token. (token -> String) -> InputOps (StreamInput token) token
streamInputOps describe = InputOps streamNext streamPosition streamRestore describe

-- | Supplies a chunk to an unfinished stream, returning Nothing after finishStream.
feedStream : forall token. StreamInput token -> [token] -> Maybe (StreamInput token)
feedStream (StreamInput values offset False) chunk =
    Just (StreamInput (appendStreamValues values chunk) offset False)
feedStream (StreamInput _ _ True) _ = Nothing

-- | Appends a token chunk to a persistent stream in source order.
appendStreamValues : forall token. std.vector.Vector token -> [token] -> std.vector.Vector token
appendStreamValues values [] = values
appendStreamValues values (token :: rest) =
    appendStreamValues (std.vector.snoc values token) rest

-- | Marks a stream complete so an exhausted cursor reports final end-of-input.
finishStream : forall token. StreamInput token -> StreamInput token
finishStream (StreamInput values offset _) = StreamInput values offset True

-- | Supplies a chunk directly to an awaiting stream reply and resumes it.
-- |
-- | Completed replies are unchanged. Awaiting necessarily retains an
-- | unfinished cursor, so the feed operation succeeds by construction.
-- | Start with `runParser parser streamInput`, call supply for each available
-- | token chunk, then call finishReply when no more chunks can arrive.
supply : forall token custom value. ParseReply (StreamInput token) custom value -> [token] -> ParseReply (StreamInput token) custom value
supply (Awaiting consumption suspended continue) chunk =
    case feedStream suspended chunk of
        Just extended => continue extended
        | Nothing => Awaiting consumption suspended continue
supply (Parsed consumption value rest) _ = Parsed consumption value rest
supply (Rejected consumption rest error) _ = Rejected consumption rest error

-- | Marks an awaiting stream reply complete and resumes it at final input.
finishReply : forall token custom value. ParseReply (StreamInput token) custom value -> ParseReply (StreamInput token) custom value
finishReply (Awaiting _ suspended continue) = continue (finishStream suspended)
finishReply (Parsed consumption value rest) = Parsed consumption value rest
finishReply (Rejected consumption rest error) = Rejected consumption rest error

-- | Advances a stream cursor, awaits another chunk, or reports final end.
streamNext : forall token. StreamInput token -> InputStep (StreamInput token) token
streamNext (StreamInput values offset finished) =
    case std.vector.index values offset of
        Just token => InputToken token (StreamInput values (offset + 1) finished)
        | Nothing => if finished then InputEnd else InputAwait

-- | Reports a stream cursor position as a one-line token sequence.
streamPosition : forall token. StreamInput token -> Position
streamPosition (StreamInput _ offset _) = Position offset 1 (offset + 1)

-- | Restores an old offset against the newest persistent stream contents.
streamRestore : forall token. StreamInput token -> StreamInput token -> StreamInput token
streamRestore (StreamInput _ checkpointOffset _) (StreamInput newest _ finished) =
    StreamInput newest checkpointOffset finished

-- | Renders a structured error with deterministic expected-item ordering.
-- |
-- | The custom renderer controls only caller-defined failures; position,
-- | unexpected input, expected items, contexts, and structural reasons keep the
-- | same format for every parser.
renderError : forall custom. (custom -> String) -> ParseError custom -> String
renderError renderCustom (ParseError (Position offset line column) unexpected expectedItems contexts custom reason) =
    "line " <> show line <>
    ", column " <> show column <>
    " (offset " <> show offset <> "): " <>
    renderErrorBody unexpected (std.set.toAscList expectedItems) reason <>
    renderContexts contexts <>
    renderCustomErrors renderCustom custom

-- | Renders the structural body of a parse error.
renderErrorBody : Maybe String -> [String] -> Maybe String -> String
renderErrorBody _ _ (Just reason) = reason
renderErrorBody (Just unexpected) [] Nothing = "unexpected " <> unexpected
renderErrorBody (Just unexpected) expectedItems Nothing =
    "unexpected " <> unexpected <> "; expected " <> joinStrings ", " expectedItems
renderErrorBody Nothing [] Nothing = "parse failed"
renderErrorBody Nothing expectedItems Nothing = "expected " <> joinStrings ", " expectedItems

-- | Renders nested grammar contexts when they are present.
renderContexts : [String] -> String
renderContexts [] = ""
renderContexts contexts = "; in " <> joinStrings " inside " contexts

-- | Renders caller-defined errors after the structural diagnostic.
renderCustomErrors : forall custom. (custom -> String) -> [custom] -> String
renderCustomErrors _ [] = ""
renderCustomErrors renderCustom custom =
    "; " <> joinStrings "; " (map renderCustom custom)

-- | Joins strings with a separator without adding a trailing separator.
joinStrings : String -> [String] -> String
joinStrings _ [] = ""
joinStrings _ (value :: []) = value
joinStrings separator (value :: rest) = value <> separator <> joinStrings separator rest

instance Functor (Parser input custom) where
    fmap transform parser = mapResult transform parser

instance Applicative (Parser input custom) where
    pure value = succeed value
    apply parserFunction parserArgument = applyParser parserFunction parserArgument

instance Monad (Parser input custom) where
    bind parser continuation = andThen parser continuation
