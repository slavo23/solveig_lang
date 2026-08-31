//! Scheduler-thread allocation of structured host IO results.

use super::*;
use crate::builtins::VmCtx;
use solveig_ast::ast::CtorRuntimeRole;

impl Interpreter<'_> {
    pub(super) fn resume_worker_result(
        &mut self,
        program: &BytecodeProgram,
        result: IoResult,
    ) -> Result<Option<Value>, StepOutcome> {
        let value = match result {
            IoResult::Unit => Value::Unit,
            IoResult::Text(text) => {
                self.safe_point();
                self.heap.alloc_str(&text)
            }
            IoResult::Bytes(bytes) => {
                self.safe_point();
                self.heap.alloc_bytes(&bytes)
            }
            IoResult::Socket(socket) => Value::SocketId(socket.0),
            IoResult::Listener { socket, port } => {
                self.safe_point();
                VmCtx::new(self).alloc_data2_ctor(
                    CtorRuntimeRole::PreludeTuple(2),
                    Value::SocketId(socket.0),
                    Value::Int(i64::from(port)),
                    "listen",
                )?
            }
            IoResult::Handle(handle) => Value::HandleId(handle.0),
            IoResult::Bool(value) => {
                if value {
                    self.bools.true_val
                } else {
                    self.bools.false_val
                }
            }
            IoResult::Int(value) => Value::Int(value),
            IoResult::TextList(values) => self.alloc_string_list(&values, "listDirectory")?,
            IoResult::DirectoryChanged(path) => {
                self.io.set_current_directory(path);
                Value::Unit
            }
            IoResult::Process(output) => {
                self.alloc_process_result(output.exit_code, &output.stdout, &output.stderr)?
            }
            IoResult::StreamingProcess(process) => {
                let process = self.io.register_streaming_process(process)?;
                Value::ChildProcessId(process)
            }
            IoResult::Http(response) => {
                self.safe_point();
                let body = self.heap.alloc_bytes(&response.body);
                self.alloc_http_response(response.status, &response.headers, body)?
            }
            IoResult::HttpOpened(response) => self.alloc_http_response(
                response.status,
                &response.headers,
                Value::HandleId(response.body.0),
            )?,
            IoResult::Failed(failure) => {
                self.io_current = Some(self.alloc_io_failure_action(failure)?);
                return Ok(None);
            }
        };
        self.resume_io_value(program, value)
    }

    fn alloc_process_result(
        &mut self,
        exit_code: i64,
        stdout: &[u8],
        stderr: &[u8],
    ) -> Result<Value, RuntimeError> {
        let checkpoint = self.temporary_roots.len();
        self.safe_point();
        let stdout = self.heap.alloc_bytes(stdout);
        self.temporary_roots.push(stdout);
        self.safe_point();
        let stderr = self.heap.alloc_bytes(stderr);
        self.temporary_roots.push(stderr);
        self.safe_point();
        let stdout = *self
            .temporary_roots
            .get(checkpoint)
            .expect("ICE: process stdout root disappeared during allocation");
        let stderr = *self
            .temporary_roots
            .get(checkpoint + 1)
            .expect("ICE: process stderr root disappeared during allocation");
        let result = VmCtx::new(self).alloc_data3_ctor(
            CtorRuntimeRole::StdProcessResult,
            Value::Int(exit_code),
            stdout,
            stderr,
            "runProcess",
        );
        self.temporary_roots.truncate(checkpoint);
        result
    }

    fn alloc_http_response(
        &mut self,
        status: i64,
        headers: &[(String, String)],
        body: Value,
    ) -> Result<Value, RuntimeError> {
        let checkpoint = self.temporary_roots.len();
        self.temporary_roots.push(body);
        for (name, value) in headers {
            self.safe_point();
            let name = self.heap.alloc_str(name);
            self.temporary_roots.push(name);
            self.safe_point();
            let value = self.heap.alloc_str(value);
            self.temporary_roots.push(value);
            self.safe_point();
            let pair_start = self.temporary_roots.len() - 2;
            let name = self
                .temporary_roots
                .get(pair_start)
                .copied()
                .expect("ICE: HTTP header name root disappeared");
            let value = self
                .temporary_roots
                .get(pair_start + 1)
                .copied()
                .expect("ICE: HTTP header value root disappeared");
            let pair = VmCtx::new(self).alloc_data2_ctor(
                CtorRuntimeRole::PreludeTuple(2),
                name,
                value,
                "httpRequestRaw",
            )?;
            self.temporary_roots.truncate(pair_start);
            self.temporary_roots.push(pair);
        }
        let headers = VmCtx::new(self).alloc_list_from_root_range(
            checkpoint + 1,
            headers.len(),
            "httpRequestRaw",
        )?;
        self.temporary_roots.truncate(checkpoint + 1);
        self.temporary_roots.push(headers);
        self.safe_point();
        let headers = self
            .temporary_roots
            .get(checkpoint + 1)
            .copied()
            .expect("ICE: HTTP header-list root disappeared");
        let body = self
            .temporary_roots
            .get(checkpoint)
            .copied()
            .expect("ICE: HTTP body root disappeared");
        let result = VmCtx::new(self).alloc_data3_ctor(
            CtorRuntimeRole::PreludeTuple(3),
            Value::Int(status),
            headers,
            body,
            "httpRequestRaw",
        );
        self.temporary_roots.truncate(checkpoint);
        result
    }

    pub(super) fn alloc_string_list(
        &mut self,
        strings: &[String],
        builtin: &str,
    ) -> Result<Value, RuntimeError> {
        let checkpoint = self.temporary_roots.len();
        for string in strings {
            self.safe_point();
            let value = self.heap.alloc_str(string);
            self.temporary_roots.push(value);
        }
        let value = VmCtx::new(self).alloc_list_from_root_range(checkpoint, strings.len(), builtin);
        self.temporary_roots.truncate(checkpoint);
        value
    }

    pub(super) fn alloc_environment_list(
        &mut self,
        entries: &[(String, String)],
    ) -> Result<Value, RuntimeError> {
        let checkpoint = self.temporary_roots.len();
        for (name, value) in entries {
            self.safe_point();
            let name = self.heap.alloc_str(name);
            self.temporary_roots.push(name);
            self.safe_point();
            let value = self.heap.alloc_str(value);
            self.temporary_roots.push(value);
            self.safe_point();
            let fields_start = self.temporary_roots.len() - 2;
            let name = *self
                .temporary_roots
                .get(fields_start)
                .expect("ICE: environment name root disappeared");
            let value = *self
                .temporary_roots
                .last()
                .expect("ICE: environment value root disappeared");
            let pair = VmCtx::new(self).alloc_data2_ctor(
                CtorRuntimeRole::PreludeTuple(2),
                name,
                value,
                "getEnvironment",
            )?;
            self.temporary_roots.truncate(fields_start);
            self.temporary_roots.push(pair);
        }
        let result = VmCtx::new(self).alloc_list_from_root_range(
            checkpoint,
            entries.len(),
            "getEnvironment",
        );
        self.temporary_roots.truncate(checkpoint);
        result
    }
}
