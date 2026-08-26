import std.bytes(*)
import std.json(*)
import std.map(*)
import std.parser(*)
import std.vector(*)

replyText : forall a. (a -> String) -> std.parser.ParseReply std.parser.ByteInput std.json.JsonError a -> String
replyText showValue reply =
    std.parser.foldReply
        (fun _ value _ => showValue value)
        (fun _ _ error => std.parser.renderError std.json.renderJsonError error)
        (fun _ _ _ => "unexpected suspension")
        reply

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

largeExact : String
largeExact =
    case std.json.jsonNumberFromString "123456789012345678901234567890.0001e-99" of
        Just number => std.json.jsonNumberText number
        | Nothing => "rejected"

manualJson : std.json.Json
manualJson = std.json.JsonObject
    (std.map.fromList
        [ ("line\n", std.json.JsonString "tab\tquote\"")
        , ("flag", std.json.JsonBool True)
        ])

roundTrip : std.json.Json -> String
roundTrip value =
    case std.json.renderJson value of
        Left error => std.json.renderJsonError error
        | Right text =>
            replyText renderedJson
                (std.json.parseJson std.json.defaultJsonLimits
                    (std.bytes.stringToBytes text))

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
    let source = " { \"z\" : [true, null, -12.50e+2], \"a\" : \"snowman \\u2603 / pair \\uD83D\\uDE80\" } "
    printCase "round trip"
        (replyText renderedJson
            (std.json.parseJson std.json.defaultJsonLimits
                (std.bytes.stringToBytes source)))
    printCase "large exact" largeExact
    printCase "duplicate"
        (replyText renderedJson
            (std.json.parseJson std.json.defaultJsonLimits
                (std.bytes.stringToBytes "{\"x\":1,\"x\":2}")))
    printCase "leading zero"
        (replyText renderedJson
            (std.json.parseJson std.json.defaultJsonLimits
                (std.bytes.stringToBytes "01")))
    printCase "bad surrogate"
        (replyText renderedJson
            (std.json.parseJson std.json.defaultJsonLimits
                (std.bytes.stringToBytes "\"\\uD800x\"")))
    printCase "unescaped control"
        (replyText renderedJson
            (std.json.parseJson std.json.defaultJsonLimits
                (std.bytes.stringToBytes "\"line\nbreak\"")))
    let invalidUtf8 = std.bytes.bytesSlice (std.bytes.stringToBytes "☃") 0 1
    let invalidUtf8String =
        std.bytes.bytesAppend (std.bytes.stringToBytes "\"")
            (std.bytes.bytesAppend invalidUtf8 (std.bytes.stringToBytes "\""))
    printCase "invalid UTF-8 JSON"
        (replyText renderedJson
            (std.json.parseJson std.json.defaultJsonLimits invalidUtf8String))
    printCase "number grammar"
        (show (map validNumber
            [ "0", "-0", "1.25", "1e2", "1E-2"
            , "+1", ".1", "1.", "1e", "00", "--1"
            ]))
    printCase "string limit"
        (replyText renderedJson
            (std.json.parseJson (std.json.JsonLimits 100 8 3 10 20)
                (std.bytes.stringToBytes "\"four\"")))
    printCase "depth limit"
        (replyText renderedJson
            (std.json.parseJson (std.json.JsonLimits 100 1 20 10 20)
                (std.bytes.stringToBytes "[[0]]")))
    printCase "input limit"
        (replyText renderedJson
            (std.json.parseJson (std.json.JsonLimits 2 8 20 10 20)
                (std.bytes.stringToBytes "true")))
    printCase "number limit"
        (replyText renderedJson
            (std.json.parseJson (std.json.JsonLimits 100 8 20 10 2)
                (std.bytes.stringToBytes "123")))
    printCase "container limit"
        (replyText renderedJson
            (std.json.parseJson (std.json.JsonLimits 100 8 20 1 20)
                (std.bytes.stringToBytes "[1,2]")))
    printCase "direct strings"
        (replyText showStrings
            (std.json.decodeJson std.json.defaultJsonLimits
                (std.json.arrayDecoder std.json.stringDecoder)
                (std.bytes.stringToBytes "[\"one\",\"two\"]")))
    printCase "direct object"
        (replyText showBools
            (std.json.decodeJson std.json.defaultJsonLimits
                (std.json.objectDecoder std.json.boolDecoder)
                (std.bytes.stringToBytes "{\"b\":false,\"a\":true}")))
    printCase "render escapes" (renderedJson manualJson)
    printCase "render round trip" (roundTrip manualJson)
    printCase "forged number"
        (renderedJson (std.json.JsonNumberValue (std.json.JsonNumber "NaN")))
    let scanBytes = std.bytes.stringToBytes "123,\n"
    printCase "bulk scans"
        (show
            [ std.bytes.bytesSpanRange scanBytes 0 48 57
            , std.bytes.bytesFindAny scanBytes 0 (std.bytes.stringToBytes ",")
            , std.bytes.bytesFindBelow scanBytes 0 32
            , std.bytes.bytesSpanAny scanBytes 4 (std.bytes.stringToBytes "\n")
            ])
    let snowmanBytes = std.bytes.stringToBytes "☃"
    printCase "UTF-8 slice" (utf8SliceText (std.bytes.bytesSliceToString snowmanBytes 0 3))
    printCase "invalid UTF-8 slice"
        (utf8SliceText (std.bytes.bytesSliceToString snowmanBytes 0 1))
