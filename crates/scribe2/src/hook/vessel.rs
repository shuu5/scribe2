//! 所属の目印 `.vessel` と、repo を器へ紐づける `vessel` subcommand（設計 §2）。
//!
//! **env も HOME も読まない**（憲法 C2.2・ADR-0004 §2.4）。置き場は repo の local
//! git 設定 `<NAME>.stateDir` に置き、key 名は NAME 定数から導く。marker が自分の
//! NAME を言わないとき hook は黙る（FR24）ので、この module の判定はすべて
//! **黙る側へ倒す**（不在・読めない・parse 不能・非 repo はいずれも
//! [`Served::Absent`]）。名前の字面はこの file に書かず [`NAME`] から組む。
//!
//! hook 集合の digest と読み込み元の記録は [`digest`]（設計 consumer-sync.md §3）。

pub mod digest;

use crate::cli_args::{self, Allowed};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_REFUSED};
use crate::name::NAME;
use std::path::{Path, PathBuf};
use std::process::Command;

/// 所属の目印の固定名（ADR-0004 §2.2 面 1）。**版番号に依らず固定**する（R-O3）。
pub const MARKER: &str = ".vessel";

/// marker の 1 行目の key。
const KEY_NAME: &str = "name";
/// marker の 2 行目の key。
const KEY_VERSION: &str = "version";

/// 器の世代。`vessel init --version N` で上書きできる。
///
/// package version（`0.1.0`）とは別物である: あちらは binary の版で、こちらは
/// 「前の版と次の版」を分ける器の世代である（跨版 面 1）。
pub const GENERATION: u64 = 2;

/// marker の中身（2 行・この順）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    /// 名乗っている器の名前。
    pub name: String,
    /// 器の世代。
    pub version: u64,
}

impl Marker {
    /// 2 行・LF・この順・末尾改行・余白なしで書き出す。
    pub fn render(&self) -> String {
        format!(
            "{KEY_NAME}={}\n{KEY_VERSION}={}\n",
            self.name, self.version
        )
    }

    /// 2 行を読む。行数違い・key 違い・順序違い・整数でない版はすべて `Err`。
    ///
    /// 行末の空白だけ許す。余分な行（末尾の空行を含む）は行数違いとして落とす。
    pub fn parse(text: &str) -> Result<Self, String> {
        let lines: Vec<&str> = text.lines().collect();
        let [first, second] = lines.as_slice() else {
            return Err(format!("2 行でない（{} 行）", lines.len()));
        };
        let name = value_of(first, KEY_NAME)?;
        let raw = value_of(second, KEY_VERSION)?;
        let version = raw
            .parse::<u64>()
            .map_err(|err| format!("{KEY_VERSION} が整数でない（{raw:?}・{err}）"))?;
        Ok(Self {
            name: name.to_owned(),
            version,
        })
    }
}

/// `<key>=<値>` の 1 行から値を取る。行末の空白だけ許す。
fn value_of<'a>(line: &'a str, key: &str) -> Result<&'a str, String> {
    line.trim_end()
        .strip_prefix(key)
        .and_then(|rest| rest.strip_prefix('='))
        .ok_or_else(|| format!("{key}= で始まる行でない（{line:?}）"))
}

/// この repo に誰が仕えるか。**bool で持たない**（憲法 C11）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Served {
    /// 自分が仕える（marker 在 ∧ name 一致 ∧ state dir 設定済みの 3 条件）。値は世代。
    ByMe(u64),
    /// 別の器が名乗っている。
    ByOther(String),
    /// 誰も名乗っていない・読めない・紐づいていない（黙る側・FR24）。
    Absent,
}

/// marker の path。
pub fn marker_path(root: &Path) -> PathBuf {
    root.join(MARKER)
}

/// この repo に誰が仕えるかを判定する。**読めない側はすべて [`Served::Absent`]**。
pub fn served(root: &Path) -> Served {
    let Ok(text) = std::fs::read_to_string(marker_path(root)) else {
        return Served::Absent;
    };
    let Ok(marker) = Marker::parse(&text) else {
        return Served::Absent;
    };
    if marker.name != NAME {
        return Served::ByOther(marker.name);
    }
    match state_dir(root) {
        Some(_) => Served::ByMe(marker.version),
        None => Served::Absent,
    }
}

/// git を 1 回撃って stdout の 1 行を得る。失敗・空はいずれも `None`。
fn git_line(dir: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git").arg("-C").arg(dir).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if line.is_empty() {
        None
    } else {
        Some(line)
    }
}

/// git を 1 回撃ち、成功したかだけを見る（出力を持たない設定系に使う）。
fn git_ok(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .is_ok_and(|out| out.status.success())
}

/// `cwd` を含む repo の root。非 repo は `None`（＝黙る側）。
pub fn repo_root(cwd: &Path) -> Option<PathBuf> {
    git_line(cwd, &["rev-parse", "--show-toplevel"]).map(PathBuf::from)
}

/// `cwd` の worktree が使う git dir（worktree なら `<repo>/.git/worktrees/<名>`）。
///
/// `--git-dir` でなく `--absolute-git-dir` を撃つのは、前者が cwd 相対の
/// `.git` を返しうるためである（policy の path を組むには絶対 path が要る）。
pub fn git_dir(cwd: &Path) -> Option<PathBuf> {
    git_line(cwd, &["rev-parse", "--absolute-git-dir"]).map(PathBuf::from)
}

/// state dir を持つ git config の key。**NAME から導く**（C2.2）。
fn state_dir_key() -> String {
    format!("{NAME}.stateDir")
}

/// repo に紐づいた state dir。未設定は `None`。**env も HOME も読まない**。
pub fn state_dir(root: &Path) -> Option<PathBuf> {
    git_line(root, &["config", "--get", &state_dir_key()]).map(PathBuf::from)
}

/// `vessel` の使い方。
pub fn usage() -> String {
    format!(
        "usage: {NAME} vessel <init --state-dir D [--version N]|show|check> [ROOT]\n       {NAME} vessel update --state-dir S [--remote R] [--branch B]"
    )
}

/// `vessel init` が受ける flag（設計 pipeline.md §14 約束 5・`ROOT` は positional）。
const ALLOWED_INIT: &[cli_args::Allowed] = &[Allowed::value("--state-dir"), Allowed::value("--version")];
/// `vessel show` / `vessel check`（flag を受けない）。
const ALLOWED_BARE: &[cli_args::Allowed] = &[];
/// `vessel update`（設計 consumer-sync.md §5・`ROOT` を取らない＝repo は `[[vessel]] repo` が名指す）。
const ALLOWED_UPDATE: &[cli_args::Allowed] =
    &[Allowed::value("--state-dir"), Allowed::value("--remote"), Allowed::value("--branch")];

/// `vessel` に続く引数を捌く。既知の verb は root を解く前に閉包の検査を 1 回撃つ（未知の flag と `--help` を断る）。
pub fn dispatch(args: &[String]) -> Outcome {
    let allowed = match args.first().map(String::as_str) {
        Some("init") => Some(ALLOWED_INIT),
        Some("show" | "check") => Some(ALLOWED_BARE),
        Some("update") => Some(ALLOWED_UPDATE),
        _ => None,
    };
    let rest = args.get(1..).unwrap_or_default();
    if let Some(Err(error)) = allowed.map(|found| crate::cli_args::parse(rest, found)) {
        return crate::cli_args::refusal("vessel", &error, usage());
    }
    if args.first().is_some_and(|verb| verb == "update") {
        return update_cmd(rest);
    }
    let root = match root_of(args) {
        Ok(found) => found,
        Err(reason) => return Outcome::failed(RC_REFUSED, vec![format!("vessel: {reason}")]),
    };
    match args.first().map(String::as_str) {
        Some("init") => init(args, &root),
        Some("show") => show(&root),
        Some("check") => check(&root),
        _ => Outcome::failed(RC_REFUSED, vec![usage()]),
    }
}

/// `--<name> <値>` を読む。flag が無ければ `Ok(None)`、値が欠けていれば `Err`。
///
/// **値欠けを黙って落とさない**（SRS NFR4）。
fn flag_value<'a>(args: &'a [String], name: &str) -> Result<Option<&'a str>, String> {
    let Some(at) = args.iter().position(|arg| arg == name) else {
        return Ok(None);
    };
    match args.get(at + 1) {
        Some(found) if !found.starts_with("--") => Ok(Some(found)),
        _ => Err(format!("{name} に値が無い")),
    }
}

/// 位置引数 `ROOT` を解く。省略時のみ cwd を root とする。
///
/// flag とその値は読み飛ばす（`current_dir` は syscall であって env の読み取りでは
/// ないので C2.2 に触れない）。
fn root_of(args: &[String]) -> Result<PathBuf, String> {
    let mut rest = args.iter().skip(1);
    while let Some(arg) = rest.next() {
        if arg.starts_with("--") {
            if rest.clone().next().is_some_and(|next| !next.starts_with("--")) {
                rest.next();
            }
            continue;
        }
        return Ok(PathBuf::from(arg));
    }
    std::env::current_dir().map_err(|err| format!("cwd を解決できない: {err}"))
}

/// `--version N` を読む。省略時は [`GENERATION`]。
fn version_of(args: &[String]) -> Result<u64, String> {
    match flag_value(args, "--version")? {
        None => Ok(GENERATION),
        Some(raw) => raw
            .parse::<u64>()
            .map_err(|err| format!("--version が整数でない（{raw:?}・{err}）")),
    }
}

/// marker を書き、repo の local git 設定へ state dir を書く。
///
/// 既存 marker が**別 name**なら rc 2 で 1 byte も書かない（他の器の repo を奪わない）。
fn init(args: &[String], root: &Path) -> Outcome {
    let dir = match flag_value(args, "--state-dir").and_then(|found| {
        found.ok_or_else(|| "--state-dir が要る".to_owned())
    }) {
        Ok(found) => found,
        Err(reason) => return Outcome::failed(RC_REFUSED, vec![format!("vessel: {reason}")]),
    };
    let version = match version_of(args) {
        Ok(found) => found,
        Err(reason) => return Outcome::failed(RC_REFUSED, vec![format!("vessel: {reason}")]),
    };
    if let Served::ByOther(other) = served(root) {
        return Outcome::failed(
            RC_BROKEN,
            vec![format!("vessel: {other} が名乗っている（何も書かない）")],
        );
    }
    // git 設定を**先に**書く。marker が先だと、設定に失敗した周（非 repo・git dir が
    // 書けない）に marker だけが残り、PUBLIC repo の `git status` を汚す。設定だけが
    // 残っても marker が無い限り `served` は `Absent` なので hook は黙る。
    if !git_ok(root, &["config", "--local", &state_dir_key(), dir]) {
        return Outcome::failed(
            RC_BROKEN,
            vec!["vessel: state dir を git の local 設定へ書けない".to_owned()],
        );
    }
    let marker = Marker {
        name: NAME.to_owned(),
        version,
    };
    if let Err(err) = std::fs::write(marker_path(root), marker.render()) {
        return Outcome::failed(RC_BROKEN, vec![format!("vessel: marker を書けない: {err}")]);
    }
    Outcome::ok_line(format!(
        "vessel: init {} {KEY_VERSION}={version} stateDir={dir}",
        marker_path(root).display()
    ))
}

/// marker の 2 行と state dir を 1 行で出す。
fn show(root: &Path) -> Outcome {
    let text = match std::fs::read_to_string(marker_path(root)) {
        Ok(found) => found,
        Err(err) => return Outcome::failed(RC_REFUSED, vec![format!("vessel: marker が無い: {err}")]),
    };
    let marker = match Marker::parse(&text) {
        Ok(found) => found,
        Err(reason) => {
            return Outcome::failed(RC_BROKEN, vec![format!("vessel: marker が読めない（{reason}）")])
        }
    };
    let dir = state_dir(root).map_or_else(|| "-".to_owned(), |path| path.display().to_string());
    Outcome::ok_line(format!(
        "{KEY_NAME}={} {KEY_VERSION}={} stateDir={dir}",
        marker.name, marker.version
    ))
}

/// 誰が仕えるかを rc で表す（0 = 自分 / 1 = 不在 / 2 = 別の器）。
fn check(root: &Path) -> Outcome {
    match served(root) {
        Served::ByMe(version) => Outcome::ok_line(format!("served {KEY_VERSION}={version}")),
        Served::Absent => Outcome::failed(RC_REFUSED, vec!["vessel: 名乗りが無い".to_owned()]),
        Served::ByOther(other) => {
            Outcome::failed(RC_BROKEN, vec![format!("vessel: {other} が名乗っている")])
        }
    }
}

/// `vessel update` の既定の remote（設計 consumer-sync.md §5 (2)）。
pub const DEFAULT_REMOTE: &str = "origin";
/// `vessel update` の既定の branch（設計 consumer-sync.md §5 (2)）。
pub const DEFAULT_BRANCH: &str = "main";

/// `vessel update` の断り（設計 consumer-sync.md §8・closed・宣言順は順序固定の段の順）。
///
/// **どの断りでも `InstallRecorded` は 0 件**（成功した周だけ 1 件・C10）。rc は [`Self::rc`]。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateError {
    /// host の面に `[[vessel]] repo` が無い（宣言を読めない周は読めない理由つき）。
    Undeclared(Vec<String>),
    /// `status --porcelain` が非空か、clean と確かめられない（作業ツリーを動かさない・N1）。
    Dirty,
    /// `fetch` が落ちた（rc つき・`None` = 起動できない / signal）。
    FetchFailed(Option<i32>),
    /// `merge --ff-only` が落ちた（rebase も reset もしない＝人の手番）。
    NotFastForward,
    /// `cargo install` が落ちた（rc つき・旧 binary が残る）。
    InstallFailed(Option<i32>),
    /// install は済んだが event を積めない（sha / path が読めない・store が書けない・理由つき）。
    RecordFailed(String),
}

impl UpdateError {
    /// 断りの 1 語。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Undeclared(_) => "vessel-repo-undeclared",
            Self::Dirty => "dirty",
            Self::FetchFailed(_) => "fetch-failed",
            Self::NotFastForward => "not-fast-forward",
            Self::InstallFailed(_) => "install-failed",
            Self::RecordFailed(_) => "record-failed",
        }
    }

    /// rc（入力の拒否は既存の拒否と同じ 1・install が済んだ後の記録の失敗だけ 2・設計 §5）。
    pub fn rc(&self) -> u8 {
        match self {
            Self::RecordFailed(_) => RC_BROKEN,
            Self::Undeclared(_) | Self::Dirty | Self::FetchFailed(_) | Self::NotFastForward | Self::InstallFailed(_) => {
                RC_REFUSED
            }
        }
    }

    /// stderr の行（`vessel: <語>[ rc=<n>|（理由）]` の 1 行と、宣言を読めない周の理由の行）。
    fn lines(&self) -> Vec<String> {
        let rc_of = |rc: &Option<i32>| rc.map_or_else(|| "-".to_owned(), |found| found.to_string());
        match self {
            Self::Undeclared(reasons) => {
                std::iter::once(format!("vessel: {}", self.as_str())).chain(reasons.iter().cloned()).collect()
            }
            Self::Dirty | Self::NotFastForward => vec![format!("vessel: {}", self.as_str())],
            Self::FetchFailed(rc) | Self::InstallFailed(rc) => vec![format!("vessel: {} rc={}", self.as_str(), rc_of(rc))],
            Self::RecordFailed(reason) => vec![format!("vessel: {}（{reason}）", self.as_str())],
        }
    }
}

/// `vessel update --state-dir S [--remote R] [--branch B]` の口（引数の閉包は [`dispatch`] が済ませた）。
fn update_cmd(rest: &[String]) -> Outcome {
    let refused = |error: cli_args::ArgsError| cli_args::refusal("vessel", &error, usage());
    let parsed = match cli_args::parse(rest, ALLOWED_UPDATE) {
        Ok(found) => found,
        Err(error) => return refused(error),
    };
    if let Some(extra) = parsed.positionals().first() {
        return refused(cli_args::ArgsError::Unknown((*extra).to_owned()));
    }
    let state_dir = match parsed.need("--state-dir") {
        Ok(found) => PathBuf::from(found),
        Err(error) => return refused(error),
    };
    let remote = parsed.value("--remote").unwrap_or(DEFAULT_REMOTE);
    let branch = parsed.value("--branch").unwrap_or(DEFAULT_BRANCH);
    match update(&state_dir, remote, branch) {
        Ok(install) => Outcome::ok_line(format!("vessel: installed {}", install.render())),
        Err(error) => Outcome::failed(error.rc(), error.lines()),
    }
}

/// ff → build → install を順序固定で 1 回行い、成功した周だけ `InstallRecorded` を 1 件積む（設計 consumer-sync.md §5）。
///
/// (1) `status --porcelain` → (2) `fetch <remote>` → `merge --ff-only <remote>/<branch>` → (3) `cargo install --path
/// <repo>/crates/<NAME> --locked` → (4) event。**どの段で断っても後の段は撃たない**（作業ツリーを変えない・旧 binary が
/// 残る・event 0）。sha は (3) の後に HEAD から読む（(3) は checkout を動かさない＝install した HEAD）。
pub fn update(state_dir: &Path, remote: &str, branch: &str) -> Result<crate::fleet::Install, UpdateError> {
    let manifest = crate::rules::read(None, Some(state_dir))
        .map_err(|errors| UpdateError::Undeclared(errors.iter().map(ToString::to_string).collect()))?;
    let repo = manifest.vessel().map(|found| PathBuf::from(found.repo())).ok_or(UpdateError::Undeclared(Vec::new()))?;
    let status = git_output(&repo, &["status", "--porcelain"]).ok_or(UpdateError::Dirty)?;
    if !status.status.success() || !status.stdout.is_empty() {
        return Err(UpdateError::Dirty);
    }
    let fetched = git_output(&repo, &["fetch", remote]);
    if !fetched.as_ref().is_some_and(|out| out.status.success()) {
        return Err(UpdateError::FetchFailed(fetched.and_then(|out| out.status.code())));
    }
    if !git_ok(&repo, &["merge", "--ff-only", &format!("{remote}/{branch}")]) {
        return Err(UpdateError::NotFastForward);
    }
    let installed = Command::new("cargo")
        .arg("install")
        .arg("--path")
        .arg(repo.join("crates").join(NAME))
        .args(["--locked", "--color", "never"])
        .current_dir(&repo)
        .output();
    let installed = match installed {
        Ok(found) if found.status.success() => found,
        Ok(found) => return Err(UpdateError::InstallFailed(found.status.code())),
        Err(_) => return Err(UpdateError::InstallFailed(None)),
    };
    let head = git_line(&repo, &["rev-parse", "HEAD"])
        .ok_or_else(|| UpdateError::RecordFailed("HEAD の sha を読めない".to_owned()))?;
    let path = installed_path(&installed.stderr)
        .or_else(|| installed_path(&installed.stdout))
        .ok_or_else(|| UpdateError::RecordFailed("cargo が binary の path を報告しない".to_owned()))?;
    let install = crate::fleet::Install::parse(&format!("sha={} path={path}", head.get(..12).unwrap_or_default()))
        .ok_or_else(|| UpdateError::RecordFailed(format!("HEAD {head:?} が sha の形でない")))?;
    record_install(state_dir, &install).map_err(UpdateError::RecordFailed)?;
    Ok(install)
}

/// git を 1 回撃って出力を得る（起動できなければ `None`）。
fn git_output(dir: &Path, args: &[&str]) -> Option<std::process::Output> {
    Command::new("git").arg("-C").arg(dir).args(args).output().ok()
}

/// cargo install の出力から binary の path を引く（`Installing <abs>` か `Replacing <abs>` の最後の行・色は切ってある）。
///
/// 冒頭の `Installing <crate> v<版> (<path>)` は絶対 path で始まらないので拾わない。
fn installed_path(output: &[u8]) -> Option<String> {
    String::from_utf8_lossy(output).lines().rev().find_map(|line| {
        let line = line.trim();
        let rest = line.strip_prefix("Installing ").or_else(|| line.strip_prefix("Replacing "))?;
        rest.starts_with('/').then(|| rest.trim().to_owned())
    })
}

/// `InstallRecorded` を 1 件積む（`run` / `bead` を持たない・actor は machine・schema 1・本体は `detail` の 1 行）。
fn record_install(state_dir: &Path, install: &crate::fleet::Install) -> Result<(), String> {
    use crate::fleet::{cli as fleet_cli, store, Event, EventKind, SCHEMA};
    let kind = EventKind::InstallRecorded;
    let event = Event {
        schema: SCHEMA,
        ts: fleet_cli::now_utc(),
        kind,
        run: String::new(),
        bead: String::new(),
        host: fleet_cli::host(),
        actor: kind.default_actor().to_owned(),
        stage: None,
        seat: None,
        pid: None,
        detail: Some(install.render()),
        allowance: None,
        registration: None,
        mark: None,
        account: None,
    };
    let policy = store::LockPolicy::embedded().map_err(|err| err.to_string())?;
    store::append(state_dir, &event, policy).map(|_| ()).map_err(|err| err.to_string())
}
