-- A TCP connection cannot escape the callback that owns its host socket.
import std.net(*)

escape : Port -> IO NetError (TcpConnection ())
escape port = withTcpConnection "localhost" port pureIO

main : Task ()
main = pureIO ()
