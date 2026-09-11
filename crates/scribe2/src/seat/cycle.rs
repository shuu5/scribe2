//! session を作り直して復元する口（設計 §3・SRS FR28 / FR23・憲法 CON5 / C11 / C10）。
//!
//! `/clear` は**不可逆**である（会話は戻らない）。ゆえにこの口は 3 つの条件が同時に立つ周
//! だけ開く: 排他（lock を握れた）・退避済み（自席の未 consumed 退避物が在る）・idle
//! （打ちかけが無い）。1 つでも欠けたら **1 key も送らずに断る**——「送ったが失敗した」と
//! 「そもそも送っていない」を [`Cycle`] で分けて持つのはこのためである（bool で持たない）。

use super::{inject, is_idle, pane_of, sanitize_target, tmux_ok, WmScan};
use crate::fleet::json_lite::{self, Value};
use crate::fleet::store::{self, LockPolicy};
use crate::hook::{InjectionRecord, SCHEMA};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime};

/// 排他 marker の名前。
pub const LOCK_FILE: &str = "cycle.lock";
/// TTL を宣言する rules 行の id（**値は code に焼かない**・憲法 C5）。
const ID_TTL: &str = "seat.cycle_lock_ttl_s";
/// session を作り直す注入。
const CLEAR: &str = "/clear";
/// 復元の既定 command。
pub const DEFAULT_RESTORE: &str = "/rebrief";
/// 作り直しを待つ上限。
const CLEAR_WAIT: Duration = Duration::from_secs(30);
/// 作り直しを見に行く周期。
const CLEAR_STEP: Duration = Duration::from_millis(500);
/// 記録の who。
const WHO: &str = "seat-cycle";
/// 記録の when。
const WHEN: &str = "cycle";

/// 他の cycle が走っている。
pub const REASON_LOCK_HELD: &str = "lock-held";
/// 自席の未 consumed 退避物が無い（CON5: 退避物なしで撃たない）。
pub const REASON_WM_MISSING: &str = "wm-missing";
/// 退避物の dir を読めない（**0 件と読み替えない**）。
pub const REASON_WM_UNREADABLE: &str = "wm-unreadable";
/// 席が打ちかけである。
pub const REASON_BUSY: &str = "busy";
/// pane を読めない（idle と名乗れない）。
pub const REASON_PANE_MISSING: &str = "pane-missing";
/// TTL の宣言（rules 行）が読めない。
pub const REASON_NO_RULE: &str = "no-rule";
/// 置き場を解けない。
pub const REASON_STATE_DIR: &str = "state-dir";
/// `/clear` は送ったが作り直しを確認できない。
pub const REASON_CLEAR: &str = "clear-unconfirmed";
/// 復元 command は送ったが送達を確認できない。
pub const REASON_RESTORE: &str = "restore-unconfirmed";

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
    /// 解決済みの置き場（`<state_dir>`）。
    pub state_dir: &'a Path,
    /// 復元 command（既定 [`DEFAULT_RESTORE`]）。
    pub restore: Option<&'a str>,
}

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
    let dir = super::seat_dir(request.state_dir, request.target);
    let result = perform(request, &dir);
    record(request, &result, started);
    result
}

/// lock を取り、握っている間の手順を回して、**どの枝でも lock を返す**。
fn perform(request: &Request, dir: &Path) -> Cycle {
    let Some(ttl) = ttl_s() else {
        return Cycle::Refused(REASON_NO_RULE);
    };
    match take_lock(dir, ttl) {
        Lock::Taken => {}
        Lock::Held => return Cycle::Refused(REASON_LOCK_HELD),
        Lock::Broken => return Cycle::Refused(REASON_STATE_DIR),
    }
    let held = guarded(request);
    std::fs::remove_file(lock_path(dir)).ok();
    held
}

/// lock を握っている間の手順（順序固定）。
fn guarded(request: &Request) -> Cycle {
    match super::scan_wm(Path::new(request.wm_dir), request.target) {
        WmScan::None => return Cycle::Refused(REASON_WM_MISSING),
        WmScan::Unreadable => return Cycle::Refused(REASON_WM_UNREADABLE),
        WmScan::Unconsumed(_) => {}
    }
    let Some(pane) = pane_of(request.socket, request.target, request.capture_file) else {
        return Cycle::Refused(REASON_PANE_MISSING);
    };
    if !is_idle(&pane) {
        return Cycle::Refused(REASON_BUSY);
    }
    if !send_clear(request) {
        return Cycle::Failed(REASON_CLEAR);
    }
    if send_restore(request) {
        Cycle::Done
    } else {
        Cycle::Failed(REASON_RESTORE)
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

/// `/clear` を送り、作り直しを確認する。
///
/// **便 2 の送達確認（settle）は使えない**: `/clear` は pane を消すので「送った字面が現れる」
/// 形では測れず、成功したときほど確認が落ちる。代わりに [`cleared`]（入力欄が空 ∧ 入力行より
/// 上に消費済みの echo）で見る＝作り直された席でだけ同時に立つ。
fn send_clear(request: &Request) -> bool {
    if !send_line(request, CLEAR) {
        return false;
    }
    let deadline = Instant::now().checked_add(CLEAR_WAIT);
    while deadline.is_some_and(|at| Instant::now() < at) {
        sleep(CLEAR_STEP);
        if pane_of(request.socket, request.target, request.capture_file)
            .is_some_and(|pane| cleared(&pane))
        {
            return true;
        }
    }
    false
}

/// 作り直しの済んだ pane か: **入力欄が空 ∧ 入力行より上に消費済みの echo `❯ /clear` が在る**
/// （[`is_consumed_echo`]）。
///
/// **正の証拠で見る**（便 2 の裁定「送った字面が現れた = 送達成功・入力欄が空 = 消費」と同じ形・
/// bd `s2-07l.96`）。作り直された席は画面を消した後、消費済みの echo を新しい prompt の**直上に
/// 必ず**残す（実測 2026-09-11: banner 3 行 → `❯ /clear` → 空の prompt → statusline）。
/// 「探索域に `/clear` の字面が無い」の負の形は、この echo のせいで**構造的に偽のまま固定**され
/// ——席が 6 行以上を出力するまで真にならず、その出力は復元を送った後にしか出ない——`/clear` が
/// 通った席に復元を送らず空席のまま残した（実測 2026-09-11 admin 席・`.94` の域変更で極性が反転）。
///
/// **未 submit の `/clear` は弁別できる**: Enter だけが落ちた周の字面は**入力行そのもの**に残り
/// [`super::input_tail`] が非空になる（第 1 項で落ちる）。echo は入力行より**上**にしか無い。
///
/// **域は入力行より上の全行**（[`super::prompt_region`] の上 6 非空行に絞らない）。`/clear` は
/// 画面を消すので、見えている echo は作り直しの後に描かれたものに限られ、上へ遡る数で古い echo を
/// 除外する必要が無い。逆に 6 行で切ると、echo の下に描かれる行（hook の出力等）が版で増えた周に
/// 同じ行き止まりへ戻る。typed の証拠（SessionStart の打刻）は `s2-07l.95` の領分。
fn cleared(pane: &str) -> bool {
    let lines: Vec<&str> = pane.lines().collect();
    let Some(at) = lines.iter().rposition(|line| line.contains(super::PROMPT)) else {
        return false;
    };
    super::input_tail(pane).is_some_and(str::is_empty)
        && lines.iter().take(at).any(|line| is_consumed_echo(line))
}

/// 消費済みの echo の行か: **行頭**が prompt の字で、その右が `/clear` ちょうど。
///
/// 行頭に限るのは、assistant の本文と tool の出力が 2 桁字下げで描かれるためである——本文が
/// echo の字面を**引用**した行（`  ❯ /clear`）は行頭に来ない。字下げを剥がして比べると、引用を
/// 持つ idle な席を「作り直された」と読み、会話を捨てていない席へ復元が刺さる。右側の trim は
/// prompt が描く nbsp を含む（`str::trim` は U+00A0 を空白に数える）。
fn is_consumed_echo(line: &str) -> bool {
    line.strip_prefix(super::PROMPT)
        .is_some_and(|rest| rest.trim() == CLEAR)
}

/// 復元 command を **便 2 の inject 経路**で送る（送達確認まで込み）。
///
/// 成功と数えるのは **席がその場で消費した**（[`inject::Settled::Consumed`]）周だけ。`/clear` の
/// 直後の席には走っている turn が無いので、そこで入力欄に残った復元は「turn の終わりに消費
/// される queue」ではなく submit されなかった打鍵で、会話を捨てた（不可逆）のに復元が刺さらない
/// 席を `done` と数えることになる（lens-90 HIGH-2）。旧来の「現れた ∧ 入力欄が空」と同じ意味。
fn send_restore(request: &Request) -> bool {
    let Some(state) = request.state_dir.to_str() else {
        return false;
    };
    let payload = request.restore.unwrap_or(DEFAULT_RESTORE);
    let sent = inject::deliver(&inject::Request {
        target: request.target,
        socket: request.socket,
        payload,
        state_dir: Some(state),
    });
    matches!(sent, inject::Delivery::Delivered(_, inject::Settled::Consumed))
}

/// 1 行を literal で送り、Enter を送る。
fn send_line(request: &Request, text: &str) -> bool {
    let target = request.target;
    tmux_ok(request.socket, &["send-keys", "-t", target, "-l", text])
        && tmux_ok(request.socket, &["send-keys", "-t", target, "Enter"])
}

/// 結果を 1 語（＋理由）にする。tick 行の末尾へも同じ字面が載る。
pub fn summary(result: &Cycle) -> String {
    match *result {
        Cycle::Done => "done".to_owned(),
        Cycle::Refused(reason) => format!("refused reason={reason}"),
        Cycle::Failed(reason) => format!("failed reason={reason}"),
    }
}

/// 成立の 1 行。
pub fn render(target: &str, result: &Cycle) -> String {
    match *result {
        Cycle::Done => format!("seat: cycle done target={}", sanitize_target(target)),
        _ => format!("seat: cycle {}", summary(result)),
    }
}

/// 1 回を記録する。**置き場が解けない周は書かない**（rc は変えない）。
fn record(request: &Request, result: &Cycle, started: Instant) {
    let what = format!("cycle {}", summary(result));
    let entry = InjectionRecord {
        schema: SCHEMA,
        who: WHO.to_owned(),
        bytes: what.len() as u64,
        what,
        when: WHEN.to_owned(),
        // 数えていないことを 0 と書かない。
        tokens: None,
        wall_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    };
    let Ok(policy) = LockPolicy::embedded() else {
        return;
    };
    let path = inject::tick_path(request.state_dir, request.target);
    let _ = store::append_line(&path, &entry.to_line(), policy);
}
