//! Owned host requests, completions, and worker commands.
//! These types cross the IO boundary without VM values or heap references.

use super::*;

pub(in crate::vm) enum IoRequest {
    Print(String),
    ReadLine,
    ReadFile {
        path: PathBuf,
        maximum: Option<i64>,
    },
    ReadFileBytes {
        path: PathBuf,
        maximum: Option<i64>,
    },
    WriteFile {
        path: PathBuf,
        contents: String,
        append: bool,
    },
    WriteFileBytes {
        path: PathBuf,
        contents: Vec<u8>,
    },
    Filesystem(FilesystemRequest),
    Process(ProcessRequest),
    StartProcess(ProcessRequest),
    ReadProcessStdout {
        process: ChildProcessId,
        maximum: i64,
    },
    ReadProcessStderr {
        process: ChildProcessId,
        maximum: i64,
    },
    WriteProcessStdin {
        process: ChildProcessId,
        bytes: Vec<u8>,
    },
    CloseProcessStdin(ChildProcessId),
    WaitProcess(ChildProcessId),
    TerminateProcess(ChildProcessId),
    CloseProcess(ChildProcessId),
    SecureRandomBytes(i64),
    ResolveHost(String),
    Connect {
        host: String,
        port: i64,
    },
    Listen {
        port: i64,
    },
    Accept(SocketId),
    SendBytes {
        socket: SocketId,
        bytes: Vec<u8>,
    },
    RecvBytes {
        socket: SocketId,
        maximum: i64,
    },
    CloseSocket(SocketId),
    Handle(HandleRequest),
    Http(HttpRequest),
    OpenHttp {
        handle: HandleId,
        request: HttpRequest,
    },
}

pub(in crate::vm) enum IoResult {
    Unit,
    Text(String),
    Bytes(Vec<u8>),
    Socket(SocketId),
    Listener { socket: SocketId, port: u16 },
    Handle(HandleId),
    Bool(bool),
    Int(i64),
    TextList(Vec<String>),
    DirectoryChanged(PathBuf),
    Process(ProcessOutput),
    StreamingProcess(StreamingProcess),
    Http(HttpResponse<Vec<u8>>),
    HttpOpened(HttpResponse<HandleId>),
    Failed(IoFailure),
}

pub(in crate::vm) enum IoFailure {
    Print(io::Error),
    ReadLine(io::Error),
    ReadFile {
        path: PathBuf,
        source: io::Error,
    },
    ReadFileLimited {
        path: PathBuf,
        source: io::Error,
    },
    ReadFileTooLarge {
        path: PathBuf,
        maximum: i64,
    },
    WriteFile {
        path: PathBuf,
        append: bool,
        source: io::Error,
    },
    GetEnv(String),
    SetEnv(String),
    Filesystem(FilesystemFailure),
    Process(ProcessFailure),
    ProcessStream {
        operation: &'static str,
        source: io::Error,
    },
    SecureRandom(io::Error),
    ResolveHost {
        host: String,
        source: io::Error,
    },
    Connect {
        host: String,
        port: i64,
        source: io::Error,
    },
    Listen {
        port: i64,
        source: io::Error,
    },
    Accept(io::Error),
    Send(io::Error),
    Recv(io::Error),
    CloseSocket(io::Error),
    Handle(HandleFailure),
    Http(HttpFailure),
}

pub(super) enum IoWork {
    CloseHttp(HandleId),
    Request {
        id: RequestId,
        request: IoRequest,
    },
    Process {
        id: RequestId,
        request: ProcessRequest,
        signals: mpsc::Receiver<ProcessSignal>,
        signal_tx: mpsc::Sender<ProcessSignal>,
    },
    StartProcess {
        id: RequestId,
        request: ProcessRequest,
        signals: mpsc::Receiver<ProcessSignal>,
    },
}
