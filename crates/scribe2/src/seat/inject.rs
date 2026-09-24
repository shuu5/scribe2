//! tmux pane への注入（設計 §3・記録は SRS FR21 と同じ schema）。
//!
//! **送る前に入力欄を見る**: 人間の打ちかけと 1 行に merge する co-submit 事故を、
//! 「非空なら 1 key も送らない」で構造的に塞ぐ（prompt 行を特定できない周も送らない
//! ＝fail-closed）。rc は **0 / 1 の 2 値**だけで、v1 の偽陰性（4 / 7）を作らない。
//! 非空の残りは**器自身の記録との一致**だけで 2 つに割れる（門は 3 値・[`guard_input`] /
//! [`pass_input`]・`s2-07l.288`）: 直近の自席注入の文がそのまま残っている周（[`InputPass::OwnQueued`]）は
//! Enter を 1 回だけ送って着地させ、人間の打ちかけ（Foreign）は従来どおり 1 key も送らない。
//!
//! pane を読むのは**入力欄の門と送達の目印**（送った字面が現れた = 送達・`.90`）だけで、
//! **消費（`consumed=`）は席の打刻**で決める（送達 ts 以後の `UserPromptSubmit`・
//! [`state::evidence_after`]・設計 seat-state.md §6・`s2-07l.112`）。入力欄が空になったかは読まない。
//! 目印の照合は**空白を畳んだ字面**で行う（[`folded`]・`s2-07l.296`）: 開発 session の TUI は入力欄と
//! echo を自分の幅で折り返して描くので、pane 幅と同じ長さの目印は硬い改行と字下げで割れて現れる。
//!
//! 不可逆の口は持たない（憲法 CON5）: ここが送るのは呼び側が渡した 1 行だけで、
//! `/clear` のような session を作り直す注入はこの便では扱わない。

use crate::polarity::{OnFailure, Polarity, Timing};
use super::{input_tail, sanitize_target, state, tmux_ok, tmux_stdout, InputGate, StateDir, REASON_TMUX_FAILED};
use crate::fleet::json_lite::{self, Value};
use crate::fleet::store::{self, LockPolicy};
use crate::hook::{seat_name, InjectionRecord, SCHEMA};
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};

/// settle の 1 回あたりの待ち。
const SETTLE_STEP: Duration = Duration::from_millis(200);
/// settle の最大回数（既定の窓 = 2 s）。呼び側が窓を渡す口は [`deliver_within`]。
const SETTLE_TRIES: u32 = 10;
/// 記録に載せる payload の先頭 byte 数。
const WHAT_CAP: usize = 80;
/// 記録 file の名前。
const TICK_FILE: &str = "tick.jsonl";
/// 記録の置き場（state dir 直下の dir 名）。
const SEAT_DIR: &str = "seat";
/// 記録の who。
const WHO: &str = "seat-inject";
/// 記録の when。
const WHEN: &str = "inject";

/// 入力欄が非空（人間が打ちかけている）。
pub const REASON_BUSY: &str = "busy";
/// 入力欄を特定できない。
pub const REASON_UNKNOWN_INPUT: &str = "unknown-input";
/// payload に非空の行が 1 つも無い（送達の目印を持てない）。
pub const REASON_EMPTY: &str = "empty";
/// 送ったが pane に現れない。
pub const REASON_ABSENT: &str = "absent";

/// 注入 1 回の入力。
pub struct Request<'a> {
    /// tmux target（pane の指定）。
    pub target: &'a str,
    /// tmux の socket（既定の server を使うなら `None`）。
    pub socket: Option<&'a str>,
    /// 送る 1 行。
    pub payload: &'a str,
    /// 解決済みの置き場（出所付き・`None` = 解けない周＝記録しない・表示に 2 語を出さない）。
    /// **解決は呼び側が 1 回だけ行う**: 表示と記録が別々に解くと、settle の窓の内に git 設定が
    /// 変わった周に表示行が書いてもいない dir を名乗る（lens-100 HIGH-1・実測 2026-09-11）。
    pub state_dir: Option<&'a StateDir>,
}

/// 送達した注入を席が**その場で消費したか**（送達 ts 以後に `UserPromptSubmit` の打刻が足されたか・
/// 設計 §6・`s2-07l.112`）。
///
/// 席が busy な周は注入が入力欄に queue され、turn が終わる（次の submit）まで打刻が来ない。それでも
/// 送達は成功している（実測 2026-09-11: `inject-residual` 9 件が全部 turn の終わりに消費されていた・
/// bd `s2-07l.90`）ので、これは成功の**記録の detail** であって失敗の理由ではない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Settled {
    /// 送達 ts 以後の `UserPromptSubmit` の打刻が在る＝席が消費した。
    Consumed,
    /// 打刻 file は読めるが窓の内に新しい打刻が無い＝queue された（次の submit で消費される）。
    Queued,
    /// 消費を**測れない**（打刻 file が無い・読めない・置き場が解けない）。`false` と混ぜず、
    /// missing を消費と読み替えない（憲法 C10 の測定 / 未測定の弁別）。理由は [`Unmeasured`]。
    Unmeasured(Unmeasured),
}

/// 消費を測れない理由。**閉じた 3 値**（憲法 C11）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unmeasured {
    /// 打刻 file が無い（hook が載っていない席）。
    StateMissing,
    /// 打刻 file を読めない。
    StateUnreadable,
    /// 置き場が解けない（打刻の在処を知らない）。
    StateDir,
}

impl Unmeasured {
    /// 行に添える字面（`reason=<値>`）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StateMissing => "state-missing",
            Self::StateUnreadable => "state-unreadable",
            Self::StateDir => "state-dir",
        }
    }
}

impl Settled {
    /// 記録と表示の字面（`consumed=true|false|unknown`）。Enter が落ちた周は消費していない＝`false`。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Consumed => "true",
            Self::Queued => "false",
            Self::Unmeasured(_) => "unknown",
        }
    }

    /// `consumed=` に添える理由（queue と消費の周は `None`）。Enter が落ちた周は `false` だけでは
    /// queue と同じ顔になるので理由を名乗る（憲法 C10）。
    pub fn reason(self) -> Option<&'static str> {
        match self {
            Self::Consumed | Self::Queued => None,
            Self::Unmeasured(why) => Some(why.as_str()),
        }
    }
}

/// この境界の極性（[`Delivery::Refused`]）: 送る前に入力欄を見て、人間の打ちかけ・prompt 行を特定できない
/// 周は 1 key も送らない（自席の文が残る周に送る Enter 1 回は、その文を着地させるだけで text を運ばない）。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailClosed,
};

/// 注入 1 回の結果。
pub enum Delivery {
    /// 送って送達を確認した（byte 数・その場で消費したか）。
    Delivered(u64, Settled),
    /// **1 key も送っていない**（入力欄の状態で止めた）。
    Refused(&'static str),
    /// 送ったが送達を確認できない。
    Unconfirmed(&'static str),
}

/// 注入を 1 回行い、settle を `window` まで見続ける（注入の入口・`s2-07l.479.3` で既定窓の入口 `deliver` は口ごと
/// 消えた・送りを未確認でも記録する口は [`deliver_or_confirm`] で、門・送り・settle は同じ本体 [`attempt`] を通る）。
///
/// 成功の形は「目印が現れた = 送達・送達 ts 以後の `UserPromptSubmit` の打刻 =
/// `Consumed`・窓の終わりに無ければ `Queued`」で、呼び側が決めるのは**窓の長さだけ**。作り直し直後の席は
/// SessionStart hook の間（数秒〜十数秒）注入を入力欄に queue したまま turn を始めないので、2 s の
/// 窓では復元が正しく届く周ほど `Queued` に落ちる（bd `s2-07l.97`）。cycle は作り直しの確認と同じ
/// 上限を渡す。**窓はここで決めない**（`s2-07l.151`）: cycle 側の rules 行
/// （`seat.cycle_settle_s`）が持つ値がそのまま引数で来る＝この面は規則を読まない。
///
/// `Queued` で窓を閉じた周は、入力欄の残りがこの周の本文なら Enter を 1 回だけ再送して同じ窓でもう 1 度
/// settle する（[`nudge_own`]・設計 dispatcher.md §21）。Enter はこの呼び出しで最大 2 回・text の再送は 0 回。
pub fn deliver_within(request: &Request, window: Duration) -> Delivery {
    attempt(request, window, Ledger::Delivered).unwrap_or_else(|_| Delivery::Refused(Blocked::Foreign.as_str()))
}

/// 記録の書き方（**閉じた 2 値**・憲法 C11）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ledger<'a> {
    /// 送達を確認した周だけ [`WHO`] の名で書く（従来の注入）。
    Delivered,
    /// 送った周は送達の確認の有無に依らず `who` の名で書く（設計 account-lifecycle.md §22 形 1）。
    Sent(&'a str),
}

/// 門が `Foreign` で**直に**断った周の pane（送る前の 1 回目の読み・OwnQueued の Enter を挟んだ周は含まない）。
struct Foreign(String);

/// 門が `Foreign` で断った周に、入力欄の残りが既定の行なら Enter を 1 回だけ送る口の材料（設計 account-lifecycle.md §22 形 2）。
pub struct Confirm<'a> {
    /// 送りの記録の `who`（呼び手の名・payload の周も Enter の周も同じ名）。
    pub who: &'a str,
    /// 既定の行の literal（入力欄の残りと [`folded`] で畳んで等値で比べる）。
    pub row: &'a str,
    /// Enter を送った周の記録の `what`（固定の字面）。
    pub what: &'a str,
}

/// [`deliver_or_confirm`] の結果（**閉じた 2 値**・憲法 C11）。
pub enum Sent {
    /// payload の送り（門を通った周・門が断った周）。
    Payload(Delivery),
    /// 既定の行へ Enter を 1 回送った（`false` = tmux が落ちた）。
    Confirmed(bool),
}

/// 注入を 1 回行う口の 2 本目（設計 account-lifecycle.md §22）: [`deliver_within`] と同じ門・送り・settle を通し、送った周は
/// `Delivered` でも `Unconfirmed` でも `confirm.who` の名で記録に 1 行残す（`Refused` の周と、送る前に pane を読めない周は
/// 送っていない＝残さない）。門が `Foreign` で直に断り、入力欄の残り（畳んだ字面）が `confirm.row` に等しい周だけ、payload を
/// 送らず Enter を 1 回だけ送って `confirm.what` の 1 行を残す。残りがそれ以外の周は従来どおり 1 key も送らない（門は緩めない・N1）。
pub fn deliver_or_confirm(request: &Request, window: Duration, confirm: &Confirm) -> Sent {
    let (started, who) = (Instant::now(), confirm.who);
    let Foreign(pane) = match attempt(request, window, Ledger::Sent(who)) {
        Ok(delivery) => return Sent::Payload(delivery),
        Err(foreign) => foreign,
    };
    let at_row = input_tail(&pane).is_some_and(|tail| folded(tail) == folded(confirm.row));
    if !at_row {
        return Sent::Payload(Delivery::Refused(Blocked::Foreign.as_str()));
    }
    let sent = send_enter(request.socket, request.target);
    if sent {
        record(request, who, confirm.what, 0, started);
    }
    Sent::Confirmed(sent)
}

/// 注入 1 回の本体（[`deliver_within`] と [`deliver_or_confirm`] の共通）。門が `Foreign` で直に断った周だけ `Err` で
/// 送る前の pane を返す（呼び手が断りの字面か既定の行への Enter かを決める）。
fn attempt(request: &Request, window: Duration, ledger: Ledger<'_>) -> Result<Delivery, Foreign> {
    let started = Instant::now();
    let Some(pane) = capture(request.socket, request.target) else {
        return Ok(Delivery::Unconfirmed(REASON_TMUX_FAILED));
    };
    let own = request
        .state_dir
        .and_then(|found| last_own_payload(&found.path, request.target));
    let recapture = || capture(request.socket, request.target);
    // 門の 1 回目の読みが直に Foreign の周だけを呼び手へ返す（OwnQueued の Enter を挟んで Foreign に倒れた周は既に 1 key 送った
    // ＝同じ周にもう 1 key 送らない・設計 account-lifecycle.md §22 形 3）。
    let direct = guard_input(&pane, own.as_deref());
    match pass_input(request.socket, request.target, &pane, own.as_deref(), recapture) {
        Ok(()) => {}
        Err(Blocked::Foreign) if direct == Err(InputGate::Busy) => return Err(Foreign(pane)),
        Err(blocked) => return Ok(Delivery::Refused(blocked.as_str())),
    }
    // 送る**前**の pane で目印の出現数を数えておく: 同じ字面が先に在る（前周の pointer の写し・
    // tool の出力の引用）と `contains` 1 本では届いていない周が「届いた」に化ける（lens-90 HIGH-1）。
    let Some(marker) = needle_of(request.payload) else {
        return Ok(Delivery::Refused(REASON_EMPTY));
    };
    let before = folded(&pane).matches(marker.as_str()).count();
    // 消費の証拠を見る先も送る**前**に取る（基線と送達 ts・設計 §6）。
    let seat = request
        .state_dir
        .map(|found| super::seat_dir(&found.path, request.target));
    let watch = Watch {
        seat: seat.as_deref().map(|dir| (dir, state::baseline(dir))),
        since: state::now_secs(),
    };
    if !send(request) {
        return Ok(Delivery::Unconfirmed(REASON_TMUX_FAILED));
    }
    let tries = tries_within(window);
    // **Queued の周は同じ呼び出しの中で Enter を 1 回だけ再送する**（設計 dispatcher.md §21 形 2）: 入力欄の残りが
    // この周の本文なら Enter だけを送って同じ窓でもう 1 度 settle し、2 度目も Queued ならそのまま返す（3 回目は無い）。
    let settled = settle(request, &marker, before, tries, &watch).and_then(|found| match found {
        Settled::Queued if nudge_own(request) => settle(request, &marker, before, tries, &watch),
        other => Ok(other),
    });
    let bytes = request.payload.len() as u64;
    let what = head(request.payload, WHAT_CAP);
    Ok(match (settled, ledger) {
        (Ok(settled), Ledger::Delivered) => {
            record(request, WHO, &what, bytes, started);
            Delivery::Delivered(bytes, settled)
        }
        (Ok(settled), Ledger::Sent(who)) => {
            record(request, who, &what, bytes, started);
            Delivery::Delivered(bytes, settled)
        }
        (Err(reason), Ledger::Delivered) => Delivery::Unconfirmed(reason),
        // 送った周は目印が現れなくても残す（dialog が出て echo の無い `/exit`・設計 account-lifecycle.md §22 形 1）。
        (Err(reason), Ledger::Sent(who)) => {
            record(request, who, &what, bytes, started);
            Delivery::Unconfirmed(reason)
        }
    })
}

/// 消費の証拠を見る先（送る**前**に取る・設計 §6）。置き場が解けない周は `seat` が `None`＝測れない。
struct Watch<'a> {
    /// 席の置き場と、送る前の打刻の基線。
    seat: Option<(&'a Path, state::Baseline)>,
    /// 送達 ts（打刻と同じ時計・秒）。
    since: u64,
}

impl Watch<'_> {
    /// いまの証拠の読み（置き場が解けない周は `None`）。
    fn evidence(&self) -> Option<state::Evidence> {
        self.seat.map(|(dir, baseline)| {
            state::evidence_after(dir, baseline, state::Event::UserPromptSubmit, self.since)
        })
    }
}

/// 窓の終わりの証拠を [`Settled`] に写す（`Found` は途中で返るので届かないが、網羅のため写す）。
fn settled_of(evidence: Option<state::Evidence>) -> Settled {
    match evidence {
        Some(state::Evidence::Found(_)) => Settled::Consumed,
        Some(state::Evidence::NotYet) => Settled::Queued,
        Some(state::Evidence::Missing) => Settled::Unmeasured(Unmeasured::StateMissing),
        Some(state::Evidence::Unreadable) => Settled::Unmeasured(Unmeasured::StateUnreadable),
        None => Settled::Unmeasured(Unmeasured::StateDir),
    }
}

/// 入力欄の門を**通った**形（**閉じた 2 値**・憲法 C11・`s2-07l.288`）。3 値（Clear / OwnQueued /
/// Foreign）の残り 1 つは [`InputGate::Busy`] が持つ。
///
/// [`InputGate`] に variant を足さないのは、同じ型を shell の門（[`super::shell_input_empty`]）も返し、
/// その網羅 match（`cycle/relaunch.rs`）は本便の write-set の外だからである——shell の pane に
/// 「自席の注入文」は無い。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputPass {
    /// 入力欄が空。
    Clear,
    /// 残りが**直近の自席注入の記録**に前方一致する＝器自身が queue した文（[`own_queued`]）。
    OwnQueued,
}

/// 入力欄の門を**通せなかった**理由（**閉じた 3 値**・憲法 C11）。字面は呼び手の語彙が持つ
/// （`seat inject` は `busy` / cycle と exit は `input-busy`）ので、ここでは型のまま返す
/// ——3 呼び手がこの 1 つの enum を網羅 match する（憲法 C2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blocked {
    /// 人間の打ちかけ（記録と一致しない非空の残り）。
    Foreign,
    /// prompt 行を特定できない。
    UnknownInput,
    /// 自席の文が Enter を 1 回送った後も残る。
    OwnQueued,
}

impl Blocked {
    /// 従来の 2 値からの写し（[`InputGate`] に variant を足さない）。
    fn of(gate: InputGate) -> Self {
        match gate {
            InputGate::Busy => Self::Foreign,
            InputGate::UnknownInput => Self::UnknownInput,
        }
    }

    /// **注入の面**の字面（cycle と exit は `input-` を前置いた自分の語彙を持つ）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Foreign => REASON_BUSY,
            Self::UnknownInput => REASON_UNKNOWN_INPUT,
            Self::OwnQueued => super::cycle::REASON_INPUT_OWN_QUEUED,
        }
    }
}

/// 送る前の入力欄の門（co-submit 止め・**送達の面の唯一の字面読み**）: prompt 行を特定できない
/// pane は [`InputGate::UnknownInput`]、入力欄が非空なら [`InputGate::Busy`] で、どちらも 1 key も
/// 送らない側へ倒す。席の busy / idle の判定ではない（それは [`super::state`] が typed に持つ・
/// ADR-0015）——人間の打ちかけと 1 行に merge する事故を、送る直前の入力欄で塞ぐ門である。
/// cycle の `/clear` も同じ門を通る（第 2 の判定を作らない）。
///
/// 非空の残りは**器自身の記録との一致だけ**で 2 つに割れる（`s2-07l.288`・憲法 C3.3）: `own`
/// （直近の自席注入の記録＝[`last_own_payload`]）に前方一致すれば [`InputPass::OwnQueued`]
/// （呼び側は Enter を 1 回だけ送って着地させる）、しなければ従来どおり [`InputGate::Busy`]。
/// 記録が無い・読めない周（`own` が `None`）は非空 = Foreign のまま＝門は緩まない。
pub fn guard_input(pane: &str, own: Option<&str>) -> Result<InputPass, InputGate> {
    match input_tail(pane) {
        None => Err(InputGate::UnknownInput),
        Some("") => Ok(InputPass::Clear),
        Some(tail) if own.is_some_and(|record| own_queued(tail, record)) => Ok(InputPass::OwnQueued),
        Some(_) => Err(InputGate::Busy),
    }
}

/// 入力欄の残りが**直近の自席注入の記録**を丸ごと頭に持つか（`s2-07l.288`）。どちらも [`folded`] で
/// 畳み、**残り（`tail`）が記録（`record`）で始まる**一方向だけを見る。記録は payload の先頭
/// [`WHAT_CAP`] byte までなので、長い payload を queue した周は残りの方が長い——その形が自席の文である。
///
/// 逆向き（記録が残りの頭に一致＝`record.starts_with(tail)`）は**書かない**: 記録の頭の 1 字を打ちかけた
/// 人間の入力欄を自席の注入と読んで Enter を送る形になり、co-submit の門を緩める（N1）。代償は、折り返しを
/// 結合しない読み（cycle と exit の [`super::pane_of`]）で残りが記録より短く見える周が Foreign に倒れる
/// ことで、これは 1 key も送らない側の誤り（fail-closed）である。畳んで空の記録は弁別できない＝偽。
fn own_queued(tail: &str, record: &str) -> bool {
    let record = folded(record);
    !record.is_empty() && folded(tail).starts_with(&record)
}

/// **直近の自席注入の記録**の `what`（同じ席の [`tick_path`] を末尾から見て `seat` が一致し `who` が
/// [`WHO`] の最初の行＝[`record`] が書いた payload の先頭 [`WHAT_CAP`] byte）。file が無い・読めない・
/// 行が無い周は `None`（門は従来どおり非空 = Foreign）。
///
/// `<state_dir>/inject.jsonl` の `decision=inject …` の行は**別の記録**（`who` は seat-tick /
/// seat-launch・本文を持たない判定行）で、照合には使わない（設計 seat-autonomy.md §10 (1)）。
pub fn last_own_payload(state_dir: &Path, target: &str) -> Option<String> {
    let seat = seat_name(target)?;
    let text = std::fs::read_to_string(tick_path(state_dir, target)).ok()?;
    let last = text
        .lines()
        .rev()
        .filter_map(|line| json_lite::parse_object(line).ok())
        .find(|pairs| field(pairs, "seat") == Some(seat.as_str()) && field(pairs, "who") == Some(WHO))?;
    field(&last, "what").map(str::to_owned)
}

/// 記録 1 行の key の文字列値（`tick/exit.rs` の同じ読みと同型）。
fn field<'a>(pairs: &'a [(String, Value)], key: &str) -> Option<&'a str> {
    pairs
        .iter()
        .find(|(found, _)| found == key)
        .map(|(_, value)| value)
        .and_then(Value::as_str)
}

/// 送る前の入力欄の門を通す**3 呼び手の 1 本**（`s2-07l.288`・設計 seat-autonomy.md §10 (2)）:
/// [`guard_input`] が [`InputPass::OwnQueued`] を返した周は **Enter を 1 回だけ**送り
/// （[`nudge_enter_within`] と同じ口・text は再送しない＝二重投函にならない）、`recapture` で pane を
/// 取り直して同じ門をもう 1 度通す。空になれば通り、残れば [`Blocked::OwnQueued`] で**それ以上
/// 1 key も送らない**。
///
/// 取り直しは既定の窓（[`SETTLE_STEP`] × [`SETTLE_TRIES`]）まで刻んで見る（[`settle`] と同じ形）:
/// Enter を受けた席が入力欄を空にして prompt を描き直すまでには間が在り、1 回だけ読む形は着地した
/// 周を落とす。Enter を送れない・pane を取り直せない周も [`Blocked::OwnQueued`]（この周の中で
/// 閉じない＝次の周の門が名乗る・[`POLARITY`] は不変）。
///
/// pane の読み口を閉包で受けるのは呼び手ごとに違うためである（注入は折り返しを結合する
/// [`capture`]・cycle と exit は `--capture-file` も通る [`super::pane_of`]）。
pub fn pass_input(
    socket: Option<&str>,
    target: &str,
    pane: &str,
    own: Option<&str>,
    recapture: impl Fn() -> Option<String>,
) -> Result<(), Blocked> {
    match guard_input(pane, own) {
        Ok(InputPass::Clear) => return Ok(()),
        Ok(InputPass::OwnQueued) => {}
        Err(gate) => return Err(Blocked::of(gate)),
    }
    if !send_enter(socket, target) {
        return Err(Blocked::OwnQueued);
    }
    for _ in 0..SETTLE_TRIES {
        sleep(SETTLE_STEP);
        match recapture().as_deref().map(|found| guard_input(found, own)) {
            Some(Ok(InputPass::Clear)) => return Ok(()),
            Some(Err(gate)) => return Err(Blocked::of(gate)),
            Some(Ok(InputPass::OwnQueued)) | None => {}
        }
    }
    Err(Blocked::OwnQueued)
}

/// **Enter だけ**を 1 回送る（修復の門と入力欄の門の同じ口・`s2-07l.150` / `s2-07l.288`）。
fn send_enter(socket: Option<&str>, target: &str) -> bool {
    tmux_ok(socket, &["send-keys", "-t", target, "Enter"])
}

/// 送達の面が読む pane 本文（**折り返しを結合した論理行**・`capture-pane -p -J`・`s2-07l.148`）。
/// 撃てなければ `None`（空文字と区別する）。
///
/// 目印（payload の最初の非空行）は 1 論理行で、pane 幅より長いと端末が折り返して capture では
/// 複数行に割れる——`contains` が当たらず、届いている注入が `absent` rc 1 に倒れた（実測
/// 2026-09-12・folio2 planner・どちらも打刻の `UserPromptSubmit` で届いていた）。呼出元が再送すると
/// 二重投函になる。**読み方だけを直す**: 照合・`absent` の極性・「現れた＝送達」・消費の打刻は不変。
/// 入力欄の門（[`guard_input`]）と修復の門（[`repair_of`]）も同じ本文を読む（送達の面の読みを 1 つにする）。
/// 目印を先頭 N 字へ切り詰める案は N が規則になり、nonce 案は注入の字面を変えるので採らない。
///
/// `-J` が結合するのは**端末の折り返し**だけである。開発 session の TUI は入力欄と echo を自分の幅で
/// 折り返して描く（pane の行として硬い改行と字下げが入る）ので結合されず、pane 幅と同じ長さの目印
/// （打刻の合図 80 cell・pane 幅 80）は 1 回も当たらなかった（実測 2026-09-14: 両席の `tick.jsonl` で
/// 07:45Z 以降の `kind=pointer` が全部 `inject-absent`・memo `s2-07l.288`）。目印の照合はこの本文を
/// [`folded`] で畳んで行う（`s2-07l.296`）。
fn capture(socket: Option<&str>, target: &str) -> Option<String> {
    tmux_stdout(socket, &["capture-pane", "-p", "-J", "-t", target])
}

/// 送達の面が照合する字面（`s2-07l.296`）: ASCII の whitespace（空白・改行・tab・CR・FF）を**全部落とした**
/// 1 本の字面。TUI が入力欄と echo を自分の幅で折り返して描いた周（目印の途中に改行と字下げが入る）でも、
/// 畳めば送った字面と同じ並びになる。**全角空白（U+3000）は落とさない**＝目印の本文の一部で、TUI が
/// 折り返しで足す字ではない。
///
/// 送達の面の 3 か所の照合（送る前の出現数・settle の「増えた」・修復の門の own_draft）は**全部**この
/// 1 本を通した字面で行う（読みを 1 つにする・`.148` と同じ規律）。入力欄の門（[`guard_input`]）は
/// 畳まない: 空白だけの入力欄を非空と読む現状の極性を保つ。
fn folded(text: &str) -> String {
    text.chars().filter(|ch| !ch.is_ascii_whitespace()).collect()
}

/// payload を literal で送り、[`SETTLE_STEP`] の 1 歩を置いて Enter を送る（設計 dispatcher.md §21 形 3）。
///
/// 間を置かずに撃つと、TUI が連続入力を貼り付けと読む周に Enter が改行に畳まれ、本文が入力欄に残る。
fn send(request: &Request) -> bool {
    let target = request.target;
    if !tmux_ok(request.socket, &["send-keys", "-t", target, "-l", request.payload]) {
        return false;
    }
    sleep(SETTLE_STEP);
    tmux_ok(request.socket, &["send-keys", "-t", target, "Enter"])
}

/// Queued で窓を閉じた周の再送の門（設計 dispatcher.md §21 形 2）: pane を取り直し、入力欄の残りが**この周の本文**
/// （記録の先頭ではなく送った字面そのもの・[`own_queued`]）なら Enter を 1 回だけ送って `true`。残りが空・人の打ちかけ・
/// prompt 行を特定できない周と、pane を取れない周は 1 key も送らず `false`（text は再送しない）。
fn nudge_own(request: &Request) -> bool {
    let Some(pane) = capture(request.socket, request.target) else {
        return false;
    };
    matches!(guard_input(&pane, Some(request.payload)), Ok(InputPass::OwnQueued))
        && send_enter(request.socket, request.target)
}

/// 送達の目印 = payload の**最初の非空行**。無ければ `None`（呼び側は 1 key も送らず断る）。
///
/// 先頭行が空だと目印が空文字になり、出現数が pane の長さに化けて「pane が伸びた」だけで
/// 成立する（lens-90 再確認 NEW-1・stdin を読まない席でも `consumed=true` になった）。
fn marker_of(payload: &str) -> Option<&str> {
    payload.lines().find(|line| !line.trim().is_empty())
}

/// 送達の面が pane と突き合わせる字面 = [`marker_of`] を [`folded`] で畳んだもの。**畳んだ後にも非空を
/// 掛ける**（`s2-07l.296`）: 畳んだ字面での `matches().count()` は目印が空だと pane の長さに化ける
/// （lens-90 NEW-1 と同型）ので、畳んで空なら `None`（呼び側は [`REASON_EMPTY`] で 1 key も送らない）。
fn needle_of(payload: &str) -> Option<String> {
    let needle = folded(marker_of(payload)?);
    (!needle.is_empty()).then_some(needle)
}

/// 送達を確認する。**目印（最初の非空行を畳んだ字面・[`needle_of`]）の出現数が、畳んだ pane 本文で送る前
/// より増えた**ら成立で、消費（送達 ts 以後の `UserPromptSubmit` の打刻）が窓の内に来たかを [`Settled`]
/// として添える。
///
/// 「現れた ∧ 消費した」を成立の条件にすると、busy な席へ queue された注入（届いている）を
/// 失敗と数える（bd `s2-07l.90`・裁定: 現れた ＝ 成功）。失敗は `absent`（現れない）と
/// `tmux-failed` の 2 つだけ。「在る」でなく「増えた」で見るのは、先に同じ字面が pane に在る周
/// （tick の pointer は固定文字列で前周の写しが残る）に届いていない注入を成功と数えないため。
/// 代償は、窓の内に pane が巻き上がって**古い写しだけ**が消えた周（新しい写しが見えていても
/// 総数は増えない）が `absent` へ倒れうること（fail-closed 側の誤り・tick では重複注入になる。
/// 周ごとに一意な目印にする案は「注入の内容を変えない」の契約外＝lens-90 再確認 NEW-2）。
/// `consumed` は**窓の終わりの証拠**で決める（入力欄が空かは読まない・`s2-07l.112`）。
fn settle(
    request: &Request,
    marker: &str,
    before: usize,
    tries: u32,
    watch: &Watch<'_>,
) -> Result<Settled, &'static str> {
    let mut seen = false;
    let mut evidence = None;
    for _ in 0..tries {
        sleep(SETTLE_STEP);
        let Some(pane) = capture(request.socket, request.target) else {
            return Err(REASON_TMUX_FAILED);
        };
        seen = seen || folded(&pane).matches(marker).count() > before;
        evidence = watch.evidence();
        if seen && matches!(evidence, Some(state::Evidence::Found(_))) {
            return Ok(Settled::Consumed);
        }
    }
    if !seen {
        return Err(REASON_ABSENT);
    }
    Ok(settled_of(evidence))
}

/// 窓を settle の回数へ写す（[`SETTLE_STEP`] 刻み・**1 回は必ず見る**・既定の窓なら
/// [`SETTLE_TRIES`] と同じ値）。
fn tries_within(window: Duration) -> u32 {
    let step = SETTLE_STEP.as_millis().max(1);
    u32::try_from(window.as_millis() / step)
        .unwrap_or(u32::MAX)
        .max(1)
}

/// 記録 file の path。
pub fn tick_path(state_dir: &Path, target: &str) -> PathBuf {
    state_dir
        .join(SEAT_DIR)
        .join(sanitize_target(target))
        .join(TICK_FILE)
}

/// 送達した 1 回を記録する。**置き場が解けない周は書かない**（rc は変えない）。
///
/// `what` は payload の先頭そのまま（現物を加工しない）。その場で消費したかは tick の記録
/// （`decision=inject … consumed=…`）と `seat inject` の stdout 行が持つ（planner 裁定 2026-09-11・
/// 記録 schema は FR21 と共有ゆえ消費の field は足さない）。席（`seat`）と時刻（`ts`）は 3 面の
/// 書き手が同じ `tick.jsonl` へ混ぜる行を弁別する列で、全 writer が持つ（`s2-07l.150`）。
///
/// `who` は従来の注入なら [`WHO`]、[`deliver_or_confirm`] の周は呼び手の名（[`last_own_payload`] は [`WHO`] の行しか
/// 自席の文と読まない＝呼び手の名の行で門は緩まない）。`what` は payload の先頭か、Enter の周の固定の字面。
fn record(request: &Request, who: &str, what: &str, bytes: u64, started: Instant) {
    let Some(dir) = request.state_dir.map(|state| state.path.as_path()) else {
        return;
    };
    let entry = InjectionRecord {
        schema: SCHEMA,
        who: who.to_owned(),
        what: what.to_owned(),
        when: WHEN.to_owned(),
        bytes,
        // 数えていないことを 0 と書かない。
        tokens: None,
        wall_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        seat: seat_name(request.target),
        ts: state::now_secs(),
    };
    let Ok(policy) = LockPolicy::embedded() else {
        return;
    };
    // 記録の失敗で注入の結果（rc）を変えない（FR21 は推奨で、判定そのものではない）。
    let _ = store::append_line(&tick_path(dir, request.target), &entry.to_line(), policy);
}

/// payload の先頭 `cap` byte（**文字の途中で切らない**）。
fn head(payload: &str, cap: usize) -> String {
    let mut end = 0;
    for (at, ch) in payload.char_indices() {
        let next = at.saturating_add(ch.len_utf8());
        if next > cap {
            break;
        }
        end = next;
    }
    payload.get(..end).unwrap_or_default().to_owned()
}

#[cfg(test)]
mod tests {
    use super::{folded, needle_of, tries_within, SETTLE_STEP, SETTLE_TRIES};
    use std::time::Duration;

    /// 畳む字面の形: ASCII の whitespace（空白・tab・LF・CR・FF）だけを落とし、全角空白と本文は残す。
    /// 畳んで空になる payload は目印を持てない（[`needle_of`] は `None`＝`REASON_EMPTY` の側）。
    #[test]
    fn seat_attrib_folded_drops_ascii_whitespace_only() {
        assert_eq!(folded(" a\tb\nc\r\nd\u{c}e "), "abcde");
        assert_eq!(folded("全角\u{3000}空白"), "全角\u{3000}空白", "U+3000 は本文");
        assert_eq!(folded(""), "");
        assert_eq!(needle_of("\n  \n: seat-e2e\n2 行目"), Some(":seat-e2e".to_owned()), "最初の非空行を畳む");
        assert_eq!(needle_of(" \n\t\n"), None, "非空行が無い payload は目印を持てない");
    }

    /// 既定の窓（[`SETTLE_STEP`] × [`SETTLE_TRIES`]）は従来と同じ回数に写る（既定 2 s が
    /// 変わらないことの pin）。掛け算や剰余に化けた写しはここで落ちる。
    #[test]
    fn inject_tries_within_default_window_is_settle_tries() {
        assert_eq!(
            tries_within(SETTLE_STEP.saturating_mul(SETTLE_TRIES)),
            SETTLE_TRIES
        );
    }

    /// cycle が渡す上限（30 s）は 200 ms 刻みで 150 回（hook の実行を跨ぐ長さ）。
    #[test]
    fn inject_tries_within_cycle_limit_spans_hook_run() {
        assert_eq!(tries_within(Duration::from_secs(30)), 150);
    }

    /// 刻みより短い窓・空の窓でも **1 回は必ず見る**（0 回だと目印を見ずに `absent` に倒れる）。
    #[test]
    fn inject_tries_within_never_zero() {
        assert_eq!(tries_within(Duration::ZERO), 1);
        assert_eq!(tries_within(Duration::from_millis(100)), 1);
    }

    /// 巨大な窓は u32 で飽和させる（panic しない・C11）。
    #[test]
    fn inject_tries_within_saturates_on_huge_window() {
        assert_eq!(tries_within(Duration::MAX), u32::MAX);
    }
}
