//! runner を起動する**唯一の関数**（設計 §5.2・FR4 / FR6・憲法 C6）。
//!
//! [`spawn`] は [`Budget`] を引数に取り、`Budget` は [`super::Precheck::measure`] を
//! 消費してしか作れない。したがって「測らずに起動する」経路は型として存在しない。
//!
//! **scribe2 固有の env は 1 つも足さない**（C2.2・ADR-0004 §2.4）。runner へは
//! 親の env をそのまま継承させ、必要な値は cmd の placeholder 置換で渡す。

use super::approve::{block, Approval, Approve, RC_BLOCKED};
use super::cli::int_row;
use super::confine;
use super::follow::{gate_base, Halt, Resumption, Section, RUNNER_UNREACHABLE};
use super::declaration::{Effective, TablePlaces, DECL_FILE};
use super::gate::{fill_holes, last_json_object, record_checks, teeth_of, Counted, Limits, Logs, Shoot};
use super::land::MAIN_REF;
use super::refuse;
use super::table::{design_docs, read_table};
use super::{
    base_of_run, branch_name, contract_path, emit, git_bytes, git_line, plugin_path, record_cost, run_dir,
    runner_stderr_path, runner_stdout_path, vessel_path, worktree_path, Budget, Emit, Question, RC_QUESTION,
};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_REFUSED};
use crate::fleet::json_lite::{self, Value};
use crate::fleet::store::{acquire_with, append_line, lock_owner, owner_pid, read_all, started_ms, LockPolicy, Owner, Reclaim};
use crate::fleet::{Cost, CostSource, EventKind, Stage};
use crate::headless::runner::{stop_status, summary_usage, top_level_string};
use crate::headless::{NO_VALUE, RC_RATE_LIMIT, RC_UNREACHABLE};
use crate::name::{NAME, PLUGIN_DIR};
use crate::pipe::contract::Contract;
use crate::polarity::{OnFailure, Polarity, Timing};
use crate::rules::manifest::Manifest;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;

/// policy file の名前（guard が読む形・vessel-hook.md §5）。
const WRITE_SET_FILE: &str = "write-set.txt";

/// runner へ載せる plugin の中身（生成 dir [`PLUGIN_DIR`] 相対・設計 §6・consumer-sync.md §17 形 4）。
const PLUGIN_DIRS: [&str; 2] = [".claude-plugin", "hooks"];

/// 器の plugin manifest（`gen-manifest` の生成物＝tracked と同じ bytes・設計 §5.2 手順 5）。`include_str!` は literal
/// しか取れないので path の頭は [`PLUGIN_DIR`] の値の写しで、写しの bytes が [`PLUGIN_DIR`] の下の tracked と一致する
/// ことは e2e の `pipe_spawn_plugin_` / `plugin_payload_` の歯が [`PLUGIN_DIR`] から読んで測る。
const EMBEDDED_PLUGIN_JSON: &str = include_str!("../../../../plugin/.claude-plugin/plugin.json");

/// 器の hooks（[`EMBEDDED_PLUGIN_JSON`] と同じく生成物の埋め込み）。
const EMBEDDED_HOOKS_JSON: &str = include_str!("../../../../plugin/hooks/hooks.json");

/// 器の plugin として root の `<NAME>/` へ必ず書く 3 つ組（dir・file 名・本文）。
const EMBEDDED_PLUGIN: [(&str, &str, &str); 2] = [
    (".claude-plugin", "plugin.json", EMBEDDED_PLUGIN_JSON),
    ("hooks", "hooks.json", EMBEDDED_HOOKS_JSON),
];

/// consumer の plugin を写す root 配下の subdir 名。
const CONSUMER_DIR: &str = "consumer";

/// runner の写しの印の file の名の尾（名は [`NAME`] + この尾・中身は空・器は読まない・consumer-sync.md §19 形 3・
/// ADR-0082）。消費側の hook は `$CLAUDE_PLUGIN_ROOT` の下にこの名の file が在るかで runner の写しかを判じる。
const RUNNER_MARK_SUFFIX: &str = "-runner";

/// 終わりの門の間の印の名（run dir の直下・本文は driver の札と同じ `<pid> <起動時刻>`・設計 pipeline.md §66 形 9）。
pub(crate) const END_GATE_MARK: &str = "end-gate.pid";

/// 終わりの門の間の印の読み（[`end_gate_mark`] の返り・閉じた 3 値）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EndGateMark {
    /// 印が無い。
    Absent,
    /// 印は在るが持ち主が死んでいる（門を撃っていた process は居ない）。
    Dead,
    /// 断る周（持ち主が生きているか、印を読めない）。
    Held(Held),
}

/// [`EndGateMark::Held`] の中身（断る理由）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Held {
    /// 持ち主が生きている（その pid）。
    Alive(u32),
    /// 印を読めない（読めない本文・読めない probe・file の読み損ない）。fail-closed で断る側。
    Unreadable,
}

/// run dir の印 [`END_GATE_MARK`] の持ち主を読む（持ち主の生死は `lock_owner` と `started_ms`・読めない印は断る側）。
pub(crate) fn end_gate_mark(state_dir: &Path, id: &str) -> EndGateMark {
    let body = match std::fs::read_to_string(super::run_dir(state_dir, id).join(END_GATE_MARK)) {
        Ok(body) => body,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return EndGateMark::Absent,
        Err(_) => return EndGateMark::Held(Held::Unreadable),
    };
    match (lock_owner(&body, started_ms), owner_pid(&body)) {
        (Owner::Dead, _) => EndGateMark::Dead,
        (Owner::Live, Some(pid)) => EndGateMark::Held(Held::Alive(pid)),
        (Owner::Live | Owner::Unreadable, _) => EndGateMark::Held(Held::Unreadable),
    }
}

/// 終わりの門の record の file の名（run dir の直下・1 行の形は `verify.jsonl` と同じ・周の終わりに要約の 1 行を足す）。
const END_GATE_RECORD: &str = "end-gate.jsonl";

/// 終わりの門の赤い行の診断の file の名（`verify.stderr.log` と同じ形・周の頭に [`ROUND_HEAD`] の 1 行を置く）。
const END_GATE_STDERR: &str = "end-gate.stderr.log";

/// 診断 file の周の見出しの頭（`# end-gate=<周>`・段の見出し `## ` とは別の字面）。
const ROUND_HEAD: &str = "# end-gate=";

/// 門の赤の記帳の detail の頭（`end-gate:red:<周>:<赤い行の数>`・段は `Spawned` のまま）。
const RED_DETAIL: &str = "end-gate:red:";

/// 起こし直しの回数の上限を持つ rules 行（設計 pipeline.md §66 形 10）。
const ROW_END_GATE_ROUNDS: &str = "runner.end_gate_rounds";

/// 終わりの門の線（**閉じた 2 値**・設計 pipeline.md §66 形 8）: gate の線と起こし直しの回数の上限の対、か、読めない理由の 1 行。
/// 読めない周は門を撃たずに `Implemented` へ進める（要約の語 unmeasured）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndGate {
    /// 線を読めた。
    Line {
        /// gate と同じ線（[`Limits::of`]）。
        limits: Limits,
        /// 起こし直しの回数の上限（rules 行 `runner.end_gate_rounds`）。
        rounds: u64,
    },
    /// 線を読めない（行の無い・不発効・型違いの manifest）。値は理由の 1 行。
    Unreadable(String),
}

impl EndGate {
    /// 組む周の manifest から線を組む（**`Runner` を組む口はすべてこの 1 本**）。
    pub fn of(manifest: &Manifest) -> Self {
        match (Limits::of(manifest), int_row(manifest, ROW_END_GATE_ROUNDS)) {
            (Ok(limits), Ok(rounds)) => Self::Line { limits, rounds },
            (Err(reason), _) | (_, Err(reason)) => Self::Unreadable(reason),
        }
    }
}

/// 門の間の印を外す番（[`spawn_turn`](super::follow::spawn_turn) が輪の外側に 1 つ持つ・設計 pipeline.md §66 形 9）。
///
/// 外すのは先頭の語が自分の pid の印だけ（driver の札の `Drop` と同じ形）。印を置くのは門を撃ち始める周
/// （[`hold_mark`]）で、置かれなかった周の `Drop` は何もしない。
pub(crate) struct EndGateHold {
    /// 印の path。
    path: PathBuf,
}

impl EndGateHold {
    /// 印の外し番を持つ（印は置かない）。
    pub(crate) fn new(state_dir: &Path, run: &str) -> Self {
        Self { path: run_dir(state_dir, run).join(END_GATE_MARK) }
    }
}

impl Drop for EndGateHold {
    fn drop(&mut self) {
        if mark_is_mine(&self.path) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// 印の先頭の語が自分の pid か。
fn mark_is_mine(path: &Path) -> bool {
    std::fs::read_to_string(path).is_ok_and(|body| owner_pid(&body) == Some(std::process::id()))
}

/// 門の間の印を置く（driver の札と同じ原子的な置き方・自分の pid の印が既に在る周〔前の周の門〕はそのまま使う）。
fn hold_mark(launch: &Launch<'_>) -> Result<(), String> {
    let path = run_dir(launch.state_dir, launch.run).join(END_GATE_MARK);
    if mark_is_mine(&path) {
        return Ok(());
    }
    acquire_with(&path, launch.policy, Reclaim::DeadOnly)
        .map(|_| ())
        .map_err(|err| format!("{} を置けない: {err}", path.display()))
}

/// 前の周の門の赤（診断 file が持つ最新の周の赤い行）。
pub struct GateRed {
    /// 赤かった周の番号。
    pub round: u64,
    /// 赤い行（診断 file の順）。
    rows: Vec<RedRow>,
}

/// 赤い行 1 つ（撃った行の字・rc・診断の抜粋）。
struct RedRow {
    /// 撃った行の字（置換後）。
    cmd: String,
    /// rc の字面。
    rc: String,
    /// 診断の抜粋（段の stderr の写し）。
    text: String,
}

/// 診断の抜粋を 1 行ごとに残す行数（末尾から数える・窓の大きさで判定の閾値ではない＝rules 行にしない・`STDERR_TAIL_LINES` と同じ読み）。
const RED_LINES_PER_ROW: usize = 40;

/// 「門の赤」節に載せる抜粋の合計の字数の上限（上と同じ読み）。
const RED_CHARS_TOTAL: usize = 16000;

impl GateRed {
    /// 診断 file の最新の周（最後の [`ROUND_HEAD`] の行より後ろ）の赤い行を読む（読めない周は行が空＝節は 1 文だけ残る）。
    pub(crate) fn read(state_dir: &Path, run: &str, round: u64) -> Self {
        let log = std::fs::read_to_string(run_dir(state_dir, run).join(END_GATE_STDERR)).unwrap_or_default();
        let lines: Vec<&str> = log.lines().collect();
        let from = lines.iter().rposition(|line| line.starts_with(ROUND_HEAD)).map_or(0, |at| at.saturating_add(1));
        let mut rows: Vec<RedRow> = Vec::new();
        for line in lines.get(from..).unwrap_or_default() {
            if let Some(row) = RedRow::heading(line) {
                rows.push(row);
            } else if let Some(row) = rows.last_mut() {
                row.text.push_str(line);
                row.text.push('\n');
            }
        }
        Self { round, rows }
    }

    /// 「門の赤」節（stdin に足す字・1 行目は周の番号と 1 文）。
    fn section(&self) -> String {
        let mut body = format!("\n## 門の赤\n- 周 {}: 器が撃った次の行が赤い。直して commit してから終える\n", self.round);
        let (mut used, mut dropped) = (0_usize, 0_usize);
        for row in &self.rows {
            let room = RED_CHARS_TOTAL.saturating_sub(used);
            if room == 0 {
                dropped = dropped.saturating_add(1);
                continue;
            }
            let item = row.render(room);
            used = used.saturating_add(item.chars().count());
            body.push_str(&item);
        }
        if dropped > 0 {
            body.push_str(&format!("- 字数の上限で省いた赤い行: {dropped} 行\n"));
        }
        body
    }
}

impl RedRow {
    /// 診断の見出し（`## n=<n> rc=<rc> cmd=<cmd>`）から行を起こす（見出しでない行は `None`）。
    fn heading(line: &str) -> Option<Self> {
        let (_, rest) = line.strip_prefix("## n=")?.split_once(" rc=")?;
        let (rc, cmd) = rest.split_once(" cmd=")?;
        Some(Self { cmd: cmd.to_owned(), rc: rc.to_owned(), text: String::new() })
    }

    /// 節の 1 項目（撃った行の字・rc・診断の末尾 [`RED_LINES_PER_ROW`] 行・`room` 字まで）。
    fn render(&self, room: usize) -> String {
        let lines: Vec<&str> = self.text.lines().collect();
        let from = lines.len().saturating_sub(RED_LINES_PER_ROW);
        let mut item = format!("- `{}` rc={}\n", self.cmd, self.rc);
        if from > 0 {
            item.push_str(&format!("  （診断の先頭 {from} 行は省いた）\n"));
        }
        for line in lines.get(from..).unwrap_or_default() {
            item.push_str("  | ");
            item.push_str(line);
            item.push('\n');
        }
        let mut cut: String = item.chars().take(room).collect();
        if !cut.ends_with('\n') {
            cut.push('\n');
        }
        cut
    }
}

/// 便の最新の `RunStage` が門の赤なら、その周の番号（event log から読む・読めない周と門の赤でない周は `None`）。
pub(crate) fn red_round(state_dir: &Path, run: &str) -> Option<u64> {
    let events = read_all(state_dir).ok()?;
    let last = events.iter().rev().find(|event| event.run == run && event.kind == EventKind::RunStage)?;
    let rest = last.detail.as_deref()?.strip_prefix(RED_DETAIL)?;
    if last.stage != Some(Stage::Spawned) {
        return None;
    }
    rest.split(':').next()?.parse().ok()
}

/// 門の赤の数（設計 pipeline.md §66 形 6）: 便の `RunStage` を畳み、`Spawned` でない最新の段より後ろの、detail が
/// [`RED_DETAIL`] で始まる記帳の件数。読めない周は `None`。
fn red_count(state_dir: &Path, run: &str) -> Option<u64> {
    let events = read_all(state_dir).ok()?;
    let mut count = 0_u64;
    for event in events.iter().filter(|event| event.run == run && event.kind == EventKind::RunStage) {
        if event.stage != Some(Stage::Spawned) {
            count = 0;
        } else if event.detail.as_deref().is_some_and(|detail| detail.starts_with(RED_DETAIL)) {
            count = count.saturating_add(1);
        }
    }
    Some(count)
}

/// 門の 1 周の結果（**閉じた 4 値**・要約の行の `result` の語）。
enum Round {
    /// 全行 rc 0。
    Green,
    /// 赤が在り、起こし直す（値は赤い行の数）。
    Red(u64),
    /// 赤が在り、起こし直しの数が上限に届いた。
    Exhausted,
    /// 測れなかった（値は理由の 1 行）。
    Unmeasured(String),
}

impl Round {
    /// 要約の行の語。
    fn word(&self) -> &'static str {
        match self {
            Self::Green => "green",
            Self::Red(_) => "red",
            Self::Exhausted => "exhausted",
            Self::Unmeasured(_) => "unmeasured",
        }
    }

    /// 要約の 1 行（`{"end_gate":<周>,"result":"<語>"}`・測れなかった周は `reason` を足す）。
    fn summary(&self, round: u64) -> String {
        let mut fields = vec![("end_gate", Value::Num(round)), ("result", Value::Str(self.word().to_owned()))];
        if let Self::Unmeasured(reason) = self {
            fields.push(("reason", Value::Str(reason.clone())));
        }
        json_lite::write_object(&fields)
    }

    /// 撃った結果の数えから周の結果を決める（**測れなかったは赤より先**・赤は上限の内だけ起こし直す）。
    fn judge(counted: &Counted, prior: u64, rounds: u64) -> Self {
        if counted.unreadable {
            return Self::Unmeasured("diff の path を読めない（write-set を照合できない）".to_owned());
        }
        if let Some(reason) = counted.killed {
            return Self::Unmeasured(format!("verify の行が scope の中で死んだ（reason={}）", reason.as_str()));
        }
        if let Some(n) = counted.busy {
            return Self::Unmeasured(format!("host が混んだまま待ちの上限を超えた（n={n} 以後の行を撃っていない）"));
        }
        match counted.red {
            0 => Self::Green,
            red if prior < rounds => Self::Red(red),
            _ => Self::Exhausted,
        }
    }
}

/// 門を撃つ（印・base・契約の写しを揃えて gate と同じ 1 本で撃つ・設計 pipeline.md §66 形 2）。`Err` は測れなかった理由の 1 行。
fn shoot_gate(launch: &Launch<'_>, worktree: &Path, limits: Limits, round: u64) -> Result<Counted, String> {
    hold_mark(launch)?;
    let base = gate_base(launch.state_dir, launch.repo, launch.run)
        .ok_or_else(|| format!("run {} の base を読めない", launch.run))?;
    // 契約は撃つ時に便の写しから読み直す（`Launch` の契約は land の起こし直しで広げる前の値のことが在る）。
    let path = contract_path(launch.state_dir, launch.run);
    let contract = Contract::load(&path).map_err(|errors| {
        let lines: Vec<String> = errors.iter().map(ToString::to_string).collect();
        format!("{} を読めない: {}", path.display(), lines.join(" / "))
    })?;
    let dir = run_dir(launch.state_dir, launch.run);
    let (record, tail) = (dir.join(END_GATE_RECORD), dir.join(END_GATE_STDERR));
    append_line(&tail, &format!("{ROUND_HEAD}{round}"), launch.policy).map_err(|err| err.to_string())?;
    let shoot = Shoot { state_dir: launch.state_dir, run: launch.run, contract: &contract, limits, policy: launch.policy };
    record_checks(&shoot, worktree, &base, &Logs { record: &record, tail: &tail })
}

/// runner が rc 0 で commit を作って終わった周の門（設計 pipeline.md §66 形 1〜5）: 撃って要約を残し、赤で起こし直せる周は
/// 段を `Spawned` のまま門の赤を記帳し、ほかは `Implemented`（detail なし）にする。
fn end_gate(launch: &Launch<'_>, worktree: &Path) -> Outcome {
    let prior = red_count(launch.state_dir, launch.run);
    let round = prior.unwrap_or(0).saturating_add(1);
    let result = match (launch.gate, prior) {
        (EndGate::Unreadable(reason), _) => Round::Unmeasured(reason.clone()),
        (EndGate::Line { .. }, None) => Round::Unmeasured("便の event を読めない".to_owned()),
        (EndGate::Line { limits, rounds }, Some(prior)) => shoot_gate(launch, worktree, *limits, round)
            .map_or_else(Round::Unmeasured, |counted| Round::judge(&counted, prior, *rounds)),
    };
    // 要約の行も書けない周は書かずに進む（門の結果は段の記帳が持つ）。
    let record = run_dir(launch.state_dir, launch.run).join(END_GATE_RECORD);
    let _ = append_line(&record, &result.summary(round), launch.policy);
    match result {
        Round::Red(red) => record_stage(launch, Stage::Spawned, Some(format!("{RED_DETAIL}{round}:{red}"))),
        Round::Green | Round::Exhausted | Round::Unmeasured(_) => record_stage(launch, Stage::Implemented, None),
    }
}

/// 直前の便の gate の FAIL（設計 pipeline.md §68）: 便 id と、節の本文の行（器は分類せず写すだけ）。
pub struct PriorFail {
    /// 直前の便の run id。
    pub run: String,
    /// 本文の行（`verdict.json` を読めた周は evidence・切った字数・findings、読めない周は理由の 1 行）。
    pub lines: Vec<String>,
}

impl PriorFail {
    /// 「前の便の gate の FAIL」節（stdin に足す字・1 行目は直前の便の run id と 1 文）。
    fn section(&self) -> String {
        let mut body = format!(
            "\n## 前の便の gate の FAIL\n- {}: 同じ契約の前の便は gate で次の理由で落ちた。同じ穴を作らない\n",
            self.run
        );
        for line in &self.lines {
            body.push_str(&format!("- {line}\n"));
        }
        body
    }
}

/// 起動 1 回の材料。
pub struct Launch<'a> {
    /// 便 id。
    pub run: &'a str,
    /// 契約の bead id。
    pub bead: &'a str,
    /// 対象 repo。
    pub repo: &'a Path,
    /// 置き場。
    pub state_dir: &'a Path,
    /// 読み込み済みの契約。
    pub contract: &'a Contract,
    /// runner のコマンド（placeholder を含む）。
    pub runner: &'a str,
    /// 承認 event が在るか（replay の導出値）。
    pub approved: bool,
    /// **回答済みの質問**（`Questioned` からの再 spawn だけが持つ・設計 pipeline-question.md §5）。
    /// 在る周は同じ run の worktree と base を使い、runner の stdin に「回答」節を付ける。
    pub answered: Option<Question>,
    /// **追随の相手**（main と便の base の 2 sha・便の base が main と違い追随の形が在る〔祖先か merge-base が在る〕
    /// 周だけ・設計 pipeline-conflict.md §3・pipeline.md §38）。在る周は同じ run の worktree と base を使い、runner の
    /// stdin に「追随」節（`git rebase --onto <main> <base>` の指示）を付ける。値の出所は [`super::follow::section`]
    /// ただ 1 本である（便の消した path を名指す契約表の行の一覧〔設計 pipeline.md §34〕も同じ値が運ぶ）。
    pub follow: Option<Section>,
    /// **途中再開**（上限で止まった `RateLimited` からの再 spawn と、runner が死んだ `Spawned` からの再 spawn
    /// だけが持つ・設計 account-autonomy.md §4）。在る周は同じ run の worktree と base を使い、runner の stdin に
    /// 「途中再開」節を付ける。値の出所は [`super::follow::resumption`] ただ 1 本で、止まった理由（[`Halt`]）も
    /// そこから来る（[`Account::Resumed`] の detail の印はこの理由で分かれる）。
    pub resumed: Option<Resumption>,
    /// runner を起こす口座（閉じた 3 値・ADR-0017 §2.3・設計 account-autonomy.md §4）。label を持つ周は runner の
    /// 行に `--account-dir <state_dir>/accounts/<label>` を足し、[`Account::Inherit`] は親の環境をそのまま継承させる。
    pub account: Account<'a>,
    /// 終わりの門の線（gate の線と起こし直しの回数の上限・設計 pipeline.md §66 形 8）。
    pub gate: &'a EndGate,
    /// **前の周の門の赤**（輪の 2 周目以降だけ持つ・設計 pipeline.md §66 形 7）。在る周は同じ run の worktree と base を使い、
    /// runner の stdin に「門の赤」節を付ける。
    pub red: Option<GateRed>,
    /// **直前の便の gate の FAIL**（同じ bead の直前の便が gate の FAIL で終端し契約 file の字が同じ周だけ・設計 pipeline.md §68）。
    /// runner の stdin に「前の便の gate の FAIL」節を付ける。値の出所は [`super::follow::prior_fail`] ただ 1 本である。
    pub prior_fail: Option<PriorFail>,
    /// lock の待ち方。
    pub policy: LockPolicy,
}

/// runner を起こす口座（**閉じた 3 値**・設計 account-autonomy.md §4・FR36 / FR37）。
///
/// 段の detail の形は variant ごとに固定である: [`Inherit`](Self::Inherit) は `base:<sha>`、
/// [`Chosen`](Self::Chosen) は `base:<sha>,account:<label>`、[`Resumed`](Self::Resumed) は
/// `account:<label>,resume:rate-limit` / `account:<label>,resume:runner-dead` / `account:<label>,resume:unreachable`（印は止まった理由 [`Halt`] で
/// 分かれる・base は初回の行が持ったまま）。読み手（`base_of_run`）は `base:` の直後から最初の `,` までを sha と読む。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Account<'a> {
    /// 口座の宣言が無い周: 親の環境をそのまま継承させる（どの口座かを器は知らない）。
    Inherit,
    /// 初回の起動・承認後・回答後・衝突の起こし直しで器が便用の規則で選んだ口座。
    Chosen(&'a str),
    /// 途中再開（上限で止まった便の別口座での起こし直し・runner が死んだ便の起こし直し）で器が選んだ口座。
    Resumed(&'a str),
}

impl<'a> Account<'a> {
    /// 器が選んだ label（[`Self::Inherit`] は `None`）。`--account-dir` を足すかと `RateLimited` の detail の
    /// label はこの 1 本で決まる（選んだ経路の違いは見ない）。
    fn label(self) -> Option<&'a str> {
        match self {
            Self::Inherit => None,
            Self::Chosen(label) | Self::Resumed(label) => Some(label),
        }
    }
}

/// 口座を渡していない周に段の detail へ書く label の代わり（閉じた 1 つ）。
const INHERITED_ACCOUNT: &str = "inherited";

/// 上限で止まった便の別口座での起こし直しを段の detail に名乗る印（`Spawned detail=account:<label>,resume:rate-limit`）。
const RESUME_RATE_LIMIT: &str = "resume:rate-limit";

/// runner が死んだ便の起こし直しを段の detail に名乗る印（`Spawned detail=account:<label>,resume:runner-dead`）。
const RESUME_RUNNER_DEAD: &str = "resume:runner-dead";

/// API に届かず止まった便の起こし直しを段の detail に名乗る印（`Spawned detail=account:<label>,resume:unreachable`）。
const RESUME_UNREACHABLE: &str = "resume:unreachable";

/// 途中再開の印（閉じた 3 つ・理由 [`Halt`] の値ごとに固定）。
fn resume_mark(halt: Halt) -> &'static str {
    match halt {
        Halt::RateLimit => RESUME_RATE_LIMIT,
        Halt::RunnerDead => RESUME_RUNNER_DEAD,
        Halt::Unreachable => RESUME_UNREACHABLE,
    }
}

/// runner を起動して結果まで見届ける。**これが唯一の起動口である**。
pub fn spawn(budget: Budget, launch: &Launch<'_>) -> Outcome {
    let _ = budget.write_set();
    // **A1「実行前」の関門はここに置く**（設計 §5.5）。起動口が 1 本なので、この 1 行が
    // spawn / resume / run のすべての経路を覆う。呼び手側に置くと経路が増えるたびに
    // 素通りの穴が空く。
    match Approval::judge(launch.contract, launch.approved) {
        Approval::Required(classes) => return block(&approval(launch), classes),
        Approval::Granted => {}
    }
    let (worktree, base) = match prepare_worktree(launch) {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    let write_set = match write_policy(&worktree, &launch.contract.write_set) {
        Ok(path) => path,
        Err(reason) => return broken(reason),
    };
    let plugin = match copy_plugin(&worktree, launch.state_dir, launch.run) {
        Ok(path) => path,
        Err(reason) => return broken(reason),
    };
    // **起動行はここで組み上げる**（`Spawned` の記帳より前）。口座の断り（[`LineRefusal`]）を
    // `launch_runner` に置くと、段を記帳した後で起こさない周ができる——記帳した口座と実行が
    // 一致しない行が置き場に残る。worktree は作ったまま（`prepare_worktree` の後の断りと同じ形）。
    let line = substitute(launch, &worktree, &write_set, &plugin, &base);
    let cmd = match with_account(line, launch.account.label(), launch.state_dir) {
        Ok(line) => line,
        Err(refusal) => return refused(refusal.to_string()),
    };
    // 途中再開は `account:<label>,resume:<理由>` を名乗る（設計 account-autonomy.md §4・印は止まった理由で分かれる）。
    // base は初回の `base:<sha>` が持ったままで、読み手（`base_of_run`）は接頭辞の違う行を飛ばす。
    // 器が選んだ口座での起動は `base:<sha>,account:<label>`（読み手は最初の `,` までを sha と読む）。
    // 理由を読めない途中再開（節の材料が無い）は起こさない（印を推量しない・fail-closed）。
    let detail = match spawned_detail(launch, &base) {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    if let Err(err) = emit(
        launch.state_dir,
        &Emit {
            kind: EventKind::RunStage,
            run: launch.run,
            bead: launch.bead,
            stage: Some(Stage::Spawned),
            seat: None,
            pid: None,
            detail: Some(detail),
        },
        launch.policy,
    ) {
        return broken(err.to_string());
    }
    launch_runner(launch, &worktree, &cmd, &base)
}

/// `Spawned` の記帳の detail（**閉じた形**）: 門の赤の周は `end-gate:<周>`（口座が在れば `,account:<label>`・`base:` で始めない＝
/// base の読み手が飛ばす形・設計 pipeline.md §66 形 8）、ほかは口座の 3 値で分かれる。理由を読めない途中再開は `Err`。
fn spawned_detail(launch: &Launch<'_>, base: &str) -> Result<String, String> {
    if let Some(red) = &launch.red {
        let account = launch.account.label().map(|label| format!(",account:{label}")).unwrap_or_default();
        return Ok(format!("end-gate:{}{account}", red.round));
    }
    match (launch.account, launch.resumed.as_ref()) {
        (Account::Inherit, _) => Ok(format!("base:{base}")),
        (Account::Chosen(label), _) => Ok(format!("base:{base},account:{label}")),
        (Account::Resumed(label), Some(resumed)) => Ok(format!("account:{label},{}", resume_mark(resumed.halt))),
        (Account::Resumed(_), None) => Err(format!("run {} の途中再開の理由を読めない", launch.run)),
    }
}

/// runner を起こし、終わりまで見届けて段を決める（起動行は [`spawn`] が組み上げて渡す）。
fn launch_runner(launch: &Launch<'_>, worktree: &Path, cmd: &str, base: &str) -> Outcome {
    // **turn 開始時の tip**（ADR-0019 §2.6）。質問の判定はこの点からの commit 数で見る
    // ——base 基準だと、起こし直しの turn は便が base から持つ commit を数えてしまい、
    // 質問で止まった turn が常に「commit を作った」側へ倒れる。初回は tip = base ゆえ同値。
    // 読めない周は base へ落とす（従来の基準）。
    let tip = git_line(worktree, &["rev-parse", "HEAD"]).unwrap_or_else(|| base.to_owned());
    // **runner も cgroup の scope で包む**（設計 gate-cost.md §4.1 の 2 つ目）。箱は
    // 1 × `gate.job_memory_mb`（同 §12・裁定 id user 2026-09-15T18:2xZ）で、包めない host では
    // 素のまま撃つ（止めない・縮退する）。
    let unit = confine::unit_name(launch.run, RUNNER_STAGE, 1);
    let wrap = confine::Wrap { unit: &unit, limit: confine::Limit::PerJob(1), caps: confine::Caps::embedded(), width: None };
    let (mut command, confinement) = confine::wrap_line(cmd, &wrap);
    // **env を 1 つも足さない**: `.env()` / `.envs()` を呼ばず親の env をそのまま継承する（`TMUX_PANE` だけは外す＝confine）。
    // stdout は捕らえる（質問 record の読み面・`gate.rs::ask_lens` と同じ形）。stderr も同じ形で捕らえる
    // （起動の失敗の理由を run dir に残す・設計 dispatcher.md §12）——継承のままだと、列が起こした端末の無い
    // driver では理由の 1 行がどこにも残らない。捕らえた分は終端で呼び手の stderr へそのまま流す（[`relay_stderr`]）。
    // **先頭 process を新しい process group の leader にする**（setsid ではない・cgroup の scope とは
    // 独立）。`SeatSpawned` の pid はそのまま group id として読まれ、`pipe stop` は group 宛てに
    // 撃つ＝wrapper だけが死んで runner や claude が残る形を塞ぐ（設計 §5.6）。
    let child = command
        .process_group(0)
        .current_dir(worktree)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(found) => found,
        Err(err) => return broken(format!("runner を起動できない: {err}")),
    };
    let pid = u64::from(child.id());
    if let Err(err) = seat(launch, EventKind::SeatSpawned, Some(pid), None) {
        return broken(err);
    }
    if let Some(mut stdin) = child.stdin.take() {
        // 読まずに終える runner への write は EPIPE になる。**段は rc と stdout で決める**ので
        // ここの失敗は理由にしない（take で drop され、runner は EOF を見る）。
        let _ = stdin.write_all(prompt(launch, base).as_bytes());
    }
    // rc が要るのでここは `wait_with_output`（`Child::wait` と同じ待ち・stdout を回収する形）。
    // pid の生存だけを見る待機（`pipe stop`）は `fleet::wait` のままで、**待機の実装は
    // 増えていない**（C3.4）。
    let waited = child.wait_with_output();
    // **終端で scope を片付ける**（設計 gate-cost.md §4.4 errata・`s2-07l.234`）。runner が孤児を
    // 残しても scope を active のまま置かない。段の判定は変えない（結果は stderr の 1 行だけ）。
    let scope = confine::release_scope(&confinement).map(|released| format!("pipe: runner scope={}", released.as_str()));
    let out = match waited {
        Ok(found) => found,
        Err(err) => return broken(format!("runner の終了を待てない: {err}")),
    };
    let rc = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stopping = super::is_stopping(launch.state_dir, launch.run) == Some(true);
    let killed = box_killed(&confinement, rc, &stdout);
    // **API に届かず止まった周は `SeatStopped` に理由を載せる**（設計 account-autonomy.md §17 (2)）: 段は `Spawned` の
    // まま・この 1 件が途中再開の材料（[`super::follow::resumption`]）になる。停止中と箱の中の死は従来の終端が勝つ。
    let unreachable = (rc == i32::from(RC_UNREACHABLE) && !stopping && killed.is_none()).then_some(RUNNER_UNREACHABLE);
    if let Err(err) = seat(launch, EventKind::SeatStopped, Some(pid), unreachable.map(str::to_owned)) {
        return broken(err);
    }
    // 捕らえた stdout は診断 file へ残す（包みの観測行を端末から消さない）。書けない周は
    // 段の判定を変えない（stderr 1 行で loud）。
    let kept = keep_stdout(launch, rc, &stdout).err().map(|reason| format!("pipe: runner の stdout を残せない: {reason}"));
    // 捕らえた stderr も同じ形で並べて残し（設計 dispatcher.md §12）、呼び手の stderr へそのまま流す。
    // **段の判定の入力にはしない**（判定は rc と commit の数だけ・C3.3）。
    let kept_err = keep_stderr(launch, rc, &stderr).err().map(|reason| format!("pipe: runner の stderr を残せない: {reason}"));
    relay_stderr(&out.stderr);
    // **runner の消費は段の event の前に 1 件**（設計 gate-cost.md §26 形 (2)）。停止中の便は段と同じく書かない。
    let cost = if stopping { None } else { runner_cost(launch, &stdout) };
    let mut outcome = if stopping {
        // **停止中の便は段を 1 件も書かない**（設計 pipeline.md §23）。runner を消したのは `pipe stop` で、
        // 終端は `RunStopped` の経路が書く——ここで `Failed` を書くと stop の終端を上書きする。
        stopped_underneath(launch)
    } else if let Some(reason) = killed {
        // **箱の中で死んだ周は便を終端する**（設計 gate-cost.md §4.2）。verify 行の「測れなかった」
        // とは極性が違う——便の内容が測れないのではなく、便自身が箱の中で死んだ。
        // 理由は閉じた語彙の 1 つ（`runner-rc` と同じ終端の段）で、`Failed` から resume しない。
        record_stage(launch, Stage::Failed, Some(reason.as_str().to_owned()))
    } else if rc == i32::from(RC_QUESTION) {
        settle_question(launch, worktree, &tip, &stdout)
    } else if rc == i32::from(RC_RATE_LIMIT) {
        settle_rate_limit(launch, rc, &stdout)
    } else if unreachable.is_some() {
        settle_unreachable(launch)
    } else {
        // **rc が 76 / 75 / 77 でない周は最終行を読まない**（従来どおり）。
        settle(launch, worktree, base, rc)
    };
    outcome.err.extend(kept.into_iter().chain(kept_err).chain(cost).chain(scope));
    outcome
}

/// runner の要約行（[`summary_usage`]）の消費の 6 値を 1 件書く（揃わない周は書かない・書けない周の理由は stderr の
/// 1 行で返す＝段の判定と rc は変えない）。
fn runner_cost(launch: &Launch<'_>, stdout: &str) -> Option<String> {
    let cost = summary_usage(stdout).map(|usage| Cost { source: CostSource::Runner, usage });
    record_cost(launch.state_dir, (launch.run, launch.bead), cost, launch.policy)
}

/// 捕らえた runner の stderr を呼び手の stderr へ**そのまま**流す（設計 dispatcher.md §12「手で撃った周の
/// 見え方を変えない」）。
///
/// [`Outcome::err`] の行にしないのは 2 つの理由——(1) 行にすると `pipe:` の行と同じ層に混ざり、runner が書いた
/// byte 列（改行の有無・prefix）が変わる。(2) 連鎖（`pipe run`）は rc 0 で終わった段の stderr の行を次の段へ
/// 持ち越さないので、`Failed` に着いた便（spawn の rc は 0）の理由が呼び手に届かない。継承していたときと同じ
/// byte 列を同じ stream へ書く＝出力層の行の形（1 行 1 `eprintln`）ではなく stream の中継である。
/// 書けない周は黙る（理由は run dir の log に残っている・段の判定を変えない）。
fn relay_stderr(bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    let mut stderr = std::io::stderr().lock();
    let _ = stderr.write_all(bytes);
    let _ = stderr.flush();
}

/// runner の scope の unit 名に載せる段の名。
const RUNNER_STAGE: &str = "runner";

/// runner の包みが箱の中で死んだ理由（設計 gate-cost.md §4.2 / §4.3・pipeline.md §23）。
///
/// **oom-kill は kernel の証拠がある周だけ**＝包みが stdout の終端に出した `oom_kill` が 1 以上。
/// 終端行が在って `oom_kill` が 0 の周は箱の中の死と読まず `None`（従来の settle へ落ちる）。
/// 終端行が無い / 読めない周の **signal 死**（rc が無い＝`code()` が `None` の周・器は -1 と記す）は
/// 証拠が無いので [`confine::Reason::Unknown`]（外からの kill を oom-kill に化けさせない・C10）。
/// **包めなかった周は当たらない**。
fn box_killed(confinement: &confine::Confinement, rc: i32, stdout: &str) -> Option<confine::Reason> {
    if !confinement.confined() {
        return None;
    }
    match confine::read_usage(stdout).oom_kill {
        Some(count) => (count >= 1).then_some(confine::Reason::OomKill),
        None => (rc < 0).then_some(confine::Reason::Unknown),
    }
}

/// 停止中の便で runner が消えた周（設計 pipeline.md §23）: 段は書かず（`SeatStopped` は書き済み）、終端を
/// `pipe stop` の `RunStopped` に任せて rc 1 で止まる（呼び手が次の段へ進まない）。
fn stopped_underneath(launch: &Launch<'_>) -> Outcome {
    Outcome::failed_line(
        RC_REFUSED,
        format!("pipe: run {} は停止中に runner が消えた（終端は pipe stop が書く）", launch.run),
    )
}

/// 捕らえた runner の stdout を `<run_dir>/runner.stdout.log` へ見出し付きで append する。
/// 空の周は書かない（読む理由の無い見出しで埋めない）。
fn keep_stdout(launch: &Launch<'_>, rc: i32, stdout: &str) -> Result<(), String> {
    keep_stream(&runner_stdout_path(launch.state_dir, launch.run), rc, stdout)
}

/// 捕らえた runner の stderr を `<run_dir>/runner.stderr.log` へ stdout と同じ形で append する（設計
/// dispatcher.md §12）。空の周は書かない（file が無い＝runner は stderr に何も言わなかった）。
fn keep_stderr(launch: &Launch<'_>, rc: i32, stderr: &str) -> Result<(), String> {
    keep_stream(&runner_stderr_path(launch.state_dir, launch.run), rc, stderr)
}

/// 捕らえた stream を診断 file へ見出し行（`## <ts> rc=<rc>`）付きで append する **1 本の規律**（stdout と stderr で
/// 見出しも空の扱いも分けない・pipeline.md §5.2 の 7）。
fn keep_stream(path: &Path, rc: i32, text: &str) -> Result<(), String> {
    if text.trim().is_empty() {
        return Ok(());
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| format!("{} を開けない: {err}", path.display()))?;
    let body = format!("## {} rc={rc}\n{}\n", crate::fleet::cli::now_utc(), text.trim_end());
    file.write_all(body.as_bytes())
        .map_err(|err| format!("{} を書けない: {err}", path.display()))
}

/// 器が足す口座の flag（足す側と、行が既に持つかを見る側の**同じ 1 つの字面**）。
const ACCOUNT_DIR_FLAG: &str = "--account-dir";

/// 起動行の受付の極性（[`LineRefusal`]・設計 account-autonomy.md §16・C11.2）: runner を起こす**前**に
/// 測り、既に口座を持つ行は足さずに断る（どちらの口座が正かを器は決められない＝断る側へ倒す）。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailClosed,
};

/// 起動行の受付の断り（**閉じた 1 つ**・C11.2「境界ごとの enum が極性型を運ぶ」）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineRefusal {
    /// 起動行が既に `--account-dir` を持つ（値は**行が持っていた**方）。
    AccountDirPresent(String),
}

impl std::fmt::Display for LineRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AccountDirPresent(found) => write!(f, "起動行に {ACCOUNT_DIR_FLAG} が既に在る（{found}）"),
        }
    }
}

/// 器が選んだ口座を起動行に足す（`--account-dir <state_dir>/accounts/<label>`・FR5 の口のまま）。
///
/// placeholder でなく**末尾に足す**——runner の雛形は口座を知らず（口座は便でなく器が選ぶ）、穴を
/// 雛形に要ると、穴の無い雛形の便が黙って親の口座で起きる。渡していない周は行を変えない（親の
/// 環境をそのまま継承させる・C2.2）。label の有無だけを見る（選んだ経路が初回か再開かは見ない）。
///
/// **既に在る周は足さずに断る**（`s2-07l.411`）: 2 つ並べて渡すと読み手が最初の値を採り、記帳した口座と
/// 実際に走る口座がずれる。置換もしない——どちらが正かを器は決められない（C10）。label が `None`
/// （宣言 0）は従来どおり行を変えない＝器は口座を選んでおらず、launcher の値が唯一の口座。
///
/// **引数は label と置き場だけ**（`s2-07l.412`・設計 account-autonomy.md §15 (2)）: 足す口は runner と
/// lens で**この 1 関数**である。`Launch` 全体を取ると runner の材料を持たない gate から呼べず、
/// 「起動行に口座を足す」規則が 2 つに割れる。
pub(super) fn with_account(cmd: String, label: Option<&str>, state_dir: &Path) -> Result<String, LineRefusal> {
    let Some(label) = label else {
        return Ok(cmd);
    };
    if let Some(found) = account_dir_in(&cmd) {
        return Err(LineRefusal::AccountDirPresent(found));
    }
    Ok(format!("{cmd} {ACCOUNT_DIR_FLAG} {}", crate::fleet::account_dir(state_dir, label).display()))
}

/// 起動行が既に持つ口座の値（token [`ACCOUNT_DIR_FLAG`] の次の語・値の無い末尾は [`NO_VALUE`]）。
/// 持たない行は `None`。
fn account_dir_in(cmd: &str) -> Option<String> {
    let mut tokens = cmd.split_whitespace();
    tokens.find(|token| *token == ACCOUNT_DIR_FLAG)?;
    Some(tokens.next().map_or_else(|| NO_VALUE.to_owned(), str::to_owned))
}

/// 「共通 verify」節の本文（設計 pipeline.md §65）: 便の写しの common-verify の各行を、gate と同じ [`fill_holes`] で
/// 埋めた字（`{jobs}` と `{threads}` は受付を通らない周の 1・`{teeth}` は契約の verify 行の filter 語）で 1 行 1 項目に
/// 並べる。写しを読めない周は理由の 1 行・行が 0 本の写しは `なし`（runner は止めない）。
fn common_section(launch: &Launch<'_>, base: &str) -> String {
    let frozen = match Effective::load(&vessel_path(launch.state_dir, launch.run)) {
        Ok(found) => found,
        Err(errors) => {
            let reasons: Vec<String> = errors.iter().map(ToString::to_string).collect();
            return format!("（共通 verify の写しを読めない: {}）\n", reasons.join(" / "));
        }
    };
    common_lines(frozen.common_verify(), base, &launch.contract.verify)
}

/// [`common_section`] の本文の組み立て（写しの行と契約の verify 行から・**pure**）。0 行は `なし`。
pub fn common_lines(common: &[String], base: &str, verify: &[String]) -> String {
    let teeth = teeth_of(verify);
    let lines: Vec<String> = common.iter().map(|line| format!("- {}\n", fill_holes(line, base, 1, 1, &teeth))).collect();
    if lines.is_empty() { "なし\n".to_owned() } else { lines.concat() }
}

/// 「ほかの行の touches」節の本文（設計 reverse-index.md §15）: 便の **base の木**の契約表（[`design_docs`] の母集団）の
/// 行の touches の項目を、契約 file の design（自分の行の pointer）の行を除いて並べる。anchor の作業木は読まない。
/// 読めない周は理由の 1 行（runner は止めない）。
fn touches_section(launch: &Launch<'_>, base: &str) -> String {
    match touches_rows(launch.repo, base) {
        Ok(rows) => touches_lines(&rows, &launch.contract.design),
        Err(reason) => format!("（ほかの行の touches を読めない: {reason}）\n"),
    }
}

/// 便の base の木の契約表の行ごとの（pointer `<doc>#<id>`・touches の列）。doc の順と行の順（読めない周は doc の path を
/// 持つ理由）。
fn touches_rows(repo: &Path, base: &str) -> Result<Vec<(String, Vec<String>)>, String> {
    let listed = git_bytes(repo, &["ls-tree", "-r", "-z", "--name-only", base])
        .ok_or_else(|| format!("base {base} の木を読めない"))?;
    let tracked: Vec<String> =
        String::from_utf8_lossy(&listed).split('\0').filter(|path| !path.is_empty()).map(str::to_owned).collect();
    let places = TablePlaces::at(repo, base);
    let items = places.items().ok_or_else(|| format!("base {base} の {DECL_FILE} を読めない（契約表の置き場 contract-tables）"))?;
    let mut rows = Vec::new();
    for doc in design_docs(&tracked, items) {
        let shown = git_bytes(repo, &["show", &format!("{base}:{doc}")])
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .ok_or_else(|| format!("{doc} を base から読めない"))?;
        let (found, _) = read_table(doc, &shown).map_err(|errors| {
            let first = errors.first().map(|error| error.reason()).unwrap_or_default();
            format!("{doc} の区間を読めない: {first}")
        })?;
        rows.extend(found.into_iter().map(|row| (format!("{doc}#{}", row.id), row.touches)));
    }
    Ok(rows)
}

/// [`touches_section`] の本文の組み立て（行の pointer と touches の列・自分の pointer から・**pure**）。項目ごとに 1 行
/// `- <項目> ← <pointer>, <pointer>`（項目は辞書順・pointer は受けた順で同じ pointer は 1 回・自分の pointer の行は
/// 除く）。項目 0 は `なし`。
pub fn touches_lines(rows: &[(String, Vec<String>)], own: &str) -> String {
    let mut by_item: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (pointer, touches) in rows.iter().filter(|(pointer, _)| pointer != own) {
        for item in touches {
            let pointers = by_item.entry(item.as_str()).or_default();
            if !pointers.contains(&pointer.as_str()) {
                pointers.push(pointer.as_str());
            }
        }
    }
    if by_item.is_empty() {
        return "なし\n".to_owned();
    }
    by_item.iter().map(|(item, pointers)| format!("- {item} ← {}\n", pointers.join(", "))).collect()
}

/// runner の stdin に流す本文 = 契約の写し（再読）+ 「共通 verify」節 + 「ほかの行の touches」節 + 直前の便の gate の FAIL が在れば「前の便の gate の FAIL」節 + 門の赤が在れば「門の赤」節 +
/// 回答済みの質問が在れば「回答」節 + 途中再開なら「途中再開」節 + 追随の相手が在れば「追随」節。**順序は 契約 → 共通 verify →
/// ほかの行の touches → 前の便の gate の FAIL → 門の赤 → 回答 → 途中再開 → 追随**である（節の読み方は `headless/runner.txt` の雛形が持ち、ここは run ごとの値だけを
/// 載せる）。
fn prompt(launch: &Launch<'_>, base: &str) -> String {
    let mut body = std::fs::read_to_string(contract_path(launch.state_dir, launch.run)).unwrap_or_default();
    body.push_str(&format!("\n## 共通 verify\n{}", common_section(launch, base)));
    body.push_str(&format!("\n## ほかの行の touches\n{}", touches_section(launch, base)));
    if let Some(prior) = &launch.prior_fail {
        body.push_str(&prior.section());
    }
    if let Some(red) = &launch.red {
        body.push_str(&red.section());
    }
    if let Some(Question { question, answer: Some(answer), .. }) = &launch.answered {
        body.push_str(&format!("\n## 回答\n- 質問: {question}\n- 回答: {answer}\n"));
    }
    if let Some(resumed) = &launch.resumed {
        body.push_str(&format!(
            "\n## 途中再開\n- 前の turn は {} {}\n- base からの commit（worktree に在る・やり直さない）: {}\n- 未 commit の変更（worktree に在る・消さない・続きから commit する）: {}\n",
            resumed.stopped_at,
            resumed.halt.as_stop_clause(),
            item_list(&resumed.commits),
            item_list(&resumed.uncommitted)
        ));
    }
    if let Some(Section { main, base, stale }) = &launch.follow {
        body.push_str(&format!(
            "\n## 追随\n- main が {main} へ進んだ\n- 便の base は {base}\n- `git rebase --onto {main} {base}` を実行し（base から先の便の commit だけを main の上へ運ぶ）、衝突を解いて `git rebase --continue` で終える\n"
        ));
        if !stale.is_empty() {
            let rows: Vec<String> = stale.iter().map(|row| format!("{}#{}: {}", row.doc, row.id, row.item)).collect();
            body.push_str(&format!(
                "- 追随で入った契約表の行が、この便の消した path を write-set に名指している（行を直す・その設計 doc は写しの write-set に追記済み）: {}\n",
                item_list(&rows)
            ));
        }
    }
    body
}

/// 「途中再開」節の一覧（commit / 未 commit の変更・1 行 1 項目・無ければ `なし`）。
fn item_list(items: &[String]) -> String {
    if items.is_empty() {
        return "なし".to_owned();
    }
    let mut text = String::new();
    for line in items {
        text.push_str("\n  - ");
        text.push_str(line);
    }
    text
}

/// `from` から先の commit 数。**main に在る commit は数えない**（設計 pipeline-conflict.md §11）: turn の中の
/// rebase が HEAD に載せた main の commit は便の commit ではない。main は追随の相手と同じ [`MAIN_REF`] を
/// turn の終わりに読み、読めない周は除外なしで数える（読めなさで判定を変えない）。
/// 数えられない周は 0（**commit 0 は完了ではない**側へ倒れる）。
fn commit_count(worktree: &Path, from: &str) -> u64 {
    let range = format!("{from}..HEAD");
    let main = git_line(worktree, &["rev-parse", "--verify", "-q", MAIN_REF]).map(|sha| format!("^{sha}"));
    let mut args = vec!["rev-list", "--count", range.as_str()];
    args.extend(main.as_deref());
    git_line(worktree, &args)
        .and_then(|text| text.parse().ok())
        .unwrap_or(0)
}

/// commit の有無まで見て段を決める。**commit 0 は完了ではない**。
fn settle(launch: &Launch<'_>, worktree: &Path, base: &str, rc: i32) -> Outcome {
    let commits = commit_count(worktree, base);
    if rc == 0 && commits >= 1 {
        // **終わりの門**（設計 pipeline.md §66）: Implemented の枝だけが撃つ。
        return end_gate(launch, worktree);
    }
    record_stage(launch, Stage::Failed, Some(format!("runner-rc:{rc},commits:{commits}")))
}

/// 包みが rc [`RC_RATE_LIMIT`] で終わった周: stdout の最後の停止行を読み、
/// `RunStage(RateLimited) detail=rc:<rc>,status:<status>,account:<label>` を記帳する（設計
/// account-autonomy.md §2）。label は runner を起こした口座で、器が渡していない周は
/// [`INHERITED_ACCOUNT`]（親の環境を継承した＝どの口座かを器は知らない）。
///
/// 末尾から**停止行として読める行**を探す（包めた周は箱の終端行 `confine-usage` が停止行の後ろに
/// 付くので、素の最終行を読むと常に unknown に化ける）。
///
/// **終端でない段**である（ADR-0020 §2.1）: worktree・base・commit・質問と回答の event は保つ。
/// 停止行を読めない周は `status:unknown` で、段は変えない（読めないを `Failed` に倒さない）。
fn settle_rate_limit(launch: &Launch<'_>, rc: i32, stdout: &str) -> Outcome {
    let status = stdout.lines().rev().find_map(stop_status).unwrap_or(UNKNOWN_STATUS);
    let account = launch.account.label().unwrap_or(INHERITED_ACCOUNT);
    record_stage(launch, Stage::RateLimited, Some(format!("rc:{rc},status:{status},account:{account}")))
}

/// 包みが rc [`RC_UNREACHABLE`] で終わった周（設計 account-autonomy.md §17 (2)）: **段を書かない**＝`Spawned` のまま
/// live に残る（worktree・commit・未 commit は保つ・N1）。理由は既に記帳した `SeatStopped detail=runner-unreachable`
/// が持つ。commit の有無は見ない（0 本でも捨てない＝ネットが戻れば `pipe resume` が同じ契約で続ける）。
///
/// rc は [`RC_BLOCKED`]（口座待ちと同じ「続きは resume」の極性）——rc 0 で返すと、連鎖（`pipe run`）の次の段が
/// `Spawned` の空いた便を見て、届かないネットへ runner を起こし直し続ける。
fn settle_unreachable(launch: &Launch<'_>) -> Outcome {
    Outcome {
        out: vec![format!("run={} stage={} halt=unreachable", launch.run, Stage::Spawned.as_str())],
        err: vec![format!(
            "pipe: runner が API に届かず止まった（段は Spawned のまま・`pipe resume --run {} --runner <cmd>` が同じ worktree で続ける）",
            launch.run
        )],
        rc: RC_BLOCKED,
    }
}

/// 停止行を読めない周の status（閉じた 1 つ）。
const UNKNOWN_STATUS: &str = "unknown";

/// 包みが rc [`RC_QUESTION`] で終わった周: stdout の最終行を質問 record として読み、
/// `QuestionRaised(detail=逐語)` → `RunStage(Questioned)` の順で記帳して **rc 3 で止まる**
/// （`Blocked` と同型・設計 pipeline-question.md §3 / §4）。
///
/// record が無い・読めない周は `Failed`（`question-record-missing`・fail-closed）。record と
/// commit が同時に在る周は質問ではなく実装の失敗（runner の rc を写す）。
///
/// 数えるのは **turn で増えた commit**（`tip` 基準・ADR-0019 §2.6）である。初回の turn では
/// tip = base ゆえ `.115` の判定と同値で、起こし直しの turn では「便が base から持つ commit」を
/// 数えない——数えると、追随を解けずに質問へ倒れた turn が必ず実装の失敗に化ける。
fn settle_question(launch: &Launch<'_>, worktree: &Path, tip: &str, stdout: &str) -> Outcome {
    let commits = commit_count(worktree, tip);
    let (question, about) = match question_record(stdout) {
        Ok(found) if commits == 0 => found,
        Ok(_) => {
            return record_stage(
                launch,
                Stage::Failed,
                Some(format!("runner-rc:{RC_QUESTION},commits:{commits}")),
            )
        }
        Err(reason) => {
            return record_stage(
                launch,
                Stage::Failed,
                Some(format!("question-record-missing:{reason},commits:{commits}")),
            )
        }
    };
    let raised = emit(
        launch.state_dir,
        &Emit {
            kind: EventKind::QuestionRaised,
            run: launch.run,
            bead: launch.bead,
            stage: None,
            seat: None,
            pid: None,
            detail: Some(question.clone()),
        },
        launch.policy,
    );
    if let Err(err) = raised {
        return broken(err.to_string());
    }
    let staged = record_stage(launch, Stage::Questioned, about.map(|key| format!("about:{key}")));
    if staged.rc != 0 {
        return staged;
    }
    Outcome {
        out: vec![format!("run={} stage={} question={}", launch.run, Stage::Questioned.as_str(), launch.run)],
        err: vec![format!(
            "pipe: 質問で止まった（{question}）・`pipe answer --run {} --words \"<回答の逐語>\"`",
            launch.run
        )],
        rc: RC_BLOCKED,
    }
}

/// 質問 record（`{"question":"<1 行>","about":"<key>"}`）を stdout の最終 JSON 行から読む。
/// `question` は必須・非空・1 行。`about` は任意。
fn question_record(stdout: &str) -> Result<(String, Option<String>), String> {
    let pairs = last_json_object(stdout)?;
    let get = |key: &str| {
        pairs
            .iter()
            .find(|(found, _)| found == key)
            .and_then(|(_, value)| value.as_str())
    };
    let question = get("question")
        .filter(|text| !text.trim().is_empty())
        .ok_or("question が無いか空である")?;
    if question.contains('\n') {
        return Err("question が 1 行でない".to_owned());
    }
    Ok((question.to_owned(), get("about").map(str::to_owned)))
}

/// 段を 1 件記帳して判定行を返す。
fn record_stage(launch: &Launch<'_>, stage: Stage, detail: Option<String>) -> Outcome {
    if let Err(err) = emit(
        launch.state_dir,
        &Emit {
            kind: EventKind::RunStage,
            run: launch.run,
            bead: launch.bead,
            stage: Some(stage),
            seat: None,
            pid: None,
            detail,
        },
        launch.policy,
    ) {
        return broken(err.to_string());
    }
    Outcome::ok_line(format!("run={} stage={}", launch.run, stage.as_str()))
}

/// 席の event を 1 件書く。
fn seat(
    launch: &Launch<'_>,
    kind: EventKind,
    pid: Option<u64>,
    detail: Option<String>,
) -> Result<(), String> {
    emit(
        launch.state_dir,
        &Emit {
            kind,
            run: launch.run,
            bead: launch.bead,
            stage: None,
            seat: Some(launch.run.to_owned()),
            pid,
            detail,
        },
        launch.policy,
    )
    .map_err(|err| err.to_string())
}

/// cmd の placeholder を実値へ置く。
fn substitute(
    launch: &Launch<'_>,
    worktree: &Path,
    write_set: &Path,
    plugin: &Path,
    base: &str,
) -> String {
    launch
        .runner
        .replace("{run}", launch.run)
        .replace("{worktree}", &worktree.display().to_string())
        .replace(
            "{contract}",
            &contract_path(launch.state_dir, launch.run).display().to_string(),
        )
        .replace(
            "{vessel}",
            &vessel_path(launch.state_dir, launch.run).display().to_string(),
        )
        .replace("{write_set}", &write_set.display().to_string())
        .replace("{base}", base)
        .replace("{plugin_dir}", &plugin.display().to_string())
}

/// plugin の root を run dir 配下に組み、その path を返す（設計 §5.2 手順 5 / §6）。
///
/// root の配下は 1 dir = 1 plugin で、runner が名前順に claude の `--plugin-dir` へ渡す:
/// - `<NAME>/`: **器の plugin**。binary に埋め込んだ [`EMBEDDED_PLUGIN`] を**必ず**書く＝
///   plugin を持たない consumer repo の便にも hook の in-loop guard が載る（憲法 C16.2・
///   `s2-07l.149` 裁定 (A)）。
/// - `consumer/`: worktree が**別名の** plugin を持つ周だけ（[`consumer_plugin`]）、その
///   [`PLUGIN_DIRS`] を写し、直下に空の印の file（[`NAME`] + [`RUNNER_MARK_SUFFIX`]）を 1 つ書く。
///
/// Claude Code は**読み込んだ plugin dir の配下**を acceptEdits の自動承認から外す
/// （sensitive）。便の worktree は `<repo>/.worktrees/<NAME>/<run>` ＝ repo を
/// `--plugin-dir` に渡すと **その内側**なので、便の全 file で Edit / Write が deny される。
/// 写しを repo の外（run dir 配下）へ置くことで、「repo の plugin を載せる」意図を保った
/// まま worktree を保護対象から外す。
///
/// 写すのは **worktree の生成 dir（[`PLUGIN_DIR`]）の下の** [`PLUGIN_DIRS`]（＝便の base の内容）であって anchor の
/// 現在値ではない。写し先（`consumer/` の直下）は生成 dir を挟まない。
fn copy_plugin(worktree: &Path, state_dir: &Path, run: &str) -> Result<PathBuf, String> {
    let dest = plugin_path(state_dir, run);
    // 再走で古い写しが残らないよう、先に空にする。
    if dest.exists() {
        std::fs::remove_dir_all(&dest)
            .map_err(|err| format!("{} を空にできない: {err}", dest.display()))?;
    }
    let vessel = dest.join(NAME);
    for (dir, name, body) in EMBEDDED_PLUGIN {
        let parent = vessel.join(dir);
        std::fs::create_dir_all(&parent)
            .map_err(|err| format!("{} を作れない: {err}", parent.display()))?;
        let path = parent.join(name);
        std::fs::write(&path, body).map_err(|err| format!("{} を書けない: {err}", path.display()))?;
    }
    if consumer_plugin(worktree) {
        let consumer = dest.join(CONSUMER_DIR);
        for name in PLUGIN_DIRS {
            let from = worktree.join(PLUGIN_DIR).join(name);
            // **`Path::is_dir` では判定しない**。あれは link を辿るので、`hooks` が dir への
            // symlink（例 `hooks -> ../..`）の周に「dir だ」と読んで link 先の木を丸ごと写す
            // ＝「symlink は追わない」が top-level だけ抜ける。最終要素を辿らない
            // `symlink_metadata` で見て、link なら**写さない**（fail-closed）。
            if real_dir(&from) {
                copy_tree(&from, &consumer.join(name))?;
            }
        }
        // **印は consumer の直下にだけ書く**（§19 形 3 / 形 4）: 器の plugin の写し・anchor と便の worktree の
        // 生成 dir には書かない。中身は空で、host の値も便の値も載せない。
        let mark = consumer.join(format!("{NAME}{RUNNER_MARK_SUFFIX}"));
        std::fs::write(&mark, "").map_err(|err| format!("{} を書けない: {err}", mark.display()))?;
    }
    Ok(dest)
}

/// worktree が **consumer の plugin** を持つか（設計 §5.2 手順 5 (ii)）。
///
/// 生成 dir（[`PLUGIN_DIR`]）の下に `.claude-plugin/plugin.json` と `hooks/hooks.json` が**両方**、link を辿らずに dir の中の
/// file として在り、plugin.json の top-level の `name` が [`NAME`] と**違う**周だけ真。
/// `name` が同じ周は器自身の repo＝世代がずれていても器の 1 本だけを載せる（同じ hook を
/// 2 度走らせない）。片方だけの周・`name` が読めない周・root 直下の旧 path にしか持たない周は consumer の plugin と見ない。
fn consumer_plugin(worktree: &Path) -> bool {
    let payload = worktree.join(PLUGIN_DIR);
    let present = real_dir(&payload)
        && EMBEDDED_PLUGIN.iter().all(|(dir, file, _)| {
            let parent = payload.join(dir);
            real_dir(&parent) && std::fs::symlink_metadata(parent.join(file)).is_ok_and(|meta| meta.is_file())
        });
    let [(manifest_dir, manifest, _), _] = EMBEDDED_PLUGIN;
    present
        && std::fs::read_to_string(payload.join(manifest_dir).join(manifest))
            .ok()
            .and_then(|body| top_level_string(&body, "name"))
            .is_some_and(|name| name != NAME)
}

/// path が link でない dir か（最終要素を辿らない）。
fn real_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir())
}

/// dir を再帰 copy する。**file だけを写し、symlink は追わない**。
///
/// symlink を写すと、便の外を指す link 1 本で plugin dir の見かけが repo の外に
/// なったまま中身が repo を指す（保護を外した意味が消える）。
fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|err| format!("{} を作れない: {err}", to.display()))?;
    let entries =
        std::fs::read_dir(from).map_err(|err| format!("{} を読めない: {err}", from.display()))?;
    for entry in entries {
        let entry = entry.map_err(|err| format!("{} を読めない: {err}", from.display()))?;
        // `DirEntry::file_type` は link を辿らない（`symlink_metadata` 相当）。
        let kind = entry
            .file_type()
            .map_err(|err| format!("{} の種別を読めない: {err}", entry.path().display()))?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), &target)
                .map_err(|err| format!("{} を写せない: {err}", entry.path().display()))?;
        }
    }
    Ok(())
}

/// 便の worktree と base を用意する。
///
/// 初回は repo の HEAD を base にして worktree を**切る**。**再開の turn**——回答済みの質問
/// （[`Launch::answered`]）か追随（[`Launch::follow`]）か途中再開（[`Launch::resumed`]）か門の赤（[`Launch::red`]）を持つ周
/// ——は**同じ run の worktree と記録済みの base を使う**（設計 pipeline-question.md §5 /
/// pipeline-conflict.md §3 / account-autonomy.md §4: 再開は同じ便・worktree が無い / 別 branch に
/// 居る周は断る）。
fn prepare_worktree(launch: &Launch<'_>) -> Result<(PathBuf, String), String> {
    let worktree = worktree_path(launch.repo, launch.run);
    if launch.answered.is_none() && launch.follow.is_none() && launch.resumed.is_none() && launch.red.is_none() {
        let base = super::head_of(launch.repo)
            .ok_or_else(|| format!("{} の HEAD を読めない", launch.repo.display()))?;
        add_worktree(launch.repo, &worktree, launch.run, &base)?;
        return Ok((worktree, base));
    }
    let found = base_of_run(launch.state_dir, launch.run);
    let unreadable = found.is_unreadable();
    let base = found.known().ok_or_else(|| match unreadable {
        true => format!("run {} の base を読めない（置き場）", launch.run),
        false => format!("run {} に base が無い", launch.run),
    })?;
    let branch = branch_name(launch.run);
    let on_branch = git_line(&worktree, &["rev-parse", "--abbrev-ref", "HEAD"]).is_some_and(|found| found == branch);
    if !on_branch {
        return Err(format!("{} は branch {branch} の worktree でない", worktree.display()));
    }
    Ok((worktree, base))
}

/// worktree を切る。既に在れば断る。
fn add_worktree(repo: &Path, worktree: &Path, run: &str, base: &str) -> Result<(), String> {
    if worktree.exists() {
        return Err(format!("{} は既に在る", worktree.display()));
    }
    let branch = branch_name(run);
    let path = worktree.display().to_string();
    let added = git_line(repo, &["worktree", "add", "-b", &branch, &path, base]);
    if added.is_none() && !worktree.exists() {
        return Err(format!("worktree を切れない（branch {branch}）"));
    }
    Ok(())
}

/// write-set を worktree の git dir の私有 dir へ書く（tracked 面に触れない）。
///
/// 各項目は**接頭辞（`+` / `-`）を剥がした素の path** で書く（設計 contract-source.md §3・接頭辞は受付の宣言だけの
/// 文法で、guard は素の path を読む）。剥がす規則は [`refuse::normalize`] の 1 本で、dir 項目の末尾 `/` はそのまま残る
/// （guard の dir 判定は既存のまま）。置き場だけの項目（`=`・中身を変えない）は書かない＝guard はその file への編集を
/// write-set の外として止める（gate の段 ① も diff に在れば落とす）。
fn write_policy(worktree: &Path, write_set: &[String]) -> Result<PathBuf, String> {
    let git_dir = git_line(worktree, &["rev-parse", "--absolute-git-dir"])
        .ok_or_else(|| format!("{} の git dir を読めない", worktree.display()))?;
    let dir = PathBuf::from(git_dir).join(NAME);
    std::fs::create_dir_all(&dir).map_err(|err| format!("{} を作れない: {err}", dir.display()))?;
    let path = dir.join(WRITE_SET_FILE);
    let plain: Vec<String> = write_set
        .iter()
        .filter(|item| !item.starts_with(refuse::PLACE_ONLY_FILE))
        .map(|item| refuse::normalize(item))
        .collect();
    let body = format!("{}\n", plain.join("\n"));
    std::fs::write(&path, body).map_err(|err| format!("{} を書けない: {err}", path.display()))?;
    Ok(path)
}

/// 承認まわりの材料を組む。
fn approval<'a>(launch: &'a Launch<'a>) -> Approve<'a> {
    Approve {
        run: launch.run,
        bead: launch.bead,
        state_dir: launch.state_dir,
        words: "",
        policy: launch.policy,
    }
}

/// 前提違反・使い方の誤り（rc 1 + stderr 1 行・何もしない）。
fn refused(reason: String) -> Outcome {
    Outcome::failed_line(RC_REFUSED, format!("pipe: {reason}"))
}

/// 対象そのものが壊れている（rc 2）。
fn broken(reason: String) -> Outcome {
    Outcome::failed_line(RC_BROKEN, format!("pipe: {reason}"))
}
