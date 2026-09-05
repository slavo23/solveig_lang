export (Port,
    PortError(..),
    portFromInt,
    portToInt,
    anyPort,
    IpAddress,
    ipAddressToString,
    Endpoint,
    endpoint,
    endpointAddress,
    endpointPort,
    NetError(..),
    resolveHost,
    TcpConnection,
    TcpListener,
    withTcpConnection,
    withTcpListener,
    withAcceptedTcpConnection,
    sendTcpBytes,
    receiveTcpBytes,
    tcpBytes,
    UdpSocket,
    Datagram,
    datagramSource,
    datagramBytes,
    withUdpSocket,
    sendDatagram,
    sendDatagramTo,
    receiveDatagram,
    datagrams)

import std.prim(Socket)
import std.bytes(bytesLength)
import std.stream(Stream, unfoldScoped)
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
    if and (value >= 0) (value <= 65535) then Right (Port value) else Left (InvalidPort value)

-- | Returns a port's integer value.
portToInt : Port -> Int
portToInt (Port value) = value

-- | Requests an ephemeral port when binding a listener or datagram socket.
anyPort : Port
anyPort = Port 0

-- | One validated host IP address returned by the system resolver.
-- |
-- | The representation is opaque so arbitrary text cannot be passed to raw
-- | socket operations as though it had already been resolved.
data IpAddress = IpAddress String

instance Eq IpAddress where
    (==) (IpAddress left) (IpAddress right) = left == right

instance PartialOrd IpAddress where
    partialCompare (IpAddress left) (IpAddress right) = partialCompare left right

instance Ord IpAddress where
    compare (IpAddress left) (IpAddress right) = compare left right

instance Show IpAddress where
    show (IpAddress spelling) = spelling

instance Hashable IpAddress where
    hashWithSalt salt (IpAddress spelling) = hashWithSalt salt spelling

-- | Returns the canonical host spelling supplied by the system resolver.
ipAddressToString : IpAddress -> String
ipAddressToString (IpAddress spelling) = spelling

-- | One resolved IP address and validated port.
data Endpoint = Endpoint IpAddress Port

instance Eq Endpoint where
    (==) (Endpoint leftAddress leftPort) (Endpoint rightAddress rightPort) =
        and (leftAddress == rightAddress) (leftPort == rightPort)

instance PartialOrd Endpoint where
    partialCompare left right = partialCompare (endpointKey left) (endpointKey right)

instance Ord Endpoint where
    compare left right = compare (endpointKey left) (endpointKey right)

instance Show Endpoint where
    show (Endpoint address port) = show address <> " port " <> show port

instance Hashable Endpoint where
    hashWithSalt salt (Endpoint address port) = hashWithSalt (hashWithSalt salt address) port

endpointKey : Endpoint -> (IpAddress, Port)
endpointKey (Endpoint address port) = (address, port)

-- | Pairs a resolved address with a validated port.
endpoint : IpAddress -> Port -> Endpoint
endpoint address port = Endpoint address port

-- | Returns an endpoint's resolved IP address.
endpointAddress : Endpoint -> IpAddress
endpointAddress (Endpoint address _) = address

-- | Returns an endpoint's validated port.
endpointPort : Endpoint -> Port
endpointPort (Endpoint _ port) = port

-- | A validation, resolution, or socket failure with domain context retained.
data NetError =
    InvalidTcpChunkSize Int
    | InvalidDatagramSize Int
    | NoHostAddresses String
    | NetHostError String HostError

instance Eq NetError where
    (==) (InvalidTcpChunkSize left) (InvalidTcpChunkSize right) = left == right
    (==) (InvalidDatagramSize left) (InvalidDatagramSize right) = left == right
    (==) (NoHostAddresses left) (NoHostAddresses right) = left == right
    (==) (NetHostError leftContext leftHost) (NetHostError rightContext rightHost) =
        and (leftContext == rightContext) (leftHost == rightHost)
    (==) _ _ = False

instance Show NetError where
    show (InvalidTcpChunkSize size) =
        "invalid TCP chunk size " <> show size <> "; expected a positive Int"
    show (InvalidDatagramSize size) =
        "invalid UDP datagram bound " <> show size <> "; expected 1 through 65535 bytes"
    show (NoHostAddresses host) = "host resolved without an IP address: " <> host
    show (NetHostError context hostError) = "network " <> context <> ": " <> show hostError

atEndpoint : forall a. String -> Task a -> IO NetError a
atEndpoint context operation = mapError (NetHostError context) operation

wrapAddress : String -> IpAddress
wrapAddress = IpAddress

-- | Resolves every distinct IP address supplied by the system resolver.
-- |
-- | Resolution runs on the bounded host worker pool. IPv4 addresses precede
-- | IPv6 addresses so the loopback-only bind helpers remain portable.
resolveHost : String -> IO NetError [IpAddress]
resolveHost host = fmap (map wrapAddress) (atEndpoint host (std.prim.resolveHost host))

-- | A scoped connected TCP byte stream.
data TcpConnection scope = TcpConnection Socket

-- | A scoped TCP listener. Use withTcpListener to introduce its scope.
data TcpListener scope = TcpListener Socket

connectAddress : IpAddress -> Port -> IO NetError Socket
connectAddress address port =
    atEndpoint
        (show (endpoint address port))
        (std.prim.connect (ipAddressToString address) (portToInt port))

connectAddresses : String -> Port -> [IpAddress] -> IO NetError Socket
connectAddresses host _ [] = failIO (NoHostAddresses host)
connectAddresses host port (address :: rest) =
    case rest of
        [] => connectAddress address port
        | _ => catchError (connectAddress address port) (fun _ => connectAddresses host port rest)

openTcpConnection : String -> Port -> IO NetError (TcpConnection ())
openTcpConnection host port = do
    addresses <- resolveHost host
    socket <- connectAddresses host port addresses
    pureIO (TcpConnection socket)

closeTcpConnection : forall scope. TcpConnection scope -> IO NetError ()
closeTcpConnection (TcpConnection socket) =
    atEndpoint "close TCP connection" (std.prim.closeSocket socket)

-- | Resolves, opens, and closes a TCP connection around a rank-2 callback.
-- |
-- | The connection and every Stream carrying its scope are rejected if they
-- | escape the callback. Addresses are tried in resolver order until one opens.
withTcpConnection :
    forall result. String ->
    Port -> (forall scope. TcpConnection scope -> IO NetError result) -> IO NetError result
withTcpConnection host port use = bracket (openTcpConnection host port) closeTcpConnection use

openTcpListener : Port -> IO NetError (TcpListener (), Port)
openTcpListener port =
    fmap
        (fun (socket, actual) => (TcpListener socket, Port actual))
        (atEndpoint ("listen on port " <> show port) (std.prim.listen (portToInt port)))

closeTcpListener : forall scope. (TcpListener scope, Port) -> IO NetError ()
closeTcpListener (TcpListener listener, _) =
    atEndpoint "close TCP listener" (std.prim.closeSocket listener)

useTcpListener :
    forall result. (forall scope. TcpListener scope -> Port -> IO NetError result) ->
    (TcpListener (), Port) -> IO NetError result
useTcpListener use (listener, port) = use listener port

-- | Binds an IPv4 loopback TCP listener and closes it after a rank-2 callback.
withTcpListener :
    forall result. Port ->
    (forall scope. TcpListener scope -> Port -> IO NetError result) -> IO NetError result
withTcpListener port use = bracket (openTcpListener port) closeTcpListener (useTcpListener use)

openAcceptedTcpConnection :
    forall listenerScope. TcpListener listenerScope -> IO NetError (TcpConnection ())
openAcceptedTcpConnection (TcpListener listener) =
    fmap TcpConnection (atEndpoint "accept TCP connection" (std.prim.accept listener))

-- | Accepts and closes one TCP connection inside a fresh nested scope.
withAcceptedTcpConnection :
    forall listenerScope result. TcpListener listenerScope ->
    (forall connectionScope. TcpConnection connectionScope -> IO NetError result) ->
    IO NetError result
withAcceptedTcpConnection listener use =
    bracket (openAcceptedTcpConnection listener) closeTcpConnection use

-- | Sends the complete byte string on a connected TCP byte stream.
sendTcpBytes : forall scope. TcpConnection scope -> Bytes -> IO NetError ()
sendTcpBytes (TcpConnection socket) bytes =
    atEndpoint "send TCP bytes" (std.prim.sendBytes socket bytes)

-- | Receives at most a positive number of TCP bytes; empty bytes mean EOF.
receiveTcpBytes : forall scope. TcpConnection scope -> Int -> IO NetError Bytes
receiveTcpBytes (TcpConnection socket) maximum =
    if maximum <= 0
        then failIO (InvalidTcpChunkSize maximum)
        else atEndpoint "receive TCP bytes" (std.prim.recvBytes socket maximum)

tcpByteStep : forall scope. TcpConnection scope -> Int -> () -> IO NetError (Maybe (Bytes, ()))
tcpByteStep connection chunkSize _ = do
    bytes <- receiveTcpBytes connection chunkSize
    if bytesLength bytes == 0 then pureIO Nothing else pureIO (Just (bytes, ()))

-- | Pulls bounded TCP byte chunks until EOF without read-ahead.
tcpBytes : forall scope. TcpConnection scope -> Int -> Stream scope NetError Bytes
tcpBytes connection chunkSize = unfoldScoped (tcpByteStep connection chunkSize) ()

-- | A scoped UDP socket bound to IPv4 loopback.
data UdpSocket scope = UdpSocket Socket

-- | One complete UDP payload and its source endpoint.
data Datagram = Datagram Endpoint Bytes

instance Eq Datagram where
    (==) (Datagram leftSource leftBytes) (Datagram rightSource rightBytes) =
        and (leftSource == rightSource) (leftBytes == rightBytes)

instance Show Datagram where
    show (Datagram source bytes) =
        "datagram from " <> show source <> " (" <> show (bytesLength bytes) <> " bytes)"

-- | Returns the source endpoint recorded for a datagram.
datagramSource : Datagram -> Endpoint
datagramSource (Datagram source _) = source

-- | Returns a datagram's complete payload.
datagramBytes : Datagram -> Bytes
datagramBytes (Datagram _ bytes) = bytes

openUdpSocket : Port -> IO NetError (UdpSocket (), Port)
openUdpSocket port =
    fmap
        (fun (socket, actual) => (UdpSocket socket, Port actual))
        (atEndpoint ("bind UDP port " <> show port) (std.prim.bindUdp (portToInt port)))

closeUdpSocket : forall scope. (UdpSocket scope, Port) -> IO NetError ()
closeUdpSocket (UdpSocket socket, _) = atEndpoint "close UDP socket" (std.prim.closeSocket socket)

useUdpSocket :
    forall result. (forall scope. UdpSocket scope -> Port -> IO NetError result) ->
    (UdpSocket (), Port) -> IO NetError result
useUdpSocket use (socket, port) = use socket port

-- | Binds an IPv4 loopback UDP socket and closes it after a rank-2 callback.
withUdpSocket :
    forall result. Port ->
    (forall scope. UdpSocket scope -> Port -> IO NetError result) -> IO NetError result
withUdpSocket port use = bracket (openUdpSocket port) closeUdpSocket (useUdpSocket use)

-- | Sends one complete UDP message to a resolved endpoint.
sendDatagramTo : forall scope. UdpSocket scope -> Endpoint -> Bytes -> IO NetError ()
sendDatagramTo (UdpSocket socket) target bytes =
    atEndpoint
        ("send UDP datagram to " <> show target)
        (std.prim.sendDatagram
            socket
            (ipAddressToString (endpointAddress target), portToInt (endpointPort target), bytes))

sendDatagramAddresses :
    forall scope. UdpSocket scope -> String -> Port -> Bytes -> [IpAddress] -> IO NetError ()
sendDatagramAddresses _ host _ _ [] = failIO (NoHostAddresses host)
sendDatagramAddresses socket host port bytes (address :: rest) =
    case rest of
        [] => sendDatagramTo socket (endpoint address port) bytes
        | _ =>
            catchError
                (sendDatagramTo socket (endpoint address port) bytes)
                (fun _ => sendDatagramAddresses socket host port bytes rest)

-- | Resolves a host and sends one complete UDP message without splitting it.
sendDatagram : forall scope. UdpSocket scope -> String -> Port -> Bytes -> IO NetError ()
sendDatagram socket host port bytes = do
    addresses <- resolveHost host
    sendDatagramAddresses socket host port bytes addresses

datagramFromRaw : (Bytes, String, Int) -> Datagram
datagramFromRaw (bytes, host, port) = Datagram (Endpoint (IpAddress host) (Port port)) bytes

-- | Receives one whole UDP message up to the explicit byte bound.
-- |
-- | Oversized messages are discarded and fail with a resource-exhausted host
-- | cause rather than returning a silently truncated payload.
receiveDatagram : forall scope. UdpSocket scope -> Int -> IO NetError Datagram
receiveDatagram (UdpSocket socket) maximum =
    if or (maximum <= 0) (maximum > 65535)
        then failIO (InvalidDatagramSize maximum)
        else fmap
            datagramFromRaw
            (atEndpoint "receive UDP datagram" $ std.prim.recvDatagram socket maximum)

datagramStep : forall scope. UdpSocket scope -> Int -> () -> IO NetError (Maybe (Datagram, ()))
datagramStep socket maximum _ =
    fmap (fun message => Just (message, ())) (receiveDatagram socket maximum)

-- | Pulls one bounded UDP message per demand while preserving datagram boundaries.
datagrams : forall scope. UdpSocket scope -> Int -> Stream scope NetError Datagram
datagrams socket maximum = unfoldScoped (datagramStep socket maximum) ()
