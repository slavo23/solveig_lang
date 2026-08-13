import prelude.prim

-- | A value that may be present or absent.
data Maybe a =
    -- | Wraps a present value.
    Just a
  | -- | Represents the absence of a value.
    Nothing;

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
    MkPair a b;

-- | An error type with no possible values.
data Never

-- | Failures raised by the built-in terminal and file operations.
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

-- | How a file is opened.
data IOMode =
    ReadMode
  | WriteMode
  | AppendMode
  | ReadWriteMode

-- | A handle's buffering policy. Nothing selects the platform default block size.
data BufferMode =
    NoBuffering
  | LineBuffering
  | BlockBuffering (Maybe Int)

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
type Task a = IO IOError a;

-- | Creates an IO action that immediately returns a value.
pureIO : forall e a. a -> IO e a;
pureIO value = prelude.prim.pureIO value;

-- | Sequences an IO action and passes its result to a continuation.
bindIO : forall e a b. IO e a -> (a -> IO e b) -> IO e b;
bindIO action continuation = prelude.prim.bindIO action continuation;

-- | Creates an IO action that fails with a typed error.
failIO : forall e a. e -> IO e a;
failIO error = prelude.prim.failIO error;

-- | Raises a defect from pure code.
error : forall a. String -> a;
error message = prelude.prim.error message;

-- | Handles a typed error and may change the action's error type.
catchError : forall e f a. IO e a -> (e -> IO f a) -> IO f a;
catchError action handler = prelude.prim.catchIO action handler;

-- | Handles defects raised while executing an IO action's installed scope.
catchDefect : forall e a. IO e a -> (Defect -> IO e a) -> IO e a;
catchDefect action handler = prelude.prim.catchDefectIO action handler;

-- | Transforms the typed error of an IO action.
mapError : forall e f a. (e -> f) -> IO e a -> IO f a;
mapError transform action =
    catchError action (fun error => failIO (transform error));

-- | Uses a fallback action when the first action fails.
orElse : forall e f a. IO e a -> IO f a -> IO f a;
orElse action fallback = catchError action (fun error => fallback);

-- | Runs cleanup after an action succeeds or raises a typed error.
finally : forall e a b. IO e a -> IO e b -> IO e a;
finally action cleanup =
    prelude.prim.maskIO
        (prelude.prim.finallyIO
            (prelude.prim.restoreMaskIO action)
            cleanup);

-- | Acquires a resource and releases it after its use action finishes.
bracket : forall e resource cleanup a. IO e resource -> (resource -> IO e cleanup) -> (resource -> IO e a) -> IO e a;
bracket acquire release use =
    mask
        (bindIO acquire (fun resource =>
            prelude.prim.finallyIO
                (prelude.prim.restoreMaskIO (use resource))
                (release resource)));

-- | Runs a child action within a scope and waits for its cancellation on exit.
-- Peyton Jones, Gordon, and Finne, "Concurrent Haskell" (POPL 1996), section
-- 3.2, derives coordination abstractions from a private typed channel.
-- Control.Concurrent.Async documents `withAsync` as bracketed spawn and cancel:
-- https://hackage.haskell.org/package/async/docs/Control-Concurrent-Async.html
withThread : forall e a b. IO e a -> (ThreadId -> IO e b) -> IO e b;
withThread action body =
    bindIO newChan (fun started =>
        bindIO newChan (fun finished =>
            bracket
                (spawn
                    (finally
                        (bindIO
                            (send started ())
                            (fun firstReady =>
                                bindIO
                                    (send started ())
                                    (fun secondReady =>
                                        catchError
                                            (bindIO
                                                (prelude.prim.restoreMaskIO action)
                                                (fun value => pureIO ()))
                                            (fun actionError => pureIO ()))))
                        (send finished ())))
                (fun thread =>
                    catchError
                        (bindIO
                            (recv started)
                            (fun ready =>
                                bindIO
                                    (killThread thread)
                                    (fun killed => recv finished)))
                        (fun channelError =>
                            error "structured concurrency internal channel closed"))
                (fun thread =>
                    bindIO
                        (catchError
                            (recv started)
                            (fun channelError =>
                                error "structured concurrency internal channel closed"))
                        (fun ready => body thread))));

-- | Runs two actions and returns the first result, cancelling the loser.
-- Control.Concurrent.Async specifies `race` as two scoped children followed by
-- a first-completion wait; the private channel is Solveig's wait primitive:
-- https://hackage.haskell.org/package/async/docs/Control-Concurrent-Async.html#v:race
race : forall e a b. IO e a -> IO e b -> IO e (Either a b);
race left right =
    bindIO newChan (fun outcomes =>
        withThread
            (catchError
                (catchDefect
                    (bindIO
                        (catchError
                            (bindIO left (fun value => pureIO (Right value)))
                            (fun typedError => pureIO (Left typedError)))
                        (fun outcome => send outcomes (Left (Right outcome))))
                    (fun defect => send outcomes (Left (Left defect))))
                (fun channelError =>
                    error "structured concurrency internal channel closed"))
            (fun leftThread =>
            withThread
                (catchError
                    (catchDefect
                        (bindIO
                            (catchError
                                (bindIO right (fun value => pureIO (Right value)))
                                (fun typedError => pureIO (Left typedError)))
                            (fun outcome => send outcomes (Right (Right outcome))))
                        (fun defect => send outcomes (Right (Left defect))))
                    (fun channelError =>
                        error "structured concurrency internal channel closed"))
                (fun rightThread =>
                    bindIO
                        (catchError
                            (recv outcomes)
                            (fun channelError =>
                                error "structured concurrency internal channel closed"))
                        (fun winner =>
                            case winner of
                                Left leftOutcome =>
                                    case leftOutcome of
                                        Left defect => prelude.prim.raiseDefectIO defect
                                        | Right typedOutcome =>
                                            case typedOutcome of
                                                Left typedError => failIO typedError
                                                | Right value => pureIO (Left value)
                                | Right rightOutcome =>
                                    case rightOutcome of
                                        Left defect => prelude.prim.raiseDefectIO defect
                                        | Right typedOutcome =>
                                            case typedOutcome of
                                                Left typedError => failIO typedError
                                                | Right value => pureIO (Right value)))));

-- | Runs two actions and returns both results, cancelling either after failure.
-- Control.Concurrent.Async's `concurrently` waits for both scoped children but
-- cancels the peer as soon as one child fails:
-- https://hackage.haskell.org/package/async/docs/Control-Concurrent-Async.html#v:concurrently
concurrently : forall e a b. IO e a -> IO e b -> IO e (Pair a b);
concurrently left right =
    bindIO newChan (fun outcomes =>
        withThread
            (catchError
                (catchDefect
                    (bindIO
                        (catchError
                            (bindIO left (fun value => pureIO (Right value)))
                            (fun typedError => pureIO (Left typedError)))
                        (fun outcome => send outcomes (Left (Right outcome))))
                    (fun defect => send outcomes (Left (Left defect))))
                (fun channelError =>
                    error "structured concurrency internal channel closed"))
            (fun leftThread =>
            withThread
                (catchError
                    (catchDefect
                        (bindIO
                            (catchError
                                (bindIO right (fun value => pureIO (Right value)))
                                (fun typedError => pureIO (Left typedError)))
                            (fun outcome => send outcomes (Right (Right outcome))))
                        (fun defect => send outcomes (Right (Left defect))))
                    (fun channelError =>
                        error "structured concurrency internal channel closed"))
                (fun rightThread =>
                    bindIO
                        (catchError
                            (recv outcomes)
                            (fun channelError =>
                                error "structured concurrency internal channel closed"))
                        (fun first =>
                            let firstAction =
                                case first of
                                    Left (Left defect) => prelude.prim.raiseDefectIO defect
                                    | Left (Right (Left typedError)) => failIO typedError
                                    | Left (Right (Right leftValue)) => pureIO (Left leftValue)
                                    | Right (Left defect) => prelude.prim.raiseDefectIO defect
                                    | Right (Right (Left typedError)) => failIO typedError
                                    | Right (Right (Right rightValue)) => pureIO (Right rightValue)
                            in bindIO firstAction
                                (fun firstValue =>
                                    bindIO
                                        (catchError
                                            (recv outcomes)
                                            (fun channelError =>
                                                error "structured concurrency internal channel closed"))
                                        (fun second =>
                                            case MkPair firstValue second of
                                                MkPair (Left leftValue) (Right (Left defect)) =>
                                                    prelude.prim.raiseDefectIO defect
                                                | MkPair (Left leftValue) (Right (Right (Left typedError))) =>
                                                    failIO typedError
                                                | MkPair (Left leftValue) (Right (Right (Right rightValue))) =>
                                                    pureIO (MkPair leftValue rightValue)
                                                | MkPair (Right rightValue) (Left (Left defect)) =>
                                                    prelude.prim.raiseDefectIO defect
                                                | MkPair (Right rightValue) (Left (Right (Left typedError))) =>
                                                    failIO typedError
                                                | MkPair (Right rightValue) (Left (Right (Right leftValue))) =>
                                                    pureIO (MkPair leftValue rightValue)
                                                | MkPair (Left leftValue) (Left duplicate) =>
                                                    error "structured child sent more than one outcome"
                                                | MkPair (Right rightValue) (Right duplicate) =>
                                                    error "structured child sent more than one outcome"))))));

-- | Runs an action until its millisecond deadline and returns Nothing on expiry.
-- This is the timer-specialized form of Control.Concurrent.Async's `race`, so
-- the losing action is cancelled through the same scoped-child protocol:
-- https://hackage.haskell.org/package/async/docs/Control-Concurrent-Async.html#v:race
timeout : forall e a. Int -> IO e a -> IO e (Maybe a);
timeout milliseconds action =
    bindIO
        (race
            (catchError
                (bindIO (sleep milliseconds) (fun elapsed => pureIO Nothing))
                (fun timerError =>
                    error "structured concurrency timer failed"))
            (bindIO action (fun value => pureIO (Just value))))
        (fun winner =>
            case winner of
                Left timedOut => pureIO timedOut
                | Right completed => pureIO completed);

-- | Writes text followed by a newline.
printLine : String -> Task ();
printLine text = hPutStrLn stdout text;

-- | Writes text without adding a newline.
printStr : String -> Task ();
printStr text =
    bindIO (hPutStr stdout text) (fun written => hFlush stdout);

-- | Reads one line of input.
readLine : Task String;
readLine = hGetLine stdin;

-- | The process standard-input handle.
stdin : Handle;
stdin = prelude.prim.stdin;

-- | The process standard-output handle.
stdout : Handle;
stdout = prelude.prim.stdout;

-- | The process standard-error handle.
stderr : Handle;
stderr = prelude.prim.stderr;

-- | Opens a file handle. The handle must be closed explicitly.
openFile : String -> IOMode -> Task Handle;
openFile path mode = prelude.prim.openFile path mode;

-- | Closes a handle, flushing pending output first.
hClose : Handle -> Task ();
hClose handle = prelude.prim.hClose handle;

-- | Flushes pending output from a handle.
hFlush : Handle -> Task ();
hFlush handle = prelude.prim.hFlush handle;

-- | Reads one line without its line terminator.
hGetLine : Handle -> Task String;
hGetLine handle = prelude.prim.hGetLine handle;

-- | Strictly reads all remaining text from a handle.
hGetContents : Handle -> Task String;
hGetContents handle = prelude.prim.hGetContents handle;

-- | Reads at most the requested number of arbitrary bytes, returning empty at EOF.
hGetBytes : Handle -> Int -> Task Bytes;
hGetBytes handle maximum = prelude.prim.hGetBytes handle maximum;

-- | Writes text to a handle.
hPutStr : Handle -> String -> Task ();
hPutStr handle text = prelude.prim.hPutStr handle text;

-- | Writes text followed by a newline to a handle.
hPutStrLn : Handle -> String -> Task ();
hPutStrLn handle text = prelude.prim.hPutStrLn handle text;

-- | Writes arbitrary bytes to a handle.
hPutBytes : Handle -> Bytes -> Task ();
hPutBytes handle bytes = prelude.prim.hPutBytes handle bytes;

-- | Reports whether a readable handle is at end of input.
hIsEOF : Handle -> Task Bool;
hIsEOF handle = prelude.prim.hIsEOF handle;

-- | Changes a handle's buffering policy, flushing pending output first.
hSetBuffering : Handle -> BufferMode -> Task ();
hSetBuffering handle mode = prelude.prim.hSetBuffering handle mode;

-- | Opens a file and closes it after use, including errors and interruption.
withFile : forall a. String -> IOMode -> (Handle -> Task a) -> Task a;
withFile path mode use = bracket (openFile path mode) hClose use;

-- | Reads a file as text.
readFile : String -> Task String;
readFile path = prelude.prim.readFile path;

-- | Reads an entire file as arbitrary bytes.
readFileBytes : String -> Task Bytes;
readFileBytes path = prelude.prim.readFileBytes path;

-- | Writes text to a file, replacing its previous contents.
writeFile : String -> String -> Task ();
writeFile path contents = prelude.prim.writeFile path contents;

-- | Writes arbitrary bytes to a file, replacing its previous contents.
writeFileBytes : String -> Bytes -> Task ();
writeFileBytes path contents = prelude.prim.writeFileBytes path contents;

-- | Appends text to the end of a file.
appendFile : String -> String -> Task ();
appendFile path contents = prelude.prim.appendFile path contents;

-- | Starts an IO action in a new lightweight thread.
spawn : forall childError e. IO childError () -> IO e ThreadId;
spawn action = prelude.prim.spawn action;

-- | Requests interruption of a lightweight thread.
killThread : ThreadId -> Task ();
killThread thread = prelude.prim.killThread thread;

-- | Suspends the current thread for at least the requested milliseconds.
sleep : Int -> Task ();
sleep milliseconds = prelude.prim.sleep milliseconds;

-- | Defers interruption while an action runs, except while it is blocked.
mask : forall e a. IO e a -> IO e a;
mask action = prelude.prim.maskIO action;

-- | Creates an unbounded typed channel.
newChan : forall e a. IO e (Chan a);
newChan = prelude.prim.newChan;

-- | Creates a typed channel with a positive capacity.
newBoundedChan : forall e a. Int -> IO e (Chan a);
newBoundedChan capacity = prelude.prim.newBoundedChan capacity;

-- | Sends a value, blocking while a bounded channel is full.
send : forall a. Chan a -> a -> Task ();
send channel value = prelude.prim.send channel value;

-- | Receives the oldest queued value, blocking while an open channel is empty.
recv : forall a. Chan a -> Task a;
recv channel = prelude.prim.recv channel;

-- | Receives the oldest queued value when one is immediately available.
tryRecv : forall a. Chan a -> Task (Maybe a);
tryRecv channel = prelude.prim.tryRecv channel;

-- | Closes a channel and wakes its blocked senders and receivers.
closeChan : forall a. Chan a -> Task ();
closeChan channel = prelude.prim.closeChan channel;

-- | Encodes text as UTF-8 bytes.
stringToBytes : String -> Bytes;
stringToBytes text = prelude.prim.stringToBytes text;

-- | Decodes UTF-8 bytes without replacing invalid input.
bytesToString : Bytes -> Result IOError String;
bytesToString bytes = prelude.prim.bytesToString bytes;

-- | Returns the number of bytes.
bytesLength : Bytes -> Int;
bytesLength bytes = prelude.prim.bytesLength bytes;

-- | Returns the byte at an index when it exists.
bytesIndex : Bytes -> Int -> Maybe Int;
bytesIndex bytes index = prelude.prim.bytesIndex bytes index;

-- | Takes bytes from a starting index for a requested length.
bytesSlice : Bytes -> Int -> Int -> Bytes;
bytesSlice bytes start length = prelude.prim.bytesSlice bytes start length;

-- | Concatenates two byte strings.
bytesAppend : Bytes -> Bytes -> Bytes;
bytesAppend left right = prelude.prim.bytesAppend left right;

-- | Opens a TCP connection to an IP address or localhost.
connect : String -> Int -> Task Socket;
connect host port = prelude.prim.connect host port;

-- | Resolves a host name to an IPv4 address without blocking other green threads.
resolveHost : String -> Task String;
resolveHost host = prelude.prim.resolveHost host;

-- | Binds a loopback TCP listener and returns its actual port.
listen : Int -> Task (Pair Socket Int);
listen port = prelude.prim.listen port;

-- | Accepts the next connection from a TCP listener.
accept : Socket -> Task Socket;
accept socket = prelude.prim.accept socket;

-- | Sends an entire byte string on a connected TCP socket.
sendBytes : Socket -> Bytes -> Task ();
sendBytes socket bytes = prelude.prim.sendBytes socket bytes;

-- | Receives at most the requested number of bytes, returning empty at EOF.
recvBytes : Socket -> Int -> Task Bytes;
recvBytes socket maximum = prelude.prim.recvBytes socket maximum;

-- | Explicitly closes a TCP listener or connection.
closeSocket : Socket -> Task ();
closeSocket socket = prelude.prim.closeSocket socket;

-- | Concatenates two strings.
appendString : String -> String -> String;
appendString left right = prelude.prim.stringAppend left right;

-- | Checks whether an integer is even.
even : Int -> Bool;
even n = intEq (intMod n 2) 0;

-- | Checks whether an integer is odd.
odd : Int -> Bool;
odd n = not (even n);

-- | Flips a boolean value.
not : Bool -> Bool;
not True = False;
not False = True;

-- | Returns True only when both inputs are True.
and : Bool -> Bool -> Bool;
and True True = True;
and _ _ = False;

-- | Returns True when at least one input is True.
or : Bool -> Bool -> Bool;
or False False = False;
or _ _ = True;
