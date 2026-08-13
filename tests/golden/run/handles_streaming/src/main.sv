ignoreError : IOError -> Task ();
ignoreError ioError = pureIO ();

reportClosedRead : IOError -> Task String;
reportClosedRead ioError =
    case ioError of
        ReadHandleError message => pureIO "closed-read"
        | _ => pureIO "wrong-read-error";

reportClosedWrite : String -> IOError -> Task String;
reportClosedWrite expected ioError =
    case ioError of
        WriteHandleError message => pureIO expected
        | _ => pureIO "wrong-write-error";

reportOpen : IOError -> Task String;
reportOpen ioError =
    case ioError of
        OpenHandleError path message => pureIO "open-error"
        | _ => pureIO "wrong-open-error";

reportClose : IOError -> Task String;
reportClose ioError =
    case ioError of
        CloseHandleError message => pureIO "close-error"
        | _ => pureIO "wrong-close-error";

reportFlush : IOError -> Task String;
reportFlush ioError =
    case ioError of
        FlushHandleError message => pureIO "flush-error"
        | _ => pureIO "wrong-flush-error";

reportBuffering : IOError -> Task String;
reportBuffering ioError =
    case ioError of
        SetBufferingError message => pureIO "buffering-error"
        | _ => pureIO "wrong-buffering-error";

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
    failIO (WriteHandleError "planned");

waitAfterSharing : Chan Handle -> Chan () -> Handle -> Task ();
waitAfterSharing escaped started file = do
    send escaped file
    send started ()
    sleep 100000;

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
