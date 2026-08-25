import std.task(*)
import std.thread(*)
import std.channel(*)
import std.process(*)
import std.io(*)

ignoreError : IOError -> Task ();
ignoreError error = printLine "typed handler ran";

ignoreDefect : Defect -> Task ();
ignoreDefect defect = printLine "defect handler ran";

worker : Chan () -> Chan () -> Task ();
worker ready stop = finally (wait ready stop) (printLine "worker cleanup");

wait : Chan () -> Chan () -> Task ();
wait ready stop = do
    send ready ()
    recv stop;

childExit : Chan () -> Task ();
childExit cleaned = finally exitFailure (send cleaned ());

main : Task ();
main = do
    hSetBuffering stdout (BlockBuffering (Just 4096))
    hPutStr stdout "buffered-"
    ready <- newChan
    stop <- newChan
    childCleaned <- newChan
    spawn (childExit childCleaned)
    recv childCleaned
    spawn (worker ready stop)
    recv ready
    finally
        (catchDefect
            (catchError (exitWith 23) ignoreError)
            ignoreDefect)
        (printLine "main cleanup")
    printLine "unreachable"
