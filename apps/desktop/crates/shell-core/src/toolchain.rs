//! The toolchain doctor: which `docker`, Compose, daemon and Neon CLI the shell would use.
//!
//! An app launched from the Finder (or the Dock) inherits launchd's `PATH`
//! (`/usr/bin:/bin:/usr/sbin:/sbin`), which holds none of Homebrew, OrbStack, Docker Desktop
//! or npm's global bin. [`login_shell_path`] reads the user's shell `PATH` once at startup and
//! [`merge_path`] puts it ahead of the inherited one, so `docker` resolves the same way it does
//! in a terminal.
//!
//! Every probe runs through [`System`], so the report logic is tested against canned
//! `docker` output (`tests` below) rather than whatever is installed on the machine.

use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// How long one probe may take. `docker info` against a wedged daemon can hang.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// What the CLI prints when it can't reach a daemon (the same wordings
/// `olai-stack-runtime`'s `DockerCli` matches).
const UNREACHABLE: &[&str] = &[
    "Cannot connect to the Docker daemon",
    "failed to connect to the docker API",
    "error during connect",
    "Is the docker daemon running",
];

/// Bracket the `PATH` a login shell prints, so profile noise (banners, `echo`s) around it
/// doesn't end up in it.
const PATH_MARK: &str = "__LUMINESCENT_PATH__";

/// The output of a finished command.
#[derive(Clone, Debug, Default)]
pub struct CommandOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// The host the doctor probes: program lookup and process execution.
pub trait System {
    /// `name` on the (merged) `PATH`.
    fn which(&self, name: &str) -> Option<PathBuf>;
    /// The Neon CLI, wherever `olai-stack-runtime` would find it.
    fn locate_neon(&self) -> Option<PathBuf>;
    /// Run `program args…` with no stdin, up to [`PROBE_TIMEOUT`].
    fn run(&self, program: &Path, args: &[&str]) -> io::Result<CommandOutput>;
}

/// The real machine.
#[derive(Clone, Copy, Debug, Default)]
pub struct HostSystem;

impl System for HostSystem {
    fn which(&self, name: &str) -> Option<PathBuf> {
        find_on_path(name, std::env::var_os("PATH").as_deref())
    }

    fn locate_neon(&self) -> Option<PathBuf> {
        olai_stack_runtime::neon::locate_neon(None)
    }

    fn run(&self, program: &Path, args: &[&str]) -> io::Result<CommandOutput> {
        run_with_timeout(Command::new(program).args(args), PROBE_TIMEOUT)
    }
}

/// `name` in the first directory of `path` that holds it as a file.
pub fn find_on_path(name: &str, path: Option<&OsStr>) -> Option<PathBuf> {
    std::env::split_paths(path?)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// Run `command` with stdin closed and output captured; kill it after `timeout`.
pub fn run_with_timeout(command: &mut Command, timeout: Duration) -> io::Result<CommandOutput> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    // Drain the pipes on their own threads, so a chatty child can't block on a full pipe
    // while we wait for it.
    let stdout = child.stdout.take().map(drain);
    let stderr = child.stderr.take().map(drain);
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("no answer within {}s", timeout.as_secs()),
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let join = |h: Option<std::thread::JoinHandle<String>>| {
        h.and_then(|h| h.join().ok()).unwrap_or_default()
    };
    Ok(CommandOutput {
        success: status.success(),
        stdout: join(stdout),
        stderr: join(stderr),
    })
}

fn drain(mut pipe: impl io::Read + Send + 'static) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = pipe.read_to_end(&mut buf);
        String::from_utf8_lossy(&buf).into_owned()
    })
}

/// The `PATH` of the user's interactive login shell (`$SHELL -ilc`), or `None` if there is no
/// `$SHELL` or it doesn't answer within [`PROBE_TIMEOUT`]. Interactive as well as login,
/// because zsh users commonly set `PATH` in `.zshrc`.
pub fn login_shell_path() -> Option<String> {
    let shell = std::env::var_os("SHELL")?;
    let script = format!("printf '{PATH_MARK}%s{PATH_MARK}' \"$PATH\"");
    let out = run_with_timeout(Command::new(shell).args(["-ilc", &script]), PROBE_TIMEOUT).ok()?;
    extract_marked_path(&out.stdout)
}

fn extract_marked_path(stdout: &str) -> Option<String> {
    let start = stdout.find(PATH_MARK)? + PATH_MARK.len();
    let len = stdout[start..].find(PATH_MARK)?;
    let path = &stdout[start..start + len];
    (!path.is_empty()).then(|| path.to_owned())
}

/// `preferred`'s entries, then those of `current` it doesn't already have.
pub fn merge_path(preferred: &str, current: &str) -> String {
    let mut seen = Vec::new();
    for entry in preferred.split(':').chain(current.split(':')) {
        if !entry.is_empty() && !seen.contains(&entry) {
            seen.push(entry);
        }
    }
    seen.join(":")
}

/// A CLI the shell needs, and whether it was found.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Tool {
    /// Not on the `PATH` (nor, for Neon, in its usual places).
    Missing,
    /// Found and it answered `--version`.
    Found { path: String, version: String },
    /// Found, but asking for its version failed.
    Broken { path: String, message: String },
}

/// Which product runs the Docker daemon.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum DaemonFlavour {
    DockerDesktop,
    OrbStack,
    Colima,
    RancherDesktop,
    Podman,
    /// A plain Docker Engine (Linux), or something not recognized.
    Engine,
}

/// Whether the daemon answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Daemon {
    /// No `docker` CLI to ask.
    Unknown,
    /// The CLI can't reach it.
    Down { message: String },
    /// It answers.
    Up {
        version: String,
        flavour: DaemonFlavour,
        /// The `docker context` in use.
        context: Option<String>,
        /// The daemon's `OperatingSystem` (e.g. `Docker Desktop`, `OrbStack`, `Ubuntu 24.04`).
        os: String,
    },
}

/// Everything the doctor screen shows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct DoctorReport {
    pub docker: Tool,
    pub compose: Tool,
    pub daemon: Daemon,
    pub neon: Tool,
    /// The `PATH` the probes ran with, one entry per item.
    pub path: Vec<String>,
}

impl DoctorReport {
    /// Whether everything a local environment needs is there (Neon is optional).
    pub fn ready(&self) -> bool {
        matches!(self.docker, Tool::Found { .. })
            && matches!(self.compose, Tool::Found { .. })
            && matches!(self.daemon, Daemon::Up { .. })
    }
}

/// Probe the toolchain.
pub fn doctor(system: &impl System) -> DoctorReport {
    let docker_path = system.which("docker");
    let docker = probe_version(
        system,
        docker_path.as_deref(),
        &["--version"],
        docker_version,
    );
    let compose = probe_version(
        system,
        docker_path.as_deref(),
        &["compose", "version", "--short"],
        |out| Some(out.trim().trim_start_matches('v').to_owned()),
    );
    let daemon = match &docker_path {
        Some(path) => probe_daemon(system, path),
        None => Daemon::Unknown,
    };
    let neon_path = system.locate_neon();
    let neon = probe_version(system, neon_path.as_deref(), &["--version"], |out| {
        out.lines().next().map(|l| l.trim().to_owned())
    });
    let path = std::env::var("PATH")
        .unwrap_or_default()
        .split(':')
        .filter(|e| !e.is_empty())
        .map(str::to_owned)
        .collect();
    DoctorReport {
        docker,
        compose,
        daemon,
        neon,
        path,
    }
}

fn probe_version(
    system: &impl System,
    program: Option<&Path>,
    args: &[&str],
    parse: impl Fn(&str) -> Option<String>,
) -> Tool {
    let Some(program) = program else {
        return Tool::Missing;
    };
    let path = program.display().to_string();
    match system.run(program, args) {
        Ok(out) if out.success => match parse(&out.stdout) {
            Some(version) if !version.is_empty() => Tool::Found { path, version },
            _ => Tool::Broken {
                path,
                message: format!("unexpected output: {}", out.stdout.trim()),
            },
        },
        Ok(out) => Tool::Broken {
            path,
            message: first_line(&out.stderr, &out.stdout),
        },
        Err(e) => Tool::Broken {
            path,
            message: e.to_string(),
        },
    }
}

/// `Docker version 29.5.2, build 1234567` → `29.5.2`.
fn docker_version(out: &str) -> Option<String> {
    let rest = out.trim().strip_prefix("Docker version ")?;
    Some(rest.split(',').next()?.trim().to_owned())
}

/// The fields of `docker info --format '{{json .}}'` the doctor reads.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
struct DockerInfo {
    server_version: String,
    operating_system: String,
    name: String,
    server_errors: Vec<String>,
    client_info: ClientInfo,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
struct ClientInfo {
    context: Option<String>,
}

fn probe_daemon(system: &impl System, docker: &Path) -> Daemon {
    let out = match system.run(docker, &["info", "--format", "{{json .}}"]) {
        Ok(out) => out,
        Err(e) => {
            return Daemon::Down {
                message: e.to_string(),
            };
        }
    };
    // With the daemon down, `docker info` still prints the client half (with
    // `ServerErrors`) and exits non-zero.
    let info: Option<DockerInfo> = serde_json::from_str(out.stdout.trim()).ok();
    match info {
        Some(info) if info.server_errors.is_empty() && !info.server_version.is_empty() => {
            Daemon::Up {
                flavour: flavour(&info),
                version: info.server_version,
                context: info.client_info.context,
                os: info.operating_system,
            }
        }
        Some(info) if !info.server_errors.is_empty() => Daemon::Down {
            message: info.server_errors.join("; "),
        },
        _ => Daemon::Down {
            message: unreachable_message(&out.stderr)
                .unwrap_or_else(|| first_line(&out.stderr, &out.stdout)),
        },
    }
}

fn flavour(info: &DockerInfo) -> DaemonFlavour {
    let os = info.operating_system.to_ascii_lowercase();
    let name = info.name.to_ascii_lowercase();
    let context = info
        .client_info
        .context
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let any =
        |needle: &str| os.contains(needle) || name.contains(needle) || context.contains(needle);
    if any("orbstack") {
        DaemonFlavour::OrbStack
    } else if any("docker desktop") || name == "docker-desktop" || context == "desktop-linux" {
        DaemonFlavour::DockerDesktop
    } else if any("rancher") {
        DaemonFlavour::RancherDesktop
    } else if any("colima") {
        DaemonFlavour::Colima
    } else if any("podman") {
        DaemonFlavour::Podman
    } else {
        DaemonFlavour::Engine
    }
}

fn unreachable_message(stderr: &str) -> Option<String> {
    stderr
        .lines()
        .find(|line| UNREACHABLE.iter().any(|m| line.contains(m)))
        .map(|line| line.trim().to_owned())
}

fn first_line(preferred: &str, fallback: &str) -> String {
    [preferred, fallback]
        .into_iter()
        .flat_map(str::lines)
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("no output")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    /// A machine with canned answers, keyed by the joined argument list.
    #[derive(Default)]
    struct Canned {
        docker: Option<PathBuf>,
        neon: Option<PathBuf>,
        answers: HashMap<String, io::Result<CommandOutput>>,
    }

    impl Canned {
        fn with_docker() -> Self {
            Canned {
                docker: Some(PathBuf::from("/opt/homebrew/bin/docker")),
                ..Default::default()
            }
            .answer("--version", ok("Docker version 29.5.2, build 79f2e0f\n"))
            .answer("compose version --short", ok("v5.6.1\n"))
        }

        fn answer(mut self, args: &str, out: io::Result<CommandOutput>) -> Self {
            self.answers.insert(args.to_owned(), out);
            self
        }

        fn info(self, json: &str) -> Self {
            self.answer("info --format {{json .}}", ok(json))
        }
    }

    impl System for Canned {
        fn which(&self, name: &str) -> Option<PathBuf> {
            (name == "docker").then(|| self.docker.clone()).flatten()
        }
        fn locate_neon(&self) -> Option<PathBuf> {
            self.neon.clone()
        }
        fn run(&self, _program: &Path, args: &[&str]) -> io::Result<CommandOutput> {
            match self.answers.get(&args.join(" ")) {
                Some(Ok(out)) => Ok(out.clone()),
                Some(Err(e)) => Err(io::Error::new(e.kind(), e.to_string())),
                None => panic!("unexpected command: {args:?}"),
            }
        }
    }

    fn ok(stdout: &str) -> io::Result<CommandOutput> {
        Ok(CommandOutput {
            success: true,
            stdout: stdout.to_owned(),
            stderr: String::new(),
        })
    }

    fn failed(stdout: &str, stderr: &str) -> io::Result<CommandOutput> {
        Ok(CommandOutput {
            success: false,
            stdout: stdout.to_owned(),
            stderr: stderr.to_owned(),
        })
    }

    fn up(system: &Canned) -> (DaemonFlavour, Option<String>) {
        match doctor(system).daemon {
            Daemon::Up {
                flavour, context, ..
            } => (flavour, context),
            other => panic!("daemon not up: {other:?}"),
        }
    }

    #[test]
    fn docker_desktop() {
        let system = Canned::with_docker().info(
            r#"{"ServerVersion":"29.0.1","OperatingSystem":"Docker Desktop","Name":"docker-desktop","ClientInfo":{"Context":"desktop-linux"}}"#,
        );
        let report = doctor(&system);
        assert_eq!(
            report.docker,
            Tool::Found {
                path: "/opt/homebrew/bin/docker".into(),
                version: "29.5.2".into()
            }
        );
        assert_eq!(
            report.compose,
            Tool::Found {
                path: "/opt/homebrew/bin/docker".into(),
                version: "5.6.1".into()
            }
        );
        assert_eq!(
            up(&system),
            (DaemonFlavour::DockerDesktop, Some("desktop-linux".into()))
        );
        assert!(report.ready());
        assert_eq!(report.neon, Tool::Missing);
    }

    #[test]
    fn orbstack() {
        let system = Canned::with_docker().info(
            r#"{"ServerVersion":"28.5.2","OperatingSystem":"OrbStack","Name":"orbstack","ClientInfo":{"Context":"orbstack"}}"#,
        );
        assert_eq!(up(&system).0, DaemonFlavour::OrbStack);
    }

    #[test]
    fn colima() {
        let system = Canned::with_docker().info(
            r#"{"ServerVersion":"29.5.2","OperatingSystem":"Ubuntu 24.04.3 LTS","Name":"colima","ClientInfo":{"Context":"colima"}}"#,
        );
        assert_eq!(up(&system).0, DaemonFlavour::Colima);
    }

    #[test]
    fn plain_engine() {
        let system = Canned::with_docker().info(
            r#"{"ServerVersion":"29.5.2","OperatingSystem":"Ubuntu 24.04.3 LTS","Name":"runner","ClientInfo":{"Context":"default"}}"#,
        );
        assert_eq!(up(&system), (DaemonFlavour::Engine, Some("default".into())));
    }

    #[test]
    fn daemon_down_reports_server_errors() {
        let system = Canned::with_docker().answer(
            "info --format {{json .}}",
            failed(
                r#"{"ServerErrors":["Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?"],"ClientInfo":{"Context":"default"}}"#,
                "",
            ),
        );
        let report = doctor(&system);
        let Daemon::Down { message } = &report.daemon else {
            panic!("expected down: {:?}", report.daemon);
        };
        assert!(message.contains("Cannot connect"), "{message}");
        assert!(!report.ready());
    }

    #[test]
    fn daemon_down_without_json() {
        let system = Canned::with_docker().answer(
            "info --format {{json .}}",
            failed(
                "",
                "some preamble\nerror during connect: Get \"http://%2F%2F.%2Fpipe/docker_engine\": open: no such file\n",
            ),
        );
        let Daemon::Down { message } = doctor(&system).daemon else {
            panic!("expected down");
        };
        assert!(message.starts_with("error during connect"), "{message}");
    }

    #[test]
    fn no_docker() {
        let report = doctor(&Canned::default());
        assert_eq!(report.docker, Tool::Missing);
        assert_eq!(report.compose, Tool::Missing);
        assert_eq!(report.daemon, Daemon::Unknown);
        assert!(!report.ready());
    }

    #[test]
    fn compose_plugin_missing() {
        let system = Canned::with_docker()
            .answer(
                "compose version --short",
                failed("", "docker: 'compose' is not a docker command.\n"),
            )
            .info(r#"{"ServerVersion":"29.5.2","OperatingSystem":"Ubuntu","Name":"x"}"#);
        let report = doctor(&system);
        assert_eq!(
            report.compose,
            Tool::Broken {
                path: "/opt/homebrew/bin/docker".into(),
                message: "docker: 'compose' is not a docker command.".into()
            }
        );
        assert!(!report.ready());
    }

    #[test]
    fn neon_found() {
        let mut system = Canned::default().answer("--version", ok("8.16.0\n"));
        system.neon = Some(PathBuf::from("/opt/homebrew/bin/neon"));
        assert_eq!(
            doctor(&system).neon,
            Tool::Found {
                path: "/opt/homebrew/bin/neon".into(),
                version: "8.16.0".into()
            }
        );
    }

    #[test]
    fn probe_timeout_is_broken_not_panic() {
        let system = Canned::with_docker().answer(
            "info --format {{json .}}",
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "no answer within 10s",
            )),
        );
        assert_eq!(
            doctor(&system).daemon,
            Daemon::Down {
                message: "no answer within 10s".into()
            }
        );
    }

    #[test]
    fn marked_path_ignores_profile_noise() {
        let out = format!("Welcome!\n{PATH_MARK}/opt/homebrew/bin:/usr/bin{PATH_MARK}\nbye\n");
        assert_eq!(
            extract_marked_path(&out).as_deref(),
            Some("/opt/homebrew/bin:/usr/bin")
        );
        assert_eq!(extract_marked_path("no marks"), None);
        assert_eq!(
            extract_marked_path(&format!("{PATH_MARK}{PATH_MARK}")),
            None
        );
    }

    #[test]
    fn merge_keeps_order_and_drops_duplicates() {
        assert_eq!(
            merge_path("/opt/homebrew/bin:/usr/bin", "/usr/bin:/bin::/usr/sbin"),
            "/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin"
        );
    }

    #[test]
    fn run_with_timeout_kills_slow_children() {
        let err = run_with_timeout(
            Command::new("sh").args(["-c", "sleep 5"]),
            Duration::from_millis(100),
        )
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        let out = run_with_timeout(
            Command::new("sh").args(["-c", "echo out; echo err >&2; exit 3"]),
            Duration::from_secs(5),
        )
        .unwrap();
        assert!(!out.success);
        assert_eq!(
            (out.stdout.as_str(), out.stderr.as_str()),
            ("out\n", "err\n")
        );
    }
}
