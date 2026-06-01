mod approval;
mod budget;
mod policy;
mod protocol;
mod router;
mod turn;
mod windows;

use anyhow::{Context, Result};
use approval::{approval_request_for_method, meta_approves_app, ApprovalRequest};
use budget::check_request_budget;
use protocol::frame::{encode_frame, FrameDecoder};
use protocol::jsonrpc::{
    handle_jsonrpc_request_with_approval_and_turn_state, JsonRpcRequest, JsonRpcResponse,
};
use protocol::pipe::accept_named_pipe;
use protocol::stdio::{StdioRequest, StdioResponse};
use serde_json::{json, Value};
use std::{
    fs::File,
    io::{self, BufRead, Read, Write},
    time::Instant,
};

#[derive(Debug, Default)]
struct CliOptions {
    parent_pid: Option<u32>,
    frame_stdio: bool,
    native_pipe: Option<String>,
    turn_ended: Option<TurnEndedArgs>,
}

#[derive(Debug, Default)]
struct TurnEndedArgs {
    codex_home: Option<String>,
    session_id: Option<String>,
    turn_id: Option<String>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let options = parse_args(std::env::args().skip(1))?;
    if let Some(args) = options.turn_ended {
        return write_turn_ended_interrupt(args);
    }

    monitor_parent_process(options.parent_pid);

    if options.frame_stdio {
        return run_frame_stdio();
    }

    if let Some(pipe_path) = options.native_pipe {
        return run_native_pipe(&pipe_path);
    }

    run_line_stdio()
}

fn run_line_stdio() -> Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut turn_state = turn::ActiveTurnState::default();

    for line in stdin.lock().lines() {
        let line = line.context("failed to read request line")?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let request: StdioRequest =
            serde_json::from_str(trimmed).context("failed to parse request JSON")?;
        let response = handle_stdio_request_with_turn_state(request, &mut turn_state);
        serde_json::to_writer(&mut stdout, &response).context("failed to encode response JSON")?;
        stdout.write_all(b"\n").context("failed to write response newline")?;
        stdout.flush().context("failed to flush response")?;

        if response.should_close {
            break;
        }
    }

    Ok(())
}

fn run_frame_stdio() -> Result<()> {
    let mut stdin = io::stdin();
    let mut stdout = io::stdout();
    run_framed_transport(&mut stdin, &mut stdout)
}

fn run_native_pipe(pipe_path: &str) -> Result<()> {
    let mut pipe = accept_named_pipe(pipe_path)?;
    let mut reader = pipe.try_clone().context("failed to clone named pipe handle")?;
    run_native_pipe_transport(&mut reader, &mut pipe)
}

fn run_framed_transport(reader: &mut impl Read, writer: &mut impl Write) -> Result<()> {
    let mut decoder = FrameDecoder::default();
    let mut buffer = [0_u8; 8192];
    let mut next_callback_id = 1_u64;
    let mut turn_state = turn::ActiveTurnState::default();

    loop {
        let read = reader
            .read(&mut buffer)
            .context("failed to read framed request bytes")?;
        if read == 0 {
            return Ok(());
        }

        let messages = decoder
            .push(&buffer[..read])
            .context("failed to decode frame")?;

        for message in messages {
            let request: JsonRpcRequest =
                serde_json::from_str(&message).context("failed to parse JSON-RPC request")?;
            let response = handle_framed_request(
                request,
                reader,
                writer,
                &mut decoder,
                &mut buffer,
                &mut next_callback_id,
                &mut turn_state,
            )?;
            let should_close = response.should_close;
            write_jsonrpc_response(writer, &response)?;

            if should_close {
                return Ok(());
            }
        }
    }
}

fn run_native_pipe_transport(reader: &mut File, writer: &mut File) -> Result<()> {
    let mut decoder = FrameDecoder::default();
    let mut buffer = [0_u8; 8192];
    let mut next_callback_id = 1_u64;
    let mut turn_state = turn::ActiveTurnState::default();

    loop {
        let read = reader
            .read(&mut buffer)
            .context("failed to read native-pipe request bytes")?;
        if read == 0 {
            return Ok(());
        }

        let messages = decoder
            .push(&buffer[..read])
            .context("failed to decode frame")?;

        for message in messages {
            let request: JsonRpcRequest =
                serde_json::from_str(&message).context("failed to parse JSON-RPC request")?;
            let approval_deadline = approval_deadline_for_jsonrpc_request(&request);
            let response = handle_native_pipe_request(
                request,
                reader,
                writer,
                &mut decoder,
                &mut buffer,
                &mut next_callback_id,
                &mut turn_state,
                approval_deadline,
            )?;
            let should_close = response.should_close;
            write_jsonrpc_response(writer, &response)?;

            if should_close {
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
fn handle_stdio_request(request: StdioRequest) -> StdioResponse {
    handle_stdio_request_with_turn_state(request, &mut turn::ActiveTurnState::default())
}

fn handle_stdio_request_with_turn_state(
    request: StdioRequest,
    turn_state: &mut turn::ActiveTurnState,
) -> StdioResponse {
    let StdioRequest {
        id,
        method,
        params,
        meta,
    } = request;
    turn_state.observe_request(&method, &meta);

    if !turn::method_can_bypass_interrupt(&method) && turn::is_interrupted(&meta) {
        return StdioResponse::error(id, turn::STOPPED_BY_USER_MESSAGE);
    }

    let budget = match check_request_budget(&meta, &method) {
        Ok(budget) => budget,
        Err(error) => return StdioResponse::error(id, error),
    };

    if let Some(approval_request) = approval_request_for_method(&method, &params) {
        if !meta_approves_app(&meta, &approval_request.app) {
            return StdioResponse::approval_required(id, approval_request);
        }
    }

    if let Some(budget) = &budget {
        if let Err(error) = budget.check(&method) {
            return StdioResponse::error(id, error);
        }
    }

    match router::handle_method(&method, params) {
        router::RouterOutcome::Result(result) => StdioResponse::success(id, result),
        router::RouterOutcome::Close(result) => StdioResponse::closing(id, result),
        router::RouterOutcome::Error(message) => StdioResponse::error(id, message),
    }
}

fn handle_framed_request(
    request: JsonRpcRequest,
    reader: &mut impl Read,
    writer: &mut impl Write,
    decoder: &mut FrameDecoder,
    buffer: &mut [u8; 8192],
    next_callback_id: &mut u64,
    turn_state: &mut turn::ActiveTurnState,
) -> Result<JsonRpcResponse> {
    Ok(handle_jsonrpc_request_with_approval_and_turn_state(
        request,
        turn_state,
        |approval| {
            request_native_pipe_approval(
                reader,
                writer,
                decoder,
                buffer,
                next_callback_id,
                approval,
            )
        },
    ))
}

fn handle_native_pipe_request(
    request: JsonRpcRequest,
    reader: &mut File,
    writer: &mut File,
    decoder: &mut FrameDecoder,
    buffer: &mut [u8; 8192],
    next_callback_id: &mut u64,
    turn_state: &mut turn::ActiveTurnState,
    approval_deadline: Option<Instant>,
) -> Result<JsonRpcResponse> {
    Ok(handle_jsonrpc_request_with_approval_and_turn_state(
        request,
        turn_state,
        |approval| {
            request_native_pipe_approval_with_deadline(
                reader,
                writer,
                decoder,
                buffer,
                next_callback_id,
                approval,
                approval_deadline,
            )
        },
    ))
}

fn request_native_pipe_approval(
    reader: &mut impl Read,
    writer: &mut impl Write,
    decoder: &mut FrameDecoder,
    buffer: &mut [u8; 8192],
    next_callback_id: &mut u64,
    approval: ApprovalRequest,
) -> std::result::Result<Value, String> {
    request_native_pipe_approval_inner(
        |buffer| reader
            .read(buffer)
            .map_err(|error| format!("failed to read Computer Use approval response: {error}")),
        writer,
        decoder,
        buffer,
        next_callback_id,
        approval,
    )
}

fn request_native_pipe_approval_with_deadline(
    reader: &mut File,
    writer: &mut impl Write,
    decoder: &mut FrameDecoder,
    buffer: &mut [u8; 8192],
    next_callback_id: &mut u64,
    approval: ApprovalRequest,
    deadline: Option<Instant>,
) -> std::result::Result<Value, String> {
    request_native_pipe_approval_inner(
        |buffer| read_native_pipe_with_deadline(reader, buffer, deadline)
            .map_err(|error| format!("failed to read Computer Use approval response: {error}")),
        writer,
        decoder,
        buffer,
        next_callback_id,
        approval,
    )
}

fn request_native_pipe_approval_inner(
    mut read_approval_bytes: impl FnMut(&mut [u8]) -> std::result::Result<usize, String>,
    writer: &mut impl Write,
    decoder: &mut FrameDecoder,
    buffer: &mut [u8; 8192],
    next_callback_id: &mut u64,
    approval: ApprovalRequest,
) -> std::result::Result<Value, String> {
    let callback_id = Value::String(format!("computer-use-approval:{}", *next_callback_id));
    *next_callback_id += 1;

    let request = json!({
        "id": callback_id,
        "jsonrpc": "2.0",
        "method": "requestComputerUseApproval",
        "params": approval.native_pipe_params(),
    });

    write_framed_json(writer, &request)
        .map_err(|error| format!("failed to send Computer Use approval request: {error}"))?;

    loop {
        let read = read_approval_bytes(buffer)?;
        if read == 0 {
            return Err("Computer Use native pipe closed before approval response".to_string());
        }

        let messages = decoder
            .push(&buffer[..read])
            .map_err(|error| format!("failed to decode approval response frame: {error}"))?;
        for message in messages {
            let value = serde_json::from_str::<Value>(&message)
                .map_err(|error| format!("failed to parse approval response JSON: {error}"))?;
            if value.get("id") == Some(&callback_id) {
                if let Some(error) = value.get("error") {
                    return Err(jsonrpc_error_message(error));
                }
                return Ok(value.get("result").cloned().unwrap_or(Value::Null));
            }

            respond_to_overlapping_native_message(writer, value)
                .map_err(|error| format!("failed to respond to overlapping request: {error}"))?;
        }
    }
}

fn approval_deadline_for_jsonrpc_request(request: &JsonRpcRequest) -> Option<Instant> {
    if request.method != "request" {
        return None;
    }

    let codex_turn_metadata = request
        .params
        .get("codexTurnMetadata")
        .cloned()
        .unwrap_or(Value::Null);
    let meta = request.params.get("meta").cloned().unwrap_or(Value::Null);
    let metadata = if meta.is_null() {
        codex_turn_metadata
    } else {
        let mut merged = meta;
        if let Some(object) = merged.as_object_mut() {
            object
                .entry("codexTurnMetadata")
                .or_insert(codex_turn_metadata);
        }
        merged
    };

    budget::RequestBudget::from_meta(&metadata)
        .ok()
        .flatten()
        .map(|budget| budget.deadline())
}

#[cfg(windows)]
fn read_native_pipe_with_deadline(
    reader: &mut File,
    buffer: &mut [u8],
    deadline: Option<Instant>,
) -> io::Result<usize> {
    use std::{
        os::windows::io::AsRawHandle,
        ptr::null_mut,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        thread,
    };
    use windows_sys::Win32::{
        Foundation::ERROR_OPERATION_ABORTED,
        System::IO::CancelIoEx,
    };

    let Some(deadline) = deadline else {
        return reader.read(buffer);
    };
    if Instant::now() >= deadline {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "Computer Use approval request timed out",
        ));
    }

    let completed = Arc::new(AtomicBool::new(false));
    let timer_completed = Arc::clone(&completed);
    let handle = reader.as_raw_handle() as usize;
    thread::spawn(move || {
        let now = Instant::now();
        if deadline > now {
            thread::sleep(deadline - now);
        }
        if !timer_completed.load(Ordering::SeqCst) {
            unsafe {
                CancelIoEx(handle as _, null_mut());
            }
        }
    });

    let result = reader.read(buffer);
    completed.store(true, Ordering::SeqCst);
    match result {
        Err(error)
            if Instant::now() >= deadline
                && error.raw_os_error().map(|code| code as u32)
                    == Some(ERROR_OPERATION_ABORTED) =>
        {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Computer Use approval request timed out",
            ));
        }
        other => other,
    }
}

#[cfg(not(windows))]
fn read_native_pipe_with_deadline(
    reader: &mut File,
    buffer: &mut [u8],
    _deadline: Option<Instant>,
) -> io::Result<usize> {
    reader.read(buffer)
}

fn respond_to_overlapping_native_message(writer: &mut impl Write, message: Value) -> Result<()> {
    let Some(id) = message.get("id").cloned() else {
        return Ok(());
    };
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return Ok(());
    };

    let response = if method == "close" {
        json!({ "id": id, "jsonrpc": "2.0", "result": null })
    } else if method == "request"
        && message
            .get("params")
            .and_then(|params| params.get("method"))
            .and_then(Value::as_str)
            == Some("end_turn")
    {
        json!({ "id": id, "jsonrpc": "2.0", "result": null })
    } else {
        json!({
            "id": id,
            "jsonrpc": "2.0",
            "error": {
                "code": -32000,
                "message": "Computer Use helper already has an active request"
            }
        })
    };

    write_framed_json(writer, &response)
}

fn write_jsonrpc_response(writer: &mut impl Write, response: &JsonRpcResponse) -> Result<()> {
    let response_json =
        serde_json::to_string(response).context("failed to encode JSON-RPC response")?;
    writer
        .write_all(&encode_frame(&response_json))
        .context("failed to write response frame")?;
    writer.flush().context("failed to flush response frame")
}

fn write_framed_json(writer: &mut impl Write, message: &Value) -> Result<()> {
    let response_json =
        serde_json::to_string(message).context("failed to encode JSON-RPC message")?;
    writer
        .write_all(&encode_frame(&response_json))
        .context("failed to write JSON-RPC frame")?;
    writer.flush().context("failed to flush JSON-RPC frame")
}

fn jsonrpc_error_message(error: &Value) -> String {
    error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("Computer Use approval request failed")
        .to_string()
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<CliOptions> {
    let mut options = CliOptions::default();
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "turn-ended" => {
                options.turn_ended = Some(parse_turn_ended_args(iter)?);
                break;
            }
            "--parent-pid" => {
                let value = iter.next().context("missing value for --parent-pid")?;
                options.parent_pid = Some(
                    value
                        .parse::<u32>()
                        .with_context(|| format!("invalid --parent-pid value: {value}"))?,
                );
            }
            "--frame-stdio" => {
                options.frame_stdio = true;
            }
            "--native-pipe" => {
                options.native_pipe = Some(iter.next().context("missing value for --native-pipe")?);
            }
            unknown => anyhow::bail!("unknown argument: {unknown}"),
        }
    }
    Ok(options)
}

fn parse_turn_ended_args(mut args: impl Iterator<Item = String>) -> Result<TurnEndedArgs> {
    let mut parsed = TurnEndedArgs::default();
    let mut positionals = Vec::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--codex-home" => parsed.codex_home = Some(args.next().context("missing --codex-home value")?),
            "--session-id" | "--conversation-id" => {
                parsed.session_id = Some(args.next().context("missing session id value")?)
            }
            "--turn-id" => parsed.turn_id = Some(args.next().context("missing --turn-id value")?),
            _ => positionals.push(arg),
        }
    }

    if parsed.codex_home.is_none() {
        parsed.codex_home = std::env::var("CODEX_HOME").ok().filter(|value| !value.trim().is_empty());
    }
    if parsed.session_id.is_none() {
        parsed.session_id = positionals.first().cloned();
    }
    if parsed.turn_id.is_none() {
        parsed.turn_id = positionals.get(1).cloned();
    }

    Ok(parsed)
}

fn write_turn_ended_interrupt(args: TurnEndedArgs) -> Result<()> {
    let codex_home = args
        .codex_home
        .context("turn-ended requires CODEX_HOME or --codex-home")?;
    let session_id = args
        .session_id
        .context("turn-ended requires --session-id or positional session id")?;
    let turn_id = args
        .turn_id
        .context("turn-ended requires --turn-id or positional turn id")?;

    let scope = turn::TurnScope {
        codex_home: std::path::PathBuf::from(codex_home),
        session_id,
        turn_id,
    };
    let path = scope.interrupt_path();
    turn::write_interrupt_file(&scope).with_context(|| {
        format!(
            "failed to write Computer Use turn-ended interrupt file: {}",
            path.display()
        )
    })
}

#[cfg(windows)]
fn monitor_parent_process(parent_pid: Option<u32>) {
    use std::{ffi::c_void, ptr::null_mut, thread};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, WAIT_OBJECT_0},
        Storage::FileSystem::SYNCHRONIZE,
        System::Threading::{OpenProcess, WaitForSingleObject, INFINITE},
    };

    let Some(parent_pid) = parent_pid else {
        return;
    };

    thread::spawn(move || {
        let handle = unsafe { OpenProcess(SYNCHRONIZE, 0, parent_pid) };
        if handle == null_mut::<c_void>() {
            return;
        }

        let wait = unsafe { WaitForSingleObject(handle, INFINITE) };
        unsafe {
            CloseHandle(handle);
        }
        if wait == WAIT_OBJECT_0 {
            std::process::exit(0);
        }
    });
}

#[cfg(not(windows))]
fn monitor_parent_process(_parent_pid: Option<u32>) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_parent_pid() {
        let parsed = parse_args(["--parent-pid".to_string(), "123".to_string()]).unwrap();
        assert_eq!(parsed.parent_pid, Some(123));
    }

    #[test]
    fn parses_frame_stdio_mode() {
        let parsed = parse_args(["--frame-stdio".to_string()]).unwrap();
        assert!(parsed.frame_stdio);
    }

    #[test]
    fn parses_native_pipe_path() {
        let parsed = parse_args([
            "--native-pipe".to_string(),
            r"\\.\pipe\codex-computer-use-test".to_string(),
        ])
        .unwrap();
        assert_eq!(
            parsed.native_pipe,
            Some(r"\\.\pipe\codex-computer-use-test".to_string())
        );
    }

    #[test]
    fn parses_turn_ended_args() {
        let parsed = parse_args([
            "turn-ended".to_string(),
            "--codex-home".to_string(),
            r"C:\Users\test\.codex".to_string(),
            "--session-id".to_string(),
            "session/1".to_string(),
            "--turn-id".to_string(),
            "turn:2".to_string(),
        ])
        .unwrap();

        let turn_ended = parsed.turn_ended.unwrap();
        assert_eq!(turn_ended.codex_home, Some(r"C:\Users\test\.codex".to_string()));
        assert_eq!(turn_ended.session_id, Some("session/1".to_string()));
        assert_eq!(turn_ended.turn_id, Some("turn:2".to_string()));
    }

    #[test]
    fn stdio_window_action_returns_approval_request() {
        let response = handle_stdio_request(StdioRequest {
            id: serde_json::json!(1),
            method: "get_window_state".to_string(),
            params: serde_json::json!({
                "window": {
                    "app": r"process:C:\Windows\System32\notepad.exe",
                    "id": 1,
                    "title": "notepad"
                },
                "include_text": false,
                "include_screenshot": false
            }),
            meta: Value::Null,
        });

        assert!(!response.ok);
        assert!(response.error.is_none());
        assert_eq!(
            response.approval_request.unwrap().app,
            r"process:C:\Windows\System32\notepad.exe"
        );
    }

    #[test]
    fn stdio_approved_window_action_reaches_router() {
        let app = r"process:C:\Windows\System32\notepad.exe";
        let response = handle_stdio_request(StdioRequest {
            id: serde_json::json!(1),
            method: "get_window_state".to_string(),
            params: serde_json::json!({
                "window": {
                    "app": app,
                    "id": 1,
                    "title": "notepad"
                },
                "include_text": false,
                "include_screenshot": false
            }),
            meta: serde_json::json!({
                crate::approval::APPROVED_APP_META_KEY: app
            }),
        });

        assert!(!response.ok);
        assert!(response.approval_request.is_none());
        assert!(response.error.unwrap().contains("window"));
    }

    #[test]
    fn stdio_request_rejects_interrupted_turn() {
        let codex_home = test_codex_home("stdio_request_rejects_interrupted_turn");
        let scope = turn::TurnScope {
            codex_home: codex_home.clone(),
            session_id: "session/1".to_string(),
            turn_id: "turn:2".to_string(),
        };
        turn::write_interrupt_file(&scope).unwrap();

        let response = handle_stdio_request(StdioRequest {
            id: serde_json::json!(1),
            method: "list_windows".to_string(),
            params: Value::Null,
            meta: serde_json::json!({
                "codexHome": codex_home,
                "session_id": "session/1",
                "turn_id": "turn:2"
            }),
        });

        assert!(!response.ok);
        assert_eq!(response.error.as_deref(), Some(turn::STOPPED_BY_USER_MESSAGE));
    }

    #[test]
    fn stdio_request_rejects_expired_budget() {
        let response = handle_stdio_request(StdioRequest {
            id: serde_json::json!(1),
            method: "list_windows".to_string(),
            params: Value::Null,
            meta: serde_json::json!({
                crate::budget::REQUEST_BUDGET_META_KEY: 0
            }),
        });

        assert!(!response.ok);
        assert!(response.error.unwrap().contains("request budget"));
    }

    #[test]
    fn stdio_end_turn_bypasses_interrupted_turn() {
        let codex_home = test_codex_home("stdio_end_turn_bypasses_interrupted_turn");
        let scope = turn::TurnScope {
            codex_home: codex_home.clone(),
            session_id: "session".to_string(),
            turn_id: "turn".to_string(),
        };
        turn::write_interrupt_file(&scope).unwrap();

        let response = handle_stdio_request(StdioRequest {
            id: serde_json::json!(1),
            method: "end_turn".to_string(),
            params: Value::Null,
            meta: serde_json::json!({
                "codexHome": codex_home,
                "session_id": "session",
                "turn_id": "turn"
            }),
        });

        assert!(response.ok);
        assert_eq!(response.result, Some(Value::Null));
    }

    #[test]
    fn rejects_unknown_arg() {
        let error = parse_args(["--help".to_string()]).unwrap_err();
        assert!(error.to_string().contains("unknown argument"));
    }

    fn test_codex_home(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir()
            .join("sky-tests")
            .join(format!("{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        path
    }
}

