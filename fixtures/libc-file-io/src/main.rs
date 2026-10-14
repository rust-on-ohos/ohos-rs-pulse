use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

struct Descriptor(libc::c_int);

impl Descriptor {
    fn close(mut self) -> io::Result<()> {
        let fd = std::mem::replace(&mut self.0, -1);
        // Do not retry close: an interrupted close may already release the descriptor.
        if unsafe { libc::close(fd) } == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        if self.0 != -1 {
            // Best-effort cleanup during error propagation; successful paths close explicitly.
            unsafe { libc::close(self.0) };
        }
    }
}

fn round_trip(path: &CString, writer: Descriptor) -> io::Result<()> {
    let expected = b"ohos-pulse libc file round trip\n";
    let mut written = 0;
    while written < expected.len() {
        // The slice remains valid for the duration of the call.
        let count = unsafe {
            libc::write(
                writer.0,
                expected[written..].as_ptr().cast(),
                expected.len() - written,
            )
        };
        if count < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "write made no progress",
            ));
        }
        written += count as usize;
    }
    writer.close()?;
    // The path is NUL-terminated and valid for the call.
    let fd = unsafe { libc::open(path.as_ptr(), libc::O_RDONLY) };
    if fd == -1 {
        return Err(io::Error::last_os_error());
    }
    let reader = Descriptor(fd);
    let mut actual = Vec::new();
    loop {
        let mut buffer = [0u8; 64];
        // The initialized buffer is writable for its entire declared length.
        let count = unsafe { libc::read(reader.0, buffer.as_mut_ptr().cast(), buffer.len()) };
        if count < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if count == 0 {
            break;
        }
        actual.extend_from_slice(&buffer[..count as usize]);
        if actual.len() > expected.len() {
            return Err(io::Error::other("file contained unexpected extra bytes"));
        }
    }
    reader.close()?;
    if actual != expected {
        return Err(io::Error::other(
            "file contents did not match expected bytes",
        ));
    }
    Ok(())
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 3 {
        return Err("usage: libc-file-io <writable-directory> <run-id>".into());
    }
    let id = args[2].to_str().ok_or("run-id must be UTF-8")?;
    if id.is_empty()
        || id.len() > 128
        || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err("run-id must contain 1–128 ASCII letters, digits, or hyphens".into());
    }
    let path = Path::new(&args[1]).join(format!("ohos-pulse-{id}.tmp"));
    let path = CString::new(path.as_os_str().as_bytes())?;
    // O_EXCL prevents overwriting existing files; the explicit mode is required by O_CREAT.
    let fd = unsafe {
        libc::open(
            path.as_ptr(),
            libc::O_CREAT | libc::O_EXCL | libc::O_WRONLY,
            0o600 as libc::c_uint,
        )
    };
    if fd == -1 {
        return Err(io::Error::last_os_error().into());
    }
    let outcome = round_trip(&path, Descriptor(fd));
    // Only remove the file after this invocation has successfully created it.
    let cleanup = unsafe { libc::unlink(path.as_ptr()) };
    let cleanup_error = if cleanup == -1 {
        Some(io::Error::last_os_error())
    } else {
        None
    };
    if let Some(error) = cleanup_error {
        if let Err(original) = &outcome {
            eprintln!("scenario error: {original}");
        }
        return Err(format!("file cleanup failed: {error}").into());
    }
    outcome?;
    println!("{{\"scenario_id\":\"file-round-trip\",\"run_id\":\"{id}\",\"status\":\"passed\"}}");
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("file-round-trip failed: {error}");
        std::process::exit(1);
    }
}
