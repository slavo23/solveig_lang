export (TlsTrust(..),
    TlsOptions,
    defaultTlsOptions,
    withTlsTrust,
    withTlsServerName,
    withTrustedCertificatesPem,
    tlsTrust,
    tlsServerName,
    TlsError(..),
    TlsConnection,
    withTlsConnection,
    withTlsConnectionWith,
    sendTlsBytes,
    receiveTlsBytes,
    tlsBytes)

import prelude.builtins
import prelude.prim
import std.bytes(bytesLength)
import std.net(Port, NetError, TcpConnection(..), openTcpConnection, closeTcpConnection)
import std.stream(Stream, unfoldScoped)
import std.task(bracket)

-- | Certificate authorities trusted by a TLS client connection.
-- |
-- | PlatformTrust delegates certificate-chain verification to the operating
-- | system. TrustedCertificatesPem replaces platform trust with one or more
-- | PEM-encoded CA certificates, which is useful for private services and
-- | hermetic tests. Neither policy disables certificate or server-name checks.
data TlsTrust = PlatformTrust | TrustedCertificatesPem Bytes

instance Eq TlsTrust where
    (==) PlatformTrust PlatformTrust = True
    (==) (TrustedCertificatesPem left) (TrustedCertificatesPem right) = left == right
    (==) _ _ = False

instance Show TlsTrust where
    show PlatformTrust = "PlatformTrust"
    show (TrustedCertificatesPem _) = "TrustedCertificatesPem <certificate data>"

-- | Immutable client configuration. Start from defaultTlsOptions and override
-- | only the trust or verified name needed by the connection.
data TlsOptions = TlsOptions TlsTrust (Maybe String)

-- | Uses operating-system trust and verifies the hostname passed to
-- | withTlsConnectionWith.
defaultTlsOptions : TlsOptions
defaultTlsOptions = TlsOptions PlatformTrust Nothing

-- | Replaces the connection's trust policy without weakening verification.
withTlsTrust : TlsTrust -> TlsOptions -> TlsOptions
withTlsTrust trust (TlsOptions _ serverName) = TlsOptions trust serverName

-- | Verifies certificates for this DNS name instead of the connection host.
-- | This is appropriate when a resolved address or tunnel reaches a service
-- | whose certificate names a different host.
withTlsServerName : String -> TlsOptions -> TlsOptions
withTlsServerName serverName (TlsOptions trust _) = TlsOptions trust (Just serverName)

-- | Replaces platform trust with one or more PEM-encoded CA certificates.
withTrustedCertificatesPem : Bytes -> TlsOptions -> TlsOptions
withTrustedCertificatesPem certificates = withTlsTrust (TrustedCertificatesPem certificates)

-- | Returns the configured certificate trust policy.
tlsTrust : TlsOptions -> TlsTrust
tlsTrust (TlsOptions trust _) = trust

-- | Returns the explicit verified-name override, if any.
tlsServerName : TlsOptions -> Maybe String
tlsServerName (TlsOptions _ serverName) = serverName

-- | A network, configuration, verification, or TLS transport failure.
data TlsError = InvalidTlsChunkSize Int | TlsNetworkError NetError | TlsHostError String HostError

instance Eq TlsError where
    (==) (InvalidTlsChunkSize left) (InvalidTlsChunkSize right) = left == right
    (==) (TlsNetworkError left) (TlsNetworkError right) = left == right
    (==) (TlsHostError leftContext leftHost) (TlsHostError rightContext rightHost) =
        and (leftContext == rightContext) (leftHost == rightHost)
    (==) _ _ = False

instance Show TlsError where
    show (InvalidTlsChunkSize size) =
        "invalid TLS chunk size " <> show size <> "; expected a positive Int"
    show (TlsNetworkError networkError) = "TLS network setup: " <> show networkError
    show (TlsHostError context hostError) = "TLS " <> context <> ": " <> show hostError

-- | A verified, encrypted byte stream whose lifetime is identified by scope.
data TlsConnection scope = TlsConnection Socket

closeTlsTcp : forall scope. TcpConnection scope -> IO TlsError ()
closeTlsTcp connection = mapError TlsNetworkError $ closeTcpConnection connection

trustPem : TlsTrust -> Maybe Bytes
trustPem PlatformTrust = Nothing
trustPem (TrustedCertificatesPem certificates) = Just certificates

verifiedName : String -> TlsOptions -> String
verifiedName host (TlsOptions _ Nothing) = host
verifiedName _ (TlsOptions _ (Just serverName)) = serverName

startTlsAndUse :
    forall scope result. TlsOptions ->
    String ->
    (TlsConnection scope -> IO TlsError result) -> TcpConnection scope -> IO TlsError result
startTlsAndUse options host use (TcpConnection socket) = do
    let serverName = verifiedName host options
    mapError
        (TlsHostError ("handshake with " <> serverName))
        (prelude.prim.startTlsClient socket (serverName, trustPem (tlsTrust options)))
    use (TlsConnection socket)

-- | Resolves, connects, verifies, encrypts, and closes one TLS connection.
-- | Platform certificate trust and the connection hostname are used by default.
-- | The connection and every Stream carrying its scope cannot escape callback.
withTlsConnection :
    forall result. String ->
    Port -> (forall scope. TlsConnection scope -> IO TlsError result) -> IO TlsError result
withTlsConnection host port use = withTlsConnectionWith defaultTlsOptions host port use

-- | Opens a scoped TLS connection using explicit immutable options.
withTlsConnectionWith :
    forall result. TlsOptions ->
    String ->
    Port -> (forall scope. TlsConnection scope -> IO TlsError result) -> IO TlsError result
withTlsConnectionWith options host port use =
    bracket
        (mapError TlsNetworkError $ openTcpConnection host port)
        closeTlsTcp
        (startTlsAndUse options host use)

-- | Sends the complete byte string as encrypted application data.
sendTlsBytes : forall scope. TlsConnection scope -> Bytes -> IO TlsError ()
sendTlsBytes (TlsConnection socket) bytes =
    mapError (TlsHostError "send bytes") $ prelude.prim.sendBytes socket bytes

-- | Receives at most a positive number of decrypted bytes; empty bytes mean a
-- | peer close_notify. An unannounced TCP EOF is rejected as truncated TLS.
receiveTlsBytes : forall scope. TlsConnection scope -> Int -> IO TlsError Bytes
receiveTlsBytes (TlsConnection socket) maximum =
    if maximum <= 0
        then failIO (InvalidTlsChunkSize maximum)
        else mapError (TlsHostError "receive bytes") $ prelude.prim.recvBytes socket maximum

tlsByteStep : forall scope. TlsConnection scope -> Int -> () -> IO TlsError (Maybe (Bytes, ()))
tlsByteStep connection chunkSize _ = do
    bytes <- receiveTlsBytes connection chunkSize
    if bytesLength bytes == 0 then pureIO Nothing else pureIO (Just (bytes, ()))

-- | Pulls bounded decrypted byte chunks until close_notify without read-ahead.
tlsBytes : forall scope. TlsConnection scope -> Int -> Stream scope TlsError Bytes
tlsBytes connection chunkSize = unfoldScoped (tlsByteStep connection chunkSize) ()
