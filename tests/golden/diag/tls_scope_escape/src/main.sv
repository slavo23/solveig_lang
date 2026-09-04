-- A TLS connection cannot escape the callback that owns its host socket.
import std.net(Port)
import std.tls(TlsConnection, TlsError, withTlsConnection)

escape : Port -> IO TlsError (TlsConnection ())
escape port = withTlsConnection "localhost" port pureIO

main : Task ()
main = pureIO ()
