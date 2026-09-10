import std.bytes(*)
import std.net(*)
import std.stream(StreamStep(..), next)
import std.task(withThread)

netToHost : NetError -> HostError
netToHost (NetHostError _ hostError) = hostError
netToHost error = HostError InvalidArgument "network" (show error)

hasAddress : [IpAddress] -> Bool
hasAddress [] = False
hasAddress (_ :: _) = True

decode : Bytes -> String
decode bytes =
    case bytesToString bytes of
        Right text => text
        | Left _ => "invalid UTF-8"

receiveOversized : forall scope. UdpSocket scope -> IO NetError String
receiveOversized receiver =
    catchError
        (bindIO (receiveDatagram receiver 3) (fun _ => pureIO "unexpected oversized success"))
        (fun error => pureIO (show error))

receiveStreamed : forall scope. UdpSocket scope -> IO NetError String
receiveStreamed receiver = do
    step <- next (datagrams receiver 64)
    case step of
        StreamDone => pureIO "unexpected datagram end"
        | StreamYield message _ => pureIO (decode (datagramBytes message))

waitForDatagram : forall scope. UdpSocket scope -> IO NetError ()
waitForDatagram receiver = fmap (fun _ => ()) (receiveDatagram receiver 64)

cancelPendingReceive : forall scope. UdpSocket scope -> IO NetError ()
cancelPendingReceive receiver = withThread (waitForDatagram receiver) (fun _ => pureIO ())

exchange :
    forall receiverScope senderScope. UdpSocket receiverScope ->
    Port -> UdpSocket senderScope -> Port -> IO NetError (String, String, String, String)
exchange receiver receiverPort sender senderPort = do
    cancelPendingReceive receiver
    sendDatagram sender "localhost" receiverPort (stringToBytes "hello UDP")
    first <- receiveDatagram receiver 64
    sendDatagram sender "localhost" receiverPort (stringToBytes "")
    empty <- receiveDatagram receiver 64
    sendDatagram sender "localhost" receiverPort (stringToBytes "too large")
    oversized <- receiveOversized receiver
    sendDatagram sender "localhost" receiverPort (stringToBytes "after")
    streamed <- receiveStreamed receiver
    pureIO
        (
            decode (datagramBytes first),
            show (endpointPort (datagramSource first) == senderPort),
            show (bytesLength (datagramBytes empty)),
            oversized <> " | " <> streamed
        )

withSender :
    forall receiverScope. UdpSocket receiverScope ->
    Port -> IO NetError (String, String, String, String)
withSender receiver receiverPort = withUdpSocket anyPort (exchange receiver receiverPort)

main : Task ()
main = do
    addresses <- mapError netToHost (resolveHost "localhost")
    printLine ("resolved=" <> show (hasAddress addresses))
    result <- mapError netToHost (withUdpSocket anyPort withSender)
    case result of
        (message, sourceMatches, emptySize, recovery) => do
            printLine message
            printLine ("source-port=" <> sourceMatches)
            printLine ("empty-bytes=" <> emptySize)
            printLine recovery
