export (newChan, newBoundedChan, send, recv, tryRecv, closeChan)

import std.prim(newChan, newBoundedChan, send, recv, tryRecv, closeChan)

-- | Creates an unbounded typed channel.
newChan : forall e a. IO e (Chan a)
newChan = std.prim.newChan

-- | Creates a typed channel with a supported positive capacity.
newBoundedChan : forall a. Int -> Task (Chan a)
newBoundedChan capacity =
    if capacity < 1
        then failIO (invalidCapacity capacity)
        else if capacity > 4294967295
            then failIO (invalidCapacity capacity)
            else std.prim.newBoundedChan capacity

invalidCapacity : Int -> HostError
invalidCapacity capacity =
    HostError
        InvalidArgument
        "newBoundedChan"
        ("capacity must be between 1 and 4294967295, got " <> show capacity)

-- | Sends a value, blocking while a bounded channel is full.
send : forall a. Chan a -> a -> Task ()
send channel value = std.prim.send channel value

-- | Receives the oldest queued value, blocking while an open channel is empty.
recv : forall a. Chan a -> Task a
recv channel = std.prim.recv channel

-- | Receives the oldest queued value when one is immediately available.
tryRecv : forall a. Chan a -> Task (Maybe a)
tryRecv channel = std.prim.tryRecv channel

-- | Closes a channel and wakes its blocked senders and receivers.
closeChan : forall a. Chan a -> Task ()
closeChan channel = std.prim.closeChan channel
