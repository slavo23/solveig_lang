acquire : Chan () -> Task String
acquire ready = do
    send ready ()
    pureIO "resource"

release : Chan () -> String -> Task ()
release cleaned resource = do
    printLine (appendString "released=" resource)
    send cleaned ()

use : Chan () -> String -> Task ()
use gate resource = do
    _ <- recv gate
    printLine "unexpected use return"

worker : Chan () -> Chan () -> Chan () -> Task ()
worker ready gate cleaned = bracket (acquire ready) (release cleaned) (use gate)

main : Task ()
main = do
    ready <- newChan
    gate <- newChan
    cleaned <- newChan
    thread <- spawn (worker ready gate cleaned)
    _ <- recv ready
    killThread thread
    _ <- recv cleaned
    printLine "bracket-finished"
