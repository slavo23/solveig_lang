export (spawn, killThread)

import prelude.prim

-- | Starts an IO action in a new lightweight thread.
spawn : forall childError e. IO childError () -> IO e ThreadId
spawn action = prelude.prim.spawn action

-- | Requests interruption of a lightweight thread.
killThread : ThreadId -> Task ()
killThread thread = prelude.prim.killThread thread
