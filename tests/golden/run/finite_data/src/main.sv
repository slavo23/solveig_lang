import std.bytes(
    BytesBuilder, appendBytesBuilder, buildBytes, bytesBuilder, bytesFindAny, bytesLength,
    bytesSlice, bytesToString, bytesToStringReplacing, emptyBytesBuilder, stringToBytes
)
import std.list(drop, length, range, take, takeWhile, zipWith)
import std.text(
    TextBuilder, appendTextBuilder, buildText, byteLength, byteSlice, charAt,
    emptyTextBuilder, length as textLength, lines, slice, splitOnce, startsWith,
    textBuilder, trimAscii
)

ensure : String -> Bool -> Task ()
ensure label condition =
    if condition then printLine ("ok: " <> label)
    else error ("failed: " <> label)

lastOr : forall a. a -> [a] -> a
lastOr fallback [] = fallback
lastOr fallback (value :: rest) = lastFrom value rest

lastFrom : forall a. a -> [a] -> a
lastFrom current [] = current
lastFrom current (value :: rest) = lastFrom value rest

buildTextChunks : Int -> TextBuilder -> TextBuilder
buildTextChunks count builder =
    if count <= 0 then builder
    else buildTextChunks (count - 1) (appendTextBuilder builder (textBuilder "ab"))

buildByteChunks : Int -> BytesBuilder -> BytesBuilder
buildByteChunks count builder =
    if count <= 0 then builder
    else
        buildByteChunks
            (count - 1)
            (appendBytesBuilder builder (bytesBuilder (stringToBytes "xy")))

checkLists : Task ()
checkLists =
    let values = range 0 20000 in
    let mapped = map (fun value => value + 1) values in
    let selected = filter even values in
    let prefix = take 15000 values in
    let whilePrefix = takeWhile (fun value => value < 17500) values in
    let zipped = zipWith (+) values values in
    let appended = append values [20000] in
    let flattened = concat [take 7000 values, take 6000 (drop 7000 values), [42]] in
    let selectedM = filterM (fun value => Just (even value)) values in
    ensure
        "stack-safe List transformations"
        (and
            (and
                (and (length mapped == 20000) (lastOr 0 mapped == 20000))
                (and (length selected == 10000) (lastOr 0 selected == 19998)))
            (and
                (and
                    (and (length prefix == 15000) (lastOr 0 prefix == 14999))
                    (and (length whilePrefix == 17500) (lastOr 0 whilePrefix == 17499)))
                (and
                    (and (length zipped == 20000) (lastOr 0 zipped == 39998))
                    (and
                        (and (length appended == 20001) (lastOr 0 appended == 20000))
                        (and
                            (and (length flattened == 13001) (lastOr 0 flattened == 42))
                            (selectedSummary selectedM))))))

selectedSummary : Maybe [Int] -> Bool
selectedSummary (Just kept) =
    and (length kept == 10000) (lastOr 0 kept == 19998)
selectedSummary Nothing = False

checkTextUnits : Task ()
checkTextUnits =
    let text = "aé👩‍🔬é" in
    ensure
        "UTF-8 byte and scalar units"
        (and
            (and (textLength text == 7) (byteLength text == 17))
            (and
                (charAt 1 text == Just 'é')
                (and
                    (slice 2 3 text == "👩‍🔬")
                    (and
                        (byteSlice 1 2 text == Just "é")
                        (byteSlice 2 1 text == Nothing)))))

checkPackedText : Task ()
checkPackedText =
    ensure
        "packed text scans"
        (and
            (startsWith "λ=" "λ=value")
            (and
                (trimAscii " \t café \n" == "café")
                (and
                    (lines "α\n\nβ\n" == ["α", "", "β"])
                    (splitOnce 'é' "café noir" == Just ("caf", " noir")))))

checkUtf8 : Task ()
checkUtf8 =
    let malformed = bytesSlice (stringToBytes "é") 0 1 in
    ensure
        "strict and replacement UTF-8 decoding"
        (and
            (isInvalidUtf8AtStart (bytesToString malformed))
            (bytesToStringReplacing malformed == "�"))

isInvalidUtf8AtStart : Either Utf8Error String -> Bool
isInvalidUtf8AtStart (Left (InvalidUtf8 0)) = True
isInvalidUtf8AtStart _ = False

checkBuilders : Task ()
checkBuilders =
    let textPlan = buildTextChunks 20000 (textBuilder "start:") in
    let bytePlan = buildByteChunks 20000 (bytesBuilder (stringToBytes "start:")) in
    let text = buildText (emptyTextBuilder <> textPlan <> emptyTextBuilder) in
    let bytes = buildBytes (emptyBytesBuilder <> bytePlan <> emptyBytesBuilder) in
    ensure
        "linear stack-safe builders"
        (and
            (and
                (and (byteLength text == 40006) (textLength text == 40006))
                (and (startsWith "start:abab" text) (splitOnce 'z' text == Nothing)))
            (and
                (and
                    (bytesLength bytes == 40006)
                    (bytesFindAny bytes 0 (stringToBytes "z") == 40006))
                (and
                    (bytesToString (bytesSlice bytes 0 10) == Right "start:xyxy")
                    (buildText
                        ((textBuilder "a" <> textBuilder "b") <> textBuilder "c")
                        == buildText
                            (textBuilder "a" <> (textBuilder "b" <> textBuilder "c"))))))

main : Task ()
main = do
    checkLists
    checkTextUnits
    checkPackedText
    checkUtf8
    checkBuilders
