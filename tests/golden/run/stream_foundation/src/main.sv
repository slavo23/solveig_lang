import std.bytes(
    BytesBuilder,
    buildBytes,
    bytesBuilder,
    bytesToString,
    emptyBytesBuilder,
    stringToBytes
)
import std.channel(newChan, recv, send, tryRecv)
import std.fs(FsError(..))
import std.list(range)
import std.path(Path, pathFromString)
import std.stream
import std.task(catchDefect, finally)
import std.thread(killThread, spawn)

data PullError = PlannedPull

data ObservedError = ObservedPull

fsToHost : FsError -> HostError
fsToHost (FsHostError _ hostError) = hostError
fsToHost error = HostError InvalidArgument "filesystem" (show error)

requiredPath : String -> Path
requiredPath spelling =
    case pathFromString spelling of
        Right path => path
        | Left pathError => error (show pathError)

withFileByteStream :
    forall result. Int ->
    String -> (forall scope. std.stream.Stream scope HostError Bytes -> Task result) -> Task result
withFileByteStream chunkSize spelling use =
    let path =
        requiredPath spelling
    in mapError fsToHost $
        std.fs.withFileByteStream
            chunkSize
            path
            (fun stream => mapError (FsHostError path) $ use (std.stream.mapError fsToHost stream))

ensure : String -> Bool -> Task ()
ensure label condition =
    if condition then printLine ("ok: " <> label) else error ("failed: " <> label)

sumValues : Int -> Int -> Int
sumValues total value = total + value

checkFiniteAlgebra : Task ()
checkFiniteAlgebra = do
    let values = range 0 20000
    total <- std.stream.fold sumValues 0 (std.stream.fromList values)
    transformed <-
        std.stream.fold
            sumValues
            0
            (std.stream.take
                5
                (std.stream.filter
                    even
                    (std.stream.map (fun value => value + 1) (std.stream.fromList values))))
    chunks <-
        case std.stream.chunksOf 3 (std.stream.fromList [1, 2, 3, 4, 5, 6, 7, 8]) of
            Left streamError => pureIO (Left streamError)
            | Right stream => std.stream.toListLimited 3 stream
    singletonValues <-
        std.stream.toListLimited
            1
            (std.stream.map (fun value => value + 1) (std.stream.singleton 6))
    emptyValues <- std.stream.toListLimited 0 std.stream.empty
    ensure
        "stack-safe finite stream algebra"
        (and
            (and (total == 199990000) (transformed == 30))
            (and
                (chunks == Right [[1, 2, 3], [4, 5, 6], [7, 8]])
                (and (singletonValues == Right [7]) (emptyValues == emptyIntValues))))

emptyIntValues : Either std.stream.StreamError [Int]
emptyIntValues = Right []

checkFiniteAdapters : Task ()
checkFiniteAdapters = do
    scalarCount <-
        std.stream.fold (fun count character => count + 1) 0 (std.stream.fromText "aé👩‍🔬é")
    byteValues <- std.stream.toListLimited 3 (std.stream.fromBytes (stringToBytes "Aé"))
    ensure
        "packed text and byte adapters"
        (and (scalarCount == 7) (byteValues == Right [65, 195, 169]))

countedStep : Chan Int -> Int -> Task (Maybe (Int, Int))
countedStep observed value =
    if value == 10
        then pureIO Nothing
        else do
                send observed value
                pureIO (Just (value, value + 1))

checkBackpressure : Task ()
checkBackpressure = do
    observed <- newChan
    total <-
        std.stream.fold sumValues 0 (std.stream.take 3 (std.stream.unfold (countedStep observed) 0))
    first <- tryRecv observed
    second <- tryRecv observed
    third <- tryRecv observed
    extra <- tryRecv observed
    ordered <- newChan
    std.stream.forEach (send ordered) (std.stream.fromList [3, 1, 2])
    orderedFirst <- recv ordered
    orderedSecond <- recv ordered
    orderedThird <- recv ordered
    ensure
        "pull backpressure and early take"
        (and
            (total == 3)
            (and
                (first == Just 0)
                (and
                    (second == Just 1)
                    (and
                        (third == Just 2)
                        (and
                            (extra == Nothing)
                            (and
                                (orderedFirst == 3)
                                (and (orderedSecond == 1) (orderedThird == 2))))))))

failingStep : Int -> IO PullError (Maybe (Int, Int))
failingStep value = if value == 3 then failIO PlannedPull else pureIO (Just (value, value + 1))

observePullError : ObservedError -> Task Int
observePullError ObservedPull = pureIO (-1)

checkTypedFailure : Task ()
checkTypedFailure = do
    observed <-
        catchError
            (std.stream.fold
                sumValues
                0
                (std.stream.mapError
                    (fun PlannedPull => ObservedPull)
                    (std.stream.unfold failingStep 0)))
            observePullError
    ensure "typed stream failure mapping" (observed == (-1))

checkBounds : Task ()
checkBounds = do
    tooMany <- std.stream.toListLimited 2 (std.stream.fromList [1, 2, 3])
    exact <- std.stream.toListLimited 3 (std.stream.fromList [1, 2, 3])
    invalidLimit <- std.stream.toListLimited (-1) (std.stream.fromList [1])
    let invalidChunk = invalidChunkResult
    ensure
        "bounded collection validation"
        (and
            (tooMany == Left (std.stream.StreamLimitExceeded 2))
            (and
                (exact == Right [1, 2, 3])
                (and
                    (invalidLimit == Left (std.stream.InvalidStreamLimit (-1)))
                    (isInvalidChunk invalidChunk))))

invalidChunkResult : Either std.stream.StreamError (std.stream.Stream () HostError [Int])
invalidChunkResult = std.stream.chunksOf 0 (std.stream.fromList [1])

isInvalidChunk :
    forall scope e a. Either std.stream.StreamError (std.stream.Stream scope e [a]) -> Bool
isInvalidChunk (Left (std.stream.InvalidChunkSize 0)) = True
isInvalidChunk _ = False

appendChunk : BytesBuilder -> Bytes -> BytesBuilder
appendChunk builder bytes = builder <> bytesBuilder bytes

readPrefix : forall scope. std.stream.Stream scope HostError Bytes -> Task String
readPrefix stream = do
    builder <- std.stream.fold appendChunk emptyBytesBuilder (std.stream.take 2 stream)
    case bytesToString (buildBytes builder) of
        Left utf8Error => error "fixture bytes were not UTF-8"
        | Right text => pureIO text

typedStreamFailure : forall scope. std.stream.Stream scope HostError Bytes -> Task ()
typedStreamFailure stream = failIO (HostError OtherError "stream-consumer" "planned")

reportTypedStreamFailure : HostError -> Task Bool
reportTypedStreamFailure hostError = pureIO (hostErrorOperation hostError == "stream-consumer")

defectStreamFailure : forall scope. std.stream.Stream scope HostError Bytes -> Task ()
defectStreamFailure stream = bindIO (pureIO ()) (fun ignored => pureIO (error "stream defect"))

reportDefect : Defect -> Task Bool
reportDefect (Error "stream defect") = pureIO True
reportDefect _ = pureIO False

interruptUse :
    forall scope. Chan () -> Chan () -> std.stream.Stream scope HostError Bytes -> Task ()
interruptUse ready gate stream = do
    first <- std.stream.next stream
    send ready ()
    waiting <- recv gate
    pureIO ()

streamWorker : Chan () -> Chan () -> Chan () -> Task ()
streamWorker ready gate cleaned =
    finally
        (withFileByteStream 2 "input.txt" (fun stream => interruptUse ready gate stream))
        (send cleaned ())

checkScopedFile : Task ()
checkScopedFile = do
    prefix <- withFileByteStream 3 "input.txt" readPrefix
    typed <-
        catchError
            (bindIO
                (withFileByteStream 2 "input.txt" typedStreamFailure)
                (fun ignored => pureIO False))
            reportTypedStreamFailure
    defect <-
        catchDefect
            (bindIO
                (withFileByteStream 2 "input.txt" defectStreamFailure)
                (fun ignored => pureIO False))
            reportDefect
    ready <- newChan
    gate <- newChan
    cleaned <- newChan
    worker <- spawn (streamWorker ready gate cleaned)
    started <- recv ready
    killThread worker
    finished <- recv cleaned
    ensure "rank-2 file scope cleanup" (and (prefix == "abcdef") (and typed defect))

main : Task ()
main = do
    checkFiniteAlgebra
    checkFiniteAdapters
    checkBackpressure
    checkTypedFailure
    checkBounds
    checkScopedFile
