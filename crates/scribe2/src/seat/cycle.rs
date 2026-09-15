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
//! 隣に**立て直し**（[`relaunch()`]・[`relaunch`] module・設計 account-autonomy.md §5・SRS FR38）を置く: 退避して止まった席を別口座で
//! 同じ target に起こし直し、復元の command を注入する。同じ lock・同じ cycle-stamp（再注入の back-off）・同じ
//! 作り直しの確認（`SessionStart` の打刻）を通る。
//!
//! さらに**席の起動**（[`launch()`]・[`launch`] module・設計 account-lifecycle.md §4・ADR-0026 §2.3・SRS FR59）を置く: user が席を初めて
//! 起こす口で、起動行は host の面の宣言から導き（[`derive_launch`]・雛形 file を持たない）、登録 row を**先に**書き、
//! 立て直しと**同じ 1 本**（[`relaunch::boot`]: shell の入力欄の門 → 起動行 → 立ち上がりの確認 → 復元）で shell へ注入する。
//! 立て直しとの差は「row を先に書く」「window を作れる」の 2 点だけである。
//!
//! 停止（[`stop`] module・[`terminate_group`]）を含む 4 つの題は子 module に分け（`s2-07l.319`・純移動）、この file には
//! 作り直しと、4 題が共有する定数・型を残す。外から呼ぶ path は再輸出で不変である。

mod launch;
mod relaunch;
mod stop;

pub use launch::{
    derive_launch, fill_launch, launch, render_launched, single_model, with_agent_view_off, with_model, Holes, Launch,
    Launched, HOLES,
};
pub use relaunch::{relaunch, send_exit, Relaunch, Relaunched};
pub use stop::{foreground_child, pane_pid, terminate_group, Stopped, STOPPEDS};

use crate::polarity::{OnFailure, Polarity, Timing};
use super::{inject, pane_of, sanitize_target, state, tmux_ok, StateDir, WmScan};
use crate::fleet::json_lite::{self, Value};
use crate::fleet::store::{self, LockPolicy};
use crate::hook::{seat_name, InjectionRecord, SCHEMA};
use crate::rules::manifest::Manifest;
use crate::rules::RuleValue;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime};

/// 排他 marker の名前。
pub const LOCK_FILE: &str = "cycle.lock";
/// cycle を**評価した**周の打刻の名前（`s2-07l.110`・tick の back-off の根拠）。
pub const STAMP_FILE: &str = "cycle-stamp";
/// 退避後の終了の手（`/exit`）を**送ろうとした**周の打刻の名前（`s2-07l.252`・[`STAMP_FILE`] と同じ形）。
///
/// cycle-stamp と分けるのは back-off の目的が違うためである: 終了の手の stamp は `/exit` の二重投函を防ぎ、
/// cycle-stamp は作り直し・起こし直しの二重を防ぐ。1 本を共用すると `/exit` が効いた直後から立て直しが
/// `seat.tick_stale_s` の間 `cycle-recent` で見送られた（実地 2026-09-14 04:10〜04:38Z）。
pub const EXIT_STAMP_FILE: &str = "exit-stamp";
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
pub(super) const WHO: &str = "seat-cycle";
/// 記録の when。
const WHEN: &str = "cycle";
/// `/proc` の置き場（[`foreground_child`] と `StopTarget::of` の読み先・歯は fixture の dir を渡す）。
pub(super) const PROC: &str = "/proc";
/// 立て直しの起動の記録の when。
pub(super) const WHEN_RELAUNCH: &str = "relaunch";
/// 終了の手の記録の when。
pub(super) const WHEN_EXIT: &str = "exit";
/// 席の起動（`seat launch`）の記録の when（席の `tick.jsonl` と `inject.jsonl`・`kind=` の字面は
/// [`super::tick::InjectKind::Launch`] が持つ）。
pub(super) const WHEN_LAUNCH: &str = "launch";
/// 席の起動の `inject.jsonl` の記録の who（tick の「直近の注入」の母集団に入る・[`super::tick`] の `INJECT_WRITERS`）。
pub const WHO_LAUNCH: &str = "seat-launch";
/// 口座の credential dir の置き場（`<state_dir>/accounts/<label>/`・ADR-0017 §2.3）。
pub(super) const ACCOUNTS_DIR: &str = "accounts";
/// 起動の雛形の穴（設計 account-autonomy.md §5・seat-roles.md §2）: 選んだ口座の credential dir で埋める 1 つ。
pub const HOLE: &str = "{account_dir}";
/// claude CLI の model の flag（起動行が row の `model` を運ぶ語・値は [`crate::fleet::select::Model::alias`]）。
pub(super) const MODEL_FLAG: &str = "--model";

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
/// TTL の宣言（rules 行）が読めない。埋め込みを読む周は読めなかった variant を `:` で添える
/// （[`super::RuleRead::no_rule`]・`s2-07l.205`）。
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
/// 立て直しの口座の更新（`SeatRegistered`）・起動の登録 row を書けない。
pub const REASON_REGISTER: &str = "register-unwritable";
/// `seat launch` の `--account` が宣言（tracked + host の面の `[[account]]`）に無い（row も key も書かない）。
pub const REASON_ACCOUNT_UNKNOWN: &str = "account-unknown";
/// `seat launch` の `--account` 無しの周に session 用の選定で選べる口座が無い（row も key も書かない・理由を添える）。
pub const REASON_NO_ACCOUNT: &str = "no-account";
/// `seat launch` の target の tmux session が無い（session は作らない・row も書かない）。
pub const REASON_SESSION_MISSING: &str = "session-missing";
/// `seat launch` の window を作れない（`new-window` が失敗・row は書き終えている）。
pub const REASON_WINDOW: &str = "window-unwritable";
/// `seat launch` の既存 window の前面 process が shell でない（走っている席へ起動行を送らない・row は書き終えている）。
pub const REASON_NOT_SHELL: &str = "not-a-shell";
/// `seat launch` が event log を読めない（選定の除外＝他の席の登録 row を取れない・row も key も書かない）。
pub const REASON_LOG_UNREADABLE: &str = "log-unreadable";
/// `seat launch` の `--anchor` 無しで cwd の repo root を解けない（`seat register` の `input-unreadable` と同じ形）。
pub const REASON_ANCHOR: &str = "anchor-unresolvable";
/// `seat launch` の `--model` が表（[`crate::fleet::select::Model::parse`]・表示名か別名）に無い（row も key も書かない）。
pub const REASON_MODEL_UNKNOWN: &str = "launch-model-unknown";
/// 起動行に `--model` が 2 つ載る（雛形の literal と器の 1 つ・後勝ちにせず断る・立て直しは `relaunch-` を前置く）。
pub const REASON_MODEL_DUPLICATED: &str = "launch-model-duplicated";
/// 立て直す row の `model` が表に無い（tick の前置きで `relaunch-model-unknown`・壊れた宣言値で黙って settings の model で立てない・C10）。
pub(super) const REASON_ROW_MODEL_UNKNOWN: &str = "model-unknown";

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

/// TTL（秒）を埋め込み manifest から読む。読めない周は理由付き（[`super::RuleRead`]）。
pub fn ttl_s() -> Result<u64, super::RuleRead> {
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

/// 終了の手の打刻の path（[`EXIT_STAMP_FILE`]）。
pub fn exit_stamp_path(seat_dir: &Path) -> PathBuf {
    seat_dir.join(EXIT_STAMP_FILE)
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
    let ttl = match ttl_s() {
        Ok(found) => found,
        Err(read) => return Cycle::Refused(read.no_rule()),
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
pub(super) fn write_stamp(seat_dir: &Path) -> std::io::Result<()> {
    write_secs(&stamp_path(seat_dir))
}

/// 終了の手を送ろうとした周の打刻（cycle-stamp と同じ形・同じ書き方・書くのは tick の終了の手だけ）。
pub fn write_exit_stamp(seat_dir: &Path) -> std::io::Result<()> {
    write_secs(&exit_stamp_path(seat_dir))
}

/// unix 秒を 1 行書く（2 本の打刻の共通の形）。
fn write_secs(path: &Path) -> std::io::Result<()> {
    let secs = unix_secs(SystemTime::now());
    std::fs::write(path, format!("{secs}\n"))
}

/// lock を握っている間の手順（順序固定）: 退避物 → 状態の門 → pane → 入力欄の門 → 送る。
fn guarded(request: &Request, dir: &Path) -> Cycle {
    match super::scan_wm(Path::new(request.wm_dir), request.target) {
        WmScan::None => return Cycle::Refused(REASON_WM_MISSING),
        WmScan::Unreadable => return Cycle::Refused(REASON_WM_UNREADABLE),
        WmScan::Unconsumed(_) => {}
    }
    let stale_s = match state::stale_s() {
        Ok(found) => found,
        Err(read) => return Cycle::Refused(read.no_rule()),
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
pub(super) enum Lock {
    /// 取れた。
    Taken,
    /// live な lock が在る＝譲る。
    Held,
    /// 置き場が使えない。
    Broken,
}

/// lock を `O_EXCL` で取る。失効した residue は取り除いてから取り直す。
pub(super) fn take_lock(seat_dir: &Path, ttl_s: u64) -> Lock {
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
pub(super) fn started(dir: &Path, (baseline, since): (state::Baseline, u64), settle: Duration, step: Duration) -> bool {
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
pub(super) fn send_to(socket: Option<&str>, target: &str, text: &str) -> bool {
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
