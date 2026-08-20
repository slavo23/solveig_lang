import std.bytes
import std.list
import std.map
import std.parser
import std.text
import std.vector

-- | An exact RFC 8259 number spelling.
-- |
-- | The representation retains decimal digits and exponent text rather than
-- | rounding through Float. Use the smart constructors; renderJsonWith rejects
-- | a directly forged invalid spelling. A future BigInt constructor can be
-- | added without changing Json's number case.
data JsonNumber = JsonNumber String

-- | A JSON value with persistent arrays and deterministically ordered objects.
data Json =
    JsonNull
  | JsonBool Bool
  | JsonNumberValue JsonNumber
  | JsonString String
  | JsonArray (std.vector.Vector Json)
  | JsonObject (std.map.Map String Json)

-- | Explicit resource bounds for parsing, decoding, and rendering JSON.
-- |
-- | Fields are maximum input bytes, nesting depth, raw bytes inside one string,
-- | items in one array or object, and bytes in one number token. Negative
-- | values are invalid limits rather than requests for an unbounded operation.
data JsonLimits = JsonLimits Int Int Int Int Int

-- | A structured JSON policy, resource, decoding, or string failure.
-- |
-- | Syntax position and expected tokens remain in std.parser.ParseError; these
-- | values add JSON-specific information without replacing parser diagnostics.
data JsonError =
    InvalidJsonLimit String Int
  | JsonInputTooLarge Int Int
  | JsonDepthExceeded Int
  | JsonStringTooLarge Int
  | JsonContainerTooLarge Int
  | JsonNumberTooLarge Int
  | JsonInvalidUtf8 String
  | JsonInvalidEscape String
  | JsonInvalidUnicodeEscape
  | JsonUnescapedControl Int
  | JsonUnterminatedString
  | JsonInvalidNumber String
  | JsonDuplicateObjectKey String
  | JsonExpectedValue

-- | A direct JSON decoder that parses only the requested representation.
-- |
-- | Decoders operate on the same ByteInput parser semantics as Json values.
-- | Primitive, array, and homogeneous-object decoders avoid allocating an
-- | intermediate Json tree.
data Decoder a = Decoder
    (JsonLimits -> Int -> std.parser.Parser std.parser.ByteInput JsonError a)

-- | Practical bounded defaults: 16 MiB input, depth 128, 1 MiB strings,
-- | one million container items, and 1024-byte exact number tokens.
defaultJsonLimits : JsonLimits
defaultJsonLimits = JsonLimits 16777216 128 1048576 1000000 1024

-- | Returns the maximum accepted input size in bytes.
maximumInputBytes : JsonLimits -> Int
maximumInputBytes (JsonLimits value _ _ _ _) = value

-- | Returns the maximum array/object nesting depth; zero permits only scalars.
maximumDepth : JsonLimits -> Int
maximumDepth (JsonLimits _ value _ _ _) = value

-- | Returns the maximum raw bytes between one string's quotes.
maximumStringBytes : JsonLimits -> Int
maximumStringBytes (JsonLimits _ _ value _ _) = value

-- | Returns the maximum items in one array or object.
maximumContainerItems : JsonLimits -> Int
maximumContainerItems (JsonLimits _ _ _ value _) = value

-- | Returns the maximum bytes in one exact number token.
maximumNumberBytes : JsonLimits -> Int
maximumNumberBytes (JsonLimits _ _ _ _ value) = value

-- | Returns the exact validated decimal spelling of a JSON number.
jsonNumberText : JsonNumber -> String
jsonNumberText (JsonNumber text) = text

-- | Constructs an exact JSON integer without passing through Float.
jsonNumberFromInt : Int -> JsonNumber
jsonNumberFromInt value = JsonNumber (show value)

-- | Validates one complete number spelling without accepting surrounding space.
jsonNumberFromString : String -> Maybe JsonNumber
jsonNumberFromString text = validateNumberWith defaultJsonLimits text

validateNumberWith : JsonLimits -> String -> Maybe JsonNumber
validateNumberWith limits text =
    std.parser.foldReply
        (fun _ number _ => Just number)
        (fun _ _ _ => Nothing)
        (fun _ _ _ => Nothing)
        (std.parser.runComplete
            std.parser.byteInputOps
            (numberToken limits)
            (std.parser.byteInput (std.bytes.stringToBytes text)))

-- | Converts a finite Float through its round-trippable decimal display.
-- |
-- | NaN and infinities are rejected because their displays are not JSON number
-- | spellings. The retained decimal is exact; it is not silently reparsed and
-- | rounded to another Float.
jsonNumberFromFloat : Float -> Maybe JsonNumber
jsonNumberFromFloat value = jsonNumberFromString (show value)

-- | Maps a direct decoder's successful result without building a Json value.
mapDecoder : forall a b. (a -> b) -> Decoder a -> Decoder b
mapDecoder transform (Decoder build) =
    Decoder (fun limits => fun depth =>
        std.parser.mapResult transform (build limits depth))

instance Functor Decoder where
    fmap transform decoder = mapDecoder transform decoder

-- | Directly decodes a JSON string.
stringDecoder : Decoder String
stringDecoder = Decoder (fun limits => fun _ => jsonLexeme (stringToken limits))

-- | Directly decodes a JSON Boolean.
boolDecoder : Decoder Bool
boolDecoder = Decoder (fun _ => fun _ => jsonLexeme boolToken)

-- | Directly decodes an exact JSON number spelling.
numberDecoder : Decoder JsonNumber
numberDecoder = Decoder (fun limits => fun _ => jsonLexeme (numberToken limits))

-- | Directly decodes null.
nullDecoder : Decoder ()
nullDecoder = Decoder (fun _ => fun _ => jsonLexeme nullToken)

-- | Directly decodes a homogeneous array with explicit element decoding.
arrayDecoder : forall a. Decoder a -> Decoder (std.vector.Vector a)
arrayDecoder (Decoder build) =
    Decoder (fun limits => fun depth =>
        arrayWith limits depth (build limits (depth + 1)))

-- | Directly decodes a homogeneous object, rejecting duplicate names.
objectDecoder : forall a. Decoder a -> Decoder (std.map.Map String a)
objectDecoder (Decoder build) =
    Decoder (fun limits => fun depth =>
        objectWith limits depth (build limits (depth + 1)))

-- | Decodes null as Nothing or delegates a non-null value to a decoder.
nullableDecoder : forall a. Decoder a -> Decoder (Maybe a)
nullableDecoder (Decoder build) =
    Decoder (fun limits => fun depth =>
        std.parser.choice std.parser.byteInputOps
            (std.parser.mapResult (fun _ => Nothing) (jsonLexeme nullToken))
            (std.parser.mapResult Just (build limits depth)))

-- | Builds a full Json value; use narrower decoders when a tree is unnecessary.
jsonDecoder : Decoder Json
jsonDecoder = Decoder jsonValueAt

-- | Runs one direct decoder on complete bounded UTF-8 JSON bytes.
-- |
-- | Leading and trailing JSON whitespace are accepted. The returned
-- | ParseReply retains structured position, expected-set, and custom errors.
decodeJson : forall a. JsonLimits -> Decoder a -> Bytes -> std.parser.ParseReply std.parser.ByteInput JsonError a
decodeJson limits (Decoder build) bytes =
    let input = std.parser.byteInput bytes in
    case invalidLimits limits of
        Just error => rejectJson input error
        | Nothing =>
            let actual = std.bytes.bytesLength bytes in
            if actual > maximumInputBytes limits then
                rejectJson input
                    (JsonInputTooLarge actual (maximumInputBytes limits))
            else
                std.parser.runComplete
                    std.parser.byteInputOps
                    (std.parser.keepRight jsonWhitespace
                        (std.parser.keepLeft (build limits 0) jsonWhitespace))
                    input

-- | Parses one complete bounded byte input into a Json value.
parseJson : JsonLimits -> Bytes -> std.parser.ParseReply std.parser.ByteInput JsonError Json
parseJson limits bytes = decodeJson limits jsonDecoder bytes

-- | Renders with defaultJsonLimits and deterministic ascending object keys.
renderJson : Json -> Result JsonError String
renderJson value = renderJsonWith defaultJsonLimits value

-- | Renders with explicit depth, string, container, number, and output bounds.
-- |
-- | Numbers are revalidated, strings are escaped, and object keys are emitted
-- | in std.map ascending order. The final UTF-8 output must fit maximumInputBytes.
renderJsonWith : JsonLimits -> Json -> Result JsonError String
renderJsonWith limits value =
    case invalidLimits limits of
        Just error => Err error
        | Nothing =>
            case renderValueInto limits 0 value [] of
                Err error => Err error
                | Ok characters =>
                    let text = std.text.charsToString characters in
                    let actual = std.bytes.bytesLength (std.bytes.stringToBytes text) in
                    if actual > maximumInputBytes limits then
                        Err (JsonInputTooLarge actual (maximumInputBytes limits))
                    else Ok text

-- | Renders a JSON-specific error for std.parser.renderError.
renderJsonError : JsonError -> String
renderJsonError (InvalidJsonLimit name value) =
    "invalid JSON limit " <> name <> ": " <> show value
renderJsonError (JsonInputTooLarge actual maximum) =
    "JSON input/output has " <> show actual <> " bytes; maximum is " <> show maximum
renderJsonError (JsonDepthExceeded maximum) =
    "JSON nesting exceeds maximum depth " <> show maximum
renderJsonError (JsonStringTooLarge maximum) =
    "JSON string exceeds maximum " <> show maximum <> " raw bytes"
renderJsonError (JsonContainerTooLarge maximum) =
    "JSON container exceeds maximum " <> show maximum <> " items"
renderJsonError (JsonNumberTooLarge maximum) =
    "JSON number exceeds maximum " <> show maximum <> " bytes"
renderJsonError (JsonInvalidUtf8 message) = "invalid UTF-8 in JSON string: " <> message
renderJsonError (JsonInvalidEscape escape) = "invalid JSON escape " <> show escape
renderJsonError JsonInvalidUnicodeEscape = "invalid JSON Unicode escape"
renderJsonError (JsonUnescapedControl byte) =
    "unescaped control byte " <> show byte <> " in JSON string"
renderJsonError JsonUnterminatedString = "unterminated JSON string"
renderJsonError (JsonInvalidNumber number) = "invalid JSON number " <> show number
renderJsonError (JsonDuplicateObjectKey key) = "duplicate JSON object key " <> show key
renderJsonError JsonExpectedValue = "expected a JSON value"

invalidLimits : JsonLimits -> Maybe JsonError
invalidLimits limits =
    if maximumInputBytes limits < 0 then
        Just (InvalidJsonLimit "maximumInputBytes" (maximumInputBytes limits))
    else if maximumDepth limits < 0 then
        Just (InvalidJsonLimit "maximumDepth" (maximumDepth limits))
    else if maximumStringBytes limits < 0 then
        Just (InvalidJsonLimit "maximumStringBytes" (maximumStringBytes limits))
    else if maximumContainerItems limits < 0 then
        Just (InvalidJsonLimit "maximumContainerItems" (maximumContainerItems limits))
    else if maximumNumberBytes limits < 0 then
        Just (InvalidJsonLimit "maximumNumberBytes" (maximumNumberBytes limits))
    else Nothing

-- | Exposes a decoder's parser for composing a domain-specific Decoder.
-- |
-- | The depth is the containing JSON depth. Most applications should start
-- | with the supplied primitive and container decoders; custom decoders can
-- | reuse their exact limits and structured parser behavior through this hook.
decoderParser : forall a. Decoder a -> JsonLimits -> Int -> std.parser.Parser std.parser.ByteInput JsonError a
decoderParser (Decoder build) limits depth = build limits depth

jsonWhitespaceBytes : Bytes
jsonWhitespaceBytes = std.bytes.stringToBytes " \n\u{d}\t"

jsonStringSpecialBytes : Bytes
jsonStringSpecialBytes = std.bytes.stringToBytes "\"\\"

jsonWhitespace : std.parser.Parser std.parser.ByteInput JsonError ()
jsonWhitespace =
    std.parser.Parser (fun input =>
        case input of
            ByteInput bytes offset =>
                let next = std.bytes.bytesSpanAny bytes offset jsonWhitespaceBytes in
                if next == offset then
                    std.parser.Parsed std.parser.EmptyInput () input
                else
                    std.parser.Parsed std.parser.ConsumedInput ()
                        (std.parser.ByteInput bytes next))

jsonLexeme : forall a. std.parser.Parser std.parser.ByteInput JsonError a -> std.parser.Parser std.parser.ByteInput JsonError a
jsonLexeme parser = parser <* jsonWhitespace

jsonSymbol : Int -> String -> std.parser.Parser std.parser.ByteInput JsonError Int
jsonSymbol byte name =
    std.parser.keepRight jsonWhitespace
        (jsonLexeme (std.parser.token std.parser.byteInputOps name byte))

nullToken : std.parser.Parser std.parser.ByteInput JsonError ()
nullToken =
    std.parser.mapResult (fun _ => ())
        (std.parser.tokens std.parser.byteInputOps "null"
            [110, 117, 108, 108])

boolToken : std.parser.Parser std.parser.ByteInput JsonError Bool
boolToken =
    std.parser.choice std.parser.byteInputOps
        (std.parser.mapResult (fun _ => True)
            (std.parser.tokens std.parser.byteInputOps "true"
                [116, 114, 117, 101]))
        (std.parser.mapResult (fun _ => False)
            (std.parser.tokens std.parser.byteInputOps "false"
                [102, 97, 108, 115, 101]))

jsonValueAt : JsonLimits -> Int -> std.parser.Parser std.parser.ByteInput JsonError Json
jsonValueAt limits depth =
    std.parser.keepRight jsonWhitespace
        (std.parser.Parser (fun input => dispatchJsonValue limits depth input))

dispatchJsonValue : JsonLimits -> Int -> std.parser.ByteInput -> std.parser.ParseReply std.parser.ByteInput JsonError Json
dispatchJsonValue limits depth input =
    case byteInputValue input of
        Just 110 => std.parser.runParser
            (std.parser.mapResult (fun _ => JsonNull) nullToken) input
        | Just 116 => std.parser.runParser
            (std.parser.mapResult JsonBool boolToken) input
        | Just 102 => std.parser.runParser
            (std.parser.mapResult JsonBool boolToken) input
        | Just 34 => std.parser.runParser
            (std.parser.mapResult JsonString (stringToken limits)) input
        | Just 91 => std.parser.runParser
            (std.parser.mapResult JsonArray
                (arrayWith limits depth (jsonValueAt limits (depth + 1)))) input
        | Just 123 => std.parser.runParser
            (std.parser.mapResult JsonObject
                (objectWith limits depth (jsonValueAt limits (depth + 1)))) input
        | Just 45 => std.parser.runParser
            (std.parser.mapResult JsonNumberValue (numberToken limits)) input
        | Just byte =>
            if isDigitByte byte then
                std.parser.runParser
                    (std.parser.mapResult JsonNumberValue (numberToken limits)) input
            else rejectJson input JsonExpectedValue
        | Nothing => rejectJson input JsonExpectedValue

byteInputValue : std.parser.ByteInput -> Maybe Int
byteInputValue (ByteInput bytes offset) = std.bytes.bytesIndex bytes offset

rejectJson : forall a. std.parser.ByteInput -> JsonError -> std.parser.ParseReply std.parser.ByteInput JsonError a
rejectJson input error =
    std.parser.runParser (std.parser.customFailure std.parser.byteInputOps error) input

isDigitByte : Int -> Bool
isDigitByte byte = if byte >= 48 then byte <= 57 else False

byteAt : Bytes -> Int -> Int
byteAt bytes offset =
    case std.bytes.bytesIndex bytes offset of
        Just byte => byte
        | Nothing => -1

consumedRejectJson : forall a. std.parser.ByteInput -> JsonError -> std.parser.ParseReply std.parser.ByteInput JsonError a
consumedRejectJson input error =
    std.parser.addConsumption std.parser.ConsumedInput (rejectJson input error)

numberToken : JsonLimits -> std.parser.Parser std.parser.ByteInput JsonError JsonNumber
numberToken limits = std.parser.Parser (fun input => scanNumber limits input)

-- RFC 8259 section 6 is the grammar: sign, integer, optional fraction, and
-- optional exponent. Bulk range spans implement its digit runs exactly.
scanNumber : JsonLimits -> std.parser.ByteInput -> std.parser.ParseReply std.parser.ByteInput JsonError JsonNumber
scanNumber limits input =
    case input of
        ByteInput bytes start =>
            let afterSign = if byteAt bytes start == 45 then start + 1 else start in
            let first = byteAt bytes afterSign in
            if first == 48 then
                finishInteger limits bytes start (afterSign + 1)
            else if and (first >= 49) (first <= 57) then
                finishInteger limits bytes start
                    (std.bytes.bytesSpanRange bytes (afterSign + 1) 48 57)
            else
                consumedRejectJson input (JsonInvalidNumber "expected an integer part")

finishInteger : JsonLimits -> Bytes -> Int -> Int -> std.parser.ParseReply std.parser.ByteInput JsonError JsonNumber
finishInteger limits bytes start integerEnd =
    if byteAt bytes integerEnd == 46 then
        let fractionStart = integerEnd + 1 in
        let fractionEnd = std.bytes.bytesSpanRange bytes fractionStart 48 57 in
        if fractionEnd == fractionStart then
            consumedRejectJson (std.parser.ByteInput bytes fractionStart)
                (JsonInvalidNumber "fraction requires at least one digit")
        else finishExponent limits bytes start fractionEnd
    else finishExponent limits bytes start integerEnd

finishExponent : JsonLimits -> Bytes -> Int -> Int -> std.parser.ParseReply std.parser.ByteInput JsonError JsonNumber
finishExponent limits bytes start significandEnd =
    let marker = byteAt bytes significandEnd in
    if or (marker == 69) (marker == 101) then
        let signOffset = significandEnd + 1 in
        let exponentStart =
            if or (byteAt bytes signOffset == 43) (byteAt bytes signOffset == 45) then
                signOffset + 1
            else signOffset in
        let exponentEnd = std.bytes.bytesSpanRange bytes exponentStart 48 57 in
        if exponentEnd == exponentStart then
            consumedRejectJson (std.parser.ByteInput bytes exponentStart)
                (JsonInvalidNumber "exponent requires at least one digit")
        else finishNumber limits bytes start exponentEnd
    else finishNumber limits bytes start significandEnd

finishNumber : JsonLimits -> Bytes -> Int -> Int -> std.parser.ParseReply std.parser.ByteInput JsonError JsonNumber
finishNumber limits bytes start end =
    let length = end - start in
    let rest = std.parser.ByteInput bytes end in
    if length > maximumNumberBytes limits then
        consumedRejectJson rest (JsonNumberTooLarge (maximumNumberBytes limits))
    else
        case std.bytes.bytesSliceToString bytes start length of
            Ok text => std.parser.Parsed std.parser.ConsumedInput (JsonNumber text) rest
            | Err (InvalidUtf8 message) =>
                consumedRejectJson rest (JsonInvalidUtf8 message)
            | Err _ => consumedRejectJson rest (JsonInvalidNumber "number decoding failed")

stringToken : JsonLimits -> std.parser.Parser std.parser.ByteInput JsonError String
stringToken limits =
    std.parser.keepRight
        (std.parser.token std.parser.byteInputOps "double quote" 34)
        (std.parser.Parser (fun input => scanString limits input))

scanString : JsonLimits -> std.parser.ByteInput -> std.parser.ParseReply std.parser.ByteInput JsonError String
scanString limits input =
    case input of
        ByteInput bytes start => scanStringAt limits bytes start start []

-- RFC 8259 sections 7 and 8.1 define escaping and UTF-8 validity. Following
-- the structural-scanning principle in Langdale/Lemire, ordinary UTF-8 spans
-- are found and decoded in bulk; only quotes, escapes, and controls branch.
scanStringAt : JsonLimits -> Bytes -> Int -> Int -> [Char] -> std.parser.ParseReply std.parser.ByteInput JsonError String
scanStringAt limits bytes contentStart offset reversed =
    let special = std.bytes.bytesFindAny bytes offset jsonStringSpecialBytes in
    let control = std.bytes.bytesFindBelow bytes offset 32 in
    let boundary = minOrd special control in
    let rawLength = boundary - contentStart in
    let boundaryInput = std.parser.ByteInput bytes boundary in
    if rawLength > maximumStringBytes limits then
        consumedRejectJson boundaryInput (JsonStringTooLarge (maximumStringBytes limits))
    else if control < special then
        consumedRejectJson boundaryInput (JsonUnescapedControl (byteAt bytes control))
    else
        case std.bytes.bytesSliceToString bytes offset (boundary - offset) of
            Err (InvalidUtf8 message) =>
                consumedRejectJson boundaryInput (JsonInvalidUtf8 message)
            | Err _ => consumedRejectJson boundaryInput (JsonInvalidUtf8 "string decoding failed")
            | Ok chunk =>
                let accumulated = prependReversed (std.text.stringToChars chunk) reversed in
                case std.bytes.bytesIndex bytes boundary of
                    Nothing => consumedRejectJson boundaryInput JsonUnterminatedString
                    | Just 34 =>
                        std.parser.Parsed std.parser.ConsumedInput
                            (std.text.charsToString (std.list.reverse accumulated))
                            (std.parser.ByteInput bytes (boundary + 1))
                    | Just 92 =>
                        case decodeEscape bytes (boundary + 1) of
                            Err error => consumedRejectJson boundaryInput error
                            | Ok (MkPair escaped next) =>
                                scanStringAt limits bytes contentStart next
                                    (prependReversed (std.text.stringToChars escaped) accumulated)
                    | Just byte => consumedRejectJson boundaryInput (JsonUnescapedControl byte)

prependReversed : forall a. [a] -> [a] -> [a]
prependReversed values reversed = prependReversedInto values reversed

prependReversedInto : forall a. [a] -> [a] -> [a]
prependReversedInto [] reversed = reversed
prependReversedInto (value :: rest) reversed =
    prependReversedInto rest (value :: reversed)

decodeEscape : Bytes -> Int -> Result JsonError (Pair String Int)
decodeEscape bytes offset =
    case std.bytes.bytesIndex bytes offset of
        Nothing => Err JsonUnterminatedString
        | Just 34 => Ok (MkPair "\"" (offset + 1))
        | Just 92 => Ok (MkPair "\\" (offset + 1))
        | Just 47 => Ok (MkPair "/" (offset + 1))
        | Just 98 => Ok (MkPair "\u{8}" (offset + 1))
        | Just 102 => Ok (MkPair "\u{c}" (offset + 1))
        | Just 110 => Ok (MkPair "\n" (offset + 1))
        | Just 114 => Ok (MkPair "\u{d}" (offset + 1))
        | Just 116 => Ok (MkPair "\t" (offset + 1))
        | Just 117 => decodeUnicodeEscape bytes (offset + 1)
        | Just byte => Err (JsonInvalidEscape ("byte " <> show byte))

decodeUnicodeEscape : Bytes -> Int -> Result JsonError (Pair String Int)
decodeUnicodeEscape bytes offset =
    case readHex4 bytes offset of
        Nothing => Err JsonInvalidUnicodeEscape
        | Just high =>
            if and (high >= 55296) (high <= 56319) then
                let slash = offset + 4 in
                if and (byteAt bytes slash == 92) (byteAt bytes (slash + 1) == 117) then
                    case readHex4 bytes (slash + 2) of
                        Just low =>
                            if and (low >= 56320) (low <= 57343) then
                                scalarString (65536 + ((high - 55296) * 1024) + (low - 56320))
                                    (slash + 6)
                            else Err JsonInvalidUnicodeEscape
                        | Nothing => Err JsonInvalidUnicodeEscape
                else Err JsonInvalidUnicodeEscape
            else if and (high >= 56320) (high <= 57343) then
                Err JsonInvalidUnicodeEscape
            else scalarString high (offset + 4)

scalarString : Int -> Int -> Result JsonError (Pair String Int)
scalarString codePoint next =
    case std.text.codePointToString codePoint of
        Just text => Ok (MkPair text next)
        | Nothing => Err JsonInvalidUnicodeEscape

readHex4 : Bytes -> Int -> Maybe Int
readHex4 bytes offset =
    case hexDigit (byteAt bytes offset) of
        Nothing => Nothing
        | Just a => case hexDigit (byteAt bytes (offset + 1)) of
            Nothing => Nothing
            | Just b => case hexDigit (byteAt bytes (offset + 2)) of
                Nothing => Nothing
                | Just c => case hexDigit (byteAt bytes (offset + 3)) of
                    Nothing => Nothing
                    | Just d => Just (a * 4096 + b * 256 + c * 16 + d)

hexDigit : Int -> Maybe Int
hexDigit byte =
    if and (byte >= 48) (byte <= 57) then Just (byte - 48)
    else if and (byte >= 65) (byte <= 70) then Just (byte - 55)
    else if and (byte >= 97) (byte <= 102) then Just (byte - 87)
    else Nothing

arrayWith : forall a. JsonLimits -> Int -> std.parser.Parser std.parser.ByteInput JsonError a -> std.parser.Parser std.parser.ByteInput JsonError (std.vector.Vector a)
arrayWith limits depth element =
    if depth >= maximumDepth limits then
        std.parser.customFailure std.parser.byteInputOps
            (JsonDepthExceeded (maximumDepth limits))
    else
        std.parser.keepRight (jsonSymbol 91 "[")
            (std.parser.choice std.parser.byteInputOps
                (std.parser.mapResult (fun _ => std.vector.empty) (jsonSymbol 93 "]"))
                (arrayItems limits element [] 0))

arrayItems : forall a. JsonLimits -> std.parser.Parser std.parser.ByteInput JsonError a -> [a] -> Int -> std.parser.Parser std.parser.ByteInput JsonError (std.vector.Vector a)
arrayItems limits element reversed count =
    std.parser.andThen element (fun value =>
        let nextCount = count + 1 in
        if nextCount > maximumContainerItems limits then
            std.parser.customFailure std.parser.byteInputOps
                (JsonContainerTooLarge (maximumContainerItems limits))
        else
            std.parser.choice std.parser.byteInputOps
                (std.parser.mapResult
                    (fun _ => std.vector.fromList (std.list.reverse (value :: reversed)))
                    (jsonSymbol 93 "]"))
                (std.parser.keepRight (jsonSymbol 44 ",")
                    (std.parser.defer (fun _ =>
                        arrayItems limits element (value :: reversed) nextCount))))

objectWith : forall a. JsonLimits -> Int -> std.parser.Parser std.parser.ByteInput JsonError a -> std.parser.Parser std.parser.ByteInput JsonError (std.map.Map String a)
objectWith limits depth valueParser =
    if depth >= maximumDepth limits then
        std.parser.customFailure std.parser.byteInputOps
            (JsonDepthExceeded (maximumDepth limits))
    else
        std.parser.keepRight (jsonSymbol 123 "{")
            (std.parser.choice std.parser.byteInputOps
                (std.parser.mapResult (fun _ => std.map.empty) (jsonSymbol 125 "}"))
                (objectItems limits valueParser std.map.empty 0))

objectItems : forall a. JsonLimits -> std.parser.Parser std.parser.ByteInput JsonError a -> std.map.Map String a -> Int -> std.parser.Parser std.parser.ByteInput JsonError (std.map.Map String a)
objectItems limits valueParser values count =
    std.parser.andThen (jsonLexeme (stringToken limits)) (fun key =>
        if std.map.member key values then
            std.parser.customFailure std.parser.byteInputOps (JsonDuplicateObjectKey key)
        else
            std.parser.keepRight (jsonSymbol 58 ":")
                (std.parser.andThen valueParser (fun value =>
                    let nextCount = count + 1 in
                    let nextValues = std.map.insert key value values in
                    if nextCount > maximumContainerItems limits then
                        std.parser.customFailure std.parser.byteInputOps
                            (JsonContainerTooLarge (maximumContainerItems limits))
                    else
                        std.parser.choice std.parser.byteInputOps
                            (std.parser.mapResult (fun _ => nextValues)
                                (jsonSymbol 125 "}"))
                            (std.parser.keepRight (jsonSymbol 44 ",")
                                (std.parser.defer (fun _ =>
                                    objectItems limits valueParser nextValues nextCount))))))

renderValueInto : JsonLimits -> Int -> Json -> [Char] -> Result JsonError [Char]
renderValueInto _ _ JsonNull suffix = Ok ('n' :: 'u' :: 'l' :: 'l' :: suffix)
renderValueInto _ _ (JsonBool True) suffix = Ok ('t' :: 'r' :: 'u' :: 'e' :: suffix)
renderValueInto _ _ (JsonBool False) suffix = Ok ('f' :: 'a' :: 'l' :: 's' :: 'e' :: suffix)
renderValueInto limits _ (JsonNumberValue (JsonNumber number)) suffix =
    if std.bytes.bytesLength (std.bytes.stringToBytes number) > maximumNumberBytes limits then
        Err (JsonNumberTooLarge (maximumNumberBytes limits))
    else case validateNumberWith limits number of
        Nothing => Err (JsonInvalidNumber number)
        | Just _ => Ok (prependChars number suffix)
renderValueInto limits _ (JsonString value) suffix = renderStringInto limits value suffix
renderValueInto limits depth (JsonArray values) suffix =
    if depth >= maximumDepth limits then Err (JsonDepthExceeded (maximumDepth limits))
    else if std.vector.length values > maximumContainerItems limits then
        Err (JsonContainerTooLarge (maximumContainerItems limits))
    else
        case renderArrayReverse limits (depth + 1)
                (std.list.reverse (std.vector.toList values)) True (']' :: suffix) of
            Err error => Err error
            | Ok body => Ok ('[' :: body)
renderValueInto limits depth (JsonObject values) suffix =
    if depth >= maximumDepth limits then Err (JsonDepthExceeded (maximumDepth limits))
    else if std.map.size values > maximumContainerItems limits then
        Err (JsonContainerTooLarge (maximumContainerItems limits))
    else
        case renderObjectReverse limits (depth + 1)
                (std.list.reverse (std.map.toAscList values)) True ('}' :: suffix) of
            Err error => Err error
            | Ok body => Ok ('{' :: body)

renderArrayReverse : JsonLimits -> Int -> [Json] -> Bool -> [Char] -> Result JsonError [Char]
renderArrayReverse _ _ [] _ suffix = Ok suffix
renderArrayReverse limits depth (value :: rest) first suffix =
    let separated = if first then suffix else ',' :: suffix in
    case renderValueInto limits depth value separated of
        Err error => Err error
        | Ok rendered => renderArrayReverse limits depth rest False rendered

renderObjectReverse : JsonLimits -> Int -> [std.map.Entry String Json] -> Bool -> [Char] -> Result JsonError [Char]
renderObjectReverse _ _ [] _ suffix = Ok suffix
renderObjectReverse limits depth (Entry key value :: rest) first suffix =
    let separated = if first then suffix else ',' :: suffix in
    case renderValueInto limits depth value separated of
        Err error => Err error
        | Ok renderedValue =>
            case renderStringInto limits key (':' :: renderedValue) of
                Err error => Err error
                | Ok renderedEntry =>
                    renderObjectReverse limits depth rest False renderedEntry

renderStringInto : JsonLimits -> String -> [Char] -> Result JsonError [Char]
renderStringInto limits value suffix =
    let characters = std.text.stringToChars value in
    let length = escapedStringLength characters 0 in
    if length > maximumStringBytes limits then
        Err (JsonStringTooLarge (maximumStringBytes limits))
    else Ok ('"' :: escapeStringInto (std.list.reverse characters) ('"' :: suffix))

escapedStringLength : [Char] -> Int -> Int
escapedStringLength [] length = length
escapedStringLength (character :: rest) length =
    let codePoint = std.text.charToCodePoint character in
    let width =
        if character == '"' then 2
        else if character == '\\' then 2
        else if codePoint == 8 then 2
        else if codePoint == 12 then 2
        else if codePoint == 10 then 2
        else if codePoint == 13 then 2
        else if codePoint == 9 then 2
        else if codePoint < 32 then 6
        else if codePoint < 128 then 1
        else if codePoint < 2048 then 2
        else if codePoint < 65536 then 3
        else 4 in
    escapedStringLength rest (length + width)

escapeStringInto : [Char] -> [Char] -> [Char]
escapeStringInto [] suffix = suffix
escapeStringInto (character :: rest) suffix =
    let escaped = escapeCharacterInto character suffix in
    escapeStringInto rest escaped

escapeCharacterInto : Char -> [Char] -> [Char]
escapeCharacterInto '"' suffix = '\\' :: '"' :: suffix
escapeCharacterInto '\\' suffix = '\\' :: '\\' :: suffix
escapeCharacterInto '\u{8}' suffix = '\\' :: 'b' :: suffix
escapeCharacterInto '\u{c}' suffix = '\\' :: 'f' :: suffix
escapeCharacterInto '\n' suffix = '\\' :: 'n' :: suffix
escapeCharacterInto '\u{d}' suffix = '\\' :: 'r' :: suffix
escapeCharacterInto '\t' suffix = '\\' :: 't' :: suffix
escapeCharacterInto character suffix =
    let codePoint = std.text.charToCodePoint character in
    if codePoint < 32 then
        '\\' :: 'u' :: '0' :: '0' ::
            hexCharacter (div codePoint 16) :: hexCharacter (mod codePoint 16) :: suffix
    else character :: suffix

hexCharacter : Int -> Char
hexCharacter 0 = '0'
hexCharacter 1 = '1'
hexCharacter 2 = '2'
hexCharacter 3 = '3'
hexCharacter 4 = '4'
hexCharacter 5 = '5'
hexCharacter 6 = '6'
hexCharacter 7 = '7'
hexCharacter 8 = '8'
hexCharacter 9 = '9'
hexCharacter 10 = 'a'
hexCharacter 11 = 'b'
hexCharacter 12 = 'c'
hexCharacter 13 = 'd'
hexCharacter 14 = 'e'
hexCharacter _ = 'f'

prependChars : String -> [Char] -> [Char]
prependChars text suffix = prependCharsList (std.list.reverse (std.text.stringToChars text)) suffix

prependCharsList : [Char] -> [Char] -> [Char]
prependCharsList [] suffix = suffix
prependCharsList (character :: rest) suffix = prependCharsList rest (character :: suffix)
