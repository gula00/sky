#[cfg(windows)]
use anyhow::Result;

#[cfg(windows)]
use std::{
    ffi::OsStr,
    fs::File,
    os::windows::{ffi::OsStrExt, io::FromRawHandle},
    ptr::null_mut,
};

#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{ERROR_PIPE_CONNECTED, GetLastError, INVALID_HANDLE_VALUE},
    Storage::FileSystem::PIPE_ACCESS_DUPLEX,
    System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE,
        PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    },
};

#[cfg(windows)]
const PIPE_BUFFER_SIZE: u32 = 64 * 1024;

#[cfg(windows)]
pub fn accept_named_pipe(path: &str) -> Result<File> {
    let wide_path = wide_null(path);
    let handle = unsafe {
        CreateNamedPipeW(
            wide_path.as_ptr(),
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
            PIPE_UNLIMITED_INSTANCES,
            PIPE_BUFFER_SIZE,
            PIPE_BUFFER_SIZE,
            0,
            null_mut(),
        )
    };

    if handle == INVALID_HANDLE_VALUE {
        anyhow::bail!("failed to create named pipe: {}", path);
    }

    let connected = unsafe { ConnectNamedPipe(handle, null_mut()) };
    if connected == 0 {
        let error = unsafe { GetLastError() };
        if error != ERROR_PIPE_CONNECTED {
            anyhow::bail!("failed to connect named pipe {path}: Windows error {error}");
        }
    }

    let file = unsafe { File::from_raw_handle(handle as _) };
    Ok(file)
}

#[cfg(windows)]
fn wide_null(value: &str) -> Vec<u16> {
    OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(not(windows))]
pub fn accept_named_pipe(_path: &str) -> anyhow::Result<std::fs::File> {
    anyhow::bail!("named pipe mode is only supported on Windows")
}
