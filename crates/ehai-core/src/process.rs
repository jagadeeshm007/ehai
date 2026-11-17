//! Running external commands with a timeout (every external call has one; spec §11).

use std::io::Read;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

/// Run `cmd` to completion, killing it after `timeout`. Stdout/stderr are drained
/// on background threads so a chatty child can't deadlock on a full pipe.
pub fn output(cmd: &mut Command, timeout: Duration) -> std::io::Result<Output> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let drain = |r: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut r) = r {
                let _ = r.read_to_end(&mut buf);
            }
            buf
        })
    };
    let out = drain(
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    );
    let err = drain(
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    );
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("timed out after {}s", timeout.as_secs()),
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    Ok(Output {
        status,
        stdout: out.join().unwrap_or_default(),
        stderr: err.join().unwrap_or_default(),
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn captures_output_and_times_out() {
        let o = output(
            Command::new("sh").args(["-c", "echo hi; echo err >&2"]),
            Duration::from_secs(5),
        )
        .unwrap();
        assert!(o.status.success());
        assert_eq!(String::from_utf8_lossy(&o.stdout), "hi\n");
        assert_eq!(String::from_utf8_lossy(&o.stderr), "err\n");

        let e = output(Command::new("sleep").arg("5"), Duration::from_millis(100)).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::TimedOut);
    }
}
