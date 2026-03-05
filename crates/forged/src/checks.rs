use crate::{
    AgentConfig, AgentType, CommandRunner, ForgedConfig, MODEL_PROBE_TIMEOUT, parse_models_output,
};
use std::fs;
use std::path::{Path, PathBuf};

type ModelProbeConfig<'a> = Option<(&'a str, &'a [String], Option<&'a [String]>)>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckItem {
    pub name: String,
    pub ok: bool,
    pub message: String,
}

pub fn run_checks(config: &ForgedConfig) -> Vec<CheckItem> {
    let mut items = Vec::new();
    items.push(check_data_dir(&config.data_dir));
    items.push(check_binary("tmux", &config.tmux_bin));
    for (agent, cfg) in &config.agents {
        items.push(check_agent(agent, cfg));
    }
    items
}

pub fn run_checks_with_runner<R: CommandRunner>(
    config: &ForgedConfig,
    runner: &R,
) -> Vec<CheckItem> {
    let mut items = run_checks(config);
    let claude = config.agents.get(&AgentType::Claude);
    let codex = config.agents.get(&AgentType::Codex);
    items.push(check_model_probe(
        "Claude",
        claude.map(|cfg| {
            (
                cfg.command.as_str(),
                cfg.args.as_slice(),
                cfg.models_probe_args.as_deref(),
            )
        }),
        "claude",
        runner,
    ));
    items.push(check_model_probe(
        "Codex",
        codex.map(|cfg| {
            (
                cfg.command.as_str(),
                cfg.args.as_slice(),
                cfg.models_probe_args.as_deref(),
            )
        }),
        "codex",
        runner,
    ));
    items.push(check_model_probe("Gemini", None, "gemini", runner));
    items.push(check_model_probe("OpenCode", None, "opencode", runner));
    items
}

fn check_data_dir(path: &Path) -> CheckItem {
    let name = "data_dir writable".to_string();
    if let Err(err) = fs::create_dir_all(path) {
        return CheckItem {
            name,
            ok: false,
            message: format!("create failed: {err}"),
        };
    }
    let probe = path.join(".forgemux_check");
    match fs::write(&probe, b"ok") {
        Ok(()) => {
            let _ = fs::remove_file(&probe);
            CheckItem {
                name,
                ok: true,
                message: path.display().to_string(),
            }
        }
        Err(err) => CheckItem {
            name,
            ok: false,
            message: format!("write failed: {err}"),
        },
    }
}

fn check_binary(label: &str, program: &str) -> CheckItem {
    let name = format!("{label} binary");
    match resolve_program(program) {
        Some(path) => CheckItem {
            name,
            ok: true,
            message: path.display().to_string(),
        },
        None => CheckItem {
            name,
            ok: false,
            message: format!("not found: {program}"),
        },
    }
}

fn check_agent(agent: &crate::AgentType, cfg: &AgentConfig) -> CheckItem {
    let name = format!("{agent:?} agent binary");
    match resolve_program(&cfg.command) {
        Some(path) => CheckItem {
            name,
            ok: true,
            message: path.display().to_string(),
        },
        None => CheckItem {
            name,
            ok: false,
            message: format!("not found: {}", cfg.command),
        },
    }
}

fn check_model_probe<R: CommandRunner>(
    label: &str,
    config: ModelProbeConfig<'_>,
    fallback_command: &str,
    runner: &R,
) -> CheckItem {
    let name = format!("{label} models probe");
    let (command, base_args, probe_args) = config.unwrap_or((fallback_command, &[], None));
    match resolve_program(command) {
        Some(path) => match probe_args {
            Some(probe_args) if !probe_args.is_empty() => {
                eprintln!(
                    "checking {label} models with `{}`",
                    std::iter::once(command)
                        .chain(base_args.iter().map(|value| value.as_str()))
                        .chain(probe_args.iter().map(|value| value.as_str()))
                        .collect::<Vec<_>>()
                        .join(" ")
                );
                let mut models = Vec::new();
                let mut timed_out = false;
                let output = match runner.run_with_timeout(
                    command,
                    &base_args
                        .iter()
                        .cloned()
                        .chain(probe_args.iter().cloned())
                        .collect::<Vec<_>>(),
                    MODEL_PROBE_TIMEOUT,
                ) {
                    Ok(output) => output,
                    Err(err) => {
                        if err.kind() == std::io::ErrorKind::TimedOut {
                            timed_out = true;
                        }
                        return CheckItem {
                            name,
                            ok: false,
                            message: if timed_out {
                                "probe timed out (try logging in to the CLI first)".to_string()
                            } else {
                                "probe failed".to_string()
                            },
                        };
                    }
                };
                if output.status.success() {
                    let text = String::from_utf8_lossy(&output.stdout);
                    models = parse_models_output(&text);
                }
                if models.is_empty() {
                    CheckItem {
                        name,
                        ok: false,
                        message: "probe returned no models".to_string(),
                    }
                } else {
                    let preview = if models.len() > 6 {
                        format!("{} (+{} more)", models[..6].join(", "), models.len() - 6)
                    } else {
                        models.join(", ")
                    };
                    CheckItem {
                        name,
                        ok: true,
                        message: format!("{} ({})", path.display(), preview),
                    }
                }
            }
            _ => CheckItem {
                name,
                ok: true,
                message: format!("{} (probe not configured)", path.display()),
            },
        },
        None => CheckItem {
            name,
            ok: true,
            message: format!("not installed: {command}"),
        },
    }
}

fn resolve_program(program: &str) -> Option<PathBuf> {
    let path = Path::new(program);
    if program.contains('/') {
        return if path.exists() {
            Some(path.to_path_buf())
        } else {
            None
        };
    }
    let paths = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&paths) {
        let candidate = dir.join(program);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AgentType, ForgedConfig};

    #[test]
    fn run_checks_reports_missing_binaries() {
        let tmp = tempfile::tempdir().unwrap();
        let mut config = ForgedConfig::default_with_data_dir(tmp.path().to_path_buf());
        config.tmux_bin = "/missing-tmux".to_string();
        let claude = config.agents.get_mut(&AgentType::Claude).unwrap();
        claude.command = "/missing-claude".to_string();

        let results = run_checks(&config);
        let tmux = results
            .iter()
            .find(|item| item.name == "tmux binary")
            .unwrap();
        assert!(!tmux.ok);
        let claude_item = results
            .iter()
            .find(|item| item.name == "Claude agent binary")
            .unwrap();
        assert!(!claude_item.ok);
    }

    #[test]
    fn run_checks_accepts_absolute_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let mut config = ForgedConfig::default_with_data_dir(tmp.path().to_path_buf());
        let exe = std::env::current_exe().unwrap();
        config.tmux_bin = exe.to_string_lossy().to_string();
        let claude = config.agents.get_mut(&AgentType::Claude).unwrap();
        claude.command = exe.to_string_lossy().to_string();

        let results = run_checks(&config);
        let tmux = results
            .iter()
            .find(|item| item.name == "tmux binary")
            .unwrap();
        assert!(tmux.ok);
        let claude_item = results
            .iter()
            .find(|item| item.name == "Claude agent binary")
            .unwrap();
        assert!(claude_item.ok);
    }
}
