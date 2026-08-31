export (JsonNumber(..),
    Json(..),
    JsonLimits(..),
    JsonPathElement(..),
    JsonError(..),
    DecodeError(..),
    Decoder,
    FieldDecoder,
    Encoder,
    FieldEncoder,
    VariantEncoder,
    defaultJsonLimits,
    maximumInputBytes,
    maximumDepth,
    maximumStringBytes,
    maximumContainerItems,
    maximumNumberBytes,
    jsonNumberText,
    jsonNumberFromInt,
    jsonNumberFromString,
    jsonNumberFromFloat,
    jsonNumberToInt,
    mapDecoder,
    mapDecoderError,
    validateDecoder,
    stringDecoder,
    boolDecoder,
    numberDecoder,
    nullDecoder,
    arrayDecoder,
    objectDecoder,
    nullableDecoder,
    jsonDecoder,
    mapFieldDecoder,
    applyFieldDecoder,
    requiredField,
    optionalField,
    optionalFieldWithDefault,
    recordDecoder,
    taggedDecoder,
    decodeJsonValue,
    decodeJson,
    contramapEncoder,
    stringEncoder,
    boolEncoder,
    numberEncoder,
    nullEncoder,
    arrayEncoder,
    objectEncoder,
    nullableEncoder,
    jsonEncoder,
    requiredFieldEncoder,
    optionalFieldEncoder,
    recordEncoder,
    taggedRecordEncoder,
    variantEncoder,
    taggedEncoder,
    encodeJsonValue,
    encodeJson,
    parseJson,
    renderJson,
    renderJsonWith,
    renderJsonError,
    decoderParser)

import std.bytes(*)
import std.list(*)
import std.map(*)
import std.parser(*)
import std.text(*)
import std.vector(*)

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

-- | One typed segment in a JSON decoding path.
data JsonPathElement = JsonField String | JsonIndex Int

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
    | JsonInvalidUtf8 Int
    | JsonInvalidEscape String
    | JsonInvalidUnicodeEscape
    | JsonUnescapedControl Int
    | JsonUnterminatedString
    | JsonInvalidNumber String
    | JsonDuplicateObjectKey String
    | JsonExpectedValue
    | JsonDecodeFailure [JsonPathElement] String
    | JsonEncodeFailure [JsonPathElement] String

-- | A JSON-policy failure or an application validation failure at a typed path.
-- |
-- | Syntax position and expected tokens remain in ParseError. Custom errors
-- | stay as values of e, never converted to text during decoding.
data DecodeError e = JsonDecodingError JsonError | CustomDecodeError [JsonPathElement] e

-- | A JSON decoder with direct byte parsing and composable value decoding.
-- |
-- | Decoders operate on the same ByteInput parser semantics as Json values.
-- | Primitive, array, and homogeneous-object decoders avoid allocating an
-- | intermediate Json tree. Heterogeneous record and tagged decoders reuse one
-- | bounded Json value at that composition boundary rather than adding another
-- | parser or forcing every Decoder through a tree.
data Decoder e a =
    Decoder (JsonLimits ->
        Int ->
        [JsonPathElement] ->
        std.parser.Parser std.parser.ByteInput (DecodeError e) a) ([JsonPathElement] ->
        Json -> Either (DecodeError e) a)

-- | A composable heterogeneous object-field decoder.
-- |
-- | Combine fields applicatively, then turn the result into a Decoder with
-- | recordDecoder. Unknown fields are ignored; required, optional, and
-- | explicitly defaulted fields remain distinct at the type/API boundary.
data FieldDecoder e a =
    FieldDecoder ([JsonPathElement] -> std.map.Map String Json -> Either (DecodeError e) a)

-- | A symmetric value encoder that constructs the existing Json representation.
-- |
-- | encodeJson renders with explicit limits and revalidates number spellings.
-- | numberEncoder also validates at construction; jsonEncoder passes through
-- | existing values. No encoding operation rounds JsonNumber through Float.
data Encoder a = Encoder (a -> Either JsonError Json)

-- | One field projection used by recordEncoder and taggedRecordEncoder.
data FieldEncoder a = FieldEncoder (a -> Either JsonError (Maybe (String, Json)))

-- | One projected record-shaped case of a tagged sum encoder.
data VariantEncoder a =
    VariantEncoder String (a -> Maybe (Either JsonError (std.map.Map String Json)))

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

-- | Converts a JSON number to Int only when its mathematical value is integral
-- | and lies inside the signed Int range.
-- |
-- | Decimal fractions and exponents are handled exactly from the retained
-- | spelling. No Float conversion or rounding occurs, so values such as 1.0
-- | and 12e2 succeed while 1.5 and an overflowing integer return Nothing.
jsonNumberToInt : JsonNumber -> Maybe Int
jsonNumberToInt (JsonNumber text) =
    case validateNumberWith
        (JsonLimits maximumInt maximumInt maximumInt maximumInt maximumInt)
        text of
        Nothing => Nothing
        | Just _ => exactIntFromChars (std.text.stringToChars text)

-- | Maps a direct decoder's successful result without building a Json value.
mapDecoder : forall e a b. (a -> b) -> Decoder e a -> Decoder e b
mapDecoder transform (Decoder build decodeValue) =
    Decoder
        (fun limits depth path => std.parser.mapResult transform (build limits depth path))
        (fun path value => fmap transform (decodeValue path value))

instance Functor (Decoder e) where
    fmap transform decoder = mapDecoder transform decoder

-- | Maps application errors without altering JSON failures or their paths.
mapDecoderError : forall e f a. (e -> f) -> Decoder e a -> Decoder f a
mapDecoderError transform (Decoder build decodeValue) =
    Decoder
        (fun limits depth path =>
            std.parser.mapCustomError (mapDecodeError transform) (build limits depth path)
        )
        (fun path value =>
            case decodeValue path value of
                Left error => Left (mapDecodeError transform error)
                | Right decoded => Right decoded
        )

-- | Validates or converts a decoded value with a caller-defined typed error.
-- |
-- | Validation runs once after decoding succeeds, at that value's field/index
-- | path. It preserves direct parsing and never falls back to a default.
validateDecoder : forall e a b. (a -> Either e b) -> Decoder e a -> Decoder e b
validateDecoder validate (Decoder build decodeValue) =
    Decoder
        (fun limits depth path =>
            std.parser.andThen
                (build limits depth path)
                (parsedDecodedValue (validateAt validate path))
        )
        (fun path value => decodeValue path value >>= validateAt validate path)

-- | Directly decodes a JSON string.
stringDecoder : forall e. Decoder e String
stringDecoder =
    Decoder
        (fun limits _ path => jsonParserAt path (jsonLexeme (stringToken limits)))
        decodeStringValue

-- | Directly decodes a JSON Boolean.
boolDecoder : forall e. Decoder e Bool
boolDecoder = Decoder (fun _ _ path => jsonParserAt path (jsonLexeme boolToken)) decodeBoolValue

-- | Directly decodes an exact JSON number spelling.
numberDecoder : forall e. Decoder e JsonNumber
numberDecoder =
    Decoder
        (fun limits _ path => jsonParserAt path (jsonLexeme (numberToken limits)))
        decodeNumberValue

-- | Directly decodes null.
nullDecoder : forall e. Decoder e ()
nullDecoder = Decoder (fun _ _ path => jsonParserAt path (jsonLexeme nullToken)) decodeNullValue

-- | Directly decodes a homogeneous array with explicit element decoding.
arrayDecoder : forall e a. Decoder e a -> Decoder e (std.vector.Vector a)
arrayDecoder decoder =
    case decoder of
        Decoder build _ =>
            Decoder
                (fun limits depth path =>
                    arrayWith
                        JsonDecodingError
                        limits
                        depth
                        (fun index => build limits (depth + 1) (appendPath path (JsonIndex index)))
                )
                (decodeArrayValue decoder)

-- | Directly decodes a homogeneous object, rejecting duplicate names.
objectDecoder : forall e a. Decoder e a -> Decoder e (std.map.Map String a)
objectDecoder decoder =
    case decoder of
        Decoder build _ =>
            Decoder
                (fun limits depth path =>
                    objectWith
                        JsonDecodingError
                        limits
                        depth
                        (fun key => build limits (depth + 1) (appendPath path (JsonField key)))
                )
                (decodeObjectValue decoder)

-- | Decodes null as Nothing or delegates a non-null value to a decoder.
nullableDecoder : forall e a. Decoder e a -> Decoder e (Maybe a)
nullableDecoder decoder =
    case decoder of
        Decoder build _ =>
            Decoder
                (fun limits depth path =>
                    std.parser.choice
                        std.parser.byteInputOps
                        (std.parser.mapResult (fun _ => Nothing) (jsonLexeme nullToken))
                        (std.parser.mapResult Just (build limits depth path))
                )
                (decodeNullableValue decoder)

-- | Builds a full Json value; use narrower decoders when a tree is unnecessary.
jsonDecoder : forall e. Decoder e Json
jsonDecoder =
    Decoder
        (fun limits depth path => jsonParserAt path (jsonValueAt limits depth))
        (fun _ value => Right value)

-- | Maps a heterogeneous field result while preserving its path behavior.
mapFieldDecoder : forall e a b. (a -> b) -> FieldDecoder e a -> FieldDecoder e b
mapFieldDecoder transform (FieldDecoder decodeFields) =
    FieldDecoder (fun path values => fmap transform (decodeFields path values))

-- | Applies one field-produced function to another field result.
applyFieldDecoder : forall e a b. FieldDecoder e (a -> b) -> FieldDecoder e a -> FieldDecoder e b
applyFieldDecoder (FieldDecoder decodeFunction) (FieldDecoder decodeArgument) =
    FieldDecoder
        (fun path values =>
            case decodeFunction path values of
                Left error => Left error
                | Right transform => fmap transform (decodeArgument path values)
        )

instance Functor (FieldDecoder e) where
    fmap transform decoder = mapFieldDecoder transform decoder

instance Applicative (FieldDecoder e) where
    pure value = FieldDecoder (fun _ _ => Right value)
    apply decoderFunction decoderArgument = applyFieldDecoder decoderFunction decoderArgument

-- | Requires one named object field and decodes it at that field's path.
requiredField : forall e a. String -> Decoder e a -> FieldDecoder e a
requiredField name decoder =
    FieldDecoder
        (fun path values =>
            let fieldPath =
                appendPath path (JsonField name)
            in case std.map.lookup name values of
                Nothing =>
                    Left
                        (JsonDecodingError
                            (JsonDecodeFailure fieldPath "required field is missing"))
                | Just value => decodeAt decoder fieldPath value
        )

-- | Decodes a named field when present and returns Nothing only when absent.
optionalField : forall e a. String -> Decoder e a -> FieldDecoder e (Maybe a)
optionalField name decoder =
    FieldDecoder
        (fun path values =>
            let fieldPath =
                appendPath path (JsonField name)
            in case std.map.lookup name values of
                Nothing => Right Nothing
                | Just value => fmap Just (decodeAt decoder fieldPath value)
        )

-- | Uses an explicit default only when the named field is absent.
-- |
-- | A present null or malformed value is still decoded normally and therefore
-- | cannot silently select the default.
optionalFieldWithDefault : forall e a. String -> a -> Decoder e a -> FieldDecoder e a
optionalFieldWithDefault name fallback decoder =
    mapFieldDecoder (fromOptional fallback) (optionalField name decoder)

fromOptional : forall a. a -> Maybe a -> a
fromOptional fallback Nothing = fallback
fromOptional _ (Just present) = present

-- | Turns an applicatively composed field decoder into an object Decoder.
recordDecoder : forall e a. FieldDecoder e a -> Decoder e a
recordDecoder (FieldDecoder decodeFields) =
    decoderFromValue
        (fun path value =>
            case value of
                JsonObject fields => decodeFields path fields
                | _ => typeMismatch path "object" value
        )

-- | Selects an object-field decoder by a required string tag field.
-- |
-- | Each variant sees the complete object, so its required and optional fields
-- | remain ordinary FieldDecoder values. Unknown tags fail at the tag's path.
taggedDecoder : forall e a. String -> [(String, FieldDecoder e a)] -> Decoder e a
taggedDecoder tagName variants = decoderFromValue (decodeTaggedValue tagName variants)

-- | Runs a Decoder against an existing Json value with root path `$`.
decodeJsonValue : forall e a. Decoder e a -> Json -> Either (DecodeError e) a
decodeJsonValue decoder value = decodeAt decoder [] value

-- | Runs one direct decoder on complete bounded UTF-8 JSON bytes.
-- |
-- | Leading and trailing JSON whitespace are accepted. The returned
-- | ParseReply retains structured position, expected-set, and custom errors.
decodeJson :
    forall e a. JsonLimits ->
    Decoder e a -> Bytes -> std.parser.ParseReply std.parser.ByteInput (DecodeError e) a
decodeJson limits decoder = completeJson JsonDecodingError limits (decoderParser decoder limits 0)

completeJson :
    forall e a. (JsonError -> e) ->
    JsonLimits ->
    std.parser.Parser std.parser.ByteInput e a ->
    Bytes -> std.parser.ParseReply std.parser.ByteInput e a
completeJson inject limits parser bytes =
    let input =
        std.parser.byteInput bytes
    in case invalidLimits limits of
        Just error =>
            std.parser.runParser
                (std.parser.customFailure std.parser.byteInputOps (inject error))
                input
        | Nothing =>
            let actual =
                std.bytes.bytesLength bytes
            in if actual > maximumInputBytes limits
                then std.parser.runParser
                    (std.parser.customFailure
                        std.parser.byteInputOps
                        (inject (JsonInputTooLarge actual (maximumInputBytes limits))))
                    input
                else std.parser.runComplete
                    std.parser.byteInputOps
                    (std.parser.keepRight
                        jsonWhitespace
                        (std.parser.keepLeft parser jsonWhitespace))
                    input

-- | Contramaps an encoder through a projection from the new input type.
contramapEncoder : forall a b. (b -> a) -> Encoder a -> Encoder b
contramapEncoder project (Encoder encodeValue) = Encoder (encodeValue . project)

-- | Encodes text as a JSON string value.
stringEncoder : Encoder String
stringEncoder = Encoder (Right . JsonString)

-- | Encodes a Boolean JSON value.
boolEncoder : Encoder Bool
boolEncoder = Encoder (Right . JsonBool)

-- | Encodes an exact JSON number spelling.
numberEncoder : Encoder JsonNumber
numberEncoder = Encoder encodeNumberValue

-- | Encodes unit as JSON null.
nullEncoder : Encoder ()
nullEncoder = Encoder (fun _ => Right JsonNull)

-- | Encodes a persistent vector as a JSON array.
arrayEncoder : forall a. Encoder a -> Encoder (std.vector.Vector a)
arrayEncoder encoder = Encoder (encodeArrayValue encoder)

-- | Encodes an ordered map as a JSON object.
objectEncoder : forall a. Encoder a -> Encoder (std.map.Map String a)
objectEncoder encoder = Encoder (encodeObjectValue encoder)

-- | Encodes Nothing as null and Just through the supplied encoder.
nullableEncoder : forall a. Encoder a -> Encoder (Maybe a)
nullableEncoder encoder =
    Encoder
        (fun value =>
            case value of
                Nothing => Right JsonNull
                | Just present => encodeAt encoder present
        )

-- | Encodes an already constructed Json value.
jsonEncoder : Encoder Json
jsonEncoder = Encoder Right

-- | Projects one required record field through its value encoder.
requiredFieldEncoder :
    forall record field. String -> (record -> field) -> Encoder field -> FieldEncoder record
requiredFieldEncoder name project encoder =
    FieldEncoder
        (fun record =>
            case encodeAt encoder (project record) of
                Left error => Left (atJsonPath (JsonField name) error)
                | Right value => Right (Just (name, value))
        )

-- | Projects an optional record field, omitting Nothing from the JSON object.
optionalFieldEncoder :
    forall record field. String -> (record -> Maybe field) -> Encoder field -> FieldEncoder record
optionalFieldEncoder name project encoder =
    FieldEncoder
        (fun record =>
            case project record of
                Nothing => Right Nothing
                | Just present =>
                    case encodeAt encoder present of
                        Left error => Left (atJsonPath (JsonField name) error)
                        | Right value => Right (Just (name, value))
        )

-- | Encodes a record through a finite list of named field projections.
-- |
-- | Duplicate emitted names are rejected. Optional fields are omitted rather
-- | than encoded as null, keeping omission distinct from nullable data.
recordEncoder : forall a. [FieldEncoder a] -> Encoder a
recordEncoder fields =
    Encoder (fun value => fmap JsonObject (encodeFields fields value std.map.empty))

-- | Encodes one tagged record variant with a required string tag field.
-- |
-- | The tag is emitted first semantically and object rendering remains
-- | deterministic by key. A field projection may not reuse the tag name.
taggedRecordEncoder : forall a. String -> String -> [FieldEncoder a] -> Encoder a
taggedRecordEncoder tagName tagValue fields =
    Encoder
        (fun value =>
            let initial =
                std.map.singleton tagName (JsonString tagValue)
            in fmap JsonObject (encodeFields fields value initial)
        )

-- | Defines one tagged sum case through a partial projection and record fields.
-- |
-- | Return Nothing when the source value belongs to another case. The first
-- | matching case is selected, so projections should be mutually exclusive.
variantEncoder :
    forall sum payload. String ->
    (sum -> Maybe payload) -> [FieldEncoder payload] -> VariantEncoder sum
variantEncoder tag project fields =
    VariantEncoder
        tag
        (fun value =>
            case project value of
                Nothing => Nothing
                | Just payload => Just (encodeFields fields payload std.map.empty)
        )

-- | Encodes a tagged sum by selecting one projected record-shaped case.
-- |
-- | Failure to match a case is explicit. A case field may not reuse tagName.
taggedEncoder : forall a. String -> [VariantEncoder a] -> Encoder a
taggedEncoder tagName variants = Encoder (encodeTaggedVariant tagName variants)

-- | Runs an Encoder without rendering its Json value.
encodeJsonValue : forall a. Encoder a -> a -> Either JsonError Json
encodeJsonValue = encodeAt

-- | Encodes and deterministically renders one value with explicit limits.
encodeJson : forall a. JsonLimits -> Encoder a -> a -> Either JsonError String
encodeJson limits encoder value =
    case encodeAt encoder value of
        Left error => Left error
        | Right json => renderJsonWith limits json

-- | Parses one complete bounded byte input into a Json value.
parseJson : JsonLimits -> Bytes -> std.parser.ParseReply std.parser.ByteInput JsonError Json
parseJson limits = completeJson id limits (jsonValueAt limits 0)

-- | Renders with defaultJsonLimits and deterministic ascending object keys.
renderJson : Json -> Either JsonError String
renderJson value = renderJsonWith defaultJsonLimits value

-- | Renders with explicit depth, string, container, number, and output bounds.
-- |
-- | Numbers are revalidated, strings are escaped, and object keys are emitted
-- | in std.map ascending order. The final UTF-8 output must fit maximumInputBytes.
renderJsonWith : JsonLimits -> Json -> Either JsonError String
renderJsonWith limits value =
    case invalidLimits limits of
        Just error => Left error
        | Nothing =>
            case renderValueInto limits 0 value [] of
                Left error => Left error
                | Right characters =>
                    let text = std.text.charsToString characters in
                    let actual = std.bytes.bytesLength (std.bytes.stringToBytes text) in
                    if actual > maximumInputBytes limits then
                        Left (JsonInputTooLarge actual (maximumInputBytes limits))
                    else Right text

-- | Renders a JSON-specific error for std.parser.renderError.
renderJsonError : JsonError -> String
renderJsonError (InvalidJsonLimit name value) = "invalid JSON limit " <> name <> ": " <> show value
renderJsonError (JsonInputTooLarge actual maximum) =
    "JSON input/output has " <> show actual <> " bytes; maximum is " <> show maximum
renderJsonError (JsonDepthExceeded maximum) = "JSON nesting exceeds maximum depth " <> show maximum
renderJsonError (JsonStringTooLarge maximum) =
    "JSON string exceeds maximum " <> show maximum <> " raw bytes"
renderJsonError (JsonContainerTooLarge maximum) =
    "JSON container exceeds maximum " <> show maximum <> " items"
renderJsonError (JsonNumberTooLarge maximum) =
    "JSON number exceeds maximum " <> show maximum <> " bytes"
renderJsonError (JsonInvalidUtf8 offset) = "invalid UTF-8 in JSON string at byte " <> show offset
renderJsonError (JsonInvalidEscape escape) = "invalid JSON escape " <> show escape
renderJsonError JsonInvalidUnicodeEscape = "invalid JSON Unicode escape"
renderJsonError (JsonUnescapedControl byte) =
    "unescaped control byte " <> show byte <> " in JSON string"
renderJsonError JsonUnterminatedString = "unterminated JSON string"
renderJsonError (JsonInvalidNumber number) = "invalid JSON number " <> show number
renderJsonError (JsonDuplicateObjectKey key) = "duplicate JSON object key " <> show key
renderJsonError JsonExpectedValue = "expected a JSON value"
renderJsonError (JsonDecodeFailure path reason) =
    "JSON decode at " <> renderJsonPath path <> ": " <> reason
renderJsonError (JsonEncodeFailure path reason) =
    "JSON encode at " <> renderJsonPath path <> ": " <> reason

instance Show JsonError where
    show error = renderJsonError error

instance Show e => Show (DecodeError e) where
    show (JsonDecodingError error) = show error
    show (CustomDecodeError path error) =
        "JSON decode at " <> renderJsonPath path <> ": " <> show error

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
decoderParser :
    forall e a. Decoder e a ->
    JsonLimits -> Int -> std.parser.Parser std.parser.ByteInput (DecodeError e) a
decoderParser (Decoder build _) limits depth = build limits depth []

maximumInt : Int
maximumInt = 9223372036854775807

minimumInt : Int
minimumInt = (-9223372036854775807) - 1

exactIntFromChars : [Char] -> Maybe Int
exactIntFromChars ('-' :: rest) = collectExactDigits True rest [] 0 False
exactIntFromChars characters = collectExactDigits False characters [] 0 False

collectExactDigits : Bool -> [Char] -> [Int] -> Int -> Bool -> Maybe Int
collectExactDigits negative [] reversed fractionDigits _ =
    finishExactInt negative (std.list.reverse reversed) fractionDigits 0
collectExactDigits negative ('.' :: rest) reversed _ False =
    collectExactDigits negative rest reversed 0 True
collectExactDigits negative ('e' :: rest) reversed fractionDigits _ =
    finishExactExponent negative reversed fractionDigits rest
collectExactDigits negative ('E' :: rest) reversed fractionDigits _ =
    finishExactExponent negative reversed fractionDigits rest
collectExactDigits negative (character :: rest) reversed fractionDigits inFraction =
    let nextFractionDigits =
        if inFraction then fractionDigits + 1 else fractionDigits
    in collectExactDigits
        negative
        rest
        (decimalDigit character :: reversed)
        nextFractionDigits
        inFraction

finishExactExponent : Bool -> [Int] -> Int -> [Char] -> Maybe Int
finishExactExponent negative reversed fractionDigits rest =
    let limit =
        std.list.length reversed + 19
    in finishExactInt
        negative
        (std.list.reverse reversed)
        fractionDigits
        (signedExponent limit rest)

-- RFC 8259 section 6 permits arbitrarily large decimal exponents. Beyond the
-- significand length plus Int's 19 digits, a nonzero value must overflow or be
-- fractional. Clamp relative to that length, never to a fixed token size.
signedExponent : Int -> [Char] -> Int
signedExponent limit ('-' :: rest) = 0 - boundedExponent limit rest 0
signedExponent limit ('+' :: rest) = boundedExponent limit rest 0
signedExponent limit rest = boundedExponent limit rest 0

boundedExponent : Int -> [Char] -> Int -> Int
boundedExponent _ [] value = value
boundedExponent limit (character :: rest) value =
    let digit =
        decimalDigit character
    in if value > div (limit - digit) 10
        then limit
        else boundedExponent limit rest (value * 10 + digit)

decimalDigit : Char -> Int
decimalDigit character = std.text.charToCodePoint character - 48

finishExactInt : Bool -> [Int] -> Int -> Int -> Maybe Int
finishExactInt negative digits fractionDigits exponent =
    if allZeroDigits digits
        then Just 0
        else let scale =
            exponent - fractionDigits
        in if scale >= 0
            then parseScaledInt negative digits scale
            else let discardedCount =
                0 - scale
            in let retainedCount =
                std.list.length digits - discardedCount
            in if retainedCount <= 0
                then Nothing
                else let discarded =
                    std.list.drop retainedCount digits
                in if allZeroDigits discarded
                then parseScaledInt negative (std.list.take retainedCount digits) 0
                else Nothing

allZeroDigits : [Int] -> Bool
allZeroDigits [] = True
allZeroDigits (digit :: rest) = if digit == 0 then allZeroDigits rest else False

parseScaledInt : Bool -> [Int] -> Int -> Maybe Int
parseScaledInt negative digits zeroCount =
    let limit =
        if negative then minimumInt else 0 - maximumInt
    in case accumulateDigits limit (div limit 10) 0 digits of
        Nothing => Nothing
        | Just accumulated =>
            case accumulateZeros limit (div limit 10) accumulated zeroCount of
                Nothing => Nothing
                | Just result => Just (if negative then result else 0 - result)

accumulateDigits : Int -> Int -> Int -> [Int] -> Maybe Int
accumulateDigits _ _ result [] = Just result
accumulateDigits limit multiplicationLimit result (digit :: rest) =
    if result < multiplicationLimit
        then Nothing
        else let multiplied =
            result * 10
        in if multiplied < limit + digit
            then Nothing
            else accumulateDigits limit multiplicationLimit (multiplied - digit) rest

accumulateZeros : Int -> Int -> Int -> Int -> Maybe Int
accumulateZeros _ _ result 0 = Just result
accumulateZeros limit multiplicationLimit result remaining =
    if result < multiplicationLimit
        then Nothing
        else let multiplied =
            result * 10
        in if multiplied < limit
            then Nothing
            else accumulateZeros limit multiplicationLimit multiplied (remaining - 1)

decodeAt : forall e a. Decoder e a -> [JsonPathElement] -> Json -> Either (DecodeError e) a
decodeAt (Decoder _ decodeValue) path value = decodeValue path value

encodeAt : forall a. Encoder a -> a -> Either JsonError Json
encodeAt (Encoder encodeValue) value = encodeValue value

decoderFromValue :
    forall e a. ([JsonPathElement] -> Json -> Either (DecodeError e) a) -> Decoder e a
decoderFromValue decodeValue =
    Decoder
        (fun limits depth path =>
            std.parser.andThen
                (jsonParserAt path (jsonValueAt limits depth))
                (parsedDecodedValue (decodeValue path))
        )
        decodeValue

parsedDecodedValue :
    forall e a b. (a -> Either (DecodeError e) b) ->
    a -> std.parser.Parser std.parser.ByteInput (DecodeError e) b
parsedDecodedValue decodeValue value =
    case decodeValue value of
        Left error => std.parser.customFailure std.parser.byteInputOps error
        | Right decoded => std.parser.succeed decoded

jsonParserAt :
    forall e a. [JsonPathElement] ->
    std.parser.Parser std.parser.ByteInput JsonError a ->
    std.parser.Parser std.parser.ByteInput (DecodeError e) a
jsonParserAt [] = std.parser.mapCustomError JsonDecodingError
jsonParserAt path =
    std.parser.context (renderJsonPath path) . std.parser.mapCustomError JsonDecodingError

mapDecodeError : forall e f. (e -> f) -> DecodeError e -> DecodeError f
mapDecodeError _ (JsonDecodingError error) = JsonDecodingError error
mapDecodeError transform (CustomDecodeError path error) = CustomDecodeError path (transform error)

validateAt : forall e a b. (a -> Either e b) -> [JsonPathElement] -> a -> Either (DecodeError e) b
validateAt validate path value =
    case validate value of
        Left error => Left (CustomDecodeError path error)
        | Right validated => Right validated

decodeStringValue : forall e. [JsonPathElement] -> Json -> Either (DecodeError e) String
decodeStringValue _ (JsonString value) = Right value
decodeStringValue path value = typeMismatch path "string" value

decodeBoolValue : forall e. [JsonPathElement] -> Json -> Either (DecodeError e) Bool
decodeBoolValue _ (JsonBool value) = Right value
decodeBoolValue path value = typeMismatch path "boolean" value

decodeNumberValue : forall e. [JsonPathElement] -> Json -> Either (DecodeError e) JsonNumber
decodeNumberValue _ (JsonNumberValue value) = Right value
decodeNumberValue path value = typeMismatch path "number" value

decodeNullValue : forall e. [JsonPathElement] -> Json -> Either (DecodeError e) ()
decodeNullValue _ JsonNull = Right ()
decodeNullValue path value = typeMismatch path "null" value

decodeNullableValue :
    forall e a. Decoder e a -> [JsonPathElement] -> Json -> Either (DecodeError e) (Maybe a)
decodeNullableValue _ _ JsonNull = Right Nothing
decodeNullableValue decoder path value = fmap Just (decodeAt decoder path value)

decodeArrayValue :
    forall e a. Decoder e a ->
    [JsonPathElement] -> Json -> Either (DecodeError e) (std.vector.Vector a)
decodeArrayValue decoder path (JsonArray values) =
    fmap std.vector.fromList (decodeArrayItems decoder path 0 (std.vector.toList values) [])
decodeArrayValue _ path value = typeMismatch path "array" value

decodeArrayItems :
    forall e a. Decoder e a ->
    [JsonPathElement] -> Int -> [Json] -> [a] -> Either (DecodeError e) [a]
decodeArrayItems _ _ _ [] reversed = Right (std.list.reverse reversed)
decodeArrayItems decoder path index (value :: rest) reversed =
    case decodeAt decoder (appendPath path (JsonIndex index)) value of
        Left error => Left error
        | Right decoded => decodeArrayItems decoder path (index + 1) rest (decoded :: reversed)

decodeObjectValue :
    forall e a. Decoder e a ->
    [JsonPathElement] -> Json -> Either (DecodeError e) (std.map.Map String a)
decodeObjectValue decoder path (JsonObject values) =
    decodeObjectItems decoder path (std.map.toAscList values) std.map.empty
decodeObjectValue _ path value = typeMismatch path "object" value

decodeObjectItems :
    forall e a. Decoder e a ->
    [JsonPathElement] ->
    [(String, Json)] -> std.map.Map String a -> Either (DecodeError e) (std.map.Map String a)
decodeObjectItems _ _ [] decoded = Right decoded
decodeObjectItems decoder path ((name, value) :: rest) decoded =
    case decodeAt decoder (appendPath path (JsonField name)) value of
        Left error => Left error
        | Right present => decodeObjectItems decoder path rest (std.map.insert name present decoded)

decodeTaggedValue :
    forall e a. String ->
    [(String, FieldDecoder e a)] -> [JsonPathElement] -> Json -> Either (DecodeError e) a
decodeTaggedValue tagName variants path (JsonObject fields) =
    let tagPath =
        appendPath path (JsonField tagName)
    in case std.map.lookup tagName fields of
        Nothing =>
            Left (JsonDecodingError (JsonDecodeFailure tagPath "required tag field is missing"))
        | Just tagJson =>
            case decodeAt stringDecoder tagPath tagJson of
                Left error => Left error
                | Right tag => decodeTaggedFields variants path tagPath tag fields
decodeTaggedValue _ _ path value = typeMismatch path "object" value

decodeTaggedFields :
    forall e a. [(String, FieldDecoder e a)] ->
    [JsonPathElement] ->
    [JsonPathElement] -> String -> std.map.Map String Json -> Either (DecodeError e) a
decodeTaggedFields [] _ tagPath tag _ =
    Left (JsonDecodingError (JsonDecodeFailure tagPath ("unknown variant tag " <> show tag)))
decodeTaggedFields ((wanted, FieldDecoder decodeFields) :: rest) path tagPath tag fields =
    if wanted == tag
        then decodeFields path fields
        else decodeTaggedFields rest path tagPath tag fields

typeMismatch : forall e a. [JsonPathElement] -> String -> Json -> Either (DecodeError e) a
typeMismatch path expectedKind value =
    Left
        (JsonDecodingError
            (JsonDecodeFailure path ("expected " <> expectedKind <> ", found " <> jsonKind value)))

jsonKind : Json -> String
jsonKind JsonNull = "null"
jsonKind (JsonBool _) = "boolean"
jsonKind (JsonNumberValue _) = "number"
jsonKind (JsonString _) = "string"
jsonKind (JsonArray _) = "array"
jsonKind (JsonObject _) = "object"

appendPath : [JsonPathElement] -> JsonPathElement -> [JsonPathElement]
appendPath path element = path ++ [element]

atJsonPath : JsonPathElement -> JsonError -> JsonError
atJsonPath element (JsonEncodeFailure path reason) = JsonEncodeFailure (element :: path) reason
atJsonPath element error = JsonEncodeFailure (element :: []) (renderJsonError error)

renderJsonPath : [JsonPathElement] -> String
renderJsonPath path = "$" <> renderJsonPathElements path

renderJsonPathElements : [JsonPathElement] -> String
renderJsonPathElements [] = ""
renderJsonPathElements (JsonField name :: rest) =
    "[" <> show name <> "]" <> renderJsonPathElements rest
renderJsonPathElements (JsonIndex index :: rest) =
    "[" <> show index <> "]" <> renderJsonPathElements rest

encodeNumberValue : JsonNumber -> Either JsonError Json
encodeNumberValue (JsonNumber text) =
    case validateNumberWith
        (JsonLimits maximumInt maximumInt maximumInt maximumInt maximumInt)
        text of
        Nothing => Left (JsonInvalidNumber text)
        | Just number => Right (JsonNumberValue number)

encodeArrayValue : forall a. Encoder a -> std.vector.Vector a -> Either JsonError Json
encodeArrayValue encoder values =
    fmap
        (JsonArray . std.vector.fromList)
        (encodeArrayItems encoder 0 (std.vector.toList values) [])

encodeArrayItems : forall a. Encoder a -> Int -> [a] -> [Json] -> Either JsonError [Json]
encodeArrayItems _ _ [] reversed = Right (std.list.reverse reversed)
encodeArrayItems encoder index (value :: rest) reversed =
    case encodeAt encoder value of
        Left error => Left (atJsonPath (JsonIndex index) error)
        | Right encoded => encodeArrayItems encoder (index + 1) rest (encoded :: reversed)

encodeObjectValue : forall a. Encoder a -> std.map.Map String a -> Either JsonError Json
encodeObjectValue encoder values =
    fmap JsonObject (encodeObjectItems encoder (std.map.toAscList values) std.map.empty)

encodeObjectItems :
    forall a. Encoder a ->
    [(String, a)] -> std.map.Map String Json -> Either JsonError (std.map.Map String Json)
encodeObjectItems _ [] encoded = Right encoded
encodeObjectItems encoder ((name, value) :: rest) encoded =
    case encodeAt encoder value of
        Left error => Left (atJsonPath (JsonField name) error)
        | Right json => encodeObjectItems encoder rest (std.map.insert name json encoded)

encodeFields :
    forall a. [FieldEncoder a] ->
    a -> std.map.Map String Json -> Either JsonError (std.map.Map String Json)
encodeFields [] _ values = Right values
encodeFields (FieldEncoder encodeField :: rest) value values =
    case encodeField value of
        Left error => Left error
        | Right Nothing => encodeFields rest value values
        | Right (Just (name, json)) =>
            if std.map.member name values
                then Left (JsonEncodeFailure (JsonField name :: []) "duplicate encoded field")
                else encodeFields rest value (std.map.insert name json values)

encodeTaggedVariant : forall a. String -> [VariantEncoder a] -> a -> Either JsonError Json
encodeTaggedVariant _ [] _ = Left (JsonEncodeFailure [] "no tagged encoder variant matched")
encodeTaggedVariant tagName (VariantEncoder tag project :: rest) value =
    case project value of
        Nothing => encodeTaggedVariant tagName rest value
        | Just (Left error) => Left error
        | Just (Right fields) =>
            if std.map.member tagName fields
                then Left
                    (JsonEncodeFailure
                        (JsonField tagName :: [])
                        "tag field conflicts with an encoded variant field")
                else Right (JsonObject (std.map.insert tagName (JsonString tag) fields))

jsonWhitespaceBytes : Bytes
jsonWhitespaceBytes = std.bytes.stringToBytes " \n\u{d}\t"

jsonStringSpecialBytes : Bytes
jsonStringSpecialBytes = std.bytes.stringToBytes "\"\\"

jsonWhitespace : forall e. std.parser.Parser std.parser.ByteInput e ()
jsonWhitespace =
    std.parser.Parser
        (fun input =>
            case input of
                ByteInput bytes offset =>
                    let next =
                        std.bytes.bytesSpanAny bytes offset jsonWhitespaceBytes
                    in if next == offset
                        then std.parser.Parsed std.parser.EmptyInput () input
                        else std.parser.Parsed
                            std.parser.ConsumedInput
                            ()
                            (std.parser.ByteInput bytes next)
        )

jsonLexeme :
    forall e a. std.parser.Parser std.parser.ByteInput e a ->
    std.parser.Parser std.parser.ByteInput e a
jsonLexeme parser = parser <* jsonWhitespace

jsonSymbol : forall e. Int -> String -> std.parser.Parser std.parser.ByteInput e Int
jsonSymbol byte name =
    std.parser.keepRight
        jsonWhitespace
        (jsonLexeme (std.parser.token std.parser.byteInputOps name byte))

nullToken : forall e. std.parser.Parser std.parser.ByteInput e ()
nullToken =
    std.parser.mapResult
        (fun _ => ())
        (std.parser.tokens std.parser.byteInputOps "null" [110, 117, 108, 108])

boolToken : forall e. std.parser.Parser std.parser.ByteInput e Bool
boolToken =
    std.parser.choice
        std.parser.byteInputOps
        (std.parser.mapResult
            (fun _ => True)
            (std.parser.tokens std.parser.byteInputOps "true" [116, 114, 117, 101]))
        (std.parser.mapResult
            (fun _ => False)
            (std.parser.tokens std.parser.byteInputOps "false" [102, 97, 108, 115, 101]))

jsonValueAt : JsonLimits -> Int -> std.parser.Parser std.parser.ByteInput JsonError Json
jsonValueAt limits depth =
    std.parser.keepRight jsonWhitespace
        (std.parser.Parser (fun input => dispatchJsonValue limits depth input))

dispatchJsonValue :
    JsonLimits ->
    Int -> std.parser.ByteInput -> std.parser.ParseReply std.parser.ByteInput JsonError Json
dispatchJsonValue limits depth input =
    case byteInputValue input of
        Just 110 => std.parser.runParser (std.parser.mapResult (fun _ => JsonNull) nullToken) input
        | Just 116 => std.parser.runParser (std.parser.mapResult JsonBool boolToken) input
        | Just 102 => std.parser.runParser (std.parser.mapResult JsonBool boolToken) input
        | Just 34 =>
            std.parser.runParser (std.parser.mapResult JsonString (stringToken limits)) input
        | Just 91 =>
            std.parser.runParser
                (std.parser.mapResult
                    JsonArray
                    (arrayWith id limits depth (fun _ => jsonValueAt limits (depth + 1))))
                input
        | Just 123 =>
            std.parser.runParser
                (std.parser.mapResult
                    JsonObject
                    (objectWith id limits depth (fun _ => jsonValueAt limits (depth + 1))))
                input
        | Just 45 =>
            std.parser.runParser (std.parser.mapResult JsonNumberValue (numberToken limits)) input
        | Just byte =>
            if isDigitByte byte
                then std.parser.runParser
                    (std.parser.mapResult JsonNumberValue (numberToken limits))
                    input
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
            Right text => std.parser.Parsed std.parser.ConsumedInput (JsonNumber text) rest
            | Left (InvalidUtf8 offset) =>
                consumedRejectJson rest (JsonInvalidUtf8 offset)

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
            Left (InvalidUtf8 offset) =>
                consumedRejectJson boundaryInput (JsonInvalidUtf8 offset)
            | Right chunk =>
                let accumulated = prependReversed (std.text.stringToChars chunk) reversed in
                case std.bytes.bytesIndex bytes boundary of
                    Nothing => consumedRejectJson boundaryInput JsonUnterminatedString
                    | Just 34 =>
                        std.parser.Parsed std.parser.ConsumedInput
                            (std.text.charsToString (std.list.reverse accumulated))
                            (std.parser.ByteInput bytes (boundary + 1))
                    | Just 92 =>
                        case decodeEscape bytes (boundary + 1) of
                            Left error => consumedRejectJson boundaryInput error
                            | Right (escaped, next) =>
                                scanStringAt limits bytes contentStart next
                                    (prependReversed (std.text.stringToChars escaped) accumulated)
                    | Just byte => consumedRejectJson boundaryInput (JsonUnescapedControl byte)

prependReversed : forall a. [a] -> [a] -> [a]
prependReversed values reversed = prependReversedInto values reversed

prependReversedInto : forall a. [a] -> [a] -> [a]
prependReversedInto [] reversed = reversed
prependReversedInto (value :: rest) reversed =
    prependReversedInto rest (value :: reversed)

decodeEscape : Bytes -> Int -> Either JsonError (String, Int)
decodeEscape bytes offset =
    case std.bytes.bytesIndex bytes offset of
        Nothing => Left JsonUnterminatedString
        | Just 34 => Right ("\"", (offset + 1))
        | Just 92 => Right ("\\", (offset + 1))
        | Just 47 => Right ("/", (offset + 1))
        | Just 98 => Right ("\u{8}", (offset + 1))
        | Just 102 => Right ("\u{c}", (offset + 1))
        | Just 110 => Right ("\n", (offset + 1))
        | Just 114 => Right ("\u{d}", (offset + 1))
        | Just 116 => Right ("\t", (offset + 1))
        | Just 117 => decodeUnicodeEscape bytes (offset + 1)
        | Just byte => Left (JsonInvalidEscape ("byte " <> show byte))

decodeUnicodeEscape : Bytes -> Int -> Either JsonError (String, Int)
decodeUnicodeEscape bytes offset =
    case readHex4 bytes offset of
        Nothing => Left JsonInvalidUnicodeEscape
        | Just high =>
            if and (high >= 55296) (high <= 56319) then
                let slash = offset + 4 in
                if and (byteAt bytes slash == 92) (byteAt bytes (slash + 1) == 117) then
                    case readHex4 bytes (slash + 2) of
                        Just low =>
                            if and (low >= 56320) (low <= 57343) then
                                scalarString (65536 + ((high - 55296) * 1024) + (low - 56320))
                                    (slash + 6)
                            else Left JsonInvalidUnicodeEscape
                        | Nothing => Left JsonInvalidUnicodeEscape
                else Left JsonInvalidUnicodeEscape
            else if and (high >= 56320) (high <= 57343) then
                Left JsonInvalidUnicodeEscape
            else scalarString high (offset + 4)

scalarString : Int -> Int -> Either JsonError (String, Int)
scalarString codePoint next =
    case std.text.codePointToString codePoint of
        Just text => Right (text, next)
        | Nothing => Left JsonInvalidUnicodeEscape

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

arrayWith :
    forall e a. (JsonError -> e) ->
    JsonLimits ->
    Int ->
    (Int -> std.parser.Parser std.parser.ByteInput e a) ->
    std.parser.Parser std.parser.ByteInput e (std.vector.Vector a)
arrayWith inject limits depth element =
    if depth >= maximumDepth limits
        then std.parser.customFailure
            std.parser.byteInputOps
            (inject (JsonDepthExceeded (maximumDepth limits)))
        else std.parser.keepRight
            (jsonSymbol 91 "[")
            (std.parser.choice
                std.parser.byteInputOps
                (std.parser.mapResult (fun _ => std.vector.empty) (jsonSymbol 93 "]"))
                (arrayItems inject limits element [] 0))

arrayItems :
    forall e a. (JsonError -> e) ->
    JsonLimits ->
    (Int -> std.parser.Parser std.parser.ByteInput e a) ->
    [a] -> Int -> std.parser.Parser std.parser.ByteInput e (std.vector.Vector a)
arrayItems inject limits element reversed count =
    std.parser.andThen
        (element count)
        (fun value =>
            let nextCount =
                count + 1
            in if nextCount > maximumContainerItems limits
                then std.parser.customFailure
                    std.parser.byteInputOps
                    (inject (JsonContainerTooLarge (maximumContainerItems limits)))
                else std.parser.choice
                    std.parser.byteInputOps
                    (std.parser.mapResult
                        (fun _ => std.vector.fromList (std.list.reverse (value :: reversed)))
                        (jsonSymbol 93 "]"))
                    (std.parser.keepRight
                        (jsonSymbol 44 ",")
                        (std.parser.defer
                        (fun _ => arrayItems inject limits element (value :: reversed) nextCount)))
        )

objectWith :
    forall e a. (JsonError -> e) ->
    JsonLimits ->
    Int ->
    (String -> std.parser.Parser std.parser.ByteInput e a) ->
    std.parser.Parser std.parser.ByteInput e (std.map.Map String a)
objectWith inject limits depth valueParser =
    if depth >= maximumDepth limits
        then std.parser.customFailure
            std.parser.byteInputOps
            (inject (JsonDepthExceeded (maximumDepth limits)))
        else std.parser.keepRight
            (jsonSymbol 123 "{")
            (std.parser.choice
                std.parser.byteInputOps
                (std.parser.mapResult (fun _ => std.map.empty) (jsonSymbol 125 "}"))
                (objectItems inject limits valueParser std.map.empty 0))

objectItems :
    forall e a. (JsonError -> e) ->
    JsonLimits ->
    (String -> std.parser.Parser std.parser.ByteInput e a) ->
    std.map.Map String a -> Int -> std.parser.Parser std.parser.ByteInput e (std.map.Map String a)
objectItems inject limits valueParser values count =
    std.parser.andThen
        (std.parser.mapCustomError inject (jsonLexeme (stringToken limits)))
        (fun key =>
            if std.map.member key values
                then std.parser.customFailure
                    std.parser.byteInputOps
                    (inject (JsonDuplicateObjectKey key))
                else std.parser.keepRight
                    (jsonSymbol 58 ":")
                    (std.parser.andThen
                        (valueParser key)
                        (fun value =>
                            let nextCount =
                                count + 1
                            in let nextValues =
                                std.map.insert key value values
                            in if nextCount > maximumContainerItems limits
                            then std.parser.customFailure
                            std.parser.byteInputOps
                            (inject (JsonContainerTooLarge (maximumContainerItems limits)))
                            else std.parser.choice
                            std.parser.byteInputOps
                            (std.parser.mapResult (fun _ => nextValues) (jsonSymbol 125 "}"))
                            (std.parser.keepRight
                            (jsonSymbol 44 ",")
                            (std.parser.defer
                            (fun _ => objectItems inject limits valueParser nextValues nextCount)))
                        ))
        )

renderValueInto : JsonLimits -> Int -> Json -> [Char] -> Either JsonError [Char]
renderValueInto _ _ JsonNull suffix = Right ('n' :: 'u' :: 'l' :: 'l' :: suffix)
renderValueInto _ _ (JsonBool True) suffix = Right ('t' :: 'r' :: 'u' :: 'e' :: suffix)
renderValueInto _ _ (JsonBool False) suffix = Right ('f' :: 'a' :: 'l' :: 's' :: 'e' :: suffix)
renderValueInto limits _ (JsonNumberValue (JsonNumber number)) suffix =
    if std.bytes.bytesLength (std.bytes.stringToBytes number) > maximumNumberBytes limits then
        Left (JsonNumberTooLarge (maximumNumberBytes limits))
    else case validateNumberWith limits number of
        Nothing => Left (JsonInvalidNumber number)
        | Just _ => Right (prependChars number suffix)
renderValueInto limits _ (JsonString value) suffix = renderStringInto limits value suffix
renderValueInto limits depth (JsonArray values) suffix =
    if depth >= maximumDepth limits then Left (JsonDepthExceeded (maximumDepth limits))
    else if std.vector.length values > maximumContainerItems limits then
        Left (JsonContainerTooLarge (maximumContainerItems limits))
    else
        case renderArrayReverse limits (depth + 1)
                (std.list.reverse (std.vector.toList values)) True (']' :: suffix) of
            Left error => Left error
            | Right body => Right ('[' :: body)
renderValueInto limits depth (JsonObject values) suffix =
    if depth >= maximumDepth limits then Left (JsonDepthExceeded (maximumDepth limits))
    else if std.map.size values > maximumContainerItems limits then
        Left (JsonContainerTooLarge (maximumContainerItems limits))
    else
        case renderObjectReverse limits (depth + 1)
                (std.list.reverse (std.map.toAscList values)) True ('}' :: suffix) of
            Left error => Left error
            | Right body => Right ('{' :: body)

renderArrayReverse : JsonLimits -> Int -> [Json] -> Bool -> [Char] -> Either JsonError [Char]
renderArrayReverse _ _ [] _ suffix = Right suffix
renderArrayReverse limits depth (value :: rest) first suffix =
    let separated = if first then suffix else ',' :: suffix in
    case renderValueInto limits depth value separated of
        Left error => Left error
        | Right rendered => renderArrayReverse limits depth rest False rendered

renderObjectReverse : JsonLimits -> Int -> [(String, Json)] -> Bool -> [Char] -> Either JsonError [Char]
renderObjectReverse _ _ [] _ suffix = Right suffix
renderObjectReverse limits depth ((key, value) :: rest) first suffix =
    let separated = if first then suffix else ',' :: suffix in
    case renderValueInto limits depth value separated of
        Left error => Left error
        | Right renderedValue =>
            case renderStringInto limits key (':' :: renderedValue) of
                Left error => Left error
                | Right renderedEntry =>
                    renderObjectReverse limits depth rest False renderedEntry

renderStringInto : JsonLimits -> String -> [Char] -> Either JsonError [Char]
renderStringInto limits value suffix =
    let characters = std.text.stringToChars value in
    let length = escapedStringLength characters 0 in
    if length > maximumStringBytes limits then
        Left (JsonStringTooLarge (maximumStringBytes limits))
    else Right ('"' :: escapeStringInto (std.list.reverse characters) ('"' :: suffix))

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
