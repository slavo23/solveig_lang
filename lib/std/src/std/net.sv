import prelude.prim
import std.task(bracket)

-- | Opens a TCP connection to an IP address or localhost.
connect : String -> Int -> Task Socket
connect host port = prelude.prim.connect host port

-- | Resolves a host name to an IPv4 address without blocking other green threads.
resolveHost : String -> Task String
resolveHost host = prelude.prim.resolveHost host

-- | Binds a loopback TCP listener and returns its actual port.
listen : Int -> Task (Pair Socket Int)
listen port = prelude.prim.listen port

-- | Accepts the next connection from a TCP listener.
accept : Socket -> Task Socket
accept socket = prelude.prim.accept socket

-- | Sends an entire byte string on a connected TCP socket.
sendBytes : Socket -> Bytes -> Task ()
sendBytes socket bytes = prelude.prim.sendBytes socket bytes

-- | Receives at most the requested number of bytes, returning empty at EOF.
recvBytes : Socket -> Int -> Task Bytes
recvBytes socket maximum = prelude.prim.recvBytes socket maximum

-- | Explicitly closes a TCP listener or connection.
closeSocket : Socket -> Task ()
closeSocket socket = prelude.prim.closeSocket socket

-- | Opens and closes a TCP connection around a use action.
withConnection : forall a. String -> Int -> (Socket -> Task a) -> Task a
withConnection host port use = bracket (connect host port) closeSocket use

-- | Closes the listener in a result returned by listen.
closeListener : Pair Socket Int -> Task ()
closeListener (MkPair listener _) = closeSocket listener

-- | Binds and closes a TCP listener around a use action.
withListener : forall a. Int -> (Pair Socket Int -> Task a) -> Task a
withListener port use = bracket (listen port) closeListener use

-- | Accepts and closes one TCP connection around a use action.
withAcceptedConnection : forall a. Socket -> (Socket -> Task a) -> Task a
withAcceptedConnection listener use = bracket (accept listener) closeSocket use
