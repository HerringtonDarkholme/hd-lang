//! The Windows Node host's private process helper. One request arrives on
//! stdin; its remaining lifetime is the cancellation pipe. Never kill this
//! helper during spawn: the child is suspended until its Job owns it.

use std::io::{self, BufRead as _, Write as _};
use std::process::{ExitCode, Stdio};

use process_wrap::tokio::{ChildWrapper, CommandWrap, JobObject};
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWriteExt as _};
use tokio::process::{ChildStdin, Command};

const MAX_BUFFER: usize = 1 << 30;

fn invalid(field: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("invalid process launcher {field}"),
    )
}

fn command(request: &Value) -> io::Result<Command> {
    let mut command = Command::new(
        request["program"]
            .as_str()
            .ok_or_else(|| invalid("program"))?,
    );
    for arg in request["args"].as_array().ok_or_else(|| invalid("args"))? {
        command.arg(arg.as_str().ok_or_else(|| invalid("argument"))?);
    }
    command.current_dir(request["cwd"].as_str().ok_or_else(|| invalid("cwd"))?);
    command.env_clear();
    for (key, value) in request["env"].as_object().ok_or_else(|| invalid("env"))? {
        command.env(
            key,
            value.as_str().ok_or_else(|| invalid("environment value"))?,
        );
    }
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    Ok(command)
}

async fn feed(stdin: Option<ChildStdin>, input: &[u8]) -> io::Result<()> {
    let Some(mut stdin) = stdin else {
        return Ok(());
    };
    match stdin.write_all(input).await {
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::BrokenPipe
                    | io::ErrorKind::ConnectionReset
                    | io::ErrorKind::NotConnected
            ) =>
        {
            Ok(())
        }
        result => result,
    }
}

async fn drain<R: AsyncRead + Unpin>(stream: Option<R>) -> io::Result<Vec<u8>> {
    let Some(mut stream) = stream else {
        return Ok(Vec::new());
    };
    let mut output = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        let n = stream.read(&mut buffer).await?;
        if n == 0 {
            return Ok(output);
        }
        if output.len().saturating_add(n) > MAX_BUFFER {
            return Err(io::Error::new(
                io::ErrorKind::FileTooLarge,
                "child output exceeds maxBuffer",
            ));
        }
        output.extend_from_slice(&buffer[..n]);
    }
}

async fn complete(child: &mut dyn ChildWrapper, input: &[u8]) -> io::Result<Value> {
    let (stdin, stdout, stderr) = (
        child.stdin().take(),
        child.stdout().take(),
        child.stderr().take(),
    );
    let ((), stdout, stderr) = tokio::try_join!(feed(stdin, input), drain(stdout), drain(stderr))?;
    // Normal completion waits only for the direct child, as the native
    // host does. Detached descendants with closed pipes keep running.
    // Never start JobObject::wait here: its blocking completion-port wait
    // cannot be cancelled and could consume the abort path's notification.
    let status = child.inner_mut().wait().await?;
    Ok(json!({"ok": {
        "stdout": String::from_utf8_lossy(&stdout),
        "stderr": String::from_utf8_lossy(&stderr),
        "status": status.code().unwrap_or(1),
    }}))
}

async fn execute(
    request: &Value,
    mut cancel: tokio::sync::oneshot::Receiver<()>,
) -> io::Result<Option<Value>> {
    if cancel.try_recv().is_ok() {
        return Ok(None);
    }
    let input = request["input"].as_str().ok_or_else(|| invalid("input"))?;
    let mut child = CommandWrap::from(command(request)?)
        .wrap(JobObject)
        .spawn()?;
    let result = tokio::select! {
        biased;
        _ = &mut cancel => None,
        result = complete(child.as_mut(), input.as_bytes()) => Some(result),
    };
    match result {
        Some(Ok(value)) => Ok(Some(value)),
        other => {
            // Assignment happened before any child instruction. A cancelled
            // pipe can now kill the whole Job without a descendant race.
            child.start_kill()?;
            child.wait().await?;
            other.transpose()
        }
    }
}

fn failure(error: &io::Error) -> Value {
    let code = match error.kind() {
        io::ErrorKind::NotFound => "ENOENT",
        io::ErrorKind::PermissionDenied => "EACCES",
        io::ErrorKind::FileTooLarge => "ENOBUFS",
        _ => "EIO",
    };
    json!({"error":{"code":code,"message":error.to_string()}})
}

pub(crate) fn run() -> ExitCode {
    let mut input = io::BufReader::new(io::stdin());
    let mut line = String::new();
    let outcome = input
        .read_line(&mut line)
        .map_err(|e| failure(&e))
        .and_then(|_| {
            serde_json::from_str::<Value>(&line).map_err(|_| failure(&invalid("request")))
        })
        .and_then(|request| {
            let (send, cancel) = tokio::sync::oneshot::channel();
            // A dedicated std thread may block on the lifetime pipe. It is
            // not a Tokio blocking task that would hold runtime shutdown.
            std::thread::spawn(move || {
                let mut control = String::new();
                let _ = input.read_line(&mut control);
                let _ = send.send(());
            });
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .and_then(|rt| rt.block_on(execute(&request, cancel)))
                .map_err(|e| failure(&e))
        });
    let value = match outcome {
        Ok(None) => return ExitCode::from(130),
        Ok(Some(value)) | Err(value) => value,
    };
    let mut output = io::stdout().lock();
    if serde_json::to_writer(&mut output, &value).is_err()
        || output.write_all(b"\n").is_err()
        || output.flush().is_err()
    {
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
