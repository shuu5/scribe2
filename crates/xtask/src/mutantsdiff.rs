//! `cargo xtask mutants-diff --base <ref>`: diff の中の変異を **1 行で記録する**
//! （[ADR-0009 §2.3] の検出線・rules 行 `R-C12-1`）。
//!
//! **記録であって門ではない**。`R-C12-1` の `enabled` が偽である限り、生存（missed）が
//! 在っても rc 0 を返す——deny 化は user 裁定（憲法 C5）が要る変更で、この道具の側で
//! 決めてよいことではない。`enabled` が真になった周は同じ 1 行のまま rc 1 へ倒れる。
//!
//! **「測れなかった」を 0 に化けさせない**のがもう 1 つの要点である。cargo-mutants が
//! 入っていない環境で `missed=0` を出すと、母集団 0 の緑が「歯は非空虚」と読まれる。
//! 道具が無い周は rc 2 と stderr 1 行で落ちる（`unviable` も 1 行に出るので、
//! 「測れた / 測れていない」が報告の額面で読める）。
//!
//! [ADR-0009 §2.3]: ../../../design-intent/decisions/ADR-0009-vessel-grants-runner-permissions-and-mutation-proof.html

use crate::toml_lite;
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};

/// 1 行に写す件数。**何を測ったか**（scope）は field で持たず [`Counts::line`] の引数で受ける
/// ＝範囲を名乗らない行は組めない（lens-82 MEDIUM-2: 空の scope を表現可能にしない）。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// 生成された変異の総数。
    pub total: u64,
    /// 歯が落とした数。
    pub caught: u64,
    /// **生き残った**数（検出線の主役）。
    pub missed: u64,
    /// compile できず成立しなかった数。**撃墜に数えない**。
    pub unviable: u64,
    /// 時間切れの数。
    pub timeout: u64,
}

impl Counts {
    /// stdout へ出す 1 行。既存 5 token の名前・順序・書式は据え置き、末尾に `scope=` を足す。
    ///
    /// `scope` = 測った範囲＝`cargo mutants -p` へ**実際に渡した**名前（bd `s2-07l.82`）。行は
    /// 出所から切り離されて流通する（bead notes / PR 本文 / CI log から切り出される）ので、
    /// 「core package だけを測った」という限界は報告でなく**行そのもの**に載せる。値は行を組む側の
    /// literal ではなく [`Scope`]（[`measure_args`] だけが作る）で受ける＝`-p` へ渡した名前と
    /// 別の値を行に書く形は compile できない。将来 diff が触った package を並べて測る形（案 (b)）
    /// になっても同じ引数で表せる。
    ///
    /// 末尾の `teeth=<-|n>`（設計 gate-cost.md §34 約束 3・行 z）は mutant の test に掛けた filter の
    /// 語の数（`--teeth` 無し = `-`・空 = `0`）。これも [`Scope`] から読む＝`-E` に渡した語と別の数を
    /// 行に書く形は組めない。
    ///
    /// さらに末尾の `outside=<-|dir,…>`（設計 gate-cost.md §39 形 3・行 af）は差分が触れた member のうち測る範囲の
    /// 外の dir（[`outside_of`]）。先頭の 7 token は 1 字も変えずに後ろへ 1 語だけ足す＝`total=0` の周が「生存 0」か
    /// 「範囲の外だけを触った」かを行の字面で読み分ける。**記録であって門ではない**（rc は動かない）。
    pub fn line(&self, scope: &Scope, outside: &[String]) -> String {
        format!(
            "mutants-diff: total={} caught={} missed={} unviable={} timeout={} scope={} teeth={} {}",
            self.total,
            self.caught,
            self.missed,
            self.unviable,
            self.timeout,
            scope.name(),
            scope.teeth(),
            outside_token(outside)
        )
    }
}

/// 差分が触れた member の dir（root 相対・`members` の宣言順・重複無し・設計 gate-cost.md §39 形 1）。**pure**。
///
/// `diff` は unified diff の本文で、`--- a/<path>` と `+++ b/<path>` の行から path を取る（`/dev/null` は数えない＝
/// 追加も削除も片側の path で拾う）。path が member の dir と一致するか `<dir>/` で始まるときだけその member に数え、
/// どの member にも属さない path（設計 doc など）は数えない。同じ member の 2 file は 1 件に畳む。
pub fn touched_members(diff: &str, members: &[String]) -> Vec<String> {
    let paths: Vec<&str> = diff
        .lines()
        .filter_map(|line| line.strip_prefix("--- a/").or_else(|| line.strip_prefix("+++ b/")))
        .map(|path| path.trim_end_matches('\t').trim_end())
        .collect();
    let within = |path: &str, dir: &str| path == dir || path.strip_prefix(dir).is_some_and(|rest| rest.starts_with('/'));
    let mut touched: Vec<String> = Vec::new();
    for dir in members {
        let dir = dir.trim_end_matches('/');
        if dir.is_empty() || touched.iter().any(|found| found == dir) {
            continue;
        }
        if paths.iter().any(|path| within(path, dir)) {
            touched.push(dir.to_owned());
        }
    }
    touched
}

/// 触れた member（[`touched_members`]）から測る範囲の dir（core）を除いた残り＝「触れたのに測っていない面」
/// （設計 gate-cost.md §39 形 2・順序は入力のまま）。
pub fn outside_of(touched: &[String], core: &str) -> Vec<String> {
    let core = core.trim_end_matches('/');
    touched.iter().filter(|dir| dir.as_str() != core).cloned().collect()
}

/// 判定行の末尾の 1 語（`outside=` の後ろに `,` で結ぶ・0 件は `-`・設計 gate-cost.md §39 形 3）。
fn outside_token(outside: &[String]) -> String {
    if outside.is_empty() {
        return "outside=-".to_owned();
    }
    format!("outside={}", outside.join(","))
}

/// 範囲の外の dir を差分の file から導く（読めない file は空の差分として扱う＝記録であって門ではない・§39 形 4）。
fn outside_in(diff_path: &Path, layout: &crate::check::Layout) -> Vec<String> {
    let diff = std::fs::read(diff_path).map(|bytes| String::from_utf8_lossy(&bytes).into_owned()).unwrap_or_default();
    let relative = |dir: &Path| dir.strip_prefix(&layout.root).unwrap_or(dir).to_string_lossy().into_owned();
    let members: Vec<String> = layout.member_dirs.iter().map(|dir| relative(dir)).collect();
    outside_of(&touched_members(&diff, &members), &relative(&layout.core_dir))
}

pub use scope::{measure_args, Pace, Scope};

/// [`Scope`] を作れる場所を **この module の内側だけ**にする。親（[`run`] を含む）からは field が
/// 見えないので、`-p` へ渡した名前と別の値で行を組む形は compile できない（lens-82 再確認の残余:
/// 同じ module に置くと private field でも呼び側が literal から組めた・実測）。
mod scope {
    use std::path::Path;

    /// 測った範囲＝`cargo mutants -p` へ**実際に渡した**名前。作れるのは [`measure_args`] だけ
    /// （field は private・`Default` も持たない）ので、行の `scope=` と実際に測った package が
    /// 別々の読みで食い違う形は型で組めない（lens-82 MEDIUM-1）。
    ///
    /// 2 つ目の field は mutant の test に掛けた filter の語の数（`None` = `--teeth` 無し・§34）。
    #[derive(Debug, PartialEq, Eq)]
    pub struct Scope(String, Option<usize>);

    impl Scope {
        /// 行に写す名前。
        pub fn name(&self) -> &str {
            &self.0
        }

        /// 行に写す `teeth=` の値（無し = `-`・空 = `0`・語の数）。
        pub fn teeth(&self) -> String {
            self.1.map_or_else(|| "-".to_owned(), |count| count.to_string())
        }
    }

    /// cargo-mutants へ渡す数 3 つ（並列度・thread 数・mutant の test の timeout 秒）。引数を
    /// 束ねるのは clippy の `too-many-arguments-threshold`（5）の内に収めるためで、意味は 3 つの
    /// 独立な値のまま（どれも歯が別々に pin する）。
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Pace {
        /// `--jobs`（器の受付が決めた値・§3.3）。
        pub jobs: u64,
        /// `--test-threads`（器の受付が決めた値・§31 約束 7）。
        pub threads: u64,
        /// `--timeout`（自前の baseline の壁時計から導いた秒・§33 約束 4）。
        pub timeout_s: u64,
    }

    /// `cargo` へ渡す引数（`cargo` の直後から）と、その `-p` に載せた [`Scope`]。
    ///
    /// `--in-diff <diff>` / `-p <scope>` / `-o <out>` / `--jobs <n>` はそれぞれ隣り合う対で、
    /// 歯が対のまま見る（`-o` を落とすと測った結果を読まずに `total=0` へ化ける・lens-82
    /// 再確認 MEDIUM-4）。
    ///
    /// **並列度も thread 数もこの道具は値を持たない**（設計 gate-cost.md §3.3 / §31 約束 7）。器が
    /// 受付で導いた実効値を宣言 file の `{jobs}` / `{threads}` 経由で受け取り、cargo-mutants の
    /// `--jobs` と test binary の `--test-threads` へそのまま渡すだけである。
    ///
    /// **mutant の test は fail-fast**（設計 gate-cost.md §33・行 y）: `--baseline skip` で
    /// cargo-mutants の baseline を撃たず（全数の baseline は [`super::baseline_args`] が道具の外で
    /// 1 回撃つ）、`--timeout <T>` にその壁時計から導いた秒を渡す（skip した周に固定 300 秒へ
    /// 倒れさせない）。1 つ目の `--` の後ろに `--no-fail-fast` は置かない＝撃墜は最初に落ちた
    /// binary で決まる。
    ///
    /// 2 つ目の `--` の後ろ `--test-threads <t>` は cargo test が test binary へ渡す引数
    /// （設計 gate-cost.md §22・行 m）。cargo-mutants は 1 つ目の `--` より後ろを 2 つ目の `--`
    /// ごと逐語で cargo test へ渡す。libtest の既定は core 数の thread なので、`--jobs` の各 job
    /// が全 core に広がり gate 1 本で jobs × cores 並列になる（2026-09-16 の load 57 / 16 core）。
    /// `t` の導出（cores と `gate.mutants_jobs` から）は器の受付が持つ（§31・行 w）——道具が
    /// `cores / jobs` で導くと、受け付けた枠が上限より小さい周に job あたりの値段が上がり、gate 2 本で
    /// core の 2 倍の thread を作る。
    ///
    /// **`teeth` が `Some` の周は mutant の test を nextest で走らせる**（設計 gate-cost.md §34 約束 1・
    /// 行 z）: `--timeout <T>` の後ろに `--test-tool nextest`、1 つ目の `--` の後ろは
    /// `-E <式> --test-threads <t>` だけ（nextest は引数を逐語で受けるので 2 つ目の `--` は無い）。
    /// 式は [`super::nextest_expr`]。`None` の周は上の §33 の形を 1 語も変えない。
    pub fn measure_args(
        diff: &Path,
        out: &Path,
        scope: &str,
        pace: Pace,
        teeth: Option<&[String]>,
    ) -> (Vec<String>, Scope) {
        let mut args: Vec<String> = ["mutants", "--in-diff"].iter().map(|s| (*s).to_owned()).collect();
        args.push(diff.display().to_string());
        args.push("-p".to_owned());
        args.push(scope.to_owned());
        args.extend(["--no-shuffle", "--copy-vcs", "true", "-o"].iter().map(|s| (*s).to_owned()));
        args.push(out.display().to_string());
        args.push("--jobs".to_owned());
        args.push(pace.jobs.to_string());
        args.extend(["--baseline", "skip", "--timeout"].iter().map(|s| (*s).to_owned()));
        args.push(pace.timeout_s.to_string());
        match teeth {
            Some(words) => {
                args.extend(["--test-tool", "nextest", "--", "-E"].iter().map(|s| (*s).to_owned()));
                args.push(super::nextest_expr(words));
                args.push("--test-threads".to_owned());
            }
            None => args.extend(["--", "--", "--test-threads"].iter().map(|s| (*s).to_owned())),
        }
        args.push(pace.threads.to_string());
        (args, Scope(scope.to_owned(), teeth.map(<[String]>::len)))
    }
}

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
fn place_diff(root: &Path, base: &str, given: Option<&[u8]>, path: &Path) -> Result<(), String> {
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

/// 的を絞った周の cargo-mutants の引数（[`measure_args`] の列から組み直す・設計 gate-cost.md §16 (2)）。
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

/// 数えた結果に対する rc。**極性は manifest の `R-C12-1` 行が決める**。
pub fn verdict(counts: &Counts, deny: bool) -> ExitCode {
    if deny && counts.missed > 0 {
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// `outcomes.json` の **top-level の数値 field**だけを読む（json subset を自前で・外部 crate を
/// 足さない＝A3 と C13 予算の外に居る）。
///
/// 入れ子（`outcomes[]` の各要素や `phase_results`）に同名の key が現れても拾わないよう、
/// **深さ 1 の key だけ**を見る。file 全体を grep する形にすると、値の文字列に同じ字面が
/// 現れただけで数が動く。
pub fn parse_outcomes(json: &str) -> Result<Counts, String> {
    let mut counts = Counts::default();
    // **5 つとも在ることを要求する**。1 つでも欠けたら Err＝欠けた面を 0 と読むと、
    // 「measured 0」と「書かれていない」が同じ緑になる（lens 2026-09-11 MEDIUM）。
    let mut seen = 0_u8;
    let mut depth = 0_usize;
    let mut in_string = false;
    let mut escaped = false;
    for (at, ch) in json.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        if ch == '"' {
            if depth == 1 {
                if let Some(rest) = json.get(at..) {
                    if let Some((key, value)) = top_level_number(rest) {
                        match key {
                            "total_mutants" => {
                                counts.total = value;
                                seen = seen.saturating_add(1);
                            }
                            "caught" => {
                                counts.caught = value;
                                seen = seen.saturating_add(1);
                            }
                            "missed" => {
                                counts.missed = value;
                                seen = seen.saturating_add(1);
                            }
                            "unviable" => {
                                counts.unviable = value;
                                seen = seen.saturating_add(1);
                            }
                            "timeout" => {
                                counts.timeout = value;
                                seen = seen.saturating_add(1);
                            }
                            _ => {}
                        }
                    }
                }
            }
            in_string = true;
        } else if ch == '{' || ch == '[' {
            depth = depth.saturating_add(1);
        } else if ch == '}' || ch == ']' {
            depth = depth.saturating_sub(1);
        }
    }
    if seen == 5 {
        return Ok(counts);
    }
    Err(format!(
        "outcomes.json の 5 つの数のうち {seen} つしか読めない（測れていない）"
    ))
}

/// **道具の rc を捨てない**（lens 2026-09-11 H1）。
///
/// cargo-mutants は baseline（変異を当てない木）の test が落ちた周も `outcomes.json` を書く
/// （`total_mutants=0`）。rc を見ずに件数だけ読むと、その 1 行は「測る対象が無い」周と
/// **1 bit も違わない緑**になる——suite が壊れているときほど門が緑になる、最悪の極性である。
///
/// 判定に rc の表は要らない: **非 0 の理由が件数から説明できる周だけ**を測定として受ける
/// （生存や時間切れが在れば cargo-mutants は非 0 で終える）。説明できない非 0 は
/// 「測れなかった」＝rc 2 に残す。
pub fn measured(counts: Counts, tool_succeeded: bool) -> Result<Counts, String> {
    if tool_succeeded || counts.missed > 0 || counts.timeout > 0 {
        return Ok(counts);
    }
    Err("cargo mutants が非 0 で終えたが生存も時間切れも無い（baseline が落ちた疑い・測れていない）".to_owned())
}

/// `outcomes.json` が無かった周の扱い（user 裁定を受けた planner 裁定 2026-09-11）。
///
/// **「測る対象が無い」と「測れなかった」は別の事実である**。cargo-mutants は diff に変異が
/// 1 つも無い周を **rc 0 + `No mutants to filter`** で終え、出力 dir を作らない（実測 2026-09-11）。
/// これを「測れなかった」に潰すと、core を触らない便（docs-only / xtask 便）は common-verify に
/// 載せた瞬間から**恒久 FAIL**になる——**偽の赤は偽の緑と同じく毒**で、赤が常態化すると本当の
/// 赤が読めなくなる。逆に道具の異常終了を rc 0 にすれば、道具の不在が緑に化ける。
///
/// ゆえに **rc 0 の周だけ** `total=0` の 1 行（母集団を額面に出す）へ倒し、非 0 は `Err`＝rc 2 に残す。
pub fn without_outcomes(tool_succeeded: bool) -> Result<Counts, String> {
    if tool_succeeded {
        return Ok(Counts::default());
    }
    Err("outcomes.json を読めない（測れていない）".to_owned())
}

/// rc 2 の周に stderr へ写す `baseline.log` の末尾の行数（設計 pipeline.md §5.3 の
/// 「stderr の末尾 20 行」と同じ値・rules 行ではない）。
pub const BASELINE_TAIL_LINES: usize = 20;

/// `baseline.log` の末尾 `lines` 行（順序はそのまま）。空なら「空」だと 1 行で名乗る
/// ——空 file を空文字で写すと「末尾が無い」と「写していない」が同じ字面になる。
pub fn baseline_tail(log: &str, lines: usize) -> String {
    if log.trim().is_empty() {
        return "baseline.log は空".to_owned();
    }
    let mut tail: Vec<&str> = log.lines().rev().take(lines).collect();
    tail.reverse();
    tail.join("\n")
}

/// stderr へ写す見出し（この行の次から `baseline.log` の末尾）。
const BASELINE_TAIL_HEADING: &str = "mutants-diff: baseline.log の末尾:";

/// 「測れなかった」周（`Err`）**だけ**に `baseline.log` の末尾を添える（憲法 C10: 測れなかった
/// 理由を測定値として残す・設計 pipeline.md §5.3: 診断は `verify.stderr.log` に載る）。
///
/// `s2-07l.329` run 1 の rc 2 は理由の 1 行だけが残り、原因（baseline の 1 本の歯が落ちた）は
/// 退避 worktree の `baseline.log` を手で開くまで読めなかった。**測れた周（`Ok`）には何も
/// 足さない**——緑の行に診断を残すと、緑と赤の stderr が同じ形になる。`tail` は `Err` の周に
/// だけ呼ぶ（file を読むのは理由が立った後）。
/// 的を絞った周（[`Aimed`]・設計 gate-cost.md §16）も同じ 1 本を通す（型だけが変わる）。
pub fn diagnosed<T>(outcome: Result<T, String>, tail: impl FnOnce() -> String) -> Result<T, String> {
    outcome.map_err(|reason| format!("{reason}\n{BASELINE_TAIL_HEADING}\n{}", tail()))
}

/// baseline の build（`cargo` の直後から）。秒は測らない（設計 gate-cost.md §33 約束 1 (i)）。
pub fn baseline_build_args(scope: &str) -> Vec<String> {
    ["test", "-p", scope, "--no-run"].iter().map(|s| (*s).to_owned()).collect()
}

/// baseline の test（`cargo` の直後から・設計 gate-cost.md §33 約束 1 (ii)）。**全数**を走らせる
/// ＝`--no-fail-fast` は 1 つ目の `--` の前（cargo test 自身の flag）に置く。§19 の理由（flaky
/// 1 本で落ちた歯の全数を名指せない）は baseline にだけ当たるので、ここにだけ残す。
pub fn baseline_args(scope: &str, threads: u64) -> Vec<String> {
    let mut args: Vec<String> = ["test", "-p", scope, "--no-fail-fast", "--", "--test-threads"]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    args.push(threads.to_string());
    args
}

/// mutant の test の timeout の床（秒・cargo-mutants v27 の既定と同じ値・rules 行ではない）。
const TIMEOUT_FLOOR_S: u64 = 20;

/// baseline の test の壁時計に掛ける倍率（cargo-mutants v27 の既定と同じ値）。
const TIMEOUT_MULTIPLIER: u64 = 5;

/// baseline の test の壁時計（ミリ秒の整数）から mutant の test の timeout 秒を導く:
/// `T = max(20, ceil(5 × ms / 1000))`（設計 gate-cost.md §33 約束 4）。**整数演算**で導く
/// ——float の丸めで 309 / 310 が揺れる形を作らない。
pub fn mutant_timeout_s(baseline_ms: u64) -> u64 {
    baseline_ms
        .saturating_mul(TIMEOUT_MULTIPLIER)
        .div_ceil(1000)
        .max(TIMEOUT_FLOOR_S)
}

/// 自前の baseline の判定（設計 gate-cost.md §33 約束 2）。rc ≠ 0 の周は cargo-mutants を起こさず
/// 「測れなかった」（rc 2）へ倒し、理由行の後ろに**自前の** `baseline.log` の末尾を添える
/// （[`diagnosed`] と同じ形・読む file の出所だけが変わる）。rc 0 の周は `Ok`＝cargo-mutants へ進む。
pub fn baseline_judged(succeeded: bool, log: &str) -> Result<(), String> {
    if succeeded {
        return Ok(());
    }
    diagnosed(
        Err("baseline の cargo test が非 0 で終えた（測れていない）".to_owned()),
        || baseline_tail(log, BASELINE_TAIL_LINES),
    )
}

/// `"key": <整数>` の形を 1 つ読む。数でなければ `None`（文字列や object は数えない）。
fn top_level_number(rest: &str) -> Option<(&str, u64)> {
    let after_quote = rest.strip_prefix('"')?;
    let (key, tail) = after_quote.split_once('"')?;
    let value = tail.trim_start().strip_prefix(':')?.trim_start();
    let digits: String = value.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok().map(|number| (key, number))
}

/// rules manifest の `R-C12-1` 行の `enabled`。**この道具は値を持たない**（憲法 C1: 規則値は
/// manifest に 1 つ）。行が無い周は `Some(false)`＝「門にしない」側へ倒す——無い規則を勝手に
/// 発効させない。
///
/// **行は在るのに `enabled` が読めない周は `None`**（`s2-07l.80`・裁定 id
/// `user 2026-09-11T23:59Z`）。`false` に化けさせると、極性を決める行が壊れている周ほど
/// 門が緩む側へ黙って倒れる——「不発効だと書かれている」と「書かれていない」は別の事実で、
/// 後者は**測れていない**（呼び手は rc 2 で止める）。`enabled` は manifest の必須 key なので、
/// 正しい manifest でこの枝は起きない。
///
/// section は [`toml_lite::sections`] で読む。array-of-tables の header は `[` を 1 つだけ
/// 剥がした `[rule` になる（`limits.rs` の歯と同じ字面）。
pub fn deny_line_enabled(manifest: &str) -> Option<bool> {
    for (header, pairs) in toml_lite::sections(manifest) {
        if header != RULE_HEADER {
            continue;
        }
        let id = pairs
            .iter()
            .find(|(key, _)| *key == "id")
            .and_then(|(_, value)| toml_lite::quoted(value));
        if id.as_deref() != Some(DENY_LINE_ID) {
            continue;
        }
        return match pairs.iter().find(|(key, _)| *key == "enabled") {
            Some((_, value)) if value.trim() == "true" => Some(true),
            Some((_, value)) if value.trim() == "false" => Some(false),
            // 行は在るのに bool として読めない（欠落・型違い）＝極性が決まらない。
            _ => None,
        };
    }
    Some(false)
}

/// array-of-tables の section header の字面（`sections` は `[` を 1 つだけ剥がす）。
const RULE_HEADER: &str = "[rule";

/// 極性を決める rules 行の id。
const DENY_LINE_ID: &str = "R-C12-1";

/// flag の値を取る（値が無ければ `None`）。
fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    let at = args.iter().position(|arg| arg == name)?;
    args.get(at.saturating_add(1))
        .map(String::as_str)
        .filter(|value| !value.starts_with("--"))
}

/// 使い方（rc 2 の 1 行）。
const USAGE: &str = "usage: cargo xtask mutants-diff --base <ref> [--jobs <n>] [--threads <t>] [--teeth <語,…|->] [--targets <file>] [--diff <file>]";

/// `--teeth` の字面が読めない周の理由（閉じた 1 つ・設計 gate-cost.md §34 約束 1）。
pub const TEETH_MALFORMED: &str =
    "mutants-diff: --teeth の語は , 区切りの [A-Za-z0-9_]+ か - だけ（測れていない・rc 2）";

/// `--teeth <語列>` を読む（設計 gate-cost.md §34 約束 1・行 z）。
///
/// flag が無い周は `Ok(None)`（§33 の形のまま撃つ）、`-` は `Ok(Some(空))`、それ以外は `,` で割った
/// 語が全部 `[A-Za-z0-9_]+` の周だけ `Ok(Some(語))`。値の無い flag・空の語・それ以外の字を含む語は
/// [`TEETH_MALFORMED`] の `Err`＝**測らずに rc 2**（語を黙って落とすと filter が広がる／狭まる側へ
/// 静かに倒れる・fail-closed）。
pub fn teeth_of(args: &[String]) -> Result<Option<Vec<String>>, String> {
    if !args.iter().any(|arg| arg == "--teeth") {
        return Ok(None);
    }
    let value = flag(args, "--teeth").ok_or_else(|| TEETH_MALFORMED.to_owned())?;
    if value == "-" {
        return Ok(Some(Vec::new()));
    }
    let words: Vec<String> = value.split(',').map(str::to_owned).collect();
    let well_formed = |word: &String| !word.is_empty() && word.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_');
    if words.iter().all(well_formed) {
        return Ok(Some(words));
    }
    Err(TEETH_MALFORMED.to_owned())
}

/// mutant の test に掛ける nextest の filter の式: `kind(lib) | kind(bin)`、語が在れば
/// `| test(/^(語1|語2)/)` を足す（e2e の歯は契約が名指した語で始まる分だけ・§34 約束 1）。
pub fn nextest_expr(words: &[String]) -> String {
    let base = "kind(lib) | kind(bin)";
    if words.is_empty() {
        return base.to_owned();
    }
    format!("{base} | test(/^({})/)", words.join("|"))
}

/// `--jobs` / `--threads` を渡されなかった周の並列度と thread 数（設計 gate-cost.md §2「止めない、縮退する」）。
///
/// **1 は常に許される**（従来と同じ費用）。器を通さずに人が撃つ周と、受付が枠を取れなかった
/// 周が同じ値になる形で、道具の側に「速い既定」を持たない。
const JOBS_FLOOR: u64 = 1;

/// `--jobs` の値。読めない字面は [`JOBS_FLOOR`] へ落とす（**速い側へ倒さない**）。
pub fn jobs_of(args: &[String]) -> u64 {
    floored(args, "--jobs")
}

/// `--threads` の値（job 1 つの `cargo test` に許す test thread・器の受付が決めた値・設計 gate-cost.md §31
/// 約束 7）。渡されない周と数でない周は **1**（`--jobs` と同じ向き・cores から導かない）。
pub fn threads_of(args: &[String]) -> u64 {
    floored(args, "--threads")
}

/// 数の flag の値。渡されない・数でない・0 は [`JOBS_FLOOR`]（1）へ落とす。
fn floored(args: &[String], name: &str) -> u64 {
    flag(args, name)
        .and_then(|value| value.parse().ok())
        .filter(|found| *found >= JOBS_FLOOR)
        .unwrap_or(JOBS_FLOOR)
}

/// 「測れなかった」を表す rc。**0 に化けさせない**ための第 3 の値である。
fn unmeasured(reason: &str) -> ExitCode {
    crate::emit_err(reason);
    ExitCode::from(2)
}

/// `mutants-diff` の入口。
pub fn run(args: &[String]) -> ExitCode {
    let Some(base) = flag(args, "--base") else {
        return unmeasured(USAGE);
    };
    // **語の形が悪い周・的が読めない周は何も撃たない**（rc 2・設計 gate-cost.md §34 約束 1・§16）。
    // 母集団の diff が渡された周はその byte を `in.diff` にする（読めない周も何も撃たない・設計 gate-cost.md §14）。
    let (teeth, targets, given) = match teeth_of(args).and_then(|teeth| Ok((teeth, targets_of(args)?, diff_of(args)?))) {
        Ok(found) => found,
        Err(reason) => return unmeasured(&reason),
    };
    let Ok(root) = std::env::current_dir() else {
        return unmeasured("mutants-diff: cwd を解決できない");
    };
    if !tool_present() {
        return unmeasured("mutants-diff: cargo-mutants が無い（測れていない・rc 2）");
    }
    let work = root.join("target").join("mutants-diff");
    if let Err(err) = std::fs::create_dir_all(&work) {
        return unmeasured(&format!("mutants-diff: 作業 dir を作れない: {err}"));
    }
    let diff_path = work.join("in.diff");
    // **前便の測定結果を今便の 1 行として出さない**（lens 2026-09-11 H2・実測で再現した）。
    // cargo-mutants は変異 0 の周に出力 dir へ触らないので、掃除しないと前の周の
    // `total=18 missed=6` がそのまま今の周の測定を名乗る。
    let out = work.join("out");
    if let Err(reason) = place_diff(&root, base, given.as_deref(), &diff_path).and_then(|()| clear_previous_out(&out)) {
        return unmeasured(&format!("mutants-diff: {reason}"));
    }
    // **package 名は NAME から解決する**（字面を持たない＝憲法 C2.2・`xtask check` の name-literal）。
    let layout = match crate::check::Layout::discover(&root) {
        Ok(found) => found,
        Err(reason) => return unmeasured(&format!("mutants-diff: {reason}")),
    };
    // **baseline は道具の外で 1 回、全数**（設計 gate-cost.md §33）。赤なら cargo-mutants を起こさない。
    let log_path = work.join("baseline.log");
    let threads = threads_of(args);
    let timeout_s = match own_baseline(&root, &layout.name, threads, &log_path) {
        Ok(found) => found,
        Err(reason) => return unmeasured(&format!("mutants-diff: {reason}")),
    };
    // **測る範囲は 1 つの束縛**: `-p` へ渡した名前を [`Scope`] として受け取り、行はそれでしか組めない。
    // 並列度も thread 数も**受けた値をそのまま**渡す（cores はここで読まない・設計 gate-cost.md §31 約束 7）。
    let pace = Pace { jobs: jobs_of(args), threads, timeout_s };
    let (args, scope) = measure_args(&diff_path, &out, &layout.name, pace, teeth.as_deref());
    let args = if let Some(found) = &targets { aimed_args(args, found) } else { args };
    let status = Command::new("cargo")
        .args(args)
        .current_dir(&root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    // **rc は判定に使わない**。cargo-mutants は生存が在る周に非 0 を返すが、門にするかは
    // manifest の `R-C12-1` が決める（ここで倒すと enabled を無視して deny 化してしまう）。
    let Ok(status) = status else {
        return unmeasured("mutants-diff: cargo mutants を起動できない（測れていない・rc 2）");
    };
    let outcomes = out.join("mutants.out").join("outcomes.json");
    // **範囲の外を名指す**（設計 gate-cost.md §39）: 差分が触れた member のうち core の外の dir。記録であって門ではない。
    let outside = outside_in(&diff_path, &layout);
    if let Some(found) = &targets {
        return aimed_run(&root, &out.join("mutants.out"), found, status.success(), (&scope, &log_path, &outside));
    }
    let counts = match std::fs::read_to_string(&outcomes) {
        // 読めた周も **道具の rc を見る**（[`measured`]・baseline 失敗を緑にしない）。
        Ok(json) => parse_outcomes(&json).and_then(|counts| measured(counts, status.success())),
        // **不在**の周だけ 2 つの事実を弁別する（[`without_outcomes`]）。権限や I/O の失敗を
        // 「無かった」と同じに扱うと、読めないことが `total=0` の緑に化ける。
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => without_outcomes(status.success()),
        Err(err) => Err(format!("outcomes.json を読めない: {err}（測れていない）")),
    };
    // **rc 2 の周だけ**自前の `baseline.log` の末尾を理由行の後ろに写す（cargo-mutants は
    // `--baseline skip` で baseline を書かない・設計 gate-cost.md §33 約束 2）。
    let counts = match diagnosed(counts, || baseline_log_tail(&log_path)) {
        Ok(found) => found,
        Err(reason) => return unmeasured(&format!("mutants-diff: {reason}")),
    };
    // **`-p` に渡した名前そのもの**を行に持ち回る（[`Scope`] は literal から作れない）。
    crate::emit(&counts.line(&scope, &outside));
    judged(&root, &counts)
}

/// 数えた結果の rc を manifest の `R-C12-1` で決める（diff の周も的の周も同じ 1 本）。
fn judged(root: &Path, counts: &Counts) -> ExitCode {
    let manifest = std::fs::read_to_string(root.join("rules").join("manifest.toml")).unwrap_or_default();
    // **極性が読めない周は判定しない**（rc 2）。`R-C12-1` が在るのに `enabled` を読めない
    // まま `verdict` を呼ぶと、壊れた行が「門にしない」の緑と 1 bit も違わなくなる。
    let Some(deny) = deny_line_enabled(&manifest) else {
        return unmeasured("mutants-diff: R-C12-1 の enabled を読めない（極性が決まらない・測れていない・rc 2）");
    };
    verdict(counts, deny)
}

/// 的を絞った周の後段（設計 gate-cost.md §16 (2)）: 出力 dir の `outcomes.json` と 4 つの一覧を読んで的ごとに分類し
/// （[`aimed_of`]）、判定行 [`Aimed::line`] を出す。読めない周は従来の周と同じく `baseline.log` の末尾を添えて rc 2
/// （[`diagnosed`]）。rc の極性は従来と同じ `R-C12-1` の 1 本（生存 1 本以上 ∧ deny の周だけ rc 1・absent は赤にしない）。
fn aimed_run(
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

/// 前便の出力 dir を消す（無い周は何もしない・それ以外の失敗は理由を返す）。
fn clear_previous_out(out: &Path) -> Result<(), String> {
    match std::fs::remove_dir_all(out) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(format!("前回の出力を掃除できない: {err}")),
        _ => Ok(()),
    }
}

/// 自前の `baseline.log`（作業 dir・[`own_baseline`] が書く）の末尾。無い・読めない周はその
/// 理由を 1 行で（診断が無いことも診断として残す）。
fn baseline_log_tail(log: &Path) -> String {
    match std::fs::read_to_string(log) {
        Ok(text) => baseline_tail(&text, BASELINE_TAIL_LINES),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            "baseline.log が無い（baseline に届いていない）".to_owned()
        }
        Err(err) => format!("baseline.log を読めない: {err}"),
    }
}

/// 自前の baseline（設計 gate-cost.md §33 約束 1 / 2 / 4）: build → test を 1 回ずつ撃ち、test の
/// stdout / stderr を `log` へ写す。赤なら [`baseline_judged`] の `Err`（末尾つき）、緑なら test の
/// 壁時計（ms）から導いた timeout 秒を返し、`log` の末尾に `timeout=<T>` の 1 行を足す。
fn own_baseline(root: &Path, scope: &str, threads: u64, log: &Path) -> Result<u64, String> {
    let fire = |args: Vec<String>| {
        Command::new("cargo")
            .args(args)
            .current_dir(root)
            .stdin(Stdio::null())
            .output()
            .map_err(|err| format!("baseline の cargo test を起動できない: {err}（測れていない）"))
    };
    let build = fire(baseline_build_args(scope))?;
    if !build.status.success() {
        let text = log_text(&build);
        write_log(log, &text)?;
        return baseline_judged(false, &text).map(|()| 0);
    }
    let started = std::time::Instant::now();
    let test = fire(baseline_args(scope, threads))?;
    let ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let mut text = log_text(&test);
    let judged = baseline_judged(test.status.success(), &text);
    let timeout_s = mutant_timeout_s(ms);
    if judged.is_ok() {
        text.push_str(&format!("timeout={timeout_s}\n"));
    }
    write_log(log, &text)?;
    judged.map(|()| timeout_s)
}

/// 1 手の stdout と stderr を 1 本の log の字面へ（stdout が先）。
fn log_text(output: &std::process::Output) -> String {
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

fn write_log(log: &Path, text: &str) -> Result<(), String> {
    std::fs::write(log, text).map_err(|err| format!("baseline.log を書けない: {err}（測れていない）"))
}

/// cargo-mutants が居るか（`--version` が rc 0 を返すか）。
fn tool_present() -> bool {
    Command::new("cargo")
        .args(["mutants", "--version"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// `git diff <base>...HEAD` を file へ落とす。
fn write_diff(root: &Path, base: &str, path: &Path) -> Result<(), String> {
    let output = Command::new("git")
        .arg("diff")
        .arg(format!("{base}...HEAD"))
        .current_dir(root)
        .output()
        .map_err(|err| format!("git diff を起動できない: {err}"))?;
    if !output.status.success() {
        return Err(format!("git diff が失敗した（base={base}）"));
    }
    std::fs::write(path, &output.stdout).map_err(|err| format!("diff を書けない: {err}"))
}

/// [`measure_args`] の歯。本体と同じ file に置く（設計 gate-cost.md §19・行 j の write-set）。
/// 他の `mutants_*` の歯は `main.rs` の test 区間に在り、そちらは触らない。
#[cfg(test)]
mod tests {
    use super::{
        aimed_args, aimed_of, baseline_args, diff_of, place_diff, baseline_build_args, baseline_judged, measure_args, mutant_timeout_s,
        outside_of, targets_in, targets_of, teeth_of, touched_members, Aimed, Counts, Outcome, Pace, Target,
        BASELINE_TAIL_HEADING, OUTCOMES, TEETH_MALFORMED,
    };
    use std::path::Path;

    /// **baseline の** `cargo test` は全数（憲法 C10・§19 の理由は baseline にだけ当たる・§33）:
    /// `test -p <scope> --no-fail-fast -- --test-threads <t>` を**この順**で組み、`--no-fail-fast` は
    /// 1 つ目の `--` の前（cargo test 自身の flag）。`--` は 1 つで、その直後が `--test-threads`。
    /// build の手は `test -p <scope> --no-run`（--no-fail-fast も thread も持たない）。
    #[test]
    fn no_fail_fast_is_passed_to_cargo_test_by_mutants() {
        for (scope, threads) in [("probe-pkg-3f", 3_u64), ("other-pkg-7a", 1)] {
            let args = baseline_args(scope, threads);
            let threads = threads.to_string();
            assert_eq!(
                args,
                ["test", "-p", scope, "--no-fail-fast", "--", "--test-threads", threads.as_str()],
                "baseline の引数はこの順: {args:?}"
            );
            assert_eq!(args.iter().filter(|a| *a == "--").count(), 1, "-- は 1 つ: {args:?}");
            let build = baseline_build_args(scope);
            assert_eq!(build, ["test", "-p", scope, "--no-run"], "build の手: {build:?}");
        }
    }

    /// (a) mutant の test は fail-fast: `measure_args` の `--` の後ろに `--no-fail-fast` が無く、
    /// `--baseline skip` が `--jobs <j>` の後ろ・1 つ目の `--` の前に在る。
    #[test]
    fn mutants_diff_fail_fast_mutants_skip_the_baseline_and_do_not_pass_no_fail_fast() {
        for (jobs, threads, timeout_s) in [(3_u64, 4_u64, 21_u64), (1, 1, 310)] {
            let pace = Pace { jobs, threads, timeout_s };
            let (args, _) = measure_args(Path::new("probe.diff"), Path::new("probe-out"), "probe-pkg-3f", pace, None);
            assert!(!args.iter().any(|a| a == "--no-fail-fast"), "mutant に --no-fail-fast は無い: {args:?}");
            let jobs_at = args.iter().position(|a| a == "--jobs").expect("--jobs が在る");
            let skip_at = args.iter().position(|a| a == "--baseline").expect("--baseline が在る");
            let dashes = args.iter().position(|a| a == "--").expect("-- が在る");
            assert_eq!(args.get(jobs_at + 1).map(String::as_str), Some(jobs.to_string().as_str()), "{args:?}");
            assert_eq!(skip_at, jobs_at + 2, "--baseline は --jobs <j> の直後: {args:?}");
            assert_eq!(args.get(skip_at + 1).map(String::as_str), Some("skip"), "{args:?}");
            assert!(skip_at < dashes, "--baseline skip は -- の前: {args:?}");
            assert_eq!(
                &args[dashes..],
                ["--", "--", "--test-threads", threads.to_string().as_str()],
                "-- の後ろは -- --test-threads <t> だけ: {args:?}"
            );
        }
    }

    /// (d) の後半: `--timeout <T>` が `--baseline skip` の直後・1 つ目の `--` の前に `T` の字面で載る。
    #[test]
    fn mutants_diff_fail_fast_timeout_sits_after_baseline_skip_before_dashes() {
        for timeout_s in [20_u64, 21, 310] {
            let pace = Pace { jobs: 2, threads: 3, timeout_s };
            let (args, _) = measure_args(Path::new("d"), Path::new("o"), "p", pace, None);
            let skip_at = args.iter().position(|a| a == "--baseline").expect("--baseline が在る");
            let dashes = args.iter().position(|a| a == "--").expect("-- が在る");
            assert_eq!(
                &args[skip_at..dashes],
                ["--baseline", "skip", "--timeout", timeout_s.to_string().as_str()],
                "{args:?}"
            );
            assert_eq!(args.iter().filter(|a| *a == "--timeout").count(), 1, "{args:?}");
        }
    }

    /// (d) timeout の式 `max(20, ceil(5 × ms / 1000))`: 床・切り上げ・倍率が別々に落ちる 3 点。
    #[test]
    fn mutants_diff_fail_fast_timeout_is_max_20_ceil_5x_seconds() {
        assert_eq!(mutant_timeout_s(3000), 20, "床: 5 × 3 = 15 は 20 に上がる");
        assert_eq!(mutant_timeout_s(4020), 21, "切り上げ: 5 × 4.02 = 20.1 は 21（floor / round は 20）");
        assert_eq!(mutant_timeout_s(62000), 310, "倍率 5: 5 × 62 = 310");
    }

    /// (c) baseline の rc の判定の対: rc ≠ 0 は `Err`（字面に自前の log の末尾が載る）、rc 0 は `Ok`。
    #[test]
    fn mutants_diff_fail_fast_baseline_rc_decides_whether_mutants_run() {
        let log = "running 3 tests\ntest probe_tooth_4c1 ... FAILED\ntest result: FAILED. 2 passed; 1 failed\n";
        let red = baseline_judged(false, log).expect_err("rc ≠ 0 は測れていない");
        assert!(red.contains(BASELINE_TAIL_HEADING), "見出しが載る: {red}");
        assert!(red.contains("test probe_tooth_4c1 ... FAILED"), "自前の log の末尾が載る: {red}");
        assert!(red.ends_with("test result: FAILED. 2 passed; 1 failed"), "末尾で終わる: {red}");
        assert_eq!(baseline_judged(true, log), Ok(()), "rc 0 は cargo-mutants へ進む");
    }

    /// 空白区切りの 1 行を引数の列へ（`--teeth` の値に空白を入れる周は手で組む）。
    fn argv(line: &str) -> Vec<String> {
        line.split(' ').map(str::to_owned).collect()
    }

    /// 行 z の歯が使う 1 組の値（jobs / threads / timeout は別々の値＝取り違えが字面に出る）。
    const TEETH_PACE: Pace = Pace { jobs: 3, threads: 5, timeout_s: 41 };

    /// §34 (a) `--teeth a,b`: `--test-tool nextest` が `--timeout <T>` の直後に在り、1 つ目の `--` の後ろは
    /// `-E` `kind(lib) | kind(bin) | test(/^(a|b)/)` `--test-threads <t>` **だけ**（2 つ目の `--` は無い）。
    #[test]
    fn mutants_diff_teeth_named_words_run_under_nextest_filter() {
        let words = teeth_of(&argv("--base main --teeth probe_a_,probe_b_"))
            .expect("語の形は正しい")
            .expect("--teeth が在る");
        assert_eq!(words, ["probe_a_", "probe_b_"], "語は宣言順");
        let (args, scope) = measure_args(Path::new("d"), Path::new("o"), "p", TEETH_PACE, Some(&words));
        let timeout_at = args.iter().position(|a| a == "--timeout").expect("--timeout が在る");
        let dashes = args.iter().position(|a| a == "--").expect("-- が在る");
        assert_eq!(
            &args[timeout_at..dashes],
            ["--timeout", "41", "--test-tool", "nextest"],
            "--test-tool nextest は --timeout <T> の直後・-- の前: {args:?}"
        );
        assert_eq!(
            &args[dashes..],
            ["--", "-E", "kind(lib) | kind(bin) | test(/^(probe_a_|probe_b_)/)", "--test-threads", "5"],
            "-- の後ろはこれだけ: {args:?}"
        );
        assert_eq!(args.iter().filter(|a| *a == "--").count(), 1, "2 つ目の -- は無い: {args:?}");
        assert_eq!(scope.teeth(), "2", "語の数");
        // `--teeth` より前の cargo-mutants の引数は §33 の形と同じ列。
        let (plain, _) = measure_args(Path::new("d"), Path::new("o"), "p", TEETH_PACE, None);
        assert_eq!(&args[..timeout_at + 2], &plain[..timeout_at + 2], "--timeout <T> までは不変");
    }

    /// §34 (b) `--teeth -` は空＝式は `kind(lib) | kind(bin)` だけ（`test(…)` を足さない）。
    #[test]
    fn mutants_diff_teeth_dash_is_empty_and_runs_lib_and_bin_only() {
        let words = teeth_of(&argv("--base main --teeth -")).expect("- は正しい").expect("--teeth が在る");
        assert!(words.is_empty(), "- は空: {words:?}");
        let (args, scope) = measure_args(Path::new("d"), Path::new("o"), "p", TEETH_PACE, Some(&words));
        let dashes = args.iter().position(|a| a == "--").expect("-- が在る");
        assert_eq!(&args[dashes..], ["--", "-E", "kind(lib) | kind(bin)", "--test-threads", "5"], "{args:?}");
        assert!(args.iter().any(|a| a == "nextest"), "空でも nextest で走る: {args:?}");
        assert_eq!(scope.teeth(), "0", "空 = 0");
    }

    /// §34 (c) `--teeth` 無しは §33 の形と 1 語も違わない（`mutants_diff_fail_fast_` の pin と同じ列）。
    #[test]
    fn mutants_diff_teeth_absent_keeps_the_section_33_shape() {
        assert_eq!(teeth_of(&argv("--base main --jobs 3 --threads 5")), Ok(None), "無しは None");
        let (args, scope) = measure_args(Path::new("d"), Path::new("o"), "p", TEETH_PACE, None);
        assert_eq!(
            args,
            [
                "mutants", "--in-diff", "d", "-p", "p", "--no-shuffle", "--copy-vcs", "true", "-o", "o", "--jobs", "3",
                "--baseline", "skip", "--timeout", "41", "--", "--", "--test-threads", "5"
            ],
            "{args:?}"
        );
        assert!(!args.iter().any(|a| a == "--test-tool" || a == "-E"), "nextest を名指さない: {args:?}");
        assert_eq!(scope.teeth(), "-", "無し = -");
    }

    /// §34 (d) 語に `[A-Za-z0-9_]` 以外（空白・`.`・`/`）・空の語・値の無い flag は閉じた理由 1 つの `Err`。
    #[test]
    fn mutants_diff_teeth_malformed_words_are_unmeasured() {
        let spaced = vec!["--base".to_owned(), "main".to_owned(), "--teeth".to_owned(), "a_,b c".to_owned()];
        for args in [
            spaced,
            argv("--base main --teeth a_,b.c"),
            argv("--base main --teeth a_/b"),
            argv("--base main --teeth a_,,b_"),
            argv("--base main --teeth"),
            argv("--base main --teeth --jobs 2"),
        ] {
            assert_eq!(teeth_of(&args), Err(TEETH_MALFORMED.to_owned()), "{args:?}");
        }
        assert_eq!(teeth_of(&argv("--base main --teeth Ab_9")), Ok(Some(vec!["Ab_9".to_owned()])), "英数字と _ は通る");
    }

    /// §34 (e) 記録の行の末尾が `teeth=-` / `teeth=0` / `teeth=2` の 3 形（5 数と `scope=` は不変）。
    #[test]
    fn mutants_diff_teeth_record_line_carries_three_forms() {
        let counts = Counts { total: 9, caught: 5, missed: 2, unviable: 1, timeout: 1 };
        let two = ["x_".to_owned(), "y_".to_owned()];
        let empty: [String; 0] = [];
        for (teeth, tail) in [(None, "-"), (Some(&empty[..]), "0"), (Some(&two[..]), "2")] {
            let (_, scope) = measure_args(Path::new("d"), Path::new("o"), "probe-pkg-2e", TEETH_PACE, teeth);
            assert_eq!(
                counts.line(&scope, &[]),
                format!("mutants-diff: total=9 caught=5 missed=2 unviable=1 timeout=1 scope=probe-pkg-2e teeth={tail} outside=-"),
                "teeth={tail}"
            );
        }
    }

    // ---- 契約が名指した的を撃つ口（設計 gate-cost.md §16 (2)・行 g・接頭辞 `mutants_targets_`）----

    /// 的の file の本文（1 行 1 本）を読んだ列。
    fn aimed_targets(text: &str) -> Vec<Target> {
        targets_in(text).expect("的の形は正しい")
    }

    /// 的の 4 kind 分の数を持つ outcomes.json（入れ子の `outcomes[]` は分類に使わない）。
    fn outcomes_json(caught: u64, missed: u64, unviable: u64, timeout: u64) -> String {
        let total = caught + missed + unviable + timeout;
        format!(
            "{{\"outcomes\": [], \"total_mutants\": {total}, \"caught\": {caught}, \"missed\": {missed}, \
             \"unviable\": {unviable}, \"timeout\": {timeout}, \"success\": 0}}"
        )
    }

    /// (b) 的 3 本の fixture: 一覧に当たる 2 本が caught / missed・行のずれた 1 本だけが absent に落ち、5 値の和 = 的の本数。
    /// 同じ名の別の行（missed の一覧の 11 行目）はずれた的（12 行目）を拾わない。
    #[test]
    fn mutants_targets_three_targets_fall_into_five_values_summing_to_the_population() {
        let targets = aimed_targets(
            "src/probe.rs:7:replace probe_a -> bool with true\n\nsrc/probe.rs:11: replace probe_b with ()\nsrc/probe.rs:12:replace probe_b with ()\n",
        );
        assert_eq!(targets.len(), 3, "空行は数えない");
        let caught = "src/probe.rs:7:5: replace probe_a -> bool with true\nsrc/other.rs:1:1: replace other with ()\n";
        let missed = "src/probe.rs:11:9: replace probe_b with ()\n";
        let aimed = aimed_of(&targets, Some(&outcomes_json(2, 1, 0, 0)), [Some(caught), Some(missed), Some(""), Some("")], false)
            .expect("生存の在る非 0 は測定");
        assert_eq!(aimed.0, [Outcome::Caught, Outcome::Missed, Outcome::Absent], "的ごとの分類（宣言順）");
        let values: Vec<u64> = OUTCOMES.iter().map(|kind| aimed.count(*kind)).collect();
        assert_eq!(values, [1, 1, 0, 0, 1], "caught / missed / unviable / timeout / absent");
        assert_eq!(values.iter().sum::<u64>(), 3, "5 値の和 = 的の本数");
        let (_, scope) = measure_args(Path::new("d"), Path::new("o"), "probe-pkg-5c", TEETH_PACE, None);
        assert_eq!(
            aimed.line(&scope, &[]),
            "mutants-diff: total=3 caught=1 missed=1 unviable=0 timeout=0 absent=1 scope=probe-pkg-5c teeth=- population=targets outside=-"
        );
    }

    /// (b) 4 kind が全部出る fixture と、file・名のずれ（absent）: unviable / timeout も一覧から分類され、file 違い・
    /// 名違いの的は absent。一覧の不在は数が 0 の kind に限って不在のまま受ける。
    #[test]
    fn mutants_targets_every_kind_is_read_from_its_list_and_only_drift_is_absent() {
        let targets = aimed_targets(
            "src/k.rs:1:replace a with 0\nsrc/k.rs:2:replace b with 1\nsrc/k.rs:3:replace c with 2\nsrc/k.rs:4:replace d with 3\n\
             src/other.rs:1:replace a with 0\nsrc/k.rs:2:replace b with 9\n",
        );
        let lists = [
            Some("src/k.rs:1:3: replace a with 0\n"),
            Some("src/k.rs:2:3: replace b with 1\n"),
            Some("src/k.rs:3:3: replace c with 2\n"),
            Some("src/k.rs:4:3: replace d with 3\n"),
        ];
        let aimed = aimed_of(&targets, Some(&outcomes_json(1, 1, 1, 1)), lists, false).expect("測定");
        assert_eq!(
            aimed.0,
            [Outcome::Caught, Outcome::Missed, Outcome::Unviable, Outcome::Timeout, Outcome::Absent, Outcome::Absent],
            "4 kind + file・名のずれは absent"
        );
        let clean = aimed_of(&targets[..1], Some(&outcomes_json(1, 0, 0, 0)), [Some(lists[0].unwrap_or_default()), None, None, None], true)
            .expect("数 0 の kind の一覧の不在は測定のまま");
        assert_eq!(clean.0, [Outcome::Caught]);
    }

    /// (b) outcomes を読めない周は 5 値に化けず `Err`（測れていない側）: 壊れた outcomes.json・outcomes 不在で道具が
    /// 非 0・生存も時間切れも無い非 0（baseline の疑い）・数が 1 以上なのに一覧が無い。不在で道具が rc 0 の周
    /// （変異 0 本＝的がどれも現物に無い）だけは全部 absent の測定。
    #[test]
    fn mutants_targets_unreadable_outcomes_do_not_turn_into_five_values() {
        let targets = aimed_targets("src/k.rs:1:replace a with 0\nsrc/k.rs:2:replace b with 1\n");
        let empty = [Some(""), Some(""), Some(""), Some("")];
        assert!(aimed_of(&targets, Some("{\"total_mutants\": 2"), empty, true).is_err(), "壊れた outcomes.json");
        assert!(aimed_of(&targets, None, [None; 4], false).is_err(), "outcomes 不在の非 0");
        assert!(aimed_of(&targets, Some(&outcomes_json(2, 0, 0, 0)), empty, false).is_err(), "説明できない非 0");
        let listless = aimed_of(&targets, Some(&outcomes_json(0, 1, 0, 0)), [Some(""), None, Some(""), Some("")], false);
        let reason = listless.expect_err("数の在る kind の一覧の不在は測れていない");
        assert!(reason.contains("missed.txt"), "無い一覧を名指す: {reason}");
        let nothing = aimed_of(&targets, None, [None; 4], true).expect("変異 0 本の rc 0 は測定");
        assert_eq!(nothing.count(Outcome::Absent), 2, "的は全部 absent");
    }

    /// (b) 的の file の形: 形の外れた行・0 本は `Err`（測らずに rc 2）で、桁つきの一覧の行も的として読める。
    #[test]
    fn mutants_targets_malformed_or_empty_target_files_are_unmeasured() {
        for bad in ["src/k.rs\n", "src/k.txt:1:x\n", "src/k.rs:0:x\n", "src/k.rs:1:\n", "src/k.rs:x:y\n", "\n\n"] {
            assert!(targets_in(bad).is_err(), "{bad:?}");
        }
        let found = aimed_targets("src/k.rs:4:17: replace k -> u8 with 0\n");
        assert_eq!(
            found,
            [Target { file: "src/k.rs".to_owned(), line: 4, name: "replace k -> u8 with 0".to_owned() }],
            "桁は照合に使わない"
        );
        assert_eq!(targets_of(&argv("--base main --teeth -")), Ok(None), "--targets 無しは従来の形");
        assert!(targets_of(&argv("--base main --targets")).is_err(), "値の無い flag");
        assert!(targets_of(&argv("--base main --targets /nonexistent/probe-targets-9f")).is_err(), "読めない file");
    }

    /// (b) 的を絞った口の引数: `--in-diff <diff>` の対を落とし、1 つ目の `--` の直前に file ごとの `--file` と的ごとの
    /// `--re`（名の meta 文字を逃がした式）を置く。他の語は 1 語も変えない。
    #[test]
    fn mutants_targets_args_drop_in_diff_and_name_each_target() {
        let targets = aimed_targets("src/k.rs:1:replace a -> Vec<u8> with vec![]\nsrc/k.rs:2:replace b with ()\nsrc/j.rs:3:replace c.d with 1\n");
        let (plain, _) = measure_args(Path::new("d"), Path::new("o"), "p", TEETH_PACE, None);
        let args = aimed_args(plain.clone(), &targets);
        assert!(!args.iter().any(|a| a == "--in-diff" || a == "d"), "--in-diff の対は無い: {args:?}");
        let dashes = args.iter().position(|a| a == "--").expect("-- が在る");
        let from = args.iter().position(|a| a == "--file").expect("--file が在る");
        assert_eq!(
            &args[from..dashes],
            [
                "--file", "src/k.rs", "--file", "src/j.rs", "--re", r"replace a \-> Vec<u8> with vec!\[\]", "--re",
                r"replace b with \(\)", "--re", r"replace c\.d with 1"
            ],
            "{args:?}"
        );
        let kept: Vec<&String> = plain.iter().take(1).chain(plain.iter().skip(3)).collect();
        let rest: Vec<&String> = args[..from].iter().chain(&args[dashes..]).collect();
        assert_eq!(rest, kept, "他の語は不変: {args:?}");
    }

    // ---- 母集団の diff の受け口（設計 gate-cost.md §14 約束 2・歯 (e)・接頭辞 `mutants_in_diff_`）----

    /// `--diff <file>` の在る周は `git diff` を撃たずその file が `in.diff` になる: git repo でない dir・在らない base でも
    /// 渡した byte（空の母集団も）がそのまま写る。無い周は従来どおり `git diff` を撃つ（同じ dir では撃てず `Err`）。
    #[test]
    fn mutants_in_diff_given_file_becomes_the_population_without_git_diff() {
        let dir = std::env::temp_dir().join(format!("xtask-in-diff-given-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("tmp dir を作れる");
        let given = dir.join("population.diff");
        let body = "diff --git a/src/k.rs b/src/k.rs\n--- a/src/k.rs\n+++ b/src/k.rs\n@@ -3,0 +4,1 @@\n+mod m;\n";
        std::fs::write(&given, body).expect("母集団を書ける");
        let args = argv(&format!("--base no-such-base-9f --diff {}", given.display()));
        let bytes = diff_of(&args).expect("読める").expect("--diff が在る");
        assert_eq!(bytes, body.as_bytes(), "file の byte そのもの");
        let placed = dir.join("in.diff");
        assert_eq!(place_diff(&dir, "no-such-base-9f", Some(&bytes), &placed), Ok(()), "git diff を撃たない");
        assert_eq!(std::fs::read_to_string(&placed).expect("in.diff が在る"), body, "in.diff = 渡した母集団");
        assert_eq!(place_diff(&dir, "no-such-base-9f", Some(b""), &placed), Ok(()), "空の母集団も写す");
        assert_eq!(std::fs::read(&placed).expect("in.diff が在る"), b"", "0 行の母集団");
        assert!(place_diff(&dir, "no-such-base-9f", None, &placed).is_err(), "無い周は git diff を撃つ（repo でない dir では落ちる）");
        assert_eq!(diff_of(&argv("--base main --teeth -")), Ok(None), "--diff 無しは従来の形");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 値の無い flag・読めない file は測定未了（`Err`＝rc 2 の口）で断る。
    #[test]
    fn mutants_in_diff_unreadable_or_valueless_flag_is_unmeasured() {
        assert!(diff_of(&argv("--base main --diff")).is_err(), "値の無い flag");
        assert!(diff_of(&argv("--base main --diff --jobs 2")).is_err(), "値の位置に次の flag");
        let refused = diff_of(&argv("--base main --diff /nonexistent/probe-population-9f"));
        assert!(refused.as_ref().is_err_and(|reason| reason.contains("rc 2")), "読めない file: {refused:?}");
    }

    // ---- 範囲の外を名指す 1 語（設計 gate-cost.md §39・行 af・接頭辞 `mutants_diff_outside_`）----

    /// 宣言順の member の dir（core・task runner・3 つ目）。字面は実在の配置と衝突しない probe の名。
    fn outside_members() -> Vec<String> {
        ["probe-crates/core-4a", "probe-crates/runner-4a", "probe-crates/third-4a"].iter().map(|s| (*s).to_owned()).collect()
    }

    /// 1 file 分の diff の見出し（`--- a/` と `+++ b/`）。
    fn file_diff(path: &str) -> String {
        format!("diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -1,0 +2,1 @@\n+x\n")
    }

    /// core だけ・task runner だけ・両方・member の外だけ、の 4 形の diff（両方の形は runner を先に書く）。
    fn outside_shapes() -> [String; 4] {
        let core = file_diff("probe-crates/core-4a/src/k.rs");
        let runner = file_diff("probe-crates/runner-4a/src/main.rs");
        let doc = file_diff("docs/design/probe-4a.md");
        [core.clone(), runner.clone(), format!("{runner}{core}"), doc]
    }

    /// §39 (a) 形 1: 4 形で触れた member の dir が宣言順・重複無しで出る。member の外（設計 doc・名の接頭辞だけが
    /// 同じ dir）は数えず、同じ member の 2 file・追加と削除の片側 path は 1 件に畳む。
    #[test]
    fn mutants_diff_outside_touched_members_are_declared_order_without_duplicates() {
        let members = outside_members();
        assert_eq!(members.len(), 3, "母集団 = member dir 3 件");
        let expected: [&[&str]; 4] = [
            &["probe-crates/core-4a"],
            &["probe-crates/runner-4a"],
            &["probe-crates/core-4a", "probe-crates/runner-4a"],
            &[],
        ];
        for (diff, want) in outside_shapes().iter().zip(expected) {
            assert_eq!(touched_members(diff, &members), want, "{diff}");
        }
        let twice = format!(
            "{}{}diff --git a/probe-crates/core-4a/new.rs b/probe-crates/core-4a/new.rs\n--- /dev/null\n+++ b/probe-crates/core-4a/new.rs\n\
             --- a/probe-crates/third-4a/gone.rs\n+++ /dev/null\n{}",
            file_diff("probe-crates/core-4a/src/a.rs"),
            file_diff("probe-crates/core-4a/src/b.rs"),
            file_diff("probe-crates/core-4a-sibling/src/c.rs"),
        );
        assert_eq!(
            touched_members(&twice, &members),
            ["probe-crates/core-4a", "probe-crates/third-4a"],
            "同じ member の 3 file は 1 件・削除の片側も拾う・接頭辞だけ同じ dir は数えない"
        );
    }

    /// §39 (b) 形 2: (a) の 4 形から core の dir を除いた結果が、順に 0 件・1 件・1 件・0 件。
    #[test]
    fn mutants_diff_outside_drops_the_core_dir_from_the_touched_members() {
        let members = outside_members();
        let counts: Vec<usize> = outside_shapes()
            .iter()
            .map(|diff| outside_of(&touched_members(diff, &members), "probe-crates/core-4a").len())
            .collect();
        assert_eq!(counts, [0, 1, 1, 0], "core だけ・runner だけ・両方・member の外だけ");
        let both = outside_shapes()[2].clone();
        assert_eq!(outside_of(&touched_members(&both, &members), "probe-crates/core-4a"), ["probe-crates/runner-4a"], "残りは runner");
    }

    /// §39 (c) 形 3: 判定行の末尾が `outside=-` と `outside=<dir>` の 2 形で、先頭の 7 token は 2 形とも 1 字も同じ。
    /// 的を絞った周の行も同じ 1 語を末尾に持つ。
    #[test]
    fn mutants_diff_outside_record_line_appends_one_word_and_keeps_seven_tokens() {
        let counts = Counts { total: 0, caught: 0, missed: 0, unviable: 0, timeout: 0 };
        let (_, scope) = measure_args(Path::new("d"), Path::new("o"), "probe-pkg-4a", TEETH_PACE, None);
        let none = counts.line(&scope, &[]);
        let one = counts.line(&scope, &["probe-crates/runner-4a".to_owned()]);
        let head = "mutants-diff: total=0 caught=0 missed=0 unviable=0 timeout=0 scope=probe-pkg-4a teeth=-";
        assert_eq!(none, format!("{head} outside=-"), "0 件は -");
        assert_eq!(one, format!("{head} outside=probe-crates/runner-4a"), "1 件は dir");
        let tokens = |line: &str| line.split(' ').skip(1).map(str::to_owned).collect::<Vec<String>>();
        let (none_tokens, one_tokens) = (tokens(&none), tokens(&one));
        assert_eq!((none_tokens.len(), one_tokens.len()), (8, 8), "7 token + outside=");
        assert_eq!(none_tokens[..7], one_tokens[..7], "先頭の 7 token は 2 形とも同じ");
        assert_ne!(none, one, "母集団 0 と範囲の外だけの周は字面が違う");
        let two = counts.line(&scope, &["a-4a".to_owned(), "b-4a".to_owned()]);
        assert!(two.ends_with(" outside=a-4a,b-4a"), "複数は , で結ぶ: {two}");
        let aimed = Aimed(vec![Outcome::Absent]);
        assert!(aimed.line(&scope, &["probe-crates/runner-4a".to_owned()]).ends_with(" population=targets outside=probe-crates/runner-4a"));
    }
}
