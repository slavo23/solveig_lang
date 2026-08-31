import std.bytes(*)
import std.json(*)
import std.list
import std.map(*)
import std.parser(*)
import std.text
import std.vector(*)

data Person = Person String Bool (Maybe String)

instance Show Person where
    show (Person name active nickname) =
        "Person " <> show name <> " " <> show active <> " " <> show nickname

data Event = Started String | Stopped Bool

instance Show Event where
    show (Started name) = "Started " <> show name
    show (Stopped clean) = "Stopped " <> show clean

personDecoder : std.json.Decoder () Person
personDecoder =
    std.json.recordDecoder
        (Person <$>
            std.json.requiredField "name" std.json.stringDecoder <*>
            std.json.optionalFieldWithDefault "active" True std.json.boolDecoder <*>
            std.json.optionalField "nickname" std.json.stringDecoder)

personEncoder : std.json.Encoder Person
personEncoder =
    std.json.recordEncoder
        [
            std.json.requiredFieldEncoder
                "name"
                (fun (Person name _ _) => name)
                std.json.stringEncoder,
            std.json.requiredFieldEncoder
                "active"
                (fun (Person _ active _) => active)
                std.json.boolEncoder,
            std.json.optionalFieldEncoder
                "nickname"
                (fun (Person _ _ nickname) => nickname)
                std.json.stringEncoder
        ]

eventDecoder : std.json.Decoder () Event
eventDecoder =
    std.json.taggedDecoder
        "type"
        [
            ("started", Started <$> std.json.requiredField "name" std.json.stringDecoder),
            ("stopped", Stopped <$> std.json.requiredField "clean" std.json.boolDecoder)
        ]

startedProjection : Event -> Maybe String
startedProjection (Started name) = Just name
startedProjection _ = Nothing

stoppedProjection : Event -> Maybe Bool
stoppedProjection (Stopped clean) = Just clean
stoppedProjection _ = Nothing

eventEncoder : std.json.Encoder Event
eventEncoder =
    std.json.taggedEncoder
        "type"
        [
            std.json.variantEncoder
                "started"
                startedProjection
                [std.json.requiredFieldEncoder "name" (fun name => name) std.json.stringEncoder],
            std.json.variantEncoder
                "stopped"
                stoppedProjection
                [std.json.requiredFieldEncoder "clean" (fun clean => clean) std.json.boolEncoder]
        ]

replyText :
    forall e a. Show e => (a -> String) -> std.parser.ParseReply std.parser.ByteInput e a -> String
replyText showValue reply =
    std.parser.foldReply
        (fun _ value _ => showValue value)
        (fun _ _ error => show error)
        (fun _ _ _ => "unexpected suspension")
        reply

decodedText :
    forall a. (a -> String) ->
    std.parser.ParseReply std.parser.ByteInput (std.json.DecodeError ()) a -> String
decodedText = replyText

renderedJson : std.json.Json -> String
renderedJson value =
    case std.json.renderJson value of
        Right text => text
        | Left error => std.json.renderJsonError error

showStrings : std.vector.Vector String -> String
showStrings values = show (std.vector.toList values)

showBools : std.map.Map String Bool -> String
showBools values = show (std.map.keys values) <> " => " <> show (std.map.values values)

printCase : String -> String -> Task ()
printCase name value = printLine (name <> ": " <> value)

encoded : forall a. std.json.Encoder a -> a -> String
encoded encoder value =
    case std.json.encodeJson std.json.defaultJsonLimits encoder value of
        Left error => std.json.renderJsonError error
        | Right text => text

exactInt : String -> Maybe Int
exactInt text =
    case std.json.jsonNumberFromString text of
        Nothing => Nothing
        | Just number => std.json.jsonNumberToInt number

-- No Show instance: validation errors must survive decoding as typed data.
data NameError = EmptyName

nonemptyName : String -> Either NameError String
nonemptyName "" = Left EmptyName
nonemptyName name = Right name

validatedNames : std.json.Decoder NameError (std.vector.Vector String)
validatedNames =
    std.json.arrayDecoder
        (std.json.recordDecoder
            (std.json.requiredField
                "name"
                (std.json.validateDecoder nonemptyName std.json.stringDecoder)))

customPath : Either (std.json.DecodeError NameError) (std.vector.Vector String) -> String
customPath (Left
        (std.json.CustomDecodeError
            (std.json.JsonIndex index :: std.json.JsonField name :: [])
            EmptyName)) =
    show index <> ":" <> name
customPath _ = "missing typed error or incorrect path"

customReplyPath :
    std.parser.ParseReply
        std.parser.ByteInput
        (std.json.DecodeError NameError)
        (std.vector.Vector String) ->
    String
customReplyPath =
    std.parser.foldReply
        (fun _ _ _ => "unexpected success")
        (fun _ _ error => customErrorsPath (std.parser.errorCustom error))
        (fun _ _ _ => "unexpected suspension")

customErrorsPath : [std.json.DecodeError NameError] -> String
customErrorsPath (custom :: []) = customPath (Left custom)
customErrorsPath _ = "expected one custom error"

codecRoundTrip : forall a. Show a => std.json.Encoder a -> std.json.Decoder () a -> a -> String
codecRoundTrip encoder decoder value =
    case std.json.encodeJson std.json.defaultJsonLimits encoder value of
        Left error => show error
        | Right text =>
            decodedText
                show
                (std.json.decodeJson
                    std.json.defaultJsonLimits
                    decoder
                    (std.bytes.stringToBytes text))

largeBools : std.json.Decoder () (std.vector.Vector Bool)
largeBools = std.json.arrayDecoder std.json.boolDecoder

largeArrayRoundTrip : String
largeArrayRoundTrip =
    let values =
        std.vector.fromList (map (fun n => mod n 2 == 0) (std.list.range 0 20000))
    in case std.json.encodeJsonValue (std.json.arrayEncoder std.json.boolEncoder) values of
        Left error => show error
        | Right json =>
            case std.json.decodeJsonValue largeBools json of
                Left error => show error
                | Right decoded =>
                    show (decoded == values) <> ":" <> show (std.vector.length decoded)

longExactNumbers : [Maybe Int]
longExactNumbers =
    let zeros =
        std.text.charsToString (map (const '0') (std.list.range 0 2999))
    in map
        (std.json.jsonNumberToInt . std.json.JsonNumber)
        [
            "0." <> zeros <> "1e3000",
            "1" <> zeros <> "0e-3000",
            "-0." <> zeros <> "1e3000",
            "0e999999999999999999999999999999",
            "1e999999999999999999999999999999",
            "1e-999999999999999999999999999999"
        ]

largeExact : String
largeExact =
    case std.json.jsonNumberFromString "123456789012345678901234567890.0001e-99" of
        Just number => std.json.jsonNumberText number
        | Nothing => "rejected"

manualJson : std.json.Json
manualJson =
    std.json.JsonObject
        (std.map.fromList
            [("line\n", std.json.JsonString "tab\tquote\""), ("flag", std.json.JsonBool True)])

roundTrip : std.json.Json -> String
roundTrip value =
    case std.json.renderJson value of
        Left error => std.json.renderJsonError error
        | Right text =>
            replyText
                renderedJson
                (std.json.parseJson std.json.defaultJsonLimits (std.bytes.stringToBytes text))

utf8SliceText : Either Utf8Error String -> String
utf8SliceText (Right text) = text
utf8SliceText (Left (InvalidUtf8 _)) = "invalid UTF-8"
utf8SliceText (Left _) = "unexpected error"

validNumber : String -> Bool
validNumber text =
    case std.json.jsonNumberFromString text of
        Just _ => True
        | Nothing => False

main : Task ()
main = do
    let source =
        " { \"z\" : [true, null, -12.50e+2], \"a\" : \"snowman \\u2603 / pair \\uD83D\\uDE80\" } "
    printCase
        "round trip"
        (replyText
            renderedJson
            (std.json.parseJson std.json.defaultJsonLimits (std.bytes.stringToBytes source)))
    printCase "large exact" largeExact
    printCase
        "duplicate"
        (replyText
            renderedJson
            (std.json.parseJson
                std.json.defaultJsonLimits
                (std.bytes.stringToBytes "{\"x\":1,\"x\":2}")))
    printCase
        "leading zero"
        (replyText
            renderedJson
            (std.json.parseJson std.json.defaultJsonLimits (std.bytes.stringToBytes "01")))
    printCase
        "bad surrogate"
        (replyText
            renderedJson
            (std.json.parseJson
                std.json.defaultJsonLimits
                (std.bytes.stringToBytes "\"\\uD800x\"")))
    printCase
        "unescaped control"
        (replyText
            renderedJson
            (std.json.parseJson
                std.json.defaultJsonLimits
                (std.bytes.stringToBytes "\"line\nbreak\"")))
    let invalidUtf8 = std.bytes.bytesSlice (std.bytes.stringToBytes "☃") 0 1
    let invalidUtf8String =
        std.bytes.bytesAppend
            (std.bytes.stringToBytes "\"")
            (std.bytes.bytesAppend invalidUtf8 (std.bytes.stringToBytes "\""))
    printCase
        "invalid UTF-8 JSON"
        (replyText renderedJson (std.json.parseJson std.json.defaultJsonLimits invalidUtf8String))
    printCase
        "number grammar"
        (show
            (map
                validNumber
                ["0", "-0", "1.25", "1e2", "1E-2", "+1", ".1", "1.", "1e", "00", "--1"]))
    printCase
        "string limit"
        (replyText
            renderedJson
            (std.json.parseJson
                (std.json.JsonLimits 100 8 3 10 20)
                (std.bytes.stringToBytes "\"four\"")))
    printCase
        "depth limit"
        (replyText
            renderedJson
            (std.json.parseJson
                (std.json.JsonLimits 100 1 20 10 20)
                (std.bytes.stringToBytes "[[0]]")))
    printCase
        "input limit"
        (replyText
            renderedJson
            (std.json.parseJson
                (std.json.JsonLimits 2 8 20 10 20)
                (std.bytes.stringToBytes "true")))
    printCase
        "number limit"
        (replyText
            renderedJson
            (std.json.parseJson
                (std.json.JsonLimits 100 8 20 10 2)
                (std.bytes.stringToBytes "123")))
    printCase
        "container limit"
        (replyText
            renderedJson
            (std.json.parseJson
                (std.json.JsonLimits 100 8 20 1 20)
                (std.bytes.stringToBytes "[1,2]")))
    printCase
        "direct strings"
        (decodedText
            showStrings
            (std.json.decodeJson
                std.json.defaultJsonLimits
                (std.json.arrayDecoder std.json.stringDecoder)
                (std.bytes.stringToBytes "[\"one\",\"two\"]")))
    printCase
        "direct object"
        (decodedText
            showBools
            (std.json.decodeJson
                std.json.defaultJsonLimits
                (std.json.objectDecoder std.json.boolDecoder)
                (std.bytes.stringToBytes "{\"b\":false,\"a\":true}")))
    printCase
        "record default"
        (replyText
            show
            (std.json.decodeJson
                std.json.defaultJsonLimits
                personDecoder
                (std.bytes.stringToBytes "{\"name\":\"Ada\"}")))
    printCase
        "present null is not default"
        (replyText
            show
            (std.json.decodeJson
                std.json.defaultJsonLimits
                personDecoder
                (std.bytes.stringToBytes "{\"name\":\"Ada\",\"active\":null}")))
    printCase "record encode" (encoded personEncoder (Person "Ada" False Nothing))
    let peopleDecoder =
        std.json.recordDecoder
            (std.json.requiredField "people" (std.json.arrayDecoder personDecoder))
    printCase
        "nested path"
        (replyText
            show
            (std.json.decodeJson
                std.json.defaultJsonLimits
                peopleDecoder
                (std.bytes.stringToBytes "{\"people\":[{\"name\":\"Ada\"},{\"name\":false}]}")))
    printCase
        "tagged decode"
        (replyText
            show
            (std.json.decodeJson
                std.json.defaultJsonLimits
                eventDecoder
                (std.bytes.stringToBytes "{\"type\":\"started\",\"name\":\"build\"}")))
    printCase "tagged encode" (encoded eventEncoder (Stopped True))
    printCase
        "unknown tag"
        (replyText
            show
            (std.json.decodeJson
                std.json.defaultJsonLimits
                eventDecoder
                (std.bytes.stringToBytes "{\"type\":\"paused\"}")))
    printCase
        "exact ints"
        (show
            (map
                exactInt
                [
                    "1.0",
                    "12e2",
                    "1.5",
                    "1000e-3",
                    "1.2300e2",
                    "9223372036854775807",
                    "-9223372036854775808",
                    "9223372036854775808",
                    "-9223372036854775809",
                    "0e999"
                ]))
    printCase "forged exact int" (show (std.json.jsonNumberToInt (std.json.JsonNumber "01")))
    printCase "long exact numbers" (show longExactNumbers)
    printCase "large array round trip" largeArrayRoundTrip
    printCase
        "record round trip"
        (codecRoundTrip personEncoder personDecoder (Person "Ada" False (Just "A")))
    printCase "variant round trip" (codecRoundTrip eventEncoder eventDecoder (Started "build"))
    let invalidNames = "[{\"name\":\"Ada\"},{\"name\":\"\"}]"
    printCase
        "typed direct path"
        (customReplyPath
            (std.json.decodeJson
                std.json.defaultJsonLimits
                validatedNames
                (std.bytes.stringToBytes invalidNames)))
    let invalidNameValues =
        std.json.JsonArray
            (std.vector.fromList
                [
                    std.json.JsonObject (std.map.singleton "name" (std.json.JsonString "Ada")),
                    std.json.JsonObject (std.map.singleton "name" (std.json.JsonString ""))
                ])
    printCase
        "typed value path"
        (customPath (std.json.decodeJsonValue validatedNames invalidNameValues))
    printCase
        "mapped validation"
        (replyText
            showStrings
            (std.json.decodeJson
                std.json.defaultJsonLimits
                (std.json.mapDecoderError (fun EmptyName => "empty name") validatedNames)
                (std.bytes.stringToBytes invalidNames)))
    printCase
        "direct record path"
        (decodedText
            show
            (std.json.decodeJson
                std.json.defaultJsonLimits
                (std.json.objectDecoder (std.json.arrayDecoder personDecoder))
                (std.bytes.stringToBytes "{\"team\":[{\"name\":false}]}")))
    printCase
        "direct syntax path"
        (decodedText
            showStrings
            (std.json.decodeJson
                std.json.defaultJsonLimits
                (std.json.arrayDecoder std.json.stringDecoder)
                (std.bytes.stringToBytes "[\"ok\",false]")))
    printCase
        "duplicate encoder field"
        (encoded
            (std.json.recordEncoder
                [
                    std.json.requiredFieldEncoder "x" id std.json.boolEncoder,
                    std.json.requiredFieldEncoder "x" id std.json.boolEncoder
                ])
            True)
    printCase
        "encoder tag collision"
        (encoded
            (std.json.taggedRecordEncoder
                "type"
                "flag"
                [std.json.requiredFieldEncoder "type" id std.json.boolEncoder])
            True)
    printCase "unmatched encoder variant" (encoded (std.json.taggedEncoder "type" []) True)
    printCase
        "encoder nested failure"
        (encoded
            (std.json.arrayEncoder std.json.numberEncoder)
            (std.vector.fromList [std.json.JsonNumber "1", std.json.JsonNumber "NaN"]))
    printCase "render escapes" (renderedJson manualJson)
    printCase "render round trip" (roundTrip manualJson)
    printCase "forged number" (renderedJson (std.json.JsonNumberValue (std.json.JsonNumber "NaN")))
    let scanBytes = std.bytes.stringToBytes "123,\n"
    printCase
        "bulk scans"
        (show
            [
                std.bytes.bytesSpanRange scanBytes 0 48 57,
                std.bytes.bytesFindAny scanBytes 0 (std.bytes.stringToBytes ","),
                std.bytes.bytesFindBelow scanBytes 0 32,
                std.bytes.bytesSpanAny scanBytes 4 (std.bytes.stringToBytes "\n")
            ])
    let snowmanBytes = std.bytes.stringToBytes "☃"
    printCase "UTF-8 slice" (utf8SliceText (std.bytes.bytesSliceToString snowmanBytes 0 3))
    printCase "invalid UTF-8 slice" (utf8SliceText (std.bytes.bytesSliceToString snowmanBytes 0 1))
