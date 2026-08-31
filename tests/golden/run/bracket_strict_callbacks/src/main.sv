-- Callback construction is strict, so bracket must install cleanup before applying use.
import std.channel
import std.task(bracket, catchDefect)

badUse : () -> Task ()
badUse _ = error "defect constructing the use action"

badRelease : () -> Task ()
badRelease _ = error "defect constructing the release action"

main : Task ()
main = do
    released <- std.channel.newChan
    catchDefect
        (bracket (pureIO ()) (fun _ => std.channel.send released "released") badUse)
        (fun _ => std.channel.send released "caught")
    first <- std.channel.recv released
    second <- std.channel.recv released
    printLine (first <> ":" <> second)
    used <- std.channel.newChan
    catchDefect
        (bracket (pureIO ()) badRelease (fun _ => std.channel.send used "used"))
        (fun _ => std.channel.send used "caught")
    firstUse <- std.channel.recv used
    secondUse <- std.channel.recv used
    printLine (firstUse <> ":" <> secondUse)
