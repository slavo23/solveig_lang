export (
    FsError(..), getCurrentDirectory, setCurrentDirectory, doesFileExist, doesDirectoryExist,
    listDirectory, createDirectory, removeFile, renameFile, getFileSize,
    getModificationTime
)

import prelude.prim(*)

-- | A filesystem failure with the path context known by the public operation.
data FsError = FsHostError String HostError

instance Eq FsError where
    (==) (FsHostError leftPath leftHost) (FsHostError rightPath rightHost) =
        and (leftPath == rightPath) (leftHost == rightHost)

instance Show FsError where
    show (FsHostError path hostError) =
        "filesystem " <> show path <> ": " <> show hostError

atPath : forall a. String -> Task a -> IO FsError a
atPath path operation = mapError (FsHostError path) operation

-- | Returns the runtime's current working directory.
getCurrentDirectory : IO FsError String
getCurrentDirectory = atPath "." prelude.prim.getCurrentDirectory

-- | Changes the runtime's working directory after verifying it exists.
setCurrentDirectory : String -> IO FsError ()
setCurrentDirectory path = atPath path (prelude.prim.setCurrentDirectory path)

-- | Reports whether a path names a regular file.
doesFileExist : String -> IO FsError Bool
doesFileExist path = atPath path (prelude.prim.doesFileExist path)

-- | Reports whether a path names a directory.
doesDirectoryExist : String -> IO FsError Bool
doesDirectoryExist path = atPath path (prelude.prim.doesDirectoryExist path)

-- | Lists a directory's immediate entries in lexical order.
listDirectory : String -> IO FsError [String]
listDirectory path = atPath path (prelude.prim.listDirectory path)

-- | Creates one directory; its parent must already exist.
createDirectory : String -> IO FsError ()
createDirectory path = atPath path (prelude.prim.createDirectory path)

-- | Removes one file.
removeFile : String -> IO FsError ()
removeFile path = atPath path (prelude.prim.removeFile path)

-- | Renames a file or directory.
renameFile : String -> String -> IO FsError ()
renameFile source destination =
    atPath (source <> " -> " <> destination) (prelude.prim.renameFile source destination)

-- | Returns a file's size in bytes.
getFileSize : String -> IO FsError Int
getFileSize path = atPath path (prelude.prim.getFileSize path)

-- | Returns nanoseconds since the Unix epoch for a path's modification time.
getModificationTime : String -> IO FsError Int
getModificationTime path = atPath path (prelude.prim.getModificationTime path)
