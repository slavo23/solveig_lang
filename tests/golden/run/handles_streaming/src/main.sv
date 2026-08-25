import std.task(*)
import std.time(*)
import std.thread(*)
import std.channel(*)
import std.io(*)

ignoreError : HostError -> Task ();
ignoreError ioError = pureIO ();

reportClosedRead : HostError -> Task String;
reportClosedRead ioError =
    if hostErrorOperation ioError == "handle read"
        then pureIO "closed-read"
        else pureIO "wrong-read-error";

reportClosedWrite : String -> HostError -> Task String;
reportClosedWrite expected ioError =
    if hostErrorOperation ioError == "handle write"
        then pureIO expected
        else pureIO "wrong-write-error";

reportOpen : HostError -> Task String;
reportOpen ioError =
    if hostErrorOperation ioError == "openFile"
        then pureIO "open-error"
        else pureIO "wrong-open-error";

reportClose : HostError -> Task String;
reportClose ioError =
    if hostErrorOperation ioError == "hClose"
        then pureIO "close-error"
        else pureIO "wrong-close-error";

reportFlush : HostError -> Task String;
reportFlush ioError =
    if hostErrorOperation ioError == "hFlush"
        then pureIO "flush-error"
        else pureIO "wrong-flush-error";

reportBuffering : HostError -> Task String;
reportBuffering ioError =
    if hostErrorOperation ioError == "hSetBuffering"
        then pureIO "buffering-error"
        else pureIO "wrong-buffering-error";

writeGenerated : Handle -> Task ();
writeGenerated output = do
    hSetBuffering output (BlockBuffering (Just 4))
    hPutStr output "one"
    hPutStrLn output "two"
    hFlush output;

appendGenerated : Handle -> Task ();
appendGenerated output = do
    hSetBuffering output LineBuffering
    hPutStrLn output "three";

readGenerated : Handle -> Task ();
readGenerated file = do
    hSetBuffering file (BlockBuffering Nothing)
    contents <- hGetContents file
    printStr contents;

failAfterSharing : Chan Handle -> Handle -> Task ();
failAfterSharing escaped file = do
    send escaped file
    failIO (HostError OtherError "handle write" "planned");

waitAfterSharing : Chan Handle -> Chan () -> Handle -> Task ();
waitAfterSharing escaped started file = do
    send escaped file
    send started ()
    sleep (milliseconds 100000);

main : Task ();
main = do
    input <- openFile "input.txt" ReadMode
    hSetBuffering input NoBuffering
    first <- hGetLine input
    printLine first
    more <- hIsEOF input
    if more then printLine "wrong-eof" else printLine "more"
    second <- hGetLine input
    printLine second
    eof <- hIsEOF input
    if eof then printLine "eof" else printLine "wrong-more"
    hClose input

    wrote <- withFile "generated.txt" WriteMode writeGenerated
    appended <- withFile "generated.txt" AppendMode appendGenerated
    readBack <- withFile "generated.txt" ReadWriteMode readGenerated

    closed <- openFile "generated.txt" ReadMode
    hClose closed
    closedRead <- catchError (hGetLine closed) reportClosedRead
    printLine closedRead
    closedFlush <- catchError
        (bindIO (hFlush closed) (fun flushed => pureIO "open-flush"))
        reportFlush
    printLine closedFlush
    closedBuffering <- catchError
        (bindIO
            (hSetBuffering closed NoBuffering)
            (fun changed => pureIO "open-buffering"))
        reportBuffering
    printLine closedBuffering
    closedClose <- catchError
        (bindIO (hClose closed) (fun closedAgain => pureIO "open-close"))
        reportClose
    printLine closedClose

    missing <- catchError
        (bindIO
            (openFile "missing.txt" ReadMode)
            (fun file => pureIO "opened-missing"))
        reportOpen
    printLine missing

    escapedAfterError <- newChan
    handledError <- catchError
        (withFile "generated.txt" AppendMode (failAfterSharing escapedAfterError))
        ignoreError
    errorHandle <- recv escapedAfterError
    errorClosed <- catchError
        (bindIO (hPutStr errorHandle "not-written") (fun written => pureIO "open-after-error"))
        (reportClosedWrite "closed-after-error")
    printLine errorClosed

    escapedAfterKill <- newChan
    started <- newChan
    finished <- newChan
    worker <- spawn
        (finally
            (withFile
                "generated.txt"
                AppendMode
                (waitAfterSharing escapedAfterKill started))
            (send finished ()))
    interruptedHandle <- recv escapedAfterKill
    ready <- recv started
    killed <- killThread worker
    cleaned <- recv finished
    interruptedClosed <- catchError
        (bindIO
            (hPutStr interruptedHandle "not-written")
            (fun written => pureIO "open-after-interruption"))
        (reportClosedWrite "closed-after-interruption")
    printLine interruptedClosed

    hPutStrLn stderr "stderr-handle"
    hPutStrLn stdout "done"
