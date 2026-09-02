-- Loopback protocol handshakes prove that headers and pulls precede later body bytes.
import std.bytes(*)
import std.env(getArgs)
import std.http(*)
import std.stream(Stream, StreamStep(..))
import std.stream as Streams
import std.channel
import std.task(withThread, catchDefect)
import std.time(milliseconds)

requestAt : String -> String -> String
requestAt base path = base <> path

limits : Int -> HttpOptions
limits maximum =
    withMaximumResponseBytes (Just maximum) (withTimeout (milliseconds 1000) defaultOptions)

collect : forall scope. Stream scope HttpError Bytes -> IO HttpError Bytes
collect body =
    fmap
        buildBytes
        (Streams.fold (fun builder bytes => builder <> bytesBuilder bytes) emptyBytesBuilder body)

say : String -> IO HttpError ()
say = mapError HttpHostError . printLine

classify : HttpError -> String
classify (ResponseBodyTooLarge _) = "limit"
classify (TransportError host) =
    if hostErrorKind host == TimeExpired then "timeout" else "transport"
classify (HttpHostError host) = if hostErrorKind host == TimeExpired then "timeout" else "host"
classify problem = show problem

report : String -> IO HttpError String -> IO HttpError ()
report label action =
    catchError (action >>= (say . ((label <> ":") <>))) (say . ((label <> ":") <>) . classify)

lengthAt : String -> String -> Int -> IO HttpError String
lengthAt base path maximum =
    withResponseWith
        (withChunkBytes 2 (limits maximum))
        Get
        (requestAt base path)
        (fun _ _ body =>
            fmap show (Streams.fold (fun total bytes => total + bytesLength bytes) 0 body)
        )

appendCheckedChunk : (BytesBuilder, Bool) -> Bytes -> (BytesBuilder, Bool)
appendCheckedChunk (builder, bounded) chunk =
    (builder <> bytesBuilder chunk, and bounded (bytesLength chunk <= 2))

gated : String -> IO HttpError ()
gated base =
    withResponseWith
        (withChunkBytes 2 (limits 6))
        Get
        (requestAt base "/gated")
        (fun status _ body =>
            do
                _ <- get (requestAt base "/headers-seen")
                first <- Streams.next body
                case first of
                    StreamDone => say "gated:unexpected-eof"
                    | StreamYield bytes rest =>
                        do
                            _ <- get (requestAt base "/chunk-seen")
                            (builder, bounded) <-
                                Streams.fold
                                    appendCheckedChunk
                                    (bytesBuilder bytes, bytesLength bytes <= 2)
                                    rest
                            let result = buildBytes builder
                            say
                                ("gated:" <>
                                    show (statusCode status) <>
                                    ":" <>
                                    show (map (bytesIndex result) [0, 1, 2, 3, 4, 5]) <>
                                    ":" <>
                                    show bounded)
        )

parkBody :
    forall scope. Chan () ->
    Chan () -> Status -> [Header] -> Stream scope HttpError Bytes -> IO HttpError ()
parkBody entered parked _ _ _ = do
    _ <- mapError HttpHostError (std.channel.send entered ())
    mapError HttpHostError (std.channel.recv parked)

run : String -> IO HttpError ()
run base = do
    gated base
    report "empty" (lengthAt base "/empty" 0)
    report "exact" (lengthAt base "/exact" 6)
    report "overflow" (lengthAt base "/overflow" 6)
    report "truncated" (lengthAt base "/truncated" 6)
    report
        "redirect"
        (withResponse
            Get
            (requestAt base "/redirect")
            (fun status _ _ => pureIO (show (statusCode status))))
    report
        "no-redirect"
        (withResponseWith
            (withRedirectPolicy NoRedirects (limits 6))
            Get
            (requestAt base "/redirect")
            (fun status _ _ => pureIO (show (statusCode status))))
    withResponse Get (requestAt base "/early") (fun _ _ _ => pureIO ())
    say "early:closed"
    catchError
        (withResponse Get (requestAt base "/failure") (fun _ _ _ => failIO (InvalidStatus 0)))
        (fun _ => pureIO ())
    say "error:closed"
    catchDefect
        (withResponse
            Get
            (requestAt base "/defect")
            (fun _ _ _ => pureIO (error "callback defect")))
        (fun _ => pureIO ())
    say "defect:closed"
    entered <- std.channel.newChan
    parked <- std.channel.newChan
    withThread
        (withResponse Get (requestAt base "/cancel") (parkBody entered parked))
        (fun _ => mapError HttpHostError (std.channel.recv entered))
    say "cancel:closed"
    report
        "blocked"
        (withResponseWith
            (withChunkBytes 2 (withTimeout (milliseconds 200) (limits 100)))
            Get
            (requestAt base "/blocked")
            (fun _ _ body => fmap (show . bytesLength) (collect body)))

main : Task ()
main = do
    args <- getArgs
    case args of
        base :: _ => catchError (run base) (printLine . show)
        | [] => printLine "missing fixture URL"
