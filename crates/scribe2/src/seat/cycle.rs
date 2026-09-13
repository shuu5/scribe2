//! session を作り直して復元する口（設計 §3・SRS FR28 / FR23・憲法 CON5 / C11 / C10）。
//!
//! `/clear` は**不可逆**である（会話は戻らない）。ゆえにこの口は 3 つの条件が同時に立つ周
//! だけ開く: 排他（lock を握れた）・退避済み（自席の未 consumed 退避物が在る）・idle
//! （hook の打刻の最終行が Idle・[`state`]・憲法 C3.3 / ADR-0015＝pane の字面は判定入力にしない）。
//! 1 つでも欠けたら **1 key も送らずに断る**——「送ったが失敗した」と「そもそも送っていない」を
//! [`Cycle`] で分けて持つのはこのためである（bool で持たない）。pane を読むのは送る直前の入力欄の
//! 門（[`inject::guard_input`]・注入と同じ 1 本）だけで、**作り直しの確認は席の打刻**
//! （`/clear` の送達 ts 以後に足された `SessionStart`・[`state::evidence_after`]・設計 seat-state.md §6・
//! `s2-07l.112`）で行う。echo の字面は読まない。
//!
//! 隣に**立て直し**（[`relaunch`]・設計 account-autonomy.md §5・SRS FR38）を置く: 退避して止まった席を別口座で
//! 同じ target に起こし直し、復元の command を注入する。同じ lock・同じ cycle-stamp（再注入の back-off）・同じ
//! 作り直しの確認（`SessionStart` の打刻）を通る。

use crate::polarity::{OnFailure, Polarity, Timing};
use super::{inject, pane_of, role, sanitize_target, state, tmux_ok, StateDir, WmScan};
use crate::fleet::json_lite::{self, Value};
use crate::fleet::select::{self, NoCandidate, Purpose, Selection};
use crate::fleet::store::{self, LockPolicy};
use crate::fleet::{Registration, State};
use crate::hook::{seat_name, InjectionRecord, SCHEMA};
use crate::rules::manifest::Manifest;
use crate::rules::RuleValue;
use std::collections::BTreeSet;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime};

/// 排他 marker の名前。
pub const LOCK_FILE: &str = "cycle.lock";
/// cycle を**評価した**周の打刻の名前（`s2-07l.110`・tick の back-off の根拠）。
pub const STAMP_FILE: &str = "cycle-stamp";
/// TTL を宣言する rules 行の id（**値は code に焼かない**・憲法 C5）。
const ID_TTL: &str = "seat.cycle_lock_ttl_s";
/// 作り直しと復元の確認上限（秒）を宣言する rules 行の id（`s2-07l.151`・裁定 id は manifest 行）。
const ID_SETTLE_S: &str = "seat.cycle_settle_s";
/// 確認を見に行く周期（ミリ秒）を宣言する rules 行の id。
const ID_POLL_MS: &str = "seat.cycle_poll_ms";
/// session を作り直す注入。
const CLEAR: &str = "/clear";
/// 復元の既定 command。
pub const DEFAULT_RESTORE: &str = "/rebrief";
/// 記録の who。
const WHO: &str = "seat-cycle";
/// 記録の when。
const WHEN: &str = "cycle";
/// 立て直しの起動の記録の when。
const WHEN_RELAUNCH: &str = "relaunch";
/// 口座の credential dir の置き場（`<state_dir>/accounts/<label>/`・ADR-0017 §2.3）。
const ACCOUNTS_DIR: &str = "accounts";
/// 起動の雛形の穴（設計 account-autonomy.md §5・seat-roles.md §2）: 選んだ口座の credential dir で埋める 1 つ。
pub const HOLE: &str = "{account_dir}";

/// 他の cycle が走っている。
pub const REASON_LOCK_HELD: &str = "lock-held";
/// 自席の未 consumed 退避物が無い（CON5: 退避物なしで撃たない）。
pub const REASON_WM_MISSING: &str = "wm-missing";
/// 退避物の dir を読めない（**0 件と読み替えない**）。
pub const REASON_WM_UNREADABLE: &str = "wm-unreadable";
/// 席の最終の打刻が Busy（turn が走っている）。
pub const REASON_BUSY: &str = "busy";
/// 打刻 file が無い（hook が載っていない席）。**idle と読み替えない**。
pub const REASON_STATE_MISSING: &str = "state-missing";
/// 打刻 file が読めない・最終行が壊れている。
pub const REASON_STATE_UNREADABLE: &str = "state-unreadable";
/// 最終の Busy が `seat.tick_stale_s` より古い（hook が死んだ疑い）。
pub const REASON_STATE_STALE: &str = "state-stale";
/// pane を読めない（入力欄の門を通せない席へ送らない）。
pub const REASON_PANE_MISSING: &str = "pane-missing";
/// 入力欄が非空（打ちかけと 1 行に merge する事故を送る直前で塞ぐ・注入と同じ門）。
pub const REASON_INPUT_BUSY: &str = "input-busy";
/// 入力欄を特定できない（prompt 行が無い pane へ送らない・注入と同じ門）。
pub const REASON_INPUT_UNKNOWN: &str = "input-unknown";
/// TTL の宣言（rules 行）が読めない。
pub const REASON_NO_RULE: &str = "no-rule";
/// 置き場を解けない。
pub const REASON_STATE_DIR: &str = "state-dir";
/// cycle-stamp を書けない＝`/clear` を送る前に断る（書けないまま送ると次の周も送りうる・N1）。
pub const REASON_STAMP: &str = "cycle-stamp-unwritable";
/// `/clear` は送ったが作り直しを確認できない（送達 ts 以後の `SessionStart` の打刻が窓の内に来ない）。
pub const REASON_CLEAR: &str = "clear-unconfirmed";
/// 復元 command は送ったが消費を確認できない（送達 ts 以後の `UserPromptSubmit` の打刻が来ない）。
pub const REASON_RESTORE: &str = "restore-unconfirmed";
/// 立て直しの起動 command は送ったが、立ち上がりを確認できない（送達 ts 以後の `SessionStart` の打刻が窓の内に来ない）。
pub const REASON_LAUNCH: &str = "launch-unconfirmed";
/// 立て直しの口座の更新（`SeatRegistered`）を書けない。
pub const REASON_REGISTER: &str = "register-unwritable";

/// cycle 1 回の入力。
pub struct Request<'a> {
    /// tmux target（pane の指定）。
    pub target: &'a str,
    /// 退避物の dir。
    pub wm_dir: &'a str,
    /// tmux の socket。
    pub socket: Option<&'a str>,
    /// pane 本文の代わりに読む file。
    pub capture_file: Option<&'a str>,
    /// 解決済みの置き場（`<state_dir>`・出所付き）。行と記録の末尾に 2 語を載せる（`.70`）。
    pub state_dir: &'a StateDir,
    /// 復元 command（既定 [`DEFAULT_RESTORE`]）。
    pub restore: Option<&'a str>,
    /// 作り直しと復元の確認上限（rules 行 `seat.cycle_settle_s`・[`pace_of`] が解く）。
    pub settle: Duration,
    /// 確認を見に行く周期（rules 行 `seat.cycle_poll_ms`）。
    pub step: Duration,
}

/// この境界の極性（[`Cycle`]）: `/clear` を送る前に条件を見て、1 つでも欠けたら 1 key も送らずに断る。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailClosed,
};

/// cycle 1 回の結果。
pub enum Cycle {
    /// 作り直して復元まで済んだ。
    Done,
    /// **1 key も送っていない**（条件が立たなかった）。
    Refused(&'static str),
    /// 送ったが確認できない。
    Failed(&'static str),
}

/// TTL（秒）を manifest から読む。不発効・別の形・不在は `None`。
pub fn ttl_s() -> Option<u64> {
    super::int_rule(ID_TTL)
}

/// 確認の刻み（上限, 周期）を**渡された manifest** から読む（`s2-07l.151`）。不発効・別の形・
/// 不在は `None`＝呼び側は [`REASON_NO_RULE`] で断る（fail-closed・値を code に焼かない・憲法 C5）。
///
/// 読み先が引数である点だけが [`super::int_rule`]（埋め込み専用）と違う: `seat cycle` / `seat tick` の
/// `--rules` は歯が確認上限を秒で差し替える seam で、埋め込みを読む口からは届かない。
pub fn pace_of(manifest: &Manifest) -> Option<(Duration, Duration)> {
    Some((
        Duration::from_secs(int_row(manifest, ID_SETTLE_S)?),
        Duration::from_millis(int_row(manifest, ID_POLL_MS)?),
    ))
}

/// 発効している rules 行の整数値（[`super::int_rule`] と同型・読む先が引数の manifest）。
fn int_row(manifest: &Manifest, id: &str) -> Option<u64> {
    let row = manifest.get(id)?;
    match (row.enabled, &row.value) {
        (true, &RuleValue::Int(found)) => Some(found),
        _ => None,
    }
}

/// cycle を評価した周の打刻の path。
pub fn stamp_path(seat_dir: &Path) -> PathBuf {
    seat_dir.join(STAMP_FILE)
}

/// 排他 marker の path。
pub fn lock_path(seat_dir: &Path) -> PathBuf {
    seat_dir.join(LOCK_FILE)
}

/// 排他 marker が **live** か。失効（residue）と不在は同じく `false`。
///
/// mtime を読めない周は `true`（＝撃たない側）へ倒す。読めないことを「空いている」と
/// 読むと、他の cycle と重なって `/clear` を 2 度撃つ。
pub fn lock_is_live(seat_dir: &Path, ttl_s: u64) -> bool {
    let Ok(meta) = std::fs::metadata(lock_path(seat_dir)) else {
        return false;
    };
    let Ok(modified) = meta.modified() else {
        return true;
    };
    modified.elapsed().map_or(true, |age| age.as_secs() <= ttl_s)
}

/// cycle を 1 回回す。
pub fn run(request: &Request) -> Cycle {
    let started = Instant::now();
    let dir = super::seat_dir(&request.state_dir.path, request.target);
    let result = perform(request, &dir);
    record(request, &result, started);
    result
}

/// lock を取り、握っている間の手順を回して、**どの枝でも lock を返す**。
///
/// **打刻は `/clear` より先**（write-ahead・`s2-07l.110`）: lock を取った周は手順に入る前に
/// [`STAMP_FILE`] を打つ（lock を取れない周は打たない＝他の cycle が打っているか置き場が使えない）。後から打つ形だと、打てない周や途中で死んだ周に `/clear` の記憶が残らず
/// 次の周も送りうる（不可逆の口・N1）。打てない周は 1 key も送らずに断る。tick からでも
/// `seat cycle` からでも同じ口を通るので、どちらの経路の cycle も back-off の根拠になる。
fn perform(request: &Request, dir: &Path) -> Cycle {
    let Some(ttl) = ttl_s() else {
        return Cycle::Refused(REASON_NO_RULE);
    };
    match take_lock(dir, ttl) {
        Lock::Taken => {}
        Lock::Held => return Cycle::Refused(REASON_LOCK_HELD),
        Lock::Broken => return Cycle::Refused(REASON_STATE_DIR),
    }
    let held = if write_stamp(dir).is_ok() {
        guarded(request, dir)
    } else {
        Cycle::Refused(REASON_STAMP)
    };
    std::fs::remove_file(lock_path(dir)).ok();
    held
}

/// cycle を評価した周の打刻（unix 秒を 1 行・mtime は書いた時刻＝tick が経過を読む）。
fn write_stamp(seat_dir: &Path) -> std::io::Result<()> {
    let secs = unix_secs(SystemTime::now());
    std::fs::write(stamp_path(seat_dir), format!("{secs}\n"))
}

/// lock を握っている間の手順（順序固定）: 退避物 → 状態の門 → pane → 入力欄の門 → 送る。
fn guarded(request: &Request, dir: &Path) -> Cycle {
    match super::scan_wm(Path::new(request.wm_dir), request.target) {
        WmScan::None => return Cycle::Refused(REASON_WM_MISSING),
        WmScan::Unreadable => return Cycle::Refused(REASON_WM_UNREADABLE),
        WmScan::Unconsumed(_) => {}
    }
    let Some(stale_s) = state::stale_s() else {
        return Cycle::Refused(REASON_NO_RULE);
    };
    if let Some(reason) = gate_of(state::read_last(dir, stale_s)) {
        return Cycle::Refused(reason);
    }
    let Some(pane) = pane_of(request.socket, request.target, request.capture_file) else {
        return Cycle::Refused(REASON_PANE_MISSING);
    };
    match inject::guard_input(&pane) {
        Ok(()) => {}
        Err(inject::InputGate::Busy) => return Cycle::Refused(REASON_INPUT_BUSY),
        Err(inject::InputGate::UnknownInput) => return Cycle::Refused(REASON_INPUT_UNKNOWN),
    }
    if !send_clear(request, dir) {
        return Cycle::Failed(REASON_CLEAR);
    }
    if send_restore(request) {
        Cycle::Done
    } else {
        Cycle::Failed(REASON_RESTORE)
    }
}

/// 状態の門（tick と同じ読み口・同じ極性・ADR-0015 §2.3）: **Idle だけが通る**。
fn gate_of(read: state::Read) -> Option<&'static str> {
    match read {
        state::Read::Idle(_) => None,
        state::Read::Busy(_) => Some(REASON_BUSY),
        state::Read::Missing => Some(REASON_STATE_MISSING),
        state::Read::Unreadable => Some(REASON_STATE_UNREADABLE),
        state::Read::Stale(_) => Some(REASON_STATE_STALE),
    }
}

/// lock を取った結果。**競合と故障を混ぜない**（憲法 C11）。
///
/// 混ぜると、置き場が壊れている周も `lock-held` を名乗る＝timer の記録から「他の cycle が
/// 走っていた」と「書けない」を分けられない（実測 2026-09-10・lens-384 M-8）。
enum Lock {
    /// 取れた。
    Taken,
    /// live な lock が在る＝譲る。
    Held,
    /// 置き場が使えない。
    Broken,
}

/// lock を `O_EXCL` で取る。失効した residue は取り除いてから取り直す。
fn take_lock(seat_dir: &Path, ttl_s: u64) -> Lock {
    match create_lock(seat_dir, ttl_s) {
        Ok(()) => Lock::Taken,
        Err(Lock::Held) => reclaim(seat_dir, ttl_s),
        Err(other) => other,
    }
}

/// 既存の lock が失効していれば取り直す。live なら譲る。
fn reclaim(seat_dir: &Path, ttl_s: u64) -> Lock {
    if lock_is_live(seat_dir, ttl_s) {
        return Lock::Held;
    }
    if std::fs::remove_file(lock_path(seat_dir)).is_err() {
        return Lock::Broken;
    }
    match create_lock(seat_dir, ttl_s) {
        Ok(()) => Lock::Taken,
        Err(other) => other,
    }
}

/// lock を作る。**`create_new` = `O_EXCL`** で、既に在れば失敗する（勝者は 1 つ）。
///
/// `create_dir_all` の失敗を [`Lock::Held`] へ落とさないのは、置き場の位置に file が在る周が
/// `AlreadyExists` を返す（＝競合と同じ error kind になる）ためである。
fn create_lock(seat_dir: &Path, ttl_s: u64) -> Result<(), Lock> {
    if std::fs::create_dir_all(seat_dir).is_err() {
        return Err(Lock::Broken);
    }
    let opened = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(lock_path(seat_dir));
    let mut file = match opened {
        Ok(found) => found,
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => return Err(Lock::Held),
        Err(_) => return Err(Lock::Broken),
    };
    let deadline = unix_secs(SystemTime::now()).saturating_add(ttl_s);
    let body = json_lite::write_object(&[
        ("pid", Value::Num(u64::from(std::process::id()))),
        ("deadline", Value::Num(deadline)),
    ]);
    writeln!(file, "{body}").map_err(|_| Lock::Broken)
}

/// 1970 年からの秒。読めなければ 0（deadline は表示用で、判定は mtime が持つ）。
fn unix_secs(at: SystemTime) -> u64 {
    at.duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// `/clear` を送り、作り直しを**席の打刻**で確認する（設計 seat-state.md §6・`s2-07l.112`）。
///
/// 証拠は「送達 ts 以後に、送る前の行より後ろへ足された `SessionStart` の打刻」だけ
/// （[`state::evidence_after`]・[`started`]）。pane の echo `❯ /clear` は読まない——正の形が字面である限り、
/// 前の `/clear` の echo が見えたまま今回の `/clear` が消費されなかった周を「済んだ」と読む口が
/// 塞げなかった（`.96` の残余 (1)）。打刻が窓（[`Request::settle`]）の内に来ない周は `false`＝
/// `clear-unconfirmed`（復元を送らない・既存の語）。hook の無い席・古い打刻しか足さない席も
/// 同じ側へ倒れる（作り直しを確認できない席へ復元を刺さない＝fail-closed）。
///
/// 窓と刻みは **rules 行が持つ**（`s2-07l.151`・値を code に焼かない・憲法 C5）: 確認に要る長さは
/// 席の hook の重さで決まる運用値で、歯は同じ分岐を短い窓の fixture で測れる。
fn send_clear(request: &Request, dir: &Path) -> bool {
    let baseline = state::baseline(dir);
    let since = state::now_secs();
    send_line(request, CLEAR) && started(dir, (baseline, since), request.settle, request.step)
}

/// 送る前に取った基線と送達 ts（`(baseline, since)`）より後ろに `SessionStart` の打刻が窓（`settle`）の内に
/// 足されるか（作り直しと立て直しの確認の 1 本・設計 seat-state.md §6）。刻みは `step`。
fn started(dir: &Path, (baseline, since): (state::Baseline, u64), settle: Duration, step: Duration) -> bool {
    let deadline = Instant::now().checked_add(settle);
    while deadline.is_some_and(|at| Instant::now() < at) {
        sleep(step);
        let found = state::evidence_after(dir, baseline, state::Event::SessionStart, since);
        if matches!(found, state::Evidence::Found(_)) {
            return true;
        }
    }
    false
}

/// 復元 command を **便 2 の inject 経路**で送る（送達確認まで込み）。
///
/// 成功と数えるのは **席が消費した**（[`inject::Settled::Consumed`]＝送達 ts 以後の
/// `UserPromptSubmit` の打刻・`s2-07l.112`）周だけ。`/clear` の直後の席には走っている turn が無いので、
/// 上限まで消費されない復元は「turn の終わりに消費される queue」ではなく submit されなかった打鍵で、
/// 会話を捨てた（不可逆）のに復元が刺さらない席を `done` と数えることになる（lens-90 HIGH-2）。
///
/// **窓は作り直しの確認と同じ上限**（[`Request::settle`]・bd `s2-07l.97`）: 作り直し直後の席は
/// SessionStart hook の間（数秒〜十数秒）復元を入力欄に queue したまま turn を始めないので、
/// inject の既定 2 s では**復元が正しく届く周ほど** `Queued` に落ちて `restore-unconfirmed` に
/// なった（実測 2026-09-11 `.96` A/B・記録が真の値と食い違う＝C10）。席が queue を消費して
/// 入力欄が空になるまで見続ける。上限の後も残っていれば従来どおり失敗（弁別は不変）。
///
/// 上限は**引数で渡す**（`s2-07l.151`）: [`inject`] は rules を読まない面で、窓の長さは cycle 側が
/// 解いた 1 つの値（[`Request::settle`]）から来る＝2 面が別々に規則を読んで食い違うことがない。
fn send_restore(request: &Request) -> bool {
    let payload = request.restore.unwrap_or(DEFAULT_RESTORE);
    let sent = inject::deliver_within(
        &inject::Request {
            target: request.target,
            socket: request.socket,
            payload,
            state_dir: Some(request.state_dir),
        },
        request.settle,
    );
    matches!(sent, inject::Delivery::Delivered(_, inject::Settled::Consumed))
}

/// 1 行を literal で送り、Enter を送る。
fn send_line(request: &Request, text: &str) -> bool {
    send_to(request.socket, request.target, text)
}

/// `target` へ 1 行を literal で送り、Enter を送る（cycle の `/clear` と立て直しの起動の 1 本）。
fn send_to(socket: Option<&str>, target: &str, text: &str) -> bool {
    tmux_ok(socket, &["send-keys", "-t", target, "-l", text]) && tmux_ok(socket, &["send-keys", "-t", target, "Enter"])
}

/// 結果を 1 語（＋理由）にする。tick 行の末尾へも同じ字面が載る。
pub fn summary(result: &Cycle) -> String {
    match *result {
        Cycle::Done => "done".to_owned(),
        Cycle::Refused(reason) => format!("refused reason={reason}"),
        Cycle::Failed(reason) => format!("failed reason={reason}"),
    }
}

/// 成立の 1 行。置き場が解けた周は判定（done / refused / failed）に依らず末尾に 2 語を載せ、
/// 解けない周（`state` が `None`）は載せない（tick と同じ規律・`.70`）。
pub fn render(target: &str, result: &Cycle, state: Option<&StateDir>) -> String {
    let suffix = inject::suffix_of(state);
    match *result {
        Cycle::Done => format!("seat: cycle done target={}{suffix}", sanitize_target(target)),
        _ => format!("seat: cycle {}{suffix}", summary(result)),
    }
}

/// 1 回を記録する。**置き場が解けない周は書かない**（rc は変えない）。`what` の末尾にも
/// 表示と同じ 2 語（席側の打刻行と並べるだけで別 dir を弁別できる・`.70`）。
fn record(request: &Request, result: &Cycle, started: Instant) {
    let what = format!("cycle {}{}", summary(result), request.state_dir.suffix());
    let entry = InjectionRecord {
        schema: SCHEMA,
        who: WHO.to_owned(),
        bytes: what.len() as u64,
        what,
        when: WHEN.to_owned(),
        // 数えていないことを 0 と書かない。
        tokens: None,
        wall_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        seat: seat_name(request.target),
        ts: state::now_secs(),
    };
    let Ok(policy) = LockPolicy::embedded() else {
        return;
    };
    let path = inject::tick_path(&request.state_dir.path, request.target);
    let _ = store::append_line(&path, &entry.to_line(), policy);
}

/// 起動の雛形の穴の数えが 1 でない理由（**閉じた 2 値**・憲法 C11）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Holes {
    /// 穴が無い（口座を渡せない雛形）。
    Missing,
    /// 穴が 2 つ以上（どれを埋めるか決まらない）。
    Many,
}

/// [`Holes`] の全 variant（宣言順）。
pub const HOLES: &[Holes] = &[Holes::Missing, Holes::Many];

impl Holes {
    /// 断りの字面（tick の判定行は `relaunch-` を前置きする）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "launch-no-hole",
            Self::Many => "launch-many-holes",
        }
    }
}

/// 起動の雛形の穴 [`HOLE`] **ちょうど 1 つ**を `account_dir` で埋める（pure・文字列の置換だけ）。env を読まず、
/// 雛形の中の host 名や絶対 path を解釈しない（C2.2）: 器が知るのは穴の位置だけである。
pub fn fill_launch(template: &str, account_dir: &str) -> Result<String, Holes> {
    match template.matches(HOLE).count() {
        0 => Err(Holes::Missing),
        1 => Ok(template.replacen(HOLE, account_dir, 1)),
        _ => Err(Holes::Many),
    }
}

/// 立て直し 1 回の入力（[`relaunch`]・入口の 3 条件と back-off は tick が見る）。
pub struct Relaunch<'a> {
    /// tmux target（shell へ戻った pane）。
    pub target: &'a str,
    /// tmux の socket。
    pub socket: Option<&'a str>,
    /// 解決済みの置き場（credential dir と event log の置き場）。
    pub state_dir: &'a StateDir,
    /// 復元 command（既定 [`DEFAULT_RESTORE`]＝FR28）。
    pub restore: Option<&'a str>,
    /// 立ち上がりと復元の確認上限（rules 行 `seat.cycle_settle_s`）。
    pub settle: Duration,
    /// 確認の周期（rules 行 `seat.cycle_poll_ms`）。
    pub step: Duration,
    /// 自席の登録 row（雛形・`model`・更新で写す項目の出所）。
    pub row: &'a Registration,
    /// replay の現在地（実測行と他の席の登録 row）。
    pub state: &'a State,
    /// 口座 label の列（manifest の `[[account]]`・宣言値）。
    pub labels: &'a [String],
    /// R-C9-1 の値（session 用の閾値）。
    pub threshold_pct: u64,
}

/// 立て直し 1 回の結果。**「送っていない」と「送ったが確かめられない」を分ける**（[`Cycle`] と同じ）。
pub enum Relaunched {
    /// 起動と復元を注入して復元の消費を確かめ、登録 row の口座を更新した（選んだ label・復元の消費）。
    Done(String, inject::Settled),
    /// 選べる口座が無い（**1 key も送っていない**・次の tick で選び直す）。
    None(NoCandidate),
    /// **1 key も送っていない**。
    Refused(&'static str),
    /// 送ったが確かめられない。
    Failed(&'static str),
}

/// 席を別口座で立て直す（設計 account-autonomy.md §5）: session 用の規則で口座を選び（[`choose`]）、雛形の穴を
/// 選んだ口座の credential dir（`<state_dir>/accounts/<label>`）で埋めて同じ target の shell へ注入し、立ち上がりを
/// 打刻で確かめて登録 row の口座を更新し、復元の command を注入する。
///
/// **cycle と同じ lock と cycle-stamp**（`s2-07l.110`・write-ahead）: lock を取った周は送る前に打刻し、打てない周は
/// 1 key も送らない＝立て直しの再注入の back-off は tick が読む既存の 1 本である。選べない・雛形の穴が 1 つでない
/// 周は lock も打刻も取らない（送っていない）。
pub fn relaunch(request: &Relaunch) -> Relaunched {
    let label = match choose(request) {
        Selection::Chosen(label) => label,
        Selection::None(found) => return Relaunched::None(found),
    };
    let account_dir = request.state_dir.path.join(ACCOUNTS_DIR).join(&label);
    let launch = match fill_launch(&request.row.launch, &account_dir.display().to_string()) {
        Ok(found) => found,
        Err(holes) => return Relaunched::Refused(holes.as_str()),
    };
    let Some(ttl) = ttl_s() else {
        return Relaunched::Refused(REASON_NO_RULE);
    };
    let dir = super::seat_dir(&request.state_dir.path, request.target);
    match take_lock(&dir, ttl) {
        Lock::Taken => {}
        Lock::Held => return Relaunched::Refused(REASON_LOCK_HELD),
        Lock::Broken => return Relaunched::Refused(REASON_STATE_DIR),
    }
    let held = if write_stamp(&dir).is_ok() {
        relaunch_held(request, &dir, &launch, &label)
    } else {
        Relaunched::Refused(REASON_STAMP)
    };
    std::fs::remove_file(lock_path(&dir)).ok();
    held
}

/// session 用の選定（[`select::select`] の 1 本・model = 登録 row の `model`）。除外は**他の席の**登録 row が持つ
/// 口座で、自席の row（同じ鍵）は入れない（account-autonomy.md §5）。
fn choose(request: &Relaunch) -> Selection {
    let own = (request.row.role, request.row.anchor.as_str());
    let exclude: BTreeSet<String> = request
        .state
        .registrations
        .values()
        .map(|latest| &latest.registration)
        .filter(|found| (found.role, found.anchor.as_str()) != own)
        .map(|found| found.account.clone())
        .collect();
    let now = crate::fleet::cli::now_utc();
    select::select(&select::Input {
        labels: request.labels,
        allowance: &request.state.allowance,
        purpose: Purpose::Session,
        model: request.row.model.as_deref(),
        exclude: &exclude,
        threshold_pct: request.threshold_pct,
        now: &now,
    })
}

/// lock を握っている間の手順（順序固定）: shell の入力欄の門 → 起動の雛形 → 立ち上がりの確認 → 登録 row の更新 → 復元。
///
/// 起動は**前面が shell の pane** へ撃つので、門は席の `❯` の行（[`inject::guard_input`]）でなく shell の prompt 末尾
/// （[`super::shell_input_empty`]・account-autonomy.md §5「shell への注入の門」・`s2-07l.218`）で見る。断りの字面は
/// cycle の門と同じ `input-busy` / `input-unknown`。立ち上がった後の復元は席の pane なので従来どおり注入の門を通る。
///
/// 登録 row は**立ち上がりを確かめた直後**に更新する: 起動が届いた席は選んだ口座で走っており、復元を
/// 確かめられない周でも row が旧い口座を名乗ると、選定の除外と次の周の逼迫度が別の口座を見る。
/// 成功と数えるのは復元が**消費された**周だけ（[`send_restore`] と同じ・立ち上がった直後の席に turn は無い）。
fn relaunch_held(request: &Relaunch, dir: &Path, launch: &str, label: &str) -> Relaunched {
    let Some(pane) = super::tmux_stdout(request.socket, &["capture-pane", "-p", "-J", "-t", request.target]) else {
        return Relaunched::Refused(REASON_PANE_MISSING);
    };
    match super::shell_input_empty(&pane) {
        Ok(()) => {}
        Err(inject::InputGate::Busy) => return Relaunched::Refused(REASON_INPUT_BUSY),
        Err(inject::InputGate::UnknownInput) => return Relaunched::Refused(REASON_INPUT_UNKNOWN),
    }
    let baseline = state::baseline(dir);
    let since = state::now_secs();
    let started_at = Instant::now();
    if !send_to(request.socket, request.target, launch) {
        return Relaunched::Failed(inject::REASON_TMUX_FAILED);
    }
    record_launch(request, launch, started_at);
    if !started(dir, (baseline, since), request.settle, request.step) {
        return Relaunched::Failed(REASON_LAUNCH);
    }
    if role::relabel(&request.state_dir.path, request.row, label).is_err() {
        return Relaunched::Failed(REASON_REGISTER);
    }
    match restore_when_ready(request) {
        Some(inject::Settled::Consumed) => Relaunched::Done(label.to_owned(), inject::Settled::Consumed),
        Some(_) | None => Relaunched::Failed(REASON_RESTORE),
    }
}

/// 復元 command を注入する（FR28・既定 [`DEFAULT_RESTORE`]）。立ち上がった直後の席は入力欄を描き終える前があり、
/// 入力欄の門は **1 key も送らずに断る**ので、窓（`settle`）の内は刻み（`step`）ごとに送り直す（断りは送って
/// いない＝二重投函にならない）。送達した周の消費を返し、窓の内に送達できない周は `None`。
fn restore_when_ready(request: &Relaunch) -> Option<inject::Settled> {
    let payload = request.restore.unwrap_or(DEFAULT_RESTORE);
    let deadline = Instant::now().checked_add(request.settle);
    loop {
        match inject::deliver_within(&sending(request, payload), request.settle) {
            inject::Delivery::Delivered(_, settled) => return Some(settled),
            inject::Delivery::Unconfirmed(_) => return None,
            inject::Delivery::Refused(_) => {}
        }
        if !deadline.is_some_and(|at| Instant::now() < at) {
            return None;
        }
        sleep(request.step);
    }
}

/// 立て直しの起動 1 行を席の `tick.jsonl` に記録する（`who=seat-cycle`・`when=relaunch`・`what` は起動の 1 行そのまま）。
/// 起動は shell の門（[`super::shell_input_empty`]）を通して直に送るので `seat inject` の経路の行ではない（C10）。
/// **置き場へ書けない周も結果を変えない**（記録は判定そのものではない）。
fn record_launch(request: &Relaunch, launch: &str, started: Instant) {
    let entry = InjectionRecord {
        schema: SCHEMA,
        who: WHO.to_owned(),
        what: launch.to_owned(),
        when: WHEN_RELAUNCH.to_owned(),
        bytes: launch.len() as u64,
        // 数えていないことを 0 と書かない。
        tokens: None,
        wall_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        seat: seat_name(request.target),
        ts: state::now_secs(),
    };
    let Ok(policy) = LockPolicy::embedded() else {
        return;
    };
    let path = inject::tick_path(&request.state_dir.path, request.target);
    let _ = store::append_line(&path, &entry.to_line(), policy);
}

/// 復元の 1 行を送る注入の入力（`seat inject` と同じ経路・記録は席の `tick.jsonl`）。
fn sending<'r>(request: &Relaunch<'r>, payload: &'r str) -> inject::Request<'r> {
    inject::Request {
        target: request.target,
        socket: request.socket,
        payload,
        state_dir: Some(request.state_dir),
    }
}

#[cfg(test)]
mod tests {
    use super::{fill_launch, Holes, HOLES};
    use crate::order::is_declaration_order;

    /// 雛形の穴はちょうど 1 つだけが埋まり（文字列の置換だけ・env の字面も path の字面も解釈しない）、無い・2 つ
    /// 以上は typed に断る（account-autonomy.md §5・契約 (d)）。
    #[test]
    fn seat_account_fill_launch_fills_exactly_one_hole() {
        let dir = "/state/accounts/a2";
        assert_eq!(
            fill_launch("CLAUDE_CONFIG_DIR={account_dir} claude --resume", dir),
            Ok("CLAUDE_CONFIG_DIR=/state/accounts/a2 claude --resume".to_owned())
        );
        assert_eq!(
            fill_launch("$CONFIG_ROOT/cld --host box {account_dir}", dir),
            Ok("$CONFIG_ROOT/cld --host box /state/accounts/a2".to_owned()),
            "env の字面と host 名は解釈しない（そのまま残す）"
        );
        assert_eq!(fill_launch("claude --resume", dir), Err(Holes::Missing));
        assert_eq!(fill_launch("{account-dir} {credential-dir}", dir), Err(Holes::Missing), "似た字面は穴ではない");
        assert_eq!(fill_launch("{account_dir} {account_dir}", dir), Err(Holes::Many));
        assert_eq!(fill_launch("{account_dir}{account_dir}{account_dir}", dir), Err(Holes::Many));
        let names: Vec<&str> = HOLES.iter().map(|holes| holes.as_str()).collect();
        assert_eq!(names, ["launch-no-hole", "launch-many-holes"]);
        assert!(is_declaration_order(HOLES, |holes| holes as usize));
    }
}
