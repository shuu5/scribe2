//! 所属の目印 `.vessel` と、repo を器へ紐づける `vessel` subcommand（設計 §2）。
//!
//! **env も HOME も読まない**（憲法 C2.2・ADR-0004 §2.4）。置き場は repo の local
//! git 設定 `<NAME>.stateDir` に置き、key 名は NAME 定数から導く。marker が自分の
//! NAME を言わないとき hook は黙る（FR24）ので、この module の判定はすべて
//! **黙る側へ倒す**（不在・読めない・parse 不能・非 repo はいずれも
//! [`Served::Absent`]）。名前の字面はこの file に書かず [`NAME`] から組む。

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
    format!("usage: {NAME} vessel <init --state-dir D [--version N]|show|check> [ROOT]")
}

/// `vessel` に続く引数を捌く。
pub fn dispatch(args: &[String]) -> Outcome {
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
