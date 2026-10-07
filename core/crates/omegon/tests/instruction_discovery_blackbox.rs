#![cfg(all(unix, feature = "product"))]

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
#[cfg(unix)]
use std::os::unix::fs::symlink;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::Value;

const PROCESS_DEADLINE: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(10);

struct FixtureProvider {
    address: String,
    stop: Arc<AtomicBool>,
    requests: Arc<Mutex<Vec<Value>>>,
    thread: Option<thread::JoinHandle<Result<()>>>,
}

impl FixtureProvider {
    fn start() -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let address = format!("http://{}", listener.local_addr()?);
        let stop = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let thread_stop = Arc::clone(&stop);
        let thread_requests = Arc::clone(&requests);
        let thread = thread::spawn(move || {
            while !thread_stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let body = read_request(&mut stream)?;
                        thread_requests.lock().unwrap().push(body);
                        let event = r#"{"choices":[{"index":0,"delta":{"content":"instruction fixture complete"},"finish_reason":"stop"}],"usage":{"prompt_tokens":9,"completion_tokens":3,"total_tokens":12}}"#;
                        write!(
                            stream,
                            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {event}\n\ndata: [DONE]\n\n"
                        )?;
                        stream.flush()?;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(POLL_INTERVAL);
                    }
                    Err(error) => return Err(error.into()),
                }
            }
            Ok(())
        });
        Ok(Self {
            address,
            stop,
            requests,
            thread: Some(thread),
        })
    }

    fn shutdown(mut self) -> Result<Vec<Value>> {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread
                .join()
                .map_err(|_| anyhow::anyhow!("provider fixture panicked"))??;
        }
        Ok(self.requests.lock().unwrap().clone())
    }
}

impl Drop for FixtureProvider {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn read_request(stream: &mut TcpStream) -> Result<Value> {
    // Accepted sockets can inherit the listener's nonblocking flag on macOS.
    // The request reader uses a bounded blocking timeout, not readiness polling.
    stream.set_nonblocking(false)?;
    // Agent setup may still be settling managed services when the connector opens
    // this socket. Leave headroom under the bounded CLI deadline on loaded CI.
    stream.set_read_timeout(Some(Duration::from_secs(15)))?;
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4096];
    let header_end = loop {
        if let Some(index) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
        let read = stream.read(&mut buffer)?;
        if read == 0 {
            bail!("provider request ended before headers completed");
        }
        request.extend_from_slice(&buffer[..read]);
    };
    let headers = std::str::from_utf8(&request[..header_end])?;
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    while request.len() < header_end + content_length {
        let read = stream.read(&mut buffer)?;
        if read == 0 {
            bail!("provider request ended before body completed");
        }
        request.extend_from_slice(&buffer[..read]);
    }
    Ok(serde_json::from_slice(
        &request[header_end..header_end + content_length],
    )?)
}

fn fixture(root: &Path, provider: &FixtureProvider) -> Result<(PathBuf, PathBuf, PathBuf)> {
    let worktree = root.join("linked");
    let cwd = worktree.join("crates/engine");
    let home = root.join("omegon-home");
    fs::create_dir_all(cwd.join(".omegon"))?;
    fs::create_dir_all(&home)?;
    fs::create_dir_all(root.join("main/.git/worktrees/linked"))?;
    fs::write(
        worktree.join(".git"),
        format!(
            "gitdir: {}\n",
            root.join("main/.git/worktrees/linked").display()
        ),
    )?;
    fs::write(root.join("main/AGENTS.md"), "WRONG MAIN CHECKOUT")?;
    fs::write(root.join("AGENTS.md"), "WRONG OUTSIDE POLICY")?;
    fs::write(
        worktree.join("AGENTS.md"),
        format!("ROOT BEGIN\n{}\nROOT END", "界".repeat(2000)),
    )?;
    fs::write(worktree.join("crates/AGENTS.md"), "MIDDLE POLICY")?;
    fs::write(cwd.join("AGENTS.md"), "NEAREST POLICY")?;
    fs::write(
        cwd.join(".omegon/inference.toml"),
        format!(
            r#"schema_version = 1
[[endpoints]]
id = "task-fixture"
adapter = "chat-completions"
secret_refs = ["OMEGON_PROJECT_ENDPOINT_7461736B2D66697874757265_TOKEN"]
enabled = true
[endpoints.transport]
kind = "http"
base_url = "{}/v1"
[[offerings]]
id = "task-fixture:bounded"
endpoint = "task-fixture"
native_model_id = "fixture-native-v1"
input_modalities = ["text"]
output_modalities = ["text"]
enabled = true
[offerings.capabilities]
tools = true
reasoning = true
"#,
            provider.address
        ),
    )?;
    let task = root.join("task.toml");
    fs::write(
        &task,
        r#"[task]
prompt = "Return a final response without tools."
[bounds]
max_turns = 1
timeout_secs = 20
tool_budget = 1
[agent]
model = "task-fixture:bounded"
"#,
    )?;
    Ok((cwd, home, task))
}

fn run_fixture(root: &Path, cwd: &Path, home: &Path, task: &Path) -> Result<Output> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_omegon"));
    command.process_group(0);
    let mut child = command
        .args([
            "--cwd",
            cwd.to_str().unwrap(),
            "--fresh",
            "run",
            task.to_str().unwrap(),
        ])
        .env("HOME", root)
        .env("OMEGON_HOME", home)
        .env("NO_COLOR", "1")
        .env("OMEGON_NERD_FONT", "1")
        .env("RUST_LOG", "error")
        .env(
            "OMEGON_PROJECT_ENDPOINT_7461736B2D66697874757265_TOKEN",
            "fixture-secret",
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let deadline = Instant::now() + PROCESS_DEADLINE;
    loop {
        if child.try_wait()?.is_some() {
            return child
                .wait_with_output()
                .context("collect instruction fixture output");
        }
        if Instant::now() >= deadline {
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.wait();
            bail!("instruction fixture exceeded its process deadline");
        }
        thread::sleep(POLL_INTERVAL);
    }
}

#[test]
fn linked_worktree_complete_instructions_reach_real_cli_provider_request() -> Result<()> {
    let root = tempfile::tempdir()?;
    let provider = FixtureProvider::start()?;
    let (cwd, home, task) = fixture(root.path(), &provider)?;
    let output = run_fixture(root.path(), &cwd, &home, &task)?;
    let requests = provider.shutdown().with_context(|| {
        format!(
            "CLI status {}; stdout={}; stderr={}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })?;
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let request = requests
        .first()
        .context("CLI never dispatched to fixture provider")?;
    let system = request["messages"]
        .as_array()
        .context("messages")?
        .iter()
        .filter(|message| message["role"] == "system")
        .filter_map(|message| message["content"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        system.contains(&"界".repeat(2000)),
        "long UTF-8 policy was truncated"
    );
    assert!(
        system.find("ROOT END").context("root policy")?
            < system
                .find("MIDDLE POLICY")
                .context("intermediate policy")?
    );
    assert!(
        system.find("MIDDLE POLICY").unwrap()
            < system.find("NEAREST POLICY").context("nearest policy")?
    );
    assert!(!system.contains("WRONG MAIN CHECKOUT"));
    assert!(!system.contains("WRONG OUTSIDE POLICY"));
    Ok(())
}

#[test]
fn unreadable_instructions_stop_real_cli_before_provider_dispatch() -> Result<()> {
    let root = tempfile::tempdir()?;
    let provider = FixtureProvider::start()?;
    let (cwd, home, task) = fixture(root.path(), &provider)?;
    fs::remove_file(cwd.join("AGENTS.md"))?;
    symlink("missing-policy", cwd.join("AGENTS.md"))?;
    let output = run_fixture(root.path(), &cwd, &home, &task)?;
    let requests = provider.shutdown()?;
    let diagnostic = format!(
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!output.status.success());
    assert!(
        diagnostic.contains("cannot resolve project instructions"),
        "{diagnostic}"
    );
    assert!(
        requests.is_empty(),
        "unreadable instructions still dispatched to provider"
    );
    Ok(())
}
