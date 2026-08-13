import std.task
import std.process
main : Task ();
main = finally exitSuccess (printLine "success cleanup");
