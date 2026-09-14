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
//!
//! さらに**席の起動**（[`launch`]・設計 account-lifecycle.md §4・ADR-0026 §2.3・SRS FR59）を置く: user が席を初めて
//! 起こす口で、起動行は host の面の宣言から導き（[`derive_launch`]・雛形 file を持たない）、登録 row を**先に**書き、
//! 立て直しと**同じ 1 本**（[`boot`]: shell の入力欄の門 → 起動行 → 立ち上がりの確認 → 復元）で shell へ注入する。
//! 立て直しとの差は「row を先に書く」「window を作れる」の 2 点だけである。

use crate::polarity::{OnFailure, Polarity, Timing};
use super::role::Role;
use super::{inject, pane_of, role, sanitize_target, state, tmux_ok, StateDir, WmScan};
use crate::fleet::json_lite::{self, Value};
use crate::fleet::select::{self, NoCandidate, Purpose, Selection};
use crate::fleet::store::{self, LockPolicy};
use crate::fleet::{replay, Registration, State};
use crate::headless::{ACCOUNT_ENV, AGENT_VIEW_ENV, AGENT_VIEW_OFF, DEFAULT_CLAUDE};
use crate::hook::{seat_name, InjectionRecord, SCHEMA};
use crate::rules::manifest::{LaunchArg, Manifest, PluginDir};
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
const WHO: &str = "seat-cycle";
/// 記録の when。
const WHEN: &str = "cycle";
/// 立て直しの起動の記録の when。
const WHEN_RELAUNCH: &str = "relaunch";
/// 終了の手の記録の when。
const WHEN_EXIT: &str = "exit";
/// 席の起動（`seat launch`）の記録の when（席の `tick.jsonl` と `inject.jsonl`・`kind=` の字面は
/// [`super::tick::InjectKind::Launch`] が持つ）。
const WHEN_LAUNCH: &str = "launch";
/// 席の起動の `inject.jsonl` の記録の who（tick の「直近の注入」の母集団に入る・[`super::tick`] の `INJECT_WRITERS`）。
pub const WHO_LAUNCH: &str = "seat-launch";
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
fn write_stamp(seat_dir: &Path) -> std::io::Result<()> {
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

/// 起動行の先頭に agent view を切る env（`CLAUDE_CODE_DISABLE_AGENT_VIEW=1 `）を前置する（pure・設計 account-autonomy.md
/// §5「agent view の前提」・`s2-07l.239`）。器が起こす claude は常に agent view 無しで動く——有効な session は background
/// work が残る周の `/exit` で dialog を出して止まり、器は描画を読まない（C3.3）ので答えられない。雛形は user の物で
/// 書き換えず（[`fill_launch`] は不変）、子へ設定するだけで env は読まない（C2.2）。既に同じ前置で始まる行は二重にせず、
/// 空の行はそのまま返す。
pub fn with_agent_view_off(line: &str) -> String {
    let prefix = format!("{AGENT_VIEW_ENV}={AGENT_VIEW_OFF} ");
    if line.trim().is_empty() || line.trim_start().starts_with(&prefix) {
        return line.to_owned();
    }
    format!("{prefix}{line}")
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
    let own = (request.row.role, request.row.anchor.as_str());
    let label = match choose(own, request.state, request.labels, request.row.model.as_deref(), request.threshold_pct) {
        Selection::Chosen(label) => label,
        Selection::None(found) => return Relaunched::None(found),
    };
    // 注入するのは穴を埋めた雛形に agent view off を前置した 1 行（記録にも同じ行が載る）。
    let launch = match launch_line(request.state_dir, &request.row.launch, &label) {
        Ok(found) => found,
        Err(holes) => return Relaunched::Refused(holes.as_str()),
    };
    let ttl = match ttl_s() {
        Ok(found) => found,
        Err(read) => return Relaunched::Refused(read.no_rule()),
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

/// session 用の選定（[`select::select`] の 1 本・立て直しと `seat launch` の初回の選定が同じ関数を呼ぶ）。`own` は
/// 自席の鍵 (role, anchor)・`model` は席の model。除外は**他の席の**登録 row が持つ口座で、自席の row（同じ鍵）は
/// 入れない（account-autonomy.md §5 / account-lifecycle.md §4）。
fn choose(own: (Role, &str), state: &State, labels: &[String], model: Option<&str>, threshold_pct: u64) -> Selection {
    let exclude: BTreeSet<String> = state
        .registrations
        .values()
        .map(|latest| &latest.registration)
        .filter(|found| (found.role, found.anchor.as_str()) != own)
        .map(|found| found.account.clone())
        .collect();
    let now = crate::fleet::cli::now_utc();
    select::select(&select::Input {
        labels,
        allowance: &state.allowance,
        purpose: Purpose::Session,
        model,
        exclude: &exclude,
        threshold_pct,
        now: &now,
    })
}

/// 雛形 `template` の穴を口座 `label` の credential dir（`<state_dir>/accounts/<label>`）で埋め、agent view off を前置した
/// 起動の 1 行（立て直しと `seat launch` の同じ 1 つ・記録にも同じ行が載る）。
fn launch_line(state_dir: &StateDir, template: &str, label: &str) -> Result<String, Holes> {
    let account_dir = state_dir.path.join(ACCOUNTS_DIR).join(label);
    fill_launch(template, &account_dir.display().to_string()).map(|found| with_agent_view_off(&found))
}

/// lock を握っている間の手順: 起動の 1 本（[`boot`]）に「立ち上がりの直後の登録 row の更新」を挟む。
///
/// 登録 row は**立ち上がりを確かめた直後**に更新する: 起動が届いた席は選んだ口座で走っており、復元を
/// 確かめられない周でも row が旧い口座を名乗ると、選定の除外と次の周の逼迫度が別の口座を見る。
/// 成功と数えるのは復元が**消費された**周だけ（[`send_restore`] と同じ・立ち上がった直後の席に turn は無い）。
fn relaunch_held(request: &Relaunch, dir: &Path, launch: &str, label: &str) -> Relaunched {
    let common = Boot {
        target: request.target,
        socket: request.socket,
        state_dir: request.state_dir,
        restore: Some(request.restore.unwrap_or(DEFAULT_RESTORE)),
        settle: request.settle,
        step: request.step,
    };
    let relabel = || role::relabel(&request.state_dir.path, request.row, label).map(|_| ()).map_err(|_| REASON_REGISTER);
    match boot(&common, dir, (launch, WHEN_RELAUNCH), relabel) {
        Booted::Done(Some(inject::Settled::Consumed)) => Relaunched::Done(label.to_owned(), inject::Settled::Consumed),
        Booted::Done(_) => Relaunched::Failed(REASON_RESTORE),
        Booted::Refused(reason) => Relaunched::Refused(reason),
        Booted::Failed(reason) => Relaunched::Failed(reason),
    }
}

/// 起動の注入の共通の入力（立て直し [`Relaunch`] と席の起動 [`Launch`] が同じ 1 本 [`boot`] に渡す）。
struct Boot<'a> {
    /// tmux target（前面が shell の pane）。
    target: &'a str,
    /// tmux の socket。
    socket: Option<&'a str>,
    /// 解決済みの置き場（打刻と記録の置き場）。
    state_dir: &'a StateDir,
    /// 立ち上がった後に送る復元 command（`None` は送らない＝`seat launch` の `--restore` 無し）。
    restore: Option<&'a str>,
    /// 立ち上がりと復元の確認上限（rules 行 `seat.cycle_settle_s`）。
    settle: Duration,
    /// 確認の周期（rules 行 `seat.cycle_poll_ms`）。
    step: Duration,
}

/// 起動の 1 本の結果（**「送っていない」と「送ったが確かめられない」を分ける**・[`Relaunched`] と同じ極性）。
enum Booted {
    /// 起動が届いて立ち上がりを確かめた（復元を送った周はその消費・送らない周は `None`）。
    Done(Option<inject::Settled>),
    /// **1 key も送っていない**。
    Refused(&'static str),
    /// 送ったが確かめられない。
    Failed(&'static str),
}

/// 起動の注入の **1 本**（順序固定・立て直しと `seat launch` の共通の経路・account-lifecycle.md §4）: shell の入力欄の門 →
/// 起動行（`(line, when)`）を送って記録 → 立ち上がりの確認 → `between`（立て直しは登録 row の更新・起動は何もしない）→
/// 復元。
///
/// 起動は**前面が shell の pane** へ撃つので、門は席の `❯` の行（[`inject::guard_input`]）でなく shell の prompt 末尾
/// （[`super::shell_input_empty`]・account-autonomy.md §5「shell への注入の門」・`s2-07l.218`）で見る。断りの字面は
/// cycle の門と同じ `input-busy` / `input-unknown`。立ち上がった後の復元は席の pane なので従来どおり注入の門を通る。
fn boot(common: &Boot, dir: &Path, (line, when): (&str, &str), between: impl FnOnce() -> Result<(), &'static str>) -> Booted {
    let Some(pane) = super::tmux_stdout(common.socket, &["capture-pane", "-p", "-J", "-t", common.target]) else {
        return Booted::Refused(REASON_PANE_MISSING);
    };
    match super::shell_input_empty(&pane) {
        Ok(()) => {}
        Err(inject::InputGate::Busy) => return Booted::Refused(REASON_INPUT_BUSY),
        Err(inject::InputGate::UnknownInput) => return Booted::Refused(REASON_INPUT_UNKNOWN),
    }
    let baseline = state::baseline(dir);
    let since = state::now_secs();
    let started_at = Instant::now();
    if !send_to(common.socket, common.target, line) {
        return Booted::Failed(inject::REASON_TMUX_FAILED);
    }
    record_sent(common.state_dir, common.target, (line, when), started_at);
    if !started(dir, (baseline, since), common.settle, common.step) {
        return Booted::Failed(REASON_LAUNCH);
    }
    if let Err(reason) = between() {
        return Booted::Failed(reason);
    }
    match common.restore {
        None => Booted::Done(None),
        Some(payload) => restore_when_ready(common, payload).map_or(Booted::Failed(REASON_RESTORE), |settled| Booted::Done(Some(settled))),
    }
}

/// 復元 command を注入する（FR28・立て直しの既定は [`DEFAULT_RESTORE`]）。立ち上がった直後の席は入力欄を描き終える前が
/// あり、入力欄の門は **1 key も送らずに断る**ので、窓（`settle`）の内は刻み（`step`）ごとに送り直す（断りは送って
/// いない＝二重投函にならない）。送達した周の消費を返し、窓の内に送達できない周は `None`。
fn restore_when_ready(common: &Boot, payload: &str) -> Option<inject::Settled> {
    let deadline = Instant::now().checked_add(common.settle);
    loop {
        match inject::deliver_within(&sending(common, payload), common.settle) {
            inject::Delivery::Delivered(_, settled) => return Some(settled),
            inject::Delivery::Unconfirmed(_) => return None,
            inject::Delivery::Refused(_) => {}
        }
        if !deadline.is_some_and(|at| Instant::now() < at) {
            return None;
        }
        sleep(common.step);
    }
}

/// 退避後の終了の手の 1 行（`line`）を `target` へ literal で送り、Enter を送る（`s2-07l.252`）。送れた周は席の
/// `tick.jsonl` に `who=seat-cycle`・`when=exit` の行を積む（立て直しの起動と同じ形）。送達の確認は呼び側
/// （tick）が前面 process の読みで行う: `/exit` を受けた席は終わって pane が shell に置き換わるので、目印の出現数
/// （[`inject::deliver`]）では成功が「現れない」に倒れる。
pub fn send_exit(socket: Option<&str>, target: &str, state_dir: &StateDir, line: &str) -> bool {
    let started_at = Instant::now();
    if !send_to(socket, target, line) {
        return false;
    }
    record_sent(state_dir, target, (line, WHEN_EXIT), started_at);
    true
}

/// 直に送った 1 行（`(what, when)`）を席の `tick.jsonl` に記録する（`who=seat-cycle`・`what` は送った 1 行そのまま・
/// `when` は立て直しの起動 `relaunch` か終了の手 `exit`）。直に送るので `seat inject` の経路の行ではない（C10）。
/// **置き場へ書けない周も結果を変えない**（記録は判定そのものではない）。
fn record_sent(state_dir: &StateDir, target: &str, (what, when): (&str, &str), started: Instant) {
    let entry = InjectionRecord {
        schema: SCHEMA,
        who: WHO.to_owned(),
        what: what.to_owned(),
        when: when.to_owned(),
        bytes: what.len() as u64,
        // 数えていないことを 0 と書かない。
        tokens: None,
        wall_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        seat: seat_name(target),
        ts: state::now_secs(),
    };
    let Ok(policy) = LockPolicy::embedded() else {
        return;
    };
    let path = inject::tick_path(&state_dir.path, target);
    let _ = store::append_line(&path, &entry.to_line(), policy);
}

/// 復元の 1 行を送る注入の入力（`seat inject` と同じ経路・記録は席の `tick.jsonl`）。
fn sending<'r>(common: &Boot<'r>, payload: &'r str) -> inject::Request<'r> {
    inject::Request {
        target: common.target,
        socket: common.socket,
        payload,
        state_dir: Some(common.state_dir),
    }
}

/// 起動行の導出（**pure**・設計 account-lifecycle.md §4・ADR-0026 §2.3）:
/// `CLAUDE_CODE_DISABLE_AGENT_VIEW=1 CLAUDE_CONFIG_DIR={account_dir} claude --plugin-dir <anchor> [--plugin-dir <dir>…] [<value>…]`。
///
/// 穴は [`HOLE`] の 1 つだけ（[`fill_launch`] / [`Holes`] は不変）。`claude` は語（shell の PATH が解く・器は claude の
/// 場所を持たない）。器自身の plugin は anchor（main checkout・`plugin.json` を持つ）を積み、host 固有の plugin dir と
/// 起動引数は host の面の宣言（`[[plugin]]` / `[[launch-arg]]`・宣言順）から写す（C10.2）。雛形 file は読まない・
/// 書かない。値の中の空白は解釈しない（shell が読む字面をそのまま並べる）。
pub fn derive_launch(anchor: &Path, plugins: &[PluginDir], args: &[LaunchArg]) -> String {
    let mut words = vec![
        format!("{AGENT_VIEW_ENV}={AGENT_VIEW_OFF}"),
        format!("{ACCOUNT_ENV}={HOLE}"),
        DEFAULT_CLAUDE.to_owned(),
        "--plugin-dir".to_owned(),
        anchor.display().to_string(),
    ];
    for plugin in plugins {
        words.push("--plugin-dir".to_owned());
        words.push(plugin.dir().to_owned());
    }
    words.extend(args.iter().map(|arg| arg.value().to_owned()));
    words.join(" ")
}

/// 席の起動 1 回の入力（[`launch`]・`seat launch`・account-lifecycle.md §4）。
pub struct Launch<'a> {
    /// tmux target（`session:window`・window は無ければ作る・session は作らない）。
    pub target: &'a str,
    /// tmux の socket。
    pub socket: Option<&'a str>,
    /// 解決済みの置き場（credential dir・event log・打刻の置き場）。
    pub state_dir: &'a StateDir,
    /// 立ち上がった後に送る復元 command（`--restore`・無ければ送らない）。
    pub restore: Option<&'a str>,
    /// 立ち上がりと復元の確認上限（rules 行 `seat.cycle_settle_s`）。
    pub settle: Duration,
    /// 確認の周期（rules 行 `seat.cycle_poll_ms`）。
    pub step: Duration,
    /// 席の役割（登録 row の鍵の片方・`--role`）。
    pub role: Role,
    /// 登録 row の anchor（絶対 path・`--anchor` か cwd の repo root・起動行の `--plugin-dir` の 1 つ目）。
    pub anchor: &'a Path,
    /// 明示の口座（`--account`・無ければ session 用の選定）。
    pub account: Option<&'a str>,
    /// 席の model（`--model`・登録 row と選定の両方に渡す）。
    pub model: Option<&'a str>,
    /// 開いた manifest（tracked + host の面・`[[account]]` / `[[plugin]]` / `[[launch-arg]]` の出所）。
    pub manifest: &'a Manifest,
    /// R-C9-1 の値（session 用の閾値）。
    pub threshold_pct: u64,
}

/// 席の起動 1 回の結果。**「送っていない」と「送ったが確かめられない」を分ける**（[`Relaunched`] と同じ）。
pub enum Launched {
    /// 起動行を注入して立ち上がりを確かめた（選んだ label・`--restore` を送った周はその消費）。
    Done(String, Option<inject::Settled>),
    /// 選べる口座が無い（**1 key も送らず row も書かない**）。
    None(NoCandidate),
    /// **1 key も送っていない**（row は理由による: `session-missing` 以前は書かない・門で止まる周は書き終えている）。
    Refused(&'static str),
    /// 送ったが確かめられない。
    Failed(&'static str),
}

/// 席を起こす（設計 account-lifecycle.md §4・ADR-0026 §2.3・SRS FR59 / FR40 / FR36）: 口座を決め（`--account` か
/// session 用の選定 [`choose`]）→ session の実在（無ければ `session-missing`・作らない）→ 登録 row を**先に**書く
/// （[`role::register`]・`sid` 無し・打刻の条件は掛けない・`launch` = 導出した行）→ window（無ければ `new-window`）→
/// 立て直しと同じ 1 本（[`boot`]）で穴を埋めた起動行を shell へ注入し、`--restore` が在れば復元を送る → `inject.jsonl` に
/// `kind=launch` を 1 行。lock も cycle-stamp も取らない（tick の back-off は立て直しのもので、起動は user の手番）。
pub fn launch(request: &Launch) -> Launched {
    let started_at = Instant::now();
    let label = match pick_account(request) {
        Ok(label) => label,
        Err(refused) => return refused,
    };
    let derived = match prepare(request, &label) {
        Ok(found) => found,
        Err(reason) => return Launched::Refused(reason),
    };
    let line = match launch_line(request.state_dir, &derived, &label) {
        Ok(found) => found,
        Err(holes) => return Launched::Refused(holes.as_str()),
    };
    let common = Boot {
        target: request.target,
        socket: request.socket,
        state_dir: request.state_dir,
        restore: request.restore,
        settle: request.settle,
        step: request.step,
    };
    let dir = super::seat_dir(&request.state_dir.path, request.target);
    let booted = boot(&common, &dir, (&line, WHEN_LAUNCH), || Ok(()));
    if !matches!(booted, Booted::Refused(_)) {
        record_launch(request, &label, started_at);
    }
    match booted {
        Booted::Done(Some(inject::Settled::Consumed)) => Launched::Done(label, Some(inject::Settled::Consumed)),
        Booted::Done(None) => Launched::Done(label, None),
        Booted::Done(Some(_)) => Launched::Failed(REASON_RESTORE),
        Booted::Refused(reason) => Launched::Refused(reason),
        Booted::Failed(reason) => Launched::Failed(reason),
    }
}

/// 口座を決める: `--account` は宣言（開いた manifest の `[[account]]`）に在る label だけ（無ければ `account-unknown`）・
/// 無ければ session 用の選定（除外 = 他の席の登録 row の口座・候補なしは [`Launched::None`]）。event log を読めない周は
/// 選定に入らず断る。**ここまでは row も key も書かない**。
fn pick_account(request: &Launch) -> Result<String, Launched> {
    let labels: Vec<String> = request.manifest.accounts().iter().map(|account| account.label().to_owned()).collect();
    if let Some(label) = request.account {
        return labels.iter().any(|found| found == label).then(|| label.to_owned()).ok_or(Launched::Refused(REASON_ACCOUNT_UNKNOWN));
    }
    let events = store::read_all(&request.state_dir.path).map_err(|_| Launched::Refused(REASON_LOG_UNREADABLE))?;
    let state = replay(&events);
    let anchor = request.anchor.display().to_string();
    match choose((request.role, anchor.as_str()), &state, &labels, request.model, request.threshold_pct) {
        Selection::Chosen(label) => Ok(label),
        Selection::None(found) => Err(Launched::None(found)),
    }
}

/// 起動行を送る前の 3 手（順序固定）: session の実在（無ければ `session-missing`・**row を書かない**）→ 登録 row を
/// 先に書く（`sid` 無し・`launch` = 導出した行）→ window（[`open_window`]）。導出した行（穴を埋める前）を返す。
fn prepare(request: &Launch, label: &str) -> Result<String, &'static str> {
    let Some((session, window)) = request.target.split_once(':') else {
        return Err(REASON_SESSION_MISSING);
    };
    if super::tmux_stdout(request.socket, &["has-session", "-t", &format!("={session}")]).is_none() {
        return Err(REASON_SESSION_MISSING);
    }
    let derived = derive_launch(request.anchor, request.manifest.plugins(), request.manifest.launch_args());
    let row = Registration {
        role: request.role,
        anchor: request.anchor.display().to_string(),
        target: request.target.to_owned(),
        sid: None,
        account: label.to_owned(),
        launch: derived.clone(),
        model: request.model.map(str::to_owned),
    };
    role::register(&request.state_dir.path, row).map_err(|_| REASON_REGISTER)?;
    open_window(request, session, window)?;
    Ok(derived)
}

/// window を用意する: 無ければ `new-window -t <session> -n <window>` で作り、shell の prompt が描かれるまで窓（`settle`）の内で
/// 待つ（作った直後の空の pane は門が `input-unknown` で断るので、門の前に描画を待つ・門の判定そのものは [`boot`]）。
/// 在れば前面 process が shell であることだけを確かめる（走っている席へ起動行を送らない・`not-a-shell`）。
///
/// session は `=<session>:` で名指す: `=` は前方一致でない exact の名・末尾の `:` は「その session の次の空き index」
/// （`-t <session>` の裸の名は、session と同じ名の window が在る周に **window** として解決され `index in use` で落ちる・
/// 実測 2026-09-14 tmux 3.6b）。
fn open_window(request: &Launch, session: &str, window: &str) -> Result<(), &'static str> {
    let exact = format!("={session}");
    let windows = super::tmux_stdout(request.socket, &["list-windows", "-t", &exact, "-F", "#{window_name}"]).unwrap_or_default();
    if windows.lines().any(|found| found == window) {
        return super::pane_is_shell(request.socket, request.target).then_some(()).ok_or(REASON_NOT_SHELL);
    }
    if !tmux_ok(request.socket, &["new-window", "-t", &format!("{exact}:"), "-n", window]) {
        return Err(REASON_WINDOW);
    }
    let deadline = Instant::now().checked_add(request.settle);
    while deadline.is_some_and(|at| Instant::now() < at) {
        let pane = super::tmux_stdout(request.socket, &["capture-pane", "-p", "-J", "-t", request.target]);
        if pane.is_some_and(|found| super::shell_input_empty(&found).is_ok()) {
            break;
        }
        sleep(request.step);
    }
    Ok(())
}

/// 起動を `inject.jsonl` に 1 行記録する（`who=seat-launch`・`what` は tick の判定行と同じ `decision=inject … kind=launch`
/// の形＝tick の立て直しの入口 (1) が「直近の注入」として読む・`when=launch`）。**置き場へ書けない周も結果を変えない**。
fn record_launch(request: &Launch, label: &str, started: Instant) {
    let kind = super::tick::InjectKind::Launch.as_str();
    let what = format!("decision=inject target={} kind={kind} account={label}", sanitize_target(request.target));
    let entry = InjectionRecord {
        schema: SCHEMA,
        who: WHO_LAUNCH.to_owned(),
        bytes: what.len() as u64,
        what,
        when: WHEN_LAUNCH.to_owned(),
        // 数えていないことを 0 と書かない。
        tokens: None,
        wall_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        seat: seat_name(request.target),
        ts: state::now_secs(),
    };
    let _ = crate::hook::append(&request.state_dir.path, &entry);
}

/// `seat launch` の 1 行（成立・断り・失敗）。置き場の 2 語を末尾に載せる（cycle と同じ規律）。
pub fn render_launched(target: &str, result: &Launched, state: &StateDir) -> String {
    let suffix = state.suffix();
    let target = sanitize_target(target);
    match result {
        Launched::Done(label, None) => format!("seat launch: launched target={target} account={label}{suffix}"),
        Launched::Done(label, Some(settled)) => {
            format!("seat launch: launched target={target} account={label} consumed={}{suffix}", settled.as_str())
        }
        Launched::None(found) => {
            format!("seat launch: refused reason={REASON_NO_ACCOUNT} detail={} target={target}{suffix}", found.reason.as_str())
        }
        Launched::Refused(reason) => format!("seat launch: refused reason={reason} target={target}{suffix}"),
        Launched::Failed(reason) => format!("seat launch: failed reason={reason} target={target}{suffix}"),
    }
}

#[cfg(test)]
mod tests {
    use super::{derive_launch, fill_launch, with_agent_view_off, Holes, HOLE, HOLES};
    use crate::order::is_declaration_order;
    use crate::rules::manifest::Manifest;
    use std::path::Path;

    /// 起動行の導出（契約 (6f)・account-lifecycle.md §4）: 穴は `{account_dir}` の 1 つ（[`fill_launch`] がそのまま埋める）・
    /// 順序は agent view off → 口座の env → `claude` → anchor の `--plugin-dir` → `[[plugin]]` の dir（宣言順）→
    /// `[[launch-arg]]` の value（宣言順）。plugin 0 件・引数 0 件は anchor の `--plugin-dir` だけで終わる。
    #[test]
    fn seat_launch_derive_line_orders_anchor_plugins_and_args_with_one_hole() {
        let host = "schema = 1\n\n[[plugin]]\ndir = \"/opt/p2\"\n\n[[launch-arg]]\nvalue = \"--permission-mode\"\n\n\
                    [[plugin]]\ndir = \"/opt/p1\"\n\n[[launch-arg]]\nvalue = \"bypassPermissions\"\n";
        let manifest = Manifest::parse(host).unwrap_or_default();
        assert_eq!(manifest.plugins().len(), 2, "fixture が読める");
        let line = derive_launch(Path::new("/repo/main"), manifest.plugins(), manifest.launch_args());
        assert_eq!(
            line,
            "CLAUDE_CODE_DISABLE_AGENT_VIEW=1 CLAUDE_CONFIG_DIR={account_dir} claude --plugin-dir /repo/main \
             --plugin-dir /opt/p2 --plugin-dir /opt/p1 --permission-mode bypassPermissions",
            "宣言順（p2 → p1・--permission-mode → bypassPermissions）"
        );
        assert_eq!(line.matches(HOLE).count(), 1, "穴は 1 つ");
        assert_eq!(
            fill_launch(&line, "/state/accounts/a2").as_deref(),
            Ok("CLAUDE_CODE_DISABLE_AGENT_VIEW=1 CLAUDE_CONFIG_DIR=/state/accounts/a2 claude --plugin-dir /repo/main \
                --plugin-dir /opt/p2 --plugin-dir /opt/p1 --permission-mode bypassPermissions"),
            "穴は既存の fill_launch で埋まる"
        );
        assert_eq!(with_agent_view_off(&line), line, "前置は既に在る（二重にしない）");
        let bare = derive_launch(Path::new("/repo/main"), &[], &[]);
        assert_eq!(bare, "CLAUDE_CODE_DISABLE_AGENT_VIEW=1 CLAUDE_CONFIG_DIR={account_dir} claude --plugin-dir /repo/main");
    }

    /// 起動行の先頭に agent view を切る env を 1 つだけ前置する: 行の中身は変えず、既に前置済みの行は二重にせず、空の行は
    /// そのまま（契約 (c)・`s2-07l.239`）。
    #[test]
    fn seat_agent_view_off_prefix_is_single_and_keeps_blank_lines() {
        let line = "CLAUDE_CONFIG_DIR=/state/accounts/a2 claude --resume";
        let once = with_agent_view_off(line);
        assert_eq!(once, "CLAUDE_CODE_DISABLE_AGENT_VIEW=1 CLAUDE_CONFIG_DIR=/state/accounts/a2 claude --resume");
        assert_eq!(with_agent_view_off(&once), once, "前置済みの行は二重にしない");
        assert_eq!(
            with_agent_view_off("sh l.sh /state/accounts/a2"),
            "CLAUDE_CODE_DISABLE_AGENT_VIEW=1 sh l.sh /state/accounts/a2",
            "env で始まらない雛形にも前置する"
        );
        assert_eq!(with_agent_view_off(""), "", "空の行はそのまま");
        assert_eq!(with_agent_view_off("  "), "  ", "空白だけの行もそのまま");
    }

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
