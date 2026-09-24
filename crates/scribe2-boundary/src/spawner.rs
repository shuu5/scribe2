//! 起動の実物（設計 core-boundary.md §9 採る形 3・ADR-0062）。core の起動の記述を std の Command へ写して撃つ、
//! 差し替え口の実装ただ 1 つ。境界 crate の本番の src で std の Command を構築するのはこの file だけである。
//!
//! bin の main の先頭と e2e の共通の関数が [`install`] で process に 1 回だけ据える。

use std::io;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, ExitStatus, Output};
use vessel::invocation::{Invocation, Spawner};

/// 差し替え口の実物。
pub struct Real;

impl Real {
    /// 記述を std の Command へ写す（stdio は記述から取り出す）。
    fn command_of(invocation: &mut Invocation) -> Command {
        let mut command = Command::new(invocation.get_program());
        command.args(invocation.get_args());
        if let Some(dir) = invocation.get_current_dir() {
            command.current_dir(dir);
        }
        for (key, val) in invocation.get_envs() {
            match val {
                Some(val) => command.env(key, val),
                None => command.env_remove(key),
            };
        }
        let [stdin, stdout, stderr] = invocation.take_stdio();
        if let Some(cfg) = stdin {
            command.stdin(cfg);
        }
        if let Some(cfg) = stdout {
            command.stdout(cfg);
        }
        if let Some(cfg) = stderr {
            command.stderr(cfg);
        }
        if let Some(pgroup) = invocation.get_process_group() {
            command.process_group(pgroup);
        }
        command
    }
}

impl Spawner for Real {
    fn output(&self, invocation: &mut Invocation) -> io::Result<Output> {
        Self::command_of(invocation).output()
    }

    fn status(&self, invocation: &mut Invocation) -> io::Result<ExitStatus> {
        Self::command_of(invocation).status()
    }

    fn spawn(&self, invocation: &mut Invocation) -> io::Result<Child> {
        Self::command_of(invocation).spawn()
    }

    fn exec(&self, invocation: &mut Invocation) -> io::Error {
        Self::command_of(invocation).exec()
    }
}

/// 実物を process に据える（2 回目は据えない＝`false`）。
pub fn install() -> bool {
    vessel::invocation::install(&Real).is_ok()
}
