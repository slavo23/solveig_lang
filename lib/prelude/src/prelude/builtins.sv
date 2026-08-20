import prelude.prim

-- | A value that may be present or absent.
data Maybe a =
    -- | Wraps a present value.
    Just a
  | -- | Represents the absence of a value.
    Nothing

-- | A value that can hold one of two alternatives.
data Either a b =
    -- | Holds a value from the left side.
    Left a
  | -- | Holds a value from the right side.
    Right b

-- | A successful value or an expected failure.
data Result e a =
    -- | Holds a successful value.
    Ok a
  | -- | Holds an expected failure.
    Err e

-- | A pair of two values.
data Pair a b =
    -- | Builds a pair from its first and second values.
    MkPair a b

-- | An error type with no possible values.
data Never

-- | Failures raised by built-in IO operations.
data IOError =
    -- | Writing to standard output failed.
    StdoutError String
  | -- | Reading from standard input failed.
    StdinError String
  | -- | Reading a file failed, with path and host message.
    ReadFileError String String
  | -- | Replacing a file's contents failed, with path and host message.
    WriteFileError String String
  | -- | Appending to a file failed, with path and host message.
    AppendFileError String String
  | -- | A channel operation cannot continue because the channel is closed.
    ChannelClosed
  | -- | A bounded channel capacity is outside the supported positive range.
    InvalidChannelCapacity Int
  | -- | A byte string is not valid UTF-8 text.
    InvalidUtf8 String
  | -- | Resolving a host name to an IPv4 address failed.
    ResolveHostError String
  | -- | Opening a TCP connection failed.
    ConnectError String
  | -- | Binding a TCP listener failed.
    ListenError String
  | -- | Accepting a TCP connection failed.
    AcceptError String
  | -- | Sending bytes on a TCP connection failed.
    SendError String
  | -- | Receiving bytes from a TCP connection failed.
    RecvError String
  | -- | Closing a TCP resource failed.
    CloseSocketError String
  | -- | Opening a handle failed, with path and host message.
    OpenHandleError String String
  | -- | Closing a handle failed.
    CloseHandleError String
  | -- | Flushing buffered output failed.
    FlushHandleError String
  | -- | Reading from a handle failed.
    ReadHandleError String
  | -- | Writing to a handle failed.
    WriteHandleError String
  | -- | Changing a handle's buffering failed.
    SetBufferingError String
  | -- | Reading an environment variable failed.
    GetEnvError String
  | -- | Changing an environment variable failed.
    SetEnvError String
  | -- | Reading the runtime working directory failed.
    GetCurrentDirectoryError String
  | -- | Changing the runtime working directory failed.
    SetCurrentDirectoryError String String
  | -- | Inspecting whether a file exists failed.
    DoesFileExistError String String
  | -- | Inspecting whether a directory exists failed.
    DoesDirectoryExistError String String
  | -- | Listing a directory failed.
    ListDirectoryError String String
  | -- | Creating a directory failed.
    CreateDirectoryError String String
  | -- | Removing a file failed.
    RemoveFileError String String
  | -- | Renaming a file failed, with source, destination and host message.
    RenameFileError String String String
  | -- | Reading a file's size failed.
    GetFileSizeError String String
  | -- | Reading a file's modification time failed.
    GetModificationTimeError String String
  | -- | Starting or capturing a child process failed.
    RunProcessError String String
  | -- | An HTTP request failed before a valid response was received.
    HttpTransportError String
  | -- | HTTPS certificate verification or TLS negotiation failed.
    HttpTlsError String
  | -- | An HTTP request exceeded its configured deadline.
    HttpTimeoutError String
  | -- | An HTTP redirect could not be followed under the configured policy.
    HttpRedirectError String
  | -- | An HTTP response header exceeded its configured byte bound.
    HttpHeadersTooLarge Int
  | -- | An HTTP response body exceeded its configured byte bound.
    HttpBodyTooLarge Int

-- | A defect raised by pure evaluation or a managed runtime limit.
data Defect =
    -- | An explicit unrecoverable condition raised by user code.
    Error String
  | -- | Integer division or modulo used a zero divisor.
    DivisionByZero
  | -- | A process exceeded the managed VM stack limit.
    StackExhausted
  | -- | The runtime exceeded the managed heap-object limit.
    HeapExhausted

-- | An IO action using the built-in error vocabulary.
type Task a = IO IOError a

-- | Creates an IO action that immediately returns a value.
pureIO : forall e a. a -> IO e a
pureIO value = prelude.prim.pureIO value

-- | Sequences an IO action and passes its result to a continuation.
bindIO : forall e a b. IO e a -> (a -> IO e b) -> IO e b
bindIO action continuation = prelude.prim.bindIO action continuation

-- | Creates an IO action that fails with a typed error.
failIO : forall e a. e -> IO e a
failIO error = prelude.prim.failIO error

-- | Raises a defect from pure code.
error : forall a. String -> a
error message = prelude.prim.error message

-- | Handles a typed error and may change the action's error type.
catchError : forall e f a. IO e a -> (e -> IO f a) -> IO f a
catchError action handler = prelude.prim.catchIO action handler

-- | Transforms the typed error of an IO action.
mapError : forall e f a. (e -> f) -> IO e a -> IO f a
mapError transform action =
    catchError action (fun actionError => failIO (transform actionError))

-- | Uses a fallback action when the first action fails.
orElse : forall e f a. IO e a -> IO f a -> IO f a
orElse action fallback = catchError action (fun actionError => fallback)

-- | Writes text followed by a newline.
printLine : String -> Task ()
printLine text = prelude.prim.hPutStrLn prelude.prim.stdout text

-- | Writes text without adding a newline.
printStr : String -> Task ()
printStr text =
    bindIO
        (prelude.prim.hPutStr prelude.prim.stdout text)
        (fun written => prelude.prim.hFlush prelude.prim.stdout)

-- | Reads one line of input.
readLine : Task String
readLine = prelude.prim.hGetLine prelude.prim.stdin

-- | Concatenates two strings.
appendString : String -> String -> String
appendString left right = prelude.prim.stringAppend left right

-- | Checks whether an integer is even.
even : Int -> Bool
even n = intEq (intMod n 2) 0

-- | Checks whether an integer is odd.
odd : Int -> Bool
odd n = not (even n)

-- | Flips a boolean value.
not : Bool -> Bool
not True = False
not False = True

-- | Returns True only when both inputs are True.
and : Bool -> Bool -> Bool
and True True = True
and _ _ = False

-- | Returns True when at least one input is True.
or : Bool -> Bool -> Bool
or False False = False
or _ _ = True
