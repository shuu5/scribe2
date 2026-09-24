//! 起動の記述と差し替え口（設計 core-boundary.md §9 採る形 1・2・7・10・ADR-0062）。
//!
//! core は子 process を直に起こさない。何を起こすか（program・引数・cwd・env・stdio・process group）を
//! [`Invocation`] に記述し、4 終端（`output` / `status` / `spawn` / `exec`）は記述を解釈せず差し替え口
//! [`Spawner`] へ渡すだけである。実物（記述を std の Command へ写して撃つ）は境界 crate の 1 file が持ち、
//! bin の main の先頭で [`install`] により process に 1 回だけ据える。
//!
//! 据えていない周の終端は io の `Unsupported` を返す＝各 site の既存の「撃てない」分岐に落ちる（fail-closed・
//! 新しい分岐を足さない）。cfg(test) の build では据えずに撃てる（既定は `pipe` の歯の区間の fixture の実物・
//! 歯が同じ thread に据えた記録する stub が在ればそれ）。この file は歯の区間を持たない（採る形 10）。

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, ExitStatus, Output, Stdio};
use std::sync::OnceLock;

/// 起動の記述（std の Command と同じ builder の面と 4 終端を、同じ名・同じ受け手の形で持つ）。
///
/// env は std と同じく key ごとに最後の指定が勝ち（`None` = `env_remove`）、`get_envs` は key の順に返す。
/// stdio は終端が差し替え口へ渡した先で取り出される（std の `Stdio` は複製できないため・同じ記述を 2 度
/// 撃つ周の 2 度目は std の既定の stdio になる）。
#[derive(Debug)]
pub struct Invocation {
    /// 起こす program。
    program: OsString,
    /// 引数（program を含まない）。
    args: Vec<OsString>,
    /// 子の cwd（`None` = 親の cwd）。
    current_dir: Option<PathBuf>,
    /// env の差分（`None` = 外す）。
    envs: BTreeMap<OsString, Option<OsString>>,
    /// stdin / stdout / stderr の指定（`None` = 終端の既定）。
    stdio: [Option<Stdio>; 3],
    /// process group（`None` = 親と同じ）。
    process_group: Option<i32>,
}

impl Invocation {
    /// program を起こす記述（引数・env・stdio は空）。
    pub fn new<S: AsRef<OsStr>>(program: S) -> Self {
        Self {
            program: program.as_ref().to_owned(),
            args: Vec::new(),
            current_dir: None,
            envs: BTreeMap::new(),
            stdio: [None, None, None],
            process_group: None,
        }
    }

    /// 引数を 1 つ足す。
    pub fn arg<S: AsRef<OsStr>>(&mut self, arg: S) -> &mut Self {
        self.args.push(arg.as_ref().to_owned());
        self
    }

    /// 引数を順に足す。
    pub fn args<I, S>(&mut self, args: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        for arg in args {
            self.arg(arg);
        }
        self
    }

    /// 子の cwd。
    pub fn current_dir<P: AsRef<Path>>(&mut self, dir: P) -> &mut Self {
        self.current_dir = Some(dir.as_ref().to_owned());
        self
    }

    /// env を 1 つ置く。
    pub fn env<K, V>(&mut self, key: K, val: V) -> &mut Self
    where
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        self.envs.insert(key.as_ref().to_owned(), Some(val.as_ref().to_owned()));
        self
    }

    /// env を 1 つ外す。
    pub fn env_remove<K: AsRef<OsStr>>(&mut self, key: K) -> &mut Self {
        self.envs.insert(key.as_ref().to_owned(), None);
        self
    }

    /// 子の stdin。
    pub fn stdin<T: Into<Stdio>>(&mut self, cfg: T) -> &mut Self {
        self.stdio[0] = Some(cfg.into());
        self
    }

    /// 子の stdout。
    pub fn stdout<T: Into<Stdio>>(&mut self, cfg: T) -> &mut Self {
        self.stdio[1] = Some(cfg.into());
        self
    }

    /// 子の stderr。
    pub fn stderr<T: Into<Stdio>>(&mut self, cfg: T) -> &mut Self {
        self.stdio[2] = Some(cfg.into());
        self
    }

    /// 子の process group（std の `CommandExt::process_group` と同じ値の意味）。
    pub fn process_group(&mut self, pgroup: i32) -> &mut Self {
        self.process_group = Some(pgroup);
        self
    }

    /// 起こす program。
    pub fn get_program(&self) -> &OsStr {
        &self.program
    }

    /// 引数の列（program を含まない）。
    pub fn get_args(&self) -> impl ExactSizeIterator<Item = &OsStr> {
        self.args.iter().map(OsString::as_os_str)
    }

    /// env の差分の列（key の順・`None` = 外す）。
    pub fn get_envs(&self) -> impl ExactSizeIterator<Item = (&OsStr, Option<&OsStr>)> {
        self.envs.iter().map(|(key, val)| (key.as_os_str(), val.as_deref()))
    }

    /// 子の cwd（置いていなければ `None`）。
    pub fn get_current_dir(&self) -> Option<&Path> {
        self.current_dir.as_deref()
    }

    /// 子の process group（置いていなければ `None`）。
    pub fn get_process_group(&self) -> Option<i32> {
        self.process_group
    }

    /// stdin / stdout / stderr の指定を取り出す（実物が std の Command へ写す口・取り出した後は `None`）。
    pub fn take_stdio(&mut self) -> [Option<Stdio>; 3] {
        std::mem::take(&mut self.stdio)
    }

    /// 撃って終わりまで待ち、rc と stdout / stderr を得る。
    pub fn output(&mut self) -> io::Result<Output> {
        current()?.output(self)
    }

    /// 撃って終わりまで待ち、rc を得る。
    pub fn status(&mut self) -> io::Result<ExitStatus> {
        current()?.status(self)
    }

    /// 撃って子を得る（待たない）。
    pub fn spawn(&mut self) -> io::Result<Child> {
        current()?.spawn(self)
    }

    /// 今の process を置き換える（戻るのは失敗の周だけ）。
    pub fn exec(&mut self) -> io::Error {
        match current() {
            Ok(spawner) => spawner.exec(self),
            Err(err) => err,
        }
    }
}

/// 差し替え口（method は 4 終端と 1 対 1）。実物は境界 crate の 1 file・cfg(test) の既定は `pipe` の fixture。
pub trait Spawner: Sync {
    /// [`Invocation::output`] の実体。
    fn output(&self, invocation: &mut Invocation) -> io::Result<Output>;
    /// [`Invocation::status`] の実体。
    fn status(&self, invocation: &mut Invocation) -> io::Result<ExitStatus>;
    /// [`Invocation::spawn`] の実体。
    fn spawn(&self, invocation: &mut Invocation) -> io::Result<Child>;
    /// [`Invocation::exec`] の実体。
    fn exec(&self, invocation: &mut Invocation) -> io::Error;
}

/// 据えた物を解く（純関数・`None` = 据えていない周は io の `Unsupported`）。
pub fn resolve(installed: Option<&'static dyn Spawner>) -> io::Result<&'static dyn Spawner> {
    installed.ok_or_else(|| io::Error::new(io::ErrorKind::Unsupported, "起動の差し替え口が据えられていない"))
}

/// 据える先の cell へ 1 回だけ据える（2 回目は据えず、渡した物を `Err` で返す）。
pub fn install_into(
    cell: &OnceLock<&'static dyn Spawner>,
    spawner: &'static dyn Spawner,
) -> Result<(), &'static dyn Spawner> {
    cell.set(spawner)
}

/// process の大域の置き場（本番の build が読む）。
static INSTALLED: OnceLock<&'static dyn Spawner> = OnceLock::new();

/// process の大域の置き場へ据える（bin の main の先頭と e2e の共通の関数が撃つ・2 回目は据えない）。
pub fn install(spawner: &'static dyn Spawner) -> Result<(), &'static dyn Spawner> {
    install_into(&INSTALLED, spawner)
}

/// 本番の build の終端が撃つ差し替え口（process の大域の置き場を解く）。
#[cfg(not(test))]
fn current() -> io::Result<&'static dyn Spawner> {
    resolve(INSTALLED.get().copied())
}

#[cfg(test)]
use crate::pipe::fixture::current;
