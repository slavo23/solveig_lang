export (
    NetError(..), connect, resolveHost, listen, accept, sendBytes, recvBytes, closeSocket,
    withConnection, withListener, withAcceptedConnection
)

import prelude.prim(*)
import std.task(bracket)

-- | A network failure with the endpoint or socket operation retained.
data NetError = NetHostError String HostError

instance Eq NetError where
    (==) (NetHostError leftContext leftHost) (NetHostError rightContext rightHost) =
        and (leftContext == rightContext) (leftHost == rightHost)

instance Show NetError where
    show (NetHostError context hostError) =
        "network " <> context <> ": " <> show hostError

atEndpoint : forall a. String -> Task a -> IO NetError a
atEndpoint context operation = mapError (NetHostError context) operation

-- | Opens a TCP connection to an IP address or localhost.
connect : String -> Int -> IO NetError Socket
connect host port = atEndpoint (host <> ":" <> show port) (prelude.prim.connect host port)

-- | Resolves a host name to an IPv4 address without blocking other green threads.
resolveHost : String -> IO NetError String
resolveHost host = atEndpoint host (prelude.prim.resolveHost host)

-- | Binds a loopback TCP listener and returns its actual port.
listen : Int -> IO NetError (Socket, Int)
listen port = atEndpoint ("port " <> show port) (prelude.prim.listen port)

-- | Accepts the next connection from a TCP listener.
accept : Socket -> IO NetError Socket
accept socket = atEndpoint "accept" (prelude.prim.accept socket)

-- | Sends an entire byte string on a connected TCP socket.
sendBytes : Socket -> Bytes -> IO NetError ()
sendBytes socket bytes = atEndpoint "send" (prelude.prim.sendBytes socket bytes)

-- | Receives at most the requested number of bytes, returning empty at EOF.
recvBytes : Socket -> Int -> IO NetError Bytes
recvBytes socket maximum = atEndpoint "receive" (prelude.prim.recvBytes socket maximum)

-- | Explicitly closes a TCP listener or connection.
closeSocket : Socket -> IO NetError ()
closeSocket socket = atEndpoint "close" (prelude.prim.closeSocket socket)

-- | Opens and closes a TCP connection around a use action.
withConnection : forall a. String -> Int -> (Socket -> IO NetError a) -> IO NetError a
withConnection host port use = bracket (connect host port) closeSocket use

-- | Closes the listener in a result returned by listen.
closeListener : (Socket, Int) -> IO NetError ()
closeListener (listener, _) = closeSocket listener

-- | Binds and closes a TCP listener around a use action.
withListener : forall a. Int -> ((Socket, Int) -> IO NetError a) -> IO NetError a
withListener port use = bracket (listen port) closeListener use

-- | Accepts and closes one TCP connection around a use action.
withAcceptedConnection : forall a. Socket -> (Socket -> IO NetError a) -> IO NetError a
withAcceptedConnection listener use = bracket (accept listener) closeSocket use
