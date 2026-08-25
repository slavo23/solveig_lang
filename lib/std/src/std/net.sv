export (
    Port, PortError(..), portFromInt, portToInt, anyPort, NetError(..), connect,
    resolveHost, listen, accept, sendBytes, recvBytes, closeSocket, closeListener,
    withConnection, withListener, withAcceptedConnection
)

import prelude.prim(*)
import std.task(bracket)

-- | A failure while validating a TCP or UDP port number.
data PortError = InvalidPort Int

instance Eq PortError where
    (==) (InvalidPort left) (InvalidPort right) = left == right

instance Show PortError where
    show (InvalidPort value) = "invalid port " <> show value <> "; expected 0 through 65535"

-- | A validated 16-bit network port. Zero asks the host to choose a port.
data Port = Port Int

instance Eq Port where
    (==) (Port left) (Port right) = left == right

instance PartialOrd Port where
    partialCompare (Port left) (Port right) = partialCompare left right

instance Ord Port where
    compare (Port left) (Port right) = compare left right

instance Show Port where
    show (Port value) = show value

instance Hashable Port where
    hashWithSalt salt (Port value) = hashWithSalt salt value

-- | Validates an integer as a network port.
portFromInt : Int -> Either PortError Port
portFromInt value =
    if and (value >= 0) (value <= 65535) then Right (Port value)
    else Left (InvalidPort value)

-- | Returns a port's integer value.
portToInt : Port -> Int
portToInt (Port value) = value

-- | Requests an ephemeral port when listening.
anyPort : Port
anyPort = Port 0

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
connect : String -> Port -> IO NetError Socket
connect host port =
    atEndpoint
        (host <> ":" <> show port)
        (prelude.prim.connect host (portToInt port))

-- | Resolves a host name to an IPv4 address without blocking other green threads.
resolveHost : String -> IO NetError String
resolveHost host = atEndpoint host (prelude.prim.resolveHost host)

-- | Binds a loopback TCP listener and returns its actual port.
listen : Port -> IO NetError (Socket, Port)
listen port =
    fmap
        (fun (socket, actual) => (socket, Port actual))
        (atEndpoint ("port " <> show port) (prelude.prim.listen (portToInt port)))

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
withConnection : forall a. String -> Port -> (Socket -> IO NetError a) -> IO NetError a
withConnection host port use = bracket (connect host port) closeSocket use

closeListener : (Socket, Port) -> IO NetError ()
closeListener (listener, _) = closeSocket listener

-- | Binds and closes a TCP listener around a use action.
withListener : forall a. Port -> ((Socket, Port) -> IO NetError a) -> IO NetError a
withListener port use = bracket (listen port) closeListener use

-- | Accepts and closes one TCP connection around a use action.
withAcceptedConnection : forall a. Socket -> (Socket -> IO NetError a) -> IO NetError a
withAcceptedConnection listener use = bracket (accept listener) closeSocket use
