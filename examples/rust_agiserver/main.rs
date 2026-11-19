use std::env;
use std::error::Error;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::net::TcpListener;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{self, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;

use asterisk_agi::Agi;
use serde::Deserialize;
use signal_hook::consts::signal::{SIGHUP, SIGINT, SIGTERM};
use signal_hook::flag;

mod play_worknumber;

#[derive(Clone, Deserialize)]
struct ServerConfig {
    play_worknumber: play_worknumber::Config,
}

fn main() {
    let command = env::args().nth(1);
    let result = match command.as_deref() {
        Some("-v") | Some("--version") => {
            println!(
                "rust_agiserver build_date={} git_hash={}",
                env!("RUST_AGISERVER_BUILD_DATE"),
                env!("RUST_AGISERVER_GIT_HASH")
            );
            return;
        }
        None | Some("run") | Some("foreground") | Some("--foreground") => run_server(),
        Some("daemon") | Some("start") => start_daemon(),
        Some("reload") => send_control_signal(SIGHUP, "reload"),
        Some("stop") => send_control_signal(SIGTERM, "stop"),
        Some("status") => show_status(),
        Some(argument) => {
            eprintln!(
                "unknown command: {argument}\nUsage: rust_agiserver [run|daemon|reload|stop|status|-v]"
            );
            process::exit(2);
        }
    };

    if let Err(error) = result {
        eprintln!("rust_agiserver: {error}");
        process::exit(1);
    }
}

fn run_server() -> Result<(), Box<dyn Error>> {
    let config_path = env::var("RUST_AGISERVER_CONFIG")
        .unwrap_or_else(|_| "examples/rust_agiserver/config.yaml".to_owned());
    let config = load_config(&config_path)?;

    let address = env::var("FASTAGI_ADDR").unwrap_or_else(|_| "0.0.0.0:4573".to_owned());
    let listener = TcpListener::bind(&address)?;
    listener.set_nonblocking(true)?;
    let _pid_file = PidFileGuard::create()?;

    let reload_requested = Arc::new(AtomicBool::new(false));
    let shutdown_requested = Arc::new(AtomicBool::new(false));
    let _reload_signal = flag::register(SIGHUP, Arc::clone(&reload_requested))?;
    let _terminate_signal = flag::register(SIGTERM, Arc::clone(&shutdown_requested))?;
    let _interrupt_signal = flag::register(SIGINT, Arc::clone(&shutdown_requested))?;

    let config = Arc::new(RwLock::new(config));
    println!("FastAGI server listening on {address}");
    println!("Loaded configuration from {config_path}");
    println!("PID: {}", process::id());

    while !shutdown_requested.load(Ordering::Relaxed) {
        if reload_requested.swap(false, Ordering::Relaxed) {
            match load_config(&config_path) {
                Ok(new_config) => {
                    *config.write().unwrap() = new_config;
                    println!("Reloaded configuration from {config_path}");
                }
                Err(error) => {
                    eprintln!("Configuration reload failed; keeping current config: {error}");
                }
            }
        }

        match listener.accept() {
            Ok((stream, _)) => {
                let config = Arc::clone(&config);
                thread::spawn(move || match Agi::conn(stream) {
                    Ok(agi) => {
                        let config = config.read().unwrap().clone();
                        handle_connection(agi, &config);
                    }
                    Err(error) => eprintln!("Failed to initialize AGI session: {error}"),
                });
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(100));
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error.into()),
        }
    }

    println!("FastAGI server stopping");
    Ok(())
}

fn load_config(path: &str) -> Result<ServerConfig, Box<dyn Error>> {
    let yaml = fs::read_to_string(path)?;
    Ok(serde_yaml::from_str(&yaml)?)
}

fn handle_connection(agi: Agi, config: &ServerConfig) {
    let request_uri = agi
        .variables
        .lock()
        .unwrap()
        .get("agi_request")
        .cloned()
        .unwrap_or_default();

    match request_path(&request_uri) {
        Some("play_worknumber") => {
            if let Err(error) = play_worknumber::handle(&agi, &config.play_worknumber) {
                eprintln!("play_worknumber failed: {error}");
            }
        }
        Some(path) => report_unimplemented(&agi, path),
        None => report_unimplemented(&agi, "<missing agi_request path>"),
    }

    if let Err(error) = agi.close() {
        eprintln!("Failed to close FastAGI connection: {error}");
    }
}

fn request_path(request_uri: &str) -> Option<&str> {
    let uri = request_uri.split('?').next()?.trim_end_matches('/');
    let path = uri.rsplit('/').next()?;
    if path.is_empty() {
        None
    } else {
        Some(path)
    }
}

fn report_unimplemented(agi: &Agi, path: &str) {
    let message = format!("FastAGI feature is not implemented for request path: {path}");
    eprintln!("{message}");
    if let Err(error) = agi.verbose(&message, 1) {
        eprintln!("Failed to report unsupported request to Asterisk: {error}");
    }
}

fn start_daemon() -> Result<(), Box<dyn Error>> {
    if let Ok(pid) = read_pid_file() {
        if process_is_running(pid) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("server is already running with PID {pid}"),
            )
            .into());
        }
    }

    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file_path())?;
    let stdout = log_file.try_clone()?;
    let mut command = Command::new(env::current_exe()?);
    command
        .arg("--foreground")
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(log_file));

    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }

    let mut child = command.spawn()?;
    let expected_pid = child.id() as libc::pid_t;
    for _ in 0..50 {
        if read_pid_file().ok() == Some(expected_pid) {
            println!("rust_agiserver started in background (PID {expected_pid})");
            println!("Log file: {}", log_file_path().display());
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("server exited during startup with status {status}"),
            )
            .into());
        }
        thread::sleep(Duration::from_millis(100));
    }

    let _ = child.kill();
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        "timed out waiting for daemon startup",
    )
    .into())
}

fn send_control_signal(signal: libc::c_int, action: &str) -> Result<(), Box<dyn Error>> {
    let pid = read_pid_file()?;
    if unsafe { libc::kill(pid, signal) } != 0 {
        return Err(io::Error::last_os_error().into());
    }
    println!("sent {action} signal to rust_agiserver (PID {pid})");
    Ok(())
}

fn show_status() -> Result<(), Box<dyn Error>> {
    match read_pid_file() {
        Ok(pid) if process_is_running(pid) => println!("rust_agiserver is running (PID {pid})"),
        Ok(pid) => println!("rust_agiserver is not running (stale PID file for {pid})"),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            println!("rust_agiserver is not running")
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn process_is_running(pid: libc::pid_t) -> bool {
    if pid <= 1 {
        return false;
    }

    if unsafe { libc::kill(pid, 0) } == 0 {
        return true;
    }

    io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

fn read_pid_file() -> io::Result<libc::pid_t> {
    let value = fs::read_to_string(pid_file_path())?;
    let pid = value.trim().parse::<libc::pid_t>().map_err(|error| {
        io::Error::new(io::ErrorKind::InvalidData, format!("invalid PID file: {error}"))
    })?;
    if pid <= 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "PID file contains an invalid process ID",
        ));
    }
    Ok(pid)
}

fn pid_file_path() -> PathBuf {
    env::var_os("RUST_AGISERVER_PID_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/rust_agiserver.pid"))
}

fn log_file_path() -> PathBuf {
    env::var_os("RUST_AGISERVER_LOG_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/rust_agiserver.log"))
}

struct PidFileGuard {
    path: PathBuf,
}

impl PidFileGuard {
    fn create() -> io::Result<Self> {
        let path = pid_file_path();
        match fs::read_to_string(&path) {
            Ok(contents) => {
                if let Ok(pid) = contents.trim().parse::<libc::pid_t>() {
                    if process_is_running(pid) {
                        return Err(io::Error::new(
                            io::ErrorKind::AlreadyExists,
                            format!("server is already running with PID {pid}"),
                        ));
                    }
                }
                fs::remove_file(&path)?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }

        let mut file = OpenOptions::new().write(true).create_new(true).open(&path)?;
        writeln!(file, "{}", process::id())?;
        Ok(Self { path })
    }
}

impl Drop for PidFileGuard {
    fn drop(&mut self) {
        if fs::read_to_string(&self.path)
            .map(|pid| pid.trim() == process::id().to_string())
            .unwrap_or(false)
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}
