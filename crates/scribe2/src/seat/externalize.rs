//! 作業記憶の退避（`seat externalize`・設計 docs/design/working-memory.md §5.1・ADR-0018 §2.1 / §2.3 /
//! §2.4・SRS FR23）。
//!
//! 退避物 `working-memory.<sid>.md` を**器の口だけが書く**: sid は席の打刻の最終行から得て（env と
//! pane は読まない）、自席の最新の消費済み退避物から節 1（「完了」「user 撤回」を落とす）と節 3
//! （暫定行と unresolved の行を落とし `[P0-P3]` で安定 sort）を運び、`--directives` の新規行を文法で
//! 検査し、上限（rules 行 `seat.wm_directive_cap`）を超えたら止める。**書く前に全部を判定し**、
//! 1 つでも断る周は file を作らない（FailClosed・[`ExternalizeError`]）。

use super::wm::{self, Anchor, Item, Missing, Pointer, Resolution, WmDoc};
use super::{scan_wm, seat_dir, seat_of, state, StateDir, WmScan, WM_CONSUMED, WM_PREFIX, WM_SUFFIX};
use crate::polarity::{OnFailure, Polarity, Timing};
use crate::rules::manifest::Manifest;
use crate::rules::RuleValue;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// 上限を宣言する rules 行の id（**値は code に焼かない**・憲法 C5）。
pub const ID_CAP: &str = "seat.wm_directive_cap";
/// carry 元が無い周の `carry_source:` の値。
const NO_SOURCE: &str = "none";

/// この境界の極性（[`ExternalizeError`]）: 書く前に判定し、1 つでも断る周・測れない周は退避物を作らない。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailClosed,
};

/// 退避の契機（frontmatter の `trigger:`）。**閉じた 2 値**。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// 開発 session が自分で撃った。
    Manual,
    /// tick の合図で撃った。
    Tick,
}

impl Trigger {
    /// frontmatter に書く字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Tick => "tick",
        }
    }

    /// 字面から読む。未知は `None`（使い方の誤り）。
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "manual" => Some(Self::Manual),
            "tick" => Some(Self::Tick),
            _ => None,
        }
    }
}

/// 退避 1 回の入力。
pub struct Request<'a> {
    /// tmux target（席の名乗り＝frontmatter の `seat:`）。
    pub target: &'a str,
    /// 退避物の dir。
    pub wm_dir: &'a Path,
    /// 解決済みの置き場（打刻を読む）。
    pub state_dir: &'a StateDir,
    /// 実在検査の repo root。
    pub anchor: &'a Path,
    /// 節 2 の本文の file。
    pub plan: &'a Path,
    /// 節 3 の新規行の file。
    pub directives: &'a Path,
    /// 節 1 の追記行の file。
    pub user: Option<&'a Path>,
    /// 契機。
    pub trigger: Trigger,
    /// 表示用の role（弁別には使わない）。
    pub role: Option<&'a str>,
    /// 節 3 の上限（rules 行 [`ID_CAP`]・呼び側が解く）。
    pub cap: u64,
}

/// 文法に落ちた新規行 1 つ（入力 file の行番号と、欠けた要素の全部）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrammarError {
    /// 項目の先頭行の行番号。
    pub line: usize,
    /// 欠けた要素。
    pub missing: Vec<Missing>,
}

/// 退避を止める判定（**境界の enum**・[`POLARITY`]）。どの variant でも退避物は作らない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExternalizeError {
    /// 置き場を解けない。
    StateDir,
    /// 上限の rules 行が読めない・不発効。
    NoRule,
    /// 打刻 file が無い。
    SidMissing,
    /// 打刻 file が読めない・最終行が壊れている・空。
    SidUnreadable,
    /// 最終の打刻の `sid` が空。
    SidEmpty,
    /// `sid` が file 名に使えない字を含む（dir の外を書く形を作らない）。
    SidInvalid,
    /// 自席の未 consumed 退避物が在る（二重退避を取り合わない）。
    WmExists,
    /// 退避物の dir を読めない（0 件と読み替えない）。
    WmUnreadable,
    /// anchor が dir でない。
    AnchorMissing,
    /// 入力 file を読めない（flag の名を持つ）。
    InputUnreadable(&'static str),
    /// carry 元の消費済み退避物を読めない・schema が新しい。
    CarryUnreadable,
    /// `--directives` の新規行が文法に落ちた（全件）。
    Grammar(Vec<GrammarError>),
    /// 節 3 の合計が上限を超えた（黙って切らない）。
    DirectiveCap {
        /// 合計。
        total: usize,
        /// 上限。
        cap: u64,
    },
    /// 書けない（同名の file が在る・dir に書けない）。
    Unwritable,
}

impl ExternalizeError {
    /// 断りの行の `reason=`。
    pub fn reason(&self) -> &'static str {
        match self {
            Self::StateDir => "state-dir",
            Self::NoRule => "no-rule",
            Self::SidMissing => "sid-missing",
            Self::SidUnreadable => "sid-unreadable",
            Self::SidEmpty => "sid-empty",
            Self::SidInvalid => "sid-invalid",
            Self::WmExists => "wm-exists",
            Self::WmUnreadable => "wm-unreadable",
            Self::AnchorMissing => "anchor-missing",
            Self::InputUnreadable(_) => "input-unreadable",
            Self::CarryUnreadable => "carry-unreadable",
            Self::Grammar(_) => "directive-grammar",
            Self::DirectiveCap { .. } => "directive-cap",
            Self::Unwritable => "unwritable",
        }
    }
}

/// 退避の結果（stdout 1 行の材料）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Externalized {
    /// 書いた file の名前。
    pub file: String,
    /// carry した節 3 の行数。
    pub carried: usize,
    /// carry で落とした暫定行の数。
    pub dropped_provisional: usize,
    /// carry で落とした unresolved の行の数。
    pub dropped_unresolved: usize,
    /// `--directives` の新規行の数。
    pub directives: usize,
}

/// 成功の 1 行。
pub fn render(done: &Externalized) -> String {
    format!(
        "seat: externalized file={} carried={} dropped_provisional={} dropped_unresolved={} directives={}",
        done.file, done.carried, done.dropped_provisional, done.dropped_unresolved, done.directives
    )
}

/// 断りの行（1 行目が理由・文法の断りは落ちた行を 1 行ずつ全件続ける）。
pub fn render_refused(err: &ExternalizeError) -> Vec<String> {
    let head = format!("seat: externalize refused reason={}", err.reason());
    match err {
        ExternalizeError::InputUnreadable(flag) => vec![format!("{head} flag={flag}")],
        ExternalizeError::DirectiveCap { total, cap } => vec![format!("{head} total={total} cap={cap}")],
        ExternalizeError::Grammar(errors) => {
            let mut lines = vec![format!("{head} lines={}", errors.len())];
            lines.extend(errors.iter().map(|error| {
                let missing: Vec<&str> = error.missing.iter().map(|found| found.as_str()).collect();
                format!("seat: externalize directive line={} missing={}", error.line, missing.join(","))
            }));
            lines
        }
        _ => vec![head],
    }
}

/// 上限を**渡された manifest** から読む。不発効・別の形・不在は `None`（呼び側は `no-rule` で断る）。
pub fn cap_of(manifest: &Manifest) -> Option<u64> {
    let row = manifest.get(ID_CAP)?;
    match (row.enabled, &row.value) {
        (true, RuleValue::Int(found)) => Some(*found),
        _ => None,
    }
}

/// carry-forward の結果。
struct Carry {
    /// 元の file 名（無ければ `None`）。
    source: Option<String>,
    /// 運ぶ節 1。
    user: Vec<Item>,
    /// 運ぶ節 3（P 昇順の安定 sort 済み）。
    directives: Vec<Item>,
    /// 落とした暫定行。
    dropped_provisional: usize,
    /// 落とした unresolved の行。
    dropped_unresolved: usize,
}

/// 入力 file の読み。
struct Inputs {
    /// 節 2。
    plan: String,
    /// 節 3 の新規行。
    directives: Vec<Item>,
    /// 節 1 の追記行。
    user: Vec<Item>,
}

/// 退避を 1 回行う。**判定を全部済ませてから** `create_new` で 1 回だけ書く。
pub fn run(request: &Request) -> Result<Externalized, ExternalizeError> {
    let sid = sid_of(&seat_dir(&request.state_dir.path, request.target))?;
    match scan_wm(request.wm_dir, request.target) {
        WmScan::Unconsumed(_) => return Err(ExternalizeError::WmExists),
        WmScan::Unreadable => return Err(ExternalizeError::WmUnreadable),
        WmScan::None => {}
    }
    let anchor = Anchor::open(request.anchor).ok_or(ExternalizeError::AnchorMissing)?;
    let inputs = read_inputs(request)?;
    let carry = carry_forward(request.wm_dir, request.target, &anchor)?;
    let refused: Vec<GrammarError> = inputs
        .directives
        .iter()
        .filter_map(|item| {
            let missing = wm::grammar(item);
            (!missing.is_empty()).then_some(GrammarError { line: item.line, missing })
        })
        .collect();
    if !refused.is_empty() {
        return Err(ExternalizeError::Grammar(refused));
    }
    let total = carry.directives.len().saturating_add(inputs.directives.len());
    if u64::try_from(total).map_or(true, |count| count > request.cap) {
        return Err(ExternalizeError::DirectiveCap { total, cap: request.cap });
    }
    let file = format!("{WM_PREFIX}{sid}{WM_SUFFIX}");
    let done = Externalized {
        file: file.clone(),
        carried: carry.directives.len(),
        dropped_provisional: carry.dropped_provisional,
        dropped_unresolved: carry.dropped_unresolved,
        directives: inputs.directives.len(),
    };
    let doc = compose(request, carry, inputs);
    write_new(&request.wm_dir.join(file), &doc.render())?;
    Ok(done)
}

/// 打刻の最終行の `sid`（不在 / 読めない / 空 / file 名に使えない を分けて断る）。
fn sid_of(seat: &Path) -> Result<String, ExternalizeError> {
    let text = match std::fs::read_to_string(state::path(seat)) {
        Ok(found) => found,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Err(ExternalizeError::SidMissing),
        Err(_) => return Err(ExternalizeError::SidUnreadable),
    };
    let line = text
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .ok_or(ExternalizeError::SidUnreadable)?;
    let stamp = state::Stamp::from_line(line).map_err(|_| ExternalizeError::SidUnreadable)?;
    let sid = stamp.sid.trim();
    if sid.is_empty() {
        return Err(ExternalizeError::SidEmpty);
    }
    let safe = sid.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
        && !sid.starts_with('.');
    if !safe {
        return Err(ExternalizeError::SidInvalid);
    }
    Ok(sid.to_owned())
}

/// 入力 file を読む（読めない file は flag の名で断る＝空に潰さない）。
fn read_inputs(request: &Request) -> Result<Inputs, ExternalizeError> {
    let read = |path: &Path, flag: &'static str| {
        std::fs::read_to_string(path).map_err(|_| ExternalizeError::InputUnreadable(flag))
    };
    let plan = read(request.plan, "--plan")?;
    let directives = read(request.directives, "--directives")?;
    let user = match request.user {
        Some(path) => wm::items(&read(path, "--user")?),
        None => Vec::new(),
    };
    Ok(Inputs {
        plan: wm::strip_comments(&plan).trim_matches('\n').to_owned(),
        directives: wm::items(&directives),
        user,
    })
}

/// 自席の最新の消費済み退避物（mtime 降順・同時刻は名前の降順・`seat:` 一致）。
fn latest_consumed(dir: &Path, target: &str) -> Result<Option<PathBuf>, ExternalizeError> {
    let entries = std::fs::read_dir(dir).map_err(|_| ExternalizeError::WmUnreadable)?;
    let mut best: Option<(SystemTime, String, PathBuf)> = None;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !(name.starts_with(WM_PREFIX) && name.ends_with(WM_CONSUMED)) {
            continue;
        }
        let path = entry.path();
        if seat_of(&path).is_none_or(|seat| seat != target) {
            continue;
        }
        let at = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let newer = best
            .as_ref()
            .is_none_or(|(best_at, best_name, _)| (at, &name) > (*best_at, best_name));
        if newer {
            best = Some((at, name, path));
        }
    }
    Ok(best.map(|(_, _, path)| path))
}

/// carry-forward（節 1 は閉じた状態だけ落とす・節 3 は暫定行と unresolved を落として安定 sort）。
fn carry_forward(dir: &Path, target: &str, anchor: &Anchor) -> Result<Carry, ExternalizeError> {
    let mut carry = Carry {
        source: None,
        user: Vec::new(),
        directives: Vec::new(),
        dropped_provisional: 0,
        dropped_unresolved: 0,
    };
    let Some(path) = latest_consumed(dir, target)? else {
        return Ok(carry);
    };
    let text = std::fs::read_to_string(&path).map_err(|_| ExternalizeError::CarryUnreadable)?;
    let doc = WmDoc::parse(&text).map_err(|_| ExternalizeError::CarryUnreadable)?;
    carry.source = path.file_name().map(|name| name.to_string_lossy().into_owned());
    carry.user = doc.user.into_iter().filter(|item| !wm::is_closed(item)).collect();
    for item in doc.directives {
        match wm::pointer_of(&item, anchor) {
            Pointer::Provisional => carry.dropped_provisional = carry.dropped_provisional.saturating_add(1),
            Pointer::Pointed { resolution: Resolution::Unresolved, .. } => {
                carry.dropped_unresolved = carry.dropped_unresolved.saturating_add(1);
            }
            Pointer::Pointed { .. } => carry.directives.push(item),
        }
    }
    carry.directives.sort_by_key(wm::priority_of);
    Ok(carry)
}

/// 書く退避物を組む（節 1 = carry + 追記・節 3 = carry + 新規を P 昇順の安定 sort）。
fn compose(request: &Request, carry: Carry, inputs: Inputs) -> WmDoc {
    let mut front = vec![
        ("schema".to_owned(), wm::SCHEMA.to_string()),
        ("seat".to_owned(), request.target.to_owned()),
    ];
    if let Some(role) = request.role {
        front.push(("role".to_owned(), role.to_owned()));
    }
    front.extend([
        ("externalized_at".to_owned(), crate::fleet::cli::now_utc()),
        ("trigger".to_owned(), request.trigger.as_str().to_owned()),
        ("carry_source".to_owned(), carry.source.unwrap_or_else(|| NO_SOURCE.to_owned())),
        ("carry_items".to_owned(), carry.directives.len().to_string()),
        ("carry_user_directives".to_owned(), carry.user.len().to_string()),
    ]);
    let mut user = carry.user;
    user.extend(inputs.user);
    let mut directives = carry.directives;
    directives.extend(inputs.directives);
    directives.sort_by_key(wm::priority_of);
    WmDoc {
        front,
        user,
        plan: inputs.plan,
        directives,
    }
}

/// `create_new` で書く。書き切れなかった周は作りかけを消す（退避物を作らない側へ倒す）。
fn write_new(path: &Path, body: &str) -> Result<(), ExternalizeError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| ExternalizeError::Unwritable)?;
    if file.write_all(body.as_bytes()).and_then(|()| file.sync_all()).is_err() {
        std::fs::remove_file(path).ok();
        return Err(ExternalizeError::Unwritable);
    }
    Ok(())
}
