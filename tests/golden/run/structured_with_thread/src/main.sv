import std.task(*)
import std.thread(*)
import std.channel(*)

child : Chan () -> Task ()
child gate = finally (recv gate) (printLine "child-cleaned")

body : ThreadId -> Task ()
body thread = printLine "body"

scopedChild : Chan () -> Chan () -> Task ()
scopedChild gate cleaned = finally (recv gate) (send cleaned ())

scopedBody : Chan () -> Chan () -> ThreadId -> Task ()
scopedBody entered gate thread = do
    send entered ()
    recv gate

owner : Chan () -> Chan () -> Chan () -> Chan () -> Task ()
owner entered gate cleaned ownerDone =
    finally (withThread (scopedChild gate cleaned) (scopedBody entered gate)) (send ownerDone ())

main : Task ()
main = do
    gate <- newChan
    withThread (child gate) body
    printLine "after-scope"
    entered <- newChan
    scopedGate <- newChan
    cleaned <- newChan
    ownerDone <- newChan
    ownerThread <- spawn (owner entered scopedGate cleaned ownerDone)
    _ <- recv entered
    killThread ownerThread
    _ <- recv cleaned
    _ <- recv ownerDone
    printLine "interrupted-scope-cleaned"
