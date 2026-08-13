import prelude.prim

-- | Reads the command-line arguments passed to the program.
getArgs : forall e. IO e [String]
getArgs = prelude.prim.getArgs

-- | Reads a runtime-local environment variable when it is present.
getEnv : String -> Task (Maybe String)
getEnv name = prelude.prim.getEnv name

-- | Sets a runtime-local environment variable inherited by child processes.
setEnv : String -> String -> Task ()
setEnv name value = prelude.prim.setEnv name value

-- | Returns the runtime environment sorted by variable name.
getEnvironment : Task [Pair String String]
getEnvironment = prelude.prim.getEnvironment

-- | Returns the current Solveig project's directory name.
getProgName : Task String
getProgName = prelude.prim.getProgName
