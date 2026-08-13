import prelude.prim

-- | Creates an unbounded typed channel.
newChan : forall e a. IO e (Chan a)
newChan = prelude.prim.newChan

-- | Creates a typed channel with a positive capacity.
newBoundedChan : forall e a. Int -> IO e (Chan a)
newBoundedChan capacity = prelude.prim.newBoundedChan capacity

-- | Sends a value, blocking while a bounded channel is full.
send : forall a. Chan a -> a -> Task ()
send channel value = prelude.prim.send channel value

-- | Receives the oldest queued value, blocking while an open channel is empty.
recv : forall a. Chan a -> Task a
recv channel = prelude.prim.recv channel

-- | Receives the oldest queued value when one is immediately available.
tryRecv : forall a. Chan a -> Task (Maybe a)
tryRecv channel = prelude.prim.tryRecv channel

-- | Closes a channel and wakes its blocked senders and receivers.
closeChan : forall a. Chan a -> Task ()
closeChan channel = prelude.prim.closeChan channel
