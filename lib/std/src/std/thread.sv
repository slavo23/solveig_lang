export (spawn, killThread)

import std.prim(*)

-- | Starts an IO action in a new lightweight thread.
spawn : forall childError e. IO childError () -> IO e ThreadId
spawn action = std.prim.spawn action

-- | Requests interruption of a lightweight thread.
killThread : ThreadId -> Task ()
killThread thread = std.prim.killThread thread
