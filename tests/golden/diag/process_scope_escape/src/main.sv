import std.process(Process, ProcessError, ProcessHandle, withProcess)

escape : Process -> IO ProcessError (ProcessHandle ())
escape processValue = withProcess 8 processValue escapeHandle

escapeHandle : forall scope. ProcessHandle scope -> IO ProcessError (ProcessHandle ())
escapeHandle child = pureIO child

main : Task ()
main = pureIO ()
