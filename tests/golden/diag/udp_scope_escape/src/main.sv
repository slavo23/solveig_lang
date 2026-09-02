-- A UDP socket cannot escape the callback that owns its host socket.
import std.net(*)

escape : IO NetError (UdpSocket ())
escape = withUdpSocket anyPort (fun socket _ => pureIO socket)

main : Task ()
main = pureIO ()
