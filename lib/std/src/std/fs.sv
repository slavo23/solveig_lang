export (
    FsError(..), getCurrentDirectory, setCurrentDirectory, doesFileExist,
    doesDirectoryExist, listDirectory, createDirectory, removeFile, renameFile,
    getFileSize, getModificationTime
)

import prelude.prim(*)
import std.path(Path, currentPath, pathFromHost, pathToString)
import std.time(SystemTime, systemTimeFromNanoseconds)

-- | A filesystem failure with the path context known by the public operation.
data FsError = FsHostError Path HostError

instance Eq FsError where
    (==) (FsHostError leftPath leftHost) (FsHostError rightPath rightHost) =
        and (leftPath == rightPath) (leftHost == rightHost)

instance Show FsError where
    show (FsHostError path hostError) =
        "filesystem " <> show path <> ": " <> show hostError

atPath : forall a. Path -> Task a -> IO FsError a
atPath path operation = mapError (FsHostError path) operation

-- | Returns the runtime's current working directory.
getCurrentDirectory : IO FsError Path
getCurrentDirectory =
    fmap pathFromHost (atPath currentPath prelude.prim.getCurrentDirectory)

-- | Changes the runtime's working directory after verifying it exists.
setCurrentDirectory : Path -> IO FsError ()
setCurrentDirectory path =
    atPath path (prelude.prim.setCurrentDirectory (pathToString path))

-- | Reports whether a path names a regular file.
doesFileExist : Path -> IO FsError Bool
doesFileExist path =
    atPath path (prelude.prim.doesFileExist (pathToString path))

-- | Reports whether a path names a directory.
doesDirectoryExist : Path -> IO FsError Bool
doesDirectoryExist path =
    atPath path (prelude.prim.doesDirectoryExist (pathToString path))

-- | Lists a directory's immediate entries as paths in lexical order.
listDirectory : Path -> IO FsError [Path]
listDirectory path =
    fmap
        (map pathFromHost)
        (atPath path (prelude.prim.listDirectory (pathToString path)))

-- | Creates one directory; its parent must already exist.
createDirectory : Path -> IO FsError ()
createDirectory path =
    atPath path (prelude.prim.createDirectory (pathToString path))

-- | Removes one file.
removeFile : Path -> IO FsError ()
removeFile path =
    atPath path (prelude.prim.removeFile (pathToString path))

-- | Renames a file or directory.
renameFile : Path -> Path -> IO FsError ()
renameFile source destination =
    atPath
        source
        (prelude.prim.renameFile
            (pathToString source)
            (pathToString destination))

-- | Returns a file's size in bytes.
getFileSize : Path -> IO FsError Int
getFileSize path =
    atPath path (prelude.prim.getFileSize (pathToString path))

-- | Returns a path's wall-clock modification time.
getModificationTime : Path -> IO FsError SystemTime
getModificationTime path =
    fmap
        systemTimeFromNanoseconds
        (atPath path (prelude.prim.getModificationTime (pathToString path)))
