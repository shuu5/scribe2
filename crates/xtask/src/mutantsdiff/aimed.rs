//! 検出線の「的を絞った周」（設計 docs/design/gate-cost.md §16 / §40・`s2-07l.549`）。
//!
//! 契約が名指した的を読み（[`targets_of`] / [`targets_in`]）、cargo-mutants の引数を的へ絞り（[`aimed_args`]）、
//! 出力 dir を読んで的ごとに 5 値へ分類して判定行を出す（[`aimed_of`] / [`aimed_run`]）群である。`--diff` の母集団の
//! 読み（[`diff_of`] / [`place_diff`]）も同じ群に居る。`crates/xtask/src/mutantsdiff.rs` からの**純移動**で、歯は
//! 1 本も足していない（親の歯の区間が従来どおり測る）。
//!
//! 可視性: 親の本体が呼ぶ 2 本（[`place_diff`] / [`aimed_run`]）だけが `pub(super)` に上がり、残りは移す前と同じ。
//! 逆向き（子 → 親）は `super::` でそのまま見える（Rust の可視性＝子孫は祖先の私有を見る）ので、**親側の可視性は
//! 1 語も上げていない**。

// flip-check: moved s2-07l.549

use super::{
    baseline_log_tail, diagnosed, flag, judged, measured, outside_token, parse_outcomes, unmeasured, without_outcomes,
    write_diff, Counts, Scope,
};
use std::path::Path;
use std::process::ExitCode;

/// 契約が名指した的 1 本（file・行・変異の名・設計 gate-cost.md §16）。字面は `<file>:<行>:<変異の名>` で、行の後ろに
/// `<桁>:` を 1 つ挟んでよい（cargo-mutants の一覧の行をそのまま貼れる・桁は照合に使わない）。形の規則は core の
/// 契約表の読み（`pipe/contract.rs` の `target_unfit`）と同じである。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// repo 相対の `.rs`。
    pub file: String,
    /// 1 始まりの行。
    pub line: u64,
    /// 変異の名（cargo-mutants の一覧の `: ` の後ろ・前後の空白は落とす）。
    pub name: String,
}

impl Target {
    /// 字面から読む（形に合わなければ理由）。
    pub fn parse(text: &str) -> Result<Self, String> {
        let broken = |why: &str| Err(format!("的 {text:?} が <file>:<行>:<変異の名> の形でない: {why}"));
        if text.chars().any(char::is_control) {
            return broken("制御文字");
        }
        let Some((file, rest)) = text.split_once(':') else {
            return broken(": が無い");
        };
        if file.is_empty() || file.chars().any(char::is_whitespace) || !file.ends_with(".rs") {
            return broken("file が空白を持たない .rs でない");
        }
        let Some((line, name)) = rest.split_once(':') else {
            return broken("行の後ろの : が無い");
        };
        let number = line.parse::<u64>().ok().filter(|n| *n >= 1);
        let Some(line) = number.filter(|_| line.chars().all(|found| found.is_ascii_digit())) else {
            return broken("行が 1 以上の十進でない");
        };
        let name = match name.split_once(':') {
            Some((column, after)) if !column.is_empty() && column.chars().all(|found| found.is_ascii_digit()) => after,
            _ => name,
        };
        let name = name.trim();
        if name.is_empty() {
            return broken("変異の名が空");
        }
        Ok(Self { file: file.to_owned(), line, name: name.to_owned() })
    }
}

/// 的 1 本の分類（**閉じた enum**・設計 gate-cost.md §16 (2)）。4 つは cargo-mutants の outcomes の kind（[`Counts`] が
/// 読む 4 つの数と同じ語）、[`Absent`](Self::Absent) は的が outcomes のどの一覧にも当たらない（file・行・名が現物と
/// ずれた）。noop は outcomes の上で missed と区別できないので持たない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// 歯が落とした。
    Caught,
    /// 生き残った。
    Missed,
    /// compile できず成立しなかった。
    Unviable,
    /// 時間切れ。
    Timeout,
    /// outcomes に当たらない（的が現物とずれた）。
    Absent,
}

/// [`Outcome`] の全 variant（宣言順＝判定行の token の順）。
pub const OUTCOMES: &[Outcome] = &[Outcome::Caught, Outcome::Missed, Outcome::Unviable, Outcome::Timeout, Outcome::Absent];

impl Outcome {
    /// 判定行の token の名。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Caught => "caught",
            Self::Missed => "missed",
            Self::Unviable => "unviable",
            Self::Timeout => "timeout",
            Self::Absent => "absent",
        }
    }

    /// cargo-mutants が出力 dir に書く一覧の file の名（1 行 1 変異）。absent は一覧を持たない。
    pub fn list_file(self) -> Option<&'static str> {
        match self {
            Self::Caught => Some("caught.txt"),
            Self::Missed => Some("missed.txt"),
            Self::Unviable => Some("unviable.txt"),
            Self::Timeout => Some("timeout.txt"),
            Self::Absent => None,
        }
    }

    /// outcomes.json の同じ kind の数（absent は outcomes に無い）。
    fn counted(self, counts: &Counts) -> u64 {
        match self {
            Self::Caught => counts.caught,
            Self::Missed => counts.missed,
            Self::Unviable => counts.unviable,
            Self::Timeout => counts.timeout,
            Self::Absent => 0,
        }
    }
}

/// 的を絞った周の分類（的ごとに [`Outcome`] 1 つ・宣言順）。**5 値の和 = 的の本数**である。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Aimed(pub Vec<Outcome>);

impl Aimed {
    /// `kind` に分類された的の本数。
    pub fn count(&self, kind: Outcome) -> u64 {
        u64::try_from(self.0.iter().filter(|found| **found == kind).count()).unwrap_or(u64::MAX)
    }

    /// stdout へ出す 1 行: `total=<的の本数>` と 5 値を宣言順に並べ、`scope=` / `teeth=` は従来の行と同じ・末尾の
    /// `population=targets` が「diff の追加行でなく的を母集団にした」ことを名乗る。その後ろに従来の行と同じ
    /// `outside=` の 1 語（設計 gate-cost.md §39 形 3）。
    pub fn line(&self, scope: &Scope, outside: &[String]) -> String {
        let kinds: Vec<String> = OUTCOMES.iter().map(|kind| format!("{}={}", kind.as_str(), self.count(*kind))).collect();
        format!(
            "mutants-diff: total={} {} scope={} teeth={} population=targets {}",
            self.0.len(),
            kinds.join(" "),
            scope.name(),
            scope.teeth(),
            outside_token(outside)
        )
    }
}

/// 的を絞った周の分類を出力から組む（**pure**・設計 gate-cost.md §16 (2)）。
///
/// `outcomes` は `outcomes.json` の本文（不在は `None`）、`lists` は [`OUTCOMES`] の先頭 4 つの一覧の本文（不在は
/// `None`）。outcomes は従来の周と同じ読み（[`parse_outcomes`] → [`measured`]・不在は [`without_outcomes`]）を通し、
/// 読めない周は `Err`＝**5 値に化けない**（C10）。数が 1 以上の kind の一覧が無い周も `Err`（一覧の不在を 0 と読まない）。
/// 的は一覧の行（`file:行[:桁]: 名`）と file・行・名の 3 つが一致した kind に落ち、どれにも当たらなければ absent。
pub fn aimed_of(
    targets: &[Target],
    outcomes: Option<&str>,
    lists: [Option<&str>; 4],
    tool_succeeded: bool,
) -> Result<Aimed, String> {
    let counts = match outcomes {
        Some(json) => parse_outcomes(json).and_then(|counts| measured(counts, tool_succeeded))?,
        None => without_outcomes(tool_succeeded)?,
    };
    let mut listed: Vec<(Outcome, Vec<Target>)> = Vec::new();
    for (kind, list) in OUTCOMES.iter().zip(lists) {
        match list {
            Some(text) => listed.push((*kind, text.lines().filter_map(|line| Target::parse(line).ok()).collect())),
            None if kind.counted(&counts) > 0 => {
                return Err(format!(
                    "{} が無いのに outcomes.json の {} が {}（測れていない）",
                    kind.list_file().unwrap_or_default(),
                    kind.as_str(),
                    kind.counted(&counts)
                ));
            }
            None => {}
        }
    }
    let classify = |target: &Target| {
        listed
            .iter()
            .find(|(_, found)| found.contains(target))
            .map_or(Outcome::Absent, |(kind, _)| *kind)
    };
    Ok(Aimed(targets.iter().map(classify).collect()))
}

/// `--targets <file>` を読む（設計 gate-cost.md §16 (2)）。flag が無い周は `Ok(None)`（diff の追加行を母集団にする
/// 従来の形）。値の無い flag・読めない file・形の外れた行・的 0 本は `Err`＝**測らずに rc 2**（的を黙って落とすと
/// 母集団が静かに狭まる・fail-closed）。空行は数えない。
pub fn targets_of(args: &[String]) -> Result<Option<Vec<Target>>, String> {
    if !args.iter().any(|arg| arg == "--targets") {
        return Ok(None);
    }
    let path = flag(args, "--targets").ok_or_else(|| "mutants-diff: --targets に file が無い（測れていない・rc 2）".to_owned())?;
    let text = std::fs::read_to_string(path).map_err(|err| format!("mutants-diff: 的の file {path} を読めない: {err}"))?;
    let targets = targets_in(&text)?;
    Ok(Some(targets))
}

/// `--diff <file>` を読む（設計 gate-cost.md §14 約束 2）。flag が無い周は `Ok(None)`（`git diff <base>...HEAD` を
/// 母集団にする従来の形）。在る周は file の byte を返し、呼び手はそれを `in.diff` に写して `git diff` を撃たない。
/// 値の無い flag・読めない file は `Err`＝**測らずに rc 2**（母集団を黙って従来の diff へ戻さない・fail-closed）。
pub fn diff_of(args: &[String]) -> Result<Option<Vec<u8>>, String> {
    if !args.iter().any(|arg| arg == "--diff") {
        return Ok(None);
    }
    let path = flag(args, "--diff").ok_or_else(|| "mutants-diff: --diff に file が無い（測れていない・rc 2）".to_owned())?;
    let bytes = std::fs::read(path).map_err(|err| format!("mutants-diff: 母集団の diff {path} を読めない: {err}（測れていない・rc 2）"))?;
    Ok(Some(bytes))
}

/// `in.diff` を置く: 渡された母集団（[`diff_of`]）が在ればそれを写し、無ければ `git diff <base>...HEAD` を落とす。
pub(super) fn place_diff(root: &Path, base: &str, given: Option<&[u8]>, path: &Path) -> Result<(), String> {
    match given {
        Some(bytes) => std::fs::write(path, bytes).map_err(|err| format!("diff を書けない: {err}")),
        None => write_diff(root, base, path),
    }
}

/// 的の file の本文を読む（1 行 1 本・空行は飛ばす・1 本でも形が外れたら `Err`・0 本も `Err`）。
pub fn targets_in(text: &str) -> Result<Vec<Target>, String> {
    let targets = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(Target::parse)
        .collect::<Result<Vec<Target>, String>>()
        .map_err(|reason| format!("mutants-diff: {reason}（測れていない・rc 2）"))?;
    if targets.is_empty() {
        return Err("mutants-diff: 的の file に的が 1 本も無い（測れていない・rc 2）".to_owned());
    }
    Ok(targets)
}

/// 的を絞った周の cargo-mutants の引数（[`measure_args`](super::measure_args) の列から組み直す・設計 gate-cost.md §16 (2)）。
///
/// `--in-diff <diff>` の対を落とし（歯だけの便の的は diff に無い base の行である）、1 つ目の `--` の直前に
/// 的の file ごとの `--file <file>`（初出の順・重複なし）と、的ごとの `--re <名の regex>`（名の字面を逃がした式）を
/// 置く。それ以外の語（`-p` / `-o` / `--jobs` / `--baseline skip` / `--timeout` / test の側）は 1 語も変えない。
pub fn aimed_args(args: Vec<String>, targets: &[Target]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut skip = false;
    for arg in args {
        if skip {
            skip = false;
            continue;
        }
        if arg == "--in-diff" {
            skip = true;
            continue;
        }
        out.push(arg);
    }
    let mut aimed: Vec<String> = Vec::new();
    let mut files: Vec<&str> = Vec::new();
    for target in targets {
        if !files.contains(&target.file.as_str()) {
            files.push(&target.file);
            aimed.extend(["--file".to_owned(), target.file.clone()]);
        }
    }
    for target in targets {
        aimed.extend(["--re".to_owned(), regex_literal(&target.name)]);
    }
    let at = out.iter().position(|arg| arg == "--").unwrap_or(out.len());
    out.splice(at..at, aimed);
    out
}

/// 字面をそのまま当てる regex（meta 文字を `\` で逃がす）。
fn regex_literal(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if "\\.+*?()|[]{}^$#&-~".contains(ch) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// 的を絞った周の後段（設計 gate-cost.md §16 (2)）: 出力 dir の `outcomes.json` と 4 つの一覧を読んで的ごとに分類し
/// （[`aimed_of`]）、判定行 [`Aimed::line`] を出す。読めない周は従来の周と同じく `baseline.log` の末尾を添えて rc 2
/// （[`diagnosed`]）。rc の極性は従来と同じ `R-C12-1` の 1 本（生存 1 本以上 ∧ deny の周だけ rc 1・absent は赤にしない）。
pub(super) fn aimed_run(
    root: &Path,
    dir: &Path,
    targets: &[Target],
    succeeded: bool,
    (scope, log, outside): (&Scope, &Path, &[String]),
) -> ExitCode {
    let read = |name: &str| match std::fs::read_to_string(dir.join(name)) {
        Ok(text) => Ok(Some(text)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(format!("{name} を読めない: {err}（測れていない）")),
    };
    let classified = (|| {
        let outcomes = read("outcomes.json")?;
        let mut lists: [Option<String>; 4] = Default::default();
        for (slot, kind) in lists.iter_mut().zip(OUTCOMES) {
            *slot = match kind.list_file() {
                Some(name) => read(name)?,
                None => None,
            };
        }
        aimed_of(targets, outcomes.as_deref(), lists.each_ref().map(Option::as_deref), succeeded)
    })();
    let aimed = match diagnosed(classified, || baseline_log_tail(log)) {
        Ok(found) => found,
        Err(reason) => return unmeasured(&format!("mutants-diff: {reason}")),
    };
    crate::emit(&aimed.line(scope, outside));
    let counts = Counts {
        total: u64::try_from(aimed.0.len()).unwrap_or(u64::MAX),
        caught: aimed.count(Outcome::Caught),
        missed: aimed.count(Outcome::Missed),
        unviable: aimed.count(Outcome::Unviable),
        timeout: aimed.count(Outcome::Timeout),
    };
    judged(root, &counts)
}
