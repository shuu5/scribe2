//! 管理 tick（設計 §3・裁定 (b)〜(e)・SRS FR27 / FR29 / FR21 / FR23 / FR38・憲法 R-E12 / C10 / C11 / C2.2）。
//!
//! 席の**外**（host の timer）から回り、条件を**順序固定**で見て、成立した周だけ 1 行を
//! 注入する。R-E12 のとおり席は自分で周期起動を張らず、tick は席へ event としてしか届かない。
//!
//! **順序は load-bearing である**: 退避物の走査 → **状態**（state.jsonl の最終行を読む）→ pane 取得 →
//! **context**（cap 以上なら退避の合図・状態の門の外）→ **口座**（登録 row の在る席だけ・下記・状態の門の外）→
//! 状態の門（Idle だけが進む）→ 未 consumed 退避物 → cycle lock → 打刻の合図の brake（tick-stamp・口座）。
//! **heartbeat の鮮度 gate は持たない**
//! （`s2-07l.109`）: 鮮度 gate は「走行中の席の pane を字面で読んで誤判定する」のを避ける門だったが、
//! 席の状態が hook の打刻（typed・`s2-07l.95`）になって pane を idle の判定入力にしなくなり、理由が
//! 消えた。残していた害は「打刻の直後に cap を超えた席が最大 `seat.tick_stale_s` の間 退避の合図を
//! 受けない」盲点で（`s2-07l.105` が退避物の在る周だけ飛ばす特例で一部を塞いだ）、撤去で盲点は
//! tick の周期だけになる。heartbeat / tick-stamp の file は「席が生きている」の記録として残す
//! （`seat heartbeat` と合図の文面は不変・判定入力ではない）。退避の合図を状態の門の
//! **前**に置くのは、context が cap を超えた席は
//! busy（lens 待ち・長い cargo）であり、busy を理由に noop すると誰にも止められず auto-compact に
//! 至るためである（`s2-07l.89`・実インシデント 2026-09-11・SRS FR29「idle を待たずに」＝planner
//! 裁定 2026-09-11: FR29 > ADR-0015 §2.3）。退避物を lock より先に見るのは、「退避して止まっている席」
//! を cycle の入口（裁定 (b)）へ落とすためである。順序を入れ替えると同じ条件でも別の理由が出る＝
//! 理由の字面は順序の証拠でもある。
//!
//! **口座の軸**（設計 account-autonomy.md §5・SRS FR38・`s2-07l.211`）: 登録 row（[`role::registration_of_target`]）
//! の在る席だけを評価する。その口座の最新の実測行が `seat.tick_stale_s` より古い・無い周は FR33 の計測を 1 回
//! 撃ってから逼迫度を読み、退避して止まった席（直近の注入が退避の合図 ∧ その後の `Stop` ∧ pane の前面が
//! shell）は別口座で立て直し（[`cycle::relaunch`]）、逼迫度が R-C9-1 の値以上の席へは FR29 と同じ除外の下で
//! idle を待たずに退避の合図を注入する。context と同じく状態の門の**前**に置くのは、上限に近い席は busy の
//! まま上限に当たって止まり、planner / admin の仕事が他の便まで詰まらせるためである（user 直命 2026-09-12）。
//! 測れない周は注入も停止もせず、打刻の合図だけを送らない（FR27 の条件に「使用率が閾値未満」が在る）。
//!
//! **hook 集合の軸**（consumer-sync.md §6・SRS FR62・`s2-07l.304`・[`plugin`]）: 口座の軸の**後**・状態の門の**前**
//! （逼迫の席を先に逃がし、context cap と同じく busy でも送る）。登録 row の在る席だけ、`.303` の読み込み元の記録と
//! 今の hooks.json の digest を比べ、違えば `origin=hook` の退避の合図（→ 終了の手 → 同じ口座の立て直し）。判定行の
//! `plugin=<same|drift|unrecorded|unreadable>` は `account=` の隣（測れないことを黙らせない・C10）。
//!
//! **退避後の終了の手**（account-autonomy.md §5「退避後の終了の手」・`s2-07l.226`・憲法 C9）: 立て直しの入口 (3)
//! 「前面 process が shell」は誰かが session を終えた後にしか立たない（AC13 実演 2026-09-13 では planner が
//! `/exit` を送った＝人手に依存）。登録 row の在る席で、直近の注入が**口座由来か hook 由来**の退避の合図
//! （判定行の `origin=account|hook`・[`SignalOrigin`]・`s2-07l.307`: context 由来の退避は同じ口座の `/clear` の cycle へ
//! 進み `/exit` を送らない）∧ その後の `Stop` ∧ 自席の未
//! consumed 退避物が在る ∧ 前面が shell でない、の周は器が `/exit` を席の入力欄の門（cycle の `/clear` と同じ
//! [`inject::guard_input`]）を通して注入し（`kind=exit`）、exit-stamp を打つ（cycle-stamp と別の 1 本・再送しない・
//! `s2-07l.252`）。送達は前面が shell になったかで確かめ、次の周は立て直しの入口が立つ（入口 (1) は「直近の注入が
//! `externalize` か `exit`」・立て直しは cycle-stamp だけを読む）。失うものが無い席（退避済み ∧ Stop）にだけ送る（N1）。
//! **第 2 手 = 停止**（`s2-07l.259`）: `/exit` が通らない周（背景の仕事を持つ席の終了確認 dialog・実地 2026-09-14）は
//! exit-stamp の back-off の内側で pane の shell の直下の子を TERM → KILL で止めて終了を確定する（[`stop_seat`]・
//! 待ちは唯一の wait・記録は `kind=exit detail=terminated`）。人手（Enter）を待たない（C9）。
//!
//! **席の busy / idle は hook の打刻（[`state`]）が一次で、pane の字面は判定入力にしない**（憲法 C3.3・
//! ADR-0015）。打刻が無い・読めない・Busy が古い周は理由を分けて注入しない（fail-closed）。pane を
//! 読むのは context（statusline の数値）と注入の送達確認だけである。
//!
//! **打刻の合図（pointer）の頻度は tick 自身の打刻で決める**（planner 裁定 2026-09-12・案 A）: 合図を
//! 注入した周は tick-stamp を打ち、その mtime が `seat.tick_stale_s` **未満**の周は合図を送らない
//! （`pointer-recent`・`.110` の `cycle-recent` と同型・閾値は共用）。brake が掛かるのは合図だけで、
//! 退避の合図と cycle の評価はその周も行う（heartbeat の mtime は見ない＝FR27 の合図は席の生存の
//! 記録でなく「続きを進めろ」の促し）。
//!
//! **退避の合図の再送は自席への直近の合図の記録で抑える**（`s2-07l.315`・user 裁定 2026-09-15T02:30Z・設計 §3 / §8）:
//! context の軸と口座の軸の両方で、`Signal { kind: Externalize }` の直前に `inject.jsonl` の自席への直近の注入を読み
//! （終了の手の入口 (1) と同じ母集団・出所は問わない）、それが退避の合図で ts から `seat.signal_backoff_s` 未満なら
//! 注入せず `signal-recent`（[`signal_brake`]）。記録が無い・読めない周は brake を掛けない（退避が遅れる方が失うものが
//! 大きい・N1）。timer を 1 分に縮めても compaction 中の席の queue に合図が積まれない（実地 2026-09-15: planner へ
//! 00:55 / 01:00 / 01:05 の 3 連投・`.150` / `.288` の入力欄の門の事故の型）。打刻の合図の brake（tick-stamp）と cycle の
//! back-off は不変。

mod account;
mod exit;
mod plugin;
mod render;

pub use account::account_labels;
pub use plugin::{hook_pointer, REASON_HOOK_DRIFT};
pub use render::render_no_rule;

use super::{cycle, inject, meter, pane_of, state, RuleRead, WmScan};
// 子 module の本文が `super::` で読む seat の項目（純移動＝本文の path を書き換えない・`s2-07l.279`）。
use super::{int_rule, pane_is_shell, StateDir};
use crate::cli_outcome::{Outcome, RC_REFUSED};
use crate::fleet::json_lite::{self, Value};
use crate::fleet::replay;
use crate::fleet::store;
use crate::name::NAME;
use account::{account_turn, Turn};
use plugin::{plugin_turn, Axis, Plugin};
use render::{body, body_of_error, entry_of, inject_line, record, render, Signal};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// 自打刻 marker の名前。
pub const STAMP_FILE: &str = "tick-stamp";
/// 記録の who。
pub(super) const WHO: &str = "seat-tick";
/// `inject.jsonl` に**注入**の行を書く who の列（tick と `seat launch`・hook の記録は含めない）: 立て直しの入口 (1) が
/// 「直近の注入」を引く母集団（[`last_signal`]）。
pub(super) const INJECT_WRITERS: &[&str] = &[WHO, cycle::WHO_LAUNCH];
/// 記録の when。
pub(super) const WHEN: &str = "tick";
/// 置き場を解けない。
const REASON_STATE_DIR: &str = "state-dir";
/// 注入は済んだが自打刻を書けない（次の周も撃つ＝storm になるので断る）。
pub(super) const REASON_STAMP: &str = "stamp-unwritable";
/// `/exit` を送ったが窓の内に前面が shell にならない（`exit-` を前置きして `exit-unconfirmed`・`s2-07l.252`）。
pub(super) const REASON_EXIT_UNCONFIRMED: &str = "unconfirmed";
/// 第 2 手の停止（TERM → KILL）の後も猶予の内に前面の子が消えない（`exit-unstoppable`・`s2-07l.259`・次の周も同じ入口）。
pub(super) const REASON_EXIT_UNSTOPPABLE: &str = "unstoppable";
/// 第 2 手で終了を確定した周に `kind=exit` の判定行へ足す token の値（`detail=terminated`・`InjectKind` は増やさない）。
pub(super) const DETAIL_TERMINATED: &str = "terminated";
/// 第 2 手の停止の猶予（ms）を宣言する rules 行の id（`pipe stop` / `fleet usage` と共用・新しい行を足さない・C5）。
pub(super) const ROW_GRACE: &str = "pipe.stop_grace_ms";
/// 退避を促す 1 行の skill 名（席の中で打つ command）。
const EXTERNALIZE_SKILL: &str = "/ready-compaction";
/// 退避して止まった席の session を終える 1 行（Claude Code の正規の終了・SessionEnd hook が走る・account-autonomy.md
/// §5 / §10: Ctrl-C ×2 / Ctrl-D ×2 は timing と入力欄の空に依存するので採らない）。
pub(super) const EXIT: &str = "/exit";
/// 口座の逼迫度の閾値（session 用・使用率の百分率）を宣言する rules 行の id（account-autonomy.md §3・値は code に
/// 焼かない・C5）。`seat launch` の初回の選定（[`cycle::launch`]）も同じ行を読む。
pub const ID_THRESHOLD: &str = "R-C9-1";
/// 退避の合図を同じ席へ再送するまでの back-off（秒）を宣言する rules 行の id（`s2-07l.315`・user 裁定 2026-09-15T02:30Z・
/// 値は code に焼かない・C5）。context の軸と口座の軸の両方の合図が同じ 1 行を読む（[`signal_brake`]）。
pub const ID_SIGNAL_BACKOFF: &str = "seat.signal_backoff_s";

/// tick 1 回の入力。
pub struct Request<'a> {
    /// tmux target（pane の指定）。
    pub target: &'a str,
    /// 退避物の dir。
    pub wm_dir: &'a str,
    /// 注入する 1 行（既定は [`default_pointer`]）。
    pub pointer: Option<&'a str>,
    /// tmux の socket。
    pub socket: Option<&'a str>,
    /// pane 本文の代わりに読む file。
    pub capture_file: Option<&'a str>,
    /// 記録と marker の置き場。
    pub state_dir: Option<&'a str>,
    /// cycle を回す周に渡す復元 command。
    pub restore: Option<&'a str>,
    /// cycle を回す周に渡す確認上限（rules 行 `seat.cycle_settle_s`・解くのは [`cli`] 1 箇所）。
    ///
    /// [`cycle`] 側の rules を tick がもう 1 度読まないのは、同じ 1 回の判定の中で 2 面が別の
    /// 値で走る形を作らないためである（`--rules` の seam は呼出しごとに 1 回解く）。
    pub settle: Duration,
    /// cycle を回す周に渡す確認の周期（rules 行 `seat.cycle_poll_ms`）。
    pub step: Duration,
    /// tracked の面の `[[account]]` の label 列（tick が開いた rules＝`--rules` が在ればその file・無ければ埋め込み・
    /// [`account_labels`]・`s2-07l.224`）。確認の刻みと同じく [`cli`] が 1 回だけ解く。[`run`] が置き場の
    /// `host.toml` の宣言を足してから口座の軸に使う（[`crate::rules::declared_labels`]・account-lifecycle.md §2）。
    pub accounts: &'a [String],
    /// 定期計測（`fleet usage`）へ渡す `--rules` の path（無い周は埋め込み）。計測は同じ置き場の `host.toml` も読む
    /// ＝tick と `fleet usage` は同じ宣言を読む。
    pub rules: Option<&'a str>,
}

/// tick 1 回の判定。**bool で持たない**（憲法 C11）。
///
/// 「撃たなかった」が 2 つに分かれているのは、理由が別物だからである——[`Self::Noop`] は
/// 条件のどれかが立たなかった**正常**で、[`Self::Error`] は判定そのものが回らなかった
/// 異常である。1 つに畳むと、席が静かなのか機械が壊れているのかを記録から読めなくなる。
pub enum TickDecision {
    /// 条件が揃った＝注入した（何を注入したか・席がその場で消費したかを添える）。
    Inject(InjectKind, inject::Settled),
    /// 条件が立たない＝撃たない（正常）。
    Noop(NoopReason),
    /// 実行系が回らない＝撃たない（異常・rc 1）。
    Error(String),
}

/// 注入した 1 行の種類。**bool で持たない**（憲法 C11）。
///
/// 分けるのは、促す行為が別物だからである——[`Self::Pointer`] は「打刻して続きへ」、
/// [`Self::Externalize`] は「退避せよ」、[`Self::Relaunch`] は「別口座で立て直した」、[`Self::Exit`] は
/// 「退避して止まった session を終えよ」、[`Self::Launch`] は「席を初めて起こした」。記録に種類が残らないと、席が退避の
/// pointer を何度受けたか（storm の有無）を後から数えられない。
#[derive(Clone, Copy)]
pub enum InjectKind {
    /// 席に自分の打刻を促す 1 行（idle・退避物なし・lock 空きの揃った周）。
    Pointer,
    /// context が cap 以上・または登録 row の口座が閾値以上の席へ退避を促す 1 行（idle を待たない）。
    Externalize,
    /// 退避して止まった席を別口座で立て直した（起動の雛形と復元の 2 行・account-autonomy.md §5）。
    Relaunch,
    /// 退避して止まった席（退避の合図 → `Stop` → 未 consumed 退避物 → 前面が shell でない）の session を終える
    /// [`EXIT`] の 1 行（account-autonomy.md §5「退避後の終了の手」・立て直しの入口 (3) を人手なしで立てる）。
    Exit,
    /// `seat launch` が席を初めて起こした（導出した起動行 1 行・account-lifecycle.md §4）。書くのは tick でなく
    /// [`cycle::launch`]（`who=seat-launch`）で、立て直しの入口 (1) はこれを合図に数えない（launch 直後の停止は
    /// 器が起こしたのでない停止と同じ扱い）。
    Launch,
    /// 起動したが 1 turn も始めていない席（打刻の最終行が `SessionStart` のまま＝復元が消費されていない）へ復元の
    /// command をもう一度送った（復元の第 2 手・account-autonomy.md §5・[`exit::restore_turn`]・`s2-07l.318`）。
    Restore,
}

/// [`InjectKind`] の全 variant（宣言順）。
pub const INJECT_KINDS: &[InjectKind] = &[
    InjectKind::Pointer,
    InjectKind::Externalize,
    InjectKind::Relaunch,
    InjectKind::Exit,
    InjectKind::Launch,
    InjectKind::Restore,
];

impl InjectKind {
    /// 記録と表示に使う字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pointer => "pointer",
            Self::Externalize => "externalize",
            Self::Relaunch => "relaunch",
            Self::Exit => "exit",
            Self::Launch => "launch",
            Self::Restore => "restore",
        }
    }
}

/// 退避の合図（[`InjectKind::Externalize`]）の**出所**（consumer-sync.md §6「合図の出所を typed に」・ADR-0028 §2.4・
/// `s2-07l.307`）。**閉じた enum**（憲法 C2 / C10・bool で持たない）: 終了の手の入口（[`exit::parked_entry`]）は
/// 出所で分かれる——口座由来の退避は別口座で立て直すので `/exit` の側、context 由来の退避は同じ口座の `/clear` の
/// cycle の側（不可逆の `/exit` を口座の動かない合図に送らない・N1）。実地 2026-09-15: context の退避の後に判定行の
/// `kind=externalize` だけを読んで `/exit` + 別口座の立て直しに倒れ、planner の口座が動いた。
#[derive(Clone, Copy)]
pub enum SignalOrigin {
    /// context が cap 以上（[`judge`] の context の軸）。
    Context,
    /// 登録 row の口座が閾値以上（口座の軸・[`account::account_turn`]）。
    Account,
    /// hook 集合の食い違い（FR62・consumer-sync.md §6）。構築点は `s2-07l.304` が置く（本便では無い）。
    Hook,
}

/// [`SignalOrigin`] の全 variant（宣言順）。
pub const SIGNAL_ORIGINS: &[SignalOrigin] = &[SignalOrigin::Context, SignalOrigin::Account, SignalOrigin::Hook];

impl SignalOrigin {
    /// 記録と表示に使う字面（判定行の `origin=<…>`）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Context => "context",
            Self::Account => "account",
            Self::Hook => "hook",
        }
    }
}

/// pane から読んだ context 使用率。**pane を取得した周だけ評価する**——取得しない周は
/// [`Self::Unevaluated`]（`cycle=` と同じ「評価していない」の印＝判定行に載らない）。
///
/// 測れない周は [`Self::Unmeasured`] で理由を 1 語持ち、**注入も停止もしない**（測れないことを
/// 理由に席を止めない・AC9 条 3 と同じ極性）。0% に化けさせない（FR25）。
#[derive(Clone, Copy)]
enum Context {
    /// pane を取得していない＝評価していない。
    Unevaluated,
    /// 測れた（使用率, cap）。cap を添えるのは判定と表示が同じ読みを使うため。
    Measured(u64, u64),
    /// 測れない（meter の 1 語）。
    Unmeasured(&'static str),
}

impl Context {
    /// 判定行の末尾に足す字面（評価していない周は空）。
    fn suffix(self) -> String {
        match self {
            Self::Unevaluated => String::new(),
            Self::Measured(pct, _) => format!(" context={pct}"),
            Self::Unmeasured(reason) => format!(" context=unmeasured reason={reason}"),
        }
    }
}

/// 登録 row の口座の逼迫度（account-autonomy.md §5 (2)）。**登録 row の在る席だけ評価する**——無い席・口座の軸まで
/// 進まなかった周は [`Self::Unevaluated`]（`cycle=` と同じ「評価していない」の印＝判定行に載らない）。
///
/// 測れない周は [`Self::Unmeasured`] で、注入も停止もしない（打刻の合図だけを送らない・FR27）。0% に化けさせない。
#[derive(Clone)]
pub(super) enum Account {
    /// 登録 row が無い＝評価していない。
    Unevaluated,
    /// R-C9-1 の値未満（口座 label, 逼迫度）。
    Under(String, u64),
    /// R-C9-1 の値以上（口座 label, 逼迫度, 閾値）。
    Over(String, u64, u64),
    /// 測れない（実測行なし・数える窓が Unmeasured・reset を過ぎた行だけ）（口座 label）。
    Unmeasured(String),
}

impl Account {
    /// 判定行の末尾に足す字面（`account=<label>:<pct>`・評価していない周は空）。
    fn suffix(&self) -> String {
        match self {
            Self::Unevaluated => String::new(),
            Self::Under(label, pct) | Self::Over(label, pct, _) => format!(" account={label}:{pct}"),
            Self::Unmeasured(label) => format!(" account={label}:unmeasured"),
        }
    }
}

/// 判定 1 回の全体（判定と評価の産物・context・状態の読み）。
pub(super) struct Judged {
    /// 判定と、判定までに評価したもの。
    verdict: Verdict,
    /// context の評価。
    context: Context,
    /// 席の状態の読み（rules 行が読めず判定に入らない周は `None`・判定行に載らない）。
    state: Option<state::Read>,
}

impl Judged {
    /// 状態を読む前に決まった周（context も状態も評価していない）。
    fn bare(decision: TickDecision) -> Self {
        Self {
            verdict: Verdict::of(decision),
            context: Context::Unevaluated,
            state: None,
        }
    }
}

/// pane を取得した後の判定と、そこまでに評価したもの（評価しなかったものは `None` / 未評価＝判定行に載らない）。
pub(super) struct Verdict {
    /// 判定。
    decision: TickDecision,
    /// cycle を回した周の要約（回していない周は `None`）。
    cycled: Option<String>,
    /// back-off の打刻の読み（cycle・立て直しは cycle-stamp・終了の手は exit-stamp。評価まで進まなかった周は
    /// `None`＝読んでいない。進んだ周は評価した・見送った・読めなかったのいずれでも `Some`）。
    stamp: Option<Stamped>,
    /// 口座の逼迫度。
    account: Account,
    /// hook 集合の読み（登録 row の在る席で軸まで進んだ周だけ・`s2-07l.304`）。
    plugin: Plugin,
    /// 立て直しを評価した周の結果（選んだ label か `none:<理由>`・評価していない周は `None`）。
    relaunched: Option<String>,
    /// 注入の判定に添える細目（第 2 手で終了を確定した周の [`DETAIL_TERMINATED`]・それ以外は `None`＝判定行に載らない）。
    detail: Option<&'static str>,
    /// 退避の合図の出所（[`InjectKind::Externalize`] を送った周だけ `Some`・打刻の合図・終了の手・立て直しは `None`
    /// ＝判定行に載らない・`s2-07l.307`）。
    origin: Option<SignalOrigin>,
}

impl Verdict {
    /// 判定だけの周（他は評価していない）。
    fn of(decision: TickDecision) -> Self {
        Self {
            decision,
            cycled: None,
            stamp: None,
            account: Account::Unevaluated,
            plugin: Plugin::Unevaluated,
            relaunched: None,
            detail: None,
            origin: None,
        }
    }
}

/// cycle を評価した周の打刻の読み（`s2-07l.110`）。**「無い」と「読めない」を混ぜない**（憲法 C11）:
/// 読めない周を「無い」に読み替えると不可逆の `/clear` へ倒れる（N1 の向きは繰り返さない側）。
#[derive(Clone, Copy)]
enum CycleStamp {
    /// 打刻が無い＝一度も評価していない。
    None,
    /// 打刻からの経過（秒）。
    Age(u64),
    /// 打刻を読めない（置き場が壊れている等）。
    Unreadable,
}

impl CycleStamp {
    /// `<seat_dir>/<file>`（[`cycle::STAMP_FILE`] か [`cycle::EXIT_STAMP_FILE`]）を読む。mtime が未来の
    /// 周は経過 0＝評価しない側へ倒す。
    fn read(seat_dir: &Path, file: &str) -> Self {
        match std::fs::metadata(seat_dir.join(file)) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::None,
            Err(_) => Self::Unreadable,
            Ok(meta) => match meta.modified() {
                Ok(at) => Self::Age(at.elapsed().map_or(0, |age| age.as_secs())),
                Err(_) => Self::Unreadable,
            },
        }
    }
}

/// back-off が読んだ打刻（どの file か・その読み）。判定行の token の名は file 名そのもの
/// （`cycle-stamp=` / `exit-stamp=`）＝どちらの back-off を見た周かを記録から弁別できる（`s2-07l.252`）。
#[derive(Clone, Copy)]
pub(super) struct Stamped {
    /// 読んだ file の名（[`cycle::STAMP_FILE`] か [`cycle::EXIT_STAMP_FILE`]）。
    file: &'static str,
    /// その読み。
    read: CycleStamp,
}

impl Stamped {
    /// 判定行に足す字面（cycle・立て直し・終了の手の評価まで進んだ周だけ＝評価した・back-off で見送った・
    /// 読めなかった、のいずれか）。
    fn suffix(self) -> String {
        let file = self.file;
        match self.read {
            CycleStamp::None => format!(" {file}=none"),
            CycleStamp::Age(age) => format!(" {file}={age}"),
            CycleStamp::Unreadable => format!(" {file}=unreadable"),
        }
    }
}

/// pane を取得した後に判定へ渡す材料（pane 本文そのものは持たない＝判定は字面を読まない）。
pub(super) struct Seen {
    /// 自席の退避物の数え。
    wm: WmScan,
    /// context の評価。
    context: Context,
    /// cycle lock の TTL（秒）。
    ttl_s: u64,
    /// 席の状態の読み（typed・pane の字面ではない）。
    state: state::Read,
    /// stale の閾値（秒・rules 行 `seat.tick_stale_s`）。cycle-stamp の back-off と打刻の合図の brake と
    /// 実測行の鮮度も**同じ値**を共用する（新しい閾値を足さない・C5）。
    stale_s: u64,
    /// 口座の逼迫度の閾値（rules 行 R-C9-1 の値・使用率の百分率）。
    threshold: u64,
    /// 退避の合図の再送の back-off（秒・rules 行 `seat.signal_backoff_s`・[`signal_brake`]）。
    backoff_s: u64,
}

/// 撃たなかった理由。**順序固定の条件のうち最初に立たなかったもの**を表す。
#[derive(Clone, Copy)]
pub enum NoopReason {
    /// 2. pane を読めない。
    PaneMissing,
    /// 3. 席の最終の打刻が Busy（turn が走っている）。
    Busy,
    /// 3. 打刻 file が無い（hook が載っていない席・v1 の席）。**idle と読み替えない**。
    StateMissing,
    /// 3. 打刻 file が読めない・最終行が壊れている。**idle と読み替えない**。
    StateUnreadable,
    /// 3. 最終の Busy が `seat.tick_stale_s` より古い（hook が死んだ疑い）。**busy とも idle とも言わない**。
    StateStale,
    /// 4. 自席の未 consumed 退避物が在る（＝退避して止まっている）。
    WmUnconsumed,
    /// 4. 退避物の dir を読めない（**0 件と読み替えない**）。
    WmUnreadable,
    /// 5. 他の cycle が走っている。
    CycleLive,
    /// 5. 同じ席の cycle を `seat.tick_stale_s` 未満の前に評価した（back-off・`s2-07l.110`）。
    CycleRecent,
    /// 5. cycle-stamp を読めない（**「無い」と読み替えない**＝不可逆の `/clear` へ倒さない）。
    CycleStampUnreadable,
    /// 6. 打刻の合図を `seat.tick_stale_s` 未満の前に注入した（tick-stamp・storm 止め・`s2-07l.109`）。
    PointerRecent,
    /// 6. 登録 row の口座の逼迫度を測れない（FR27 の「使用率が閾値未満」を確かめられない・注入も停止もしない）。
    AccountUnmeasured,
    /// 口座: 退避して止まった席の立て直しに選べる口座が無い（注入せず次の tick で選び直す・0 口座で起こさない）。
    AccountNoCandidate,
    /// context / 口座: 自席への直近の注入が退避の合図で、その ts から `seat.signal_backoff_s` 未満（再送しない・
    /// `s2-07l.315`）。記録が無い・読めない周はこの理由にならない（brake を掛けない側）。
    SignalRecent,
    /// 口座: 復元の第 2 手（restore-stamp）を `seat.signal_backoff_s` 未満の前に送った（二重送信の brake・`s2-07l.318`）。
    RestoreRecent,
}

/// [`NoopReason`] の全 variant（宣言順）。
pub const NOOP_REASONS: &[NoopReason] = &[
    NoopReason::PaneMissing,
    NoopReason::Busy,
    NoopReason::StateMissing,
    NoopReason::StateUnreadable,
    NoopReason::StateStale,
    NoopReason::WmUnconsumed,
    NoopReason::WmUnreadable,
    NoopReason::CycleLive,
    NoopReason::CycleRecent,
    NoopReason::CycleStampUnreadable,
    NoopReason::PointerRecent,
    NoopReason::AccountUnmeasured,
    NoopReason::AccountNoCandidate,
    NoopReason::SignalRecent,
    NoopReason::RestoreRecent,
];

impl NoopReason {
    /// 記録と表示に使う字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PaneMissing => "pane-missing",
            Self::Busy => "busy",
            Self::StateMissing => "state-missing",
            Self::StateUnreadable => "state-unreadable",
            Self::StateStale => "state-stale",
            Self::WmUnconsumed => "wm-unconsumed",
            Self::WmUnreadable => "wm-unreadable",
            Self::CycleLive => "cycle-live",
            Self::CycleRecent => "cycle-recent",
            Self::CycleStampUnreadable => "cycle-stamp-unreadable",
            Self::PointerRecent => "pointer-recent",
            Self::AccountUnmeasured => "account-unmeasured",
            Self::AccountNoCandidate => "account-no-candidate",
            Self::SignalRecent => "signal-recent",
            Self::RestoreRecent => "restore-recent",
        }
    }
}

/// 自打刻 marker の path。
pub fn stamp_path(seat_dir: &Path) -> PathBuf {
    seat_dir.join(STAMP_FILE)
}

/// 既定の注入 1 行。席に自分の打刻を促し、続きへ戻す。
///
/// **短く保つ**: 注入の送達確認は pane に現れた字面で測るので、pane の幅を超えて折り返すと
/// 送達したのに確認できない周が出る。
pub fn default_pointer(target: &str) -> String {
    format!("管理 tick: {NAME} seat heartbeat --target {target} を撃ち、続きを進めてください")
}

/// 退避を促す 1 行（context が cap 以上の席へ）。退避 skill の名と実測値・cap を含め、
/// [`default_pointer`] と同じ理由で pane の幅に収まる短さに保つ。`--pointer` では上書きしない
/// （打刻の促しの上書きであって、実測値を運ぶ行を固定文字列に置き換える口ではない）。
pub fn externalize_pointer(pct: u64, cap: u64) -> String {
    format!("退避 tick: context {pct}% ≥ cap {cap}%・{EXTERNALIZE_SKILL} で退避してください")
}

/// 退避を促す 1 行（登録 row の口座が R-C9-1 の値以上の席へ・account-autonomy.md §5）。口座の label と実測値・
/// 閾値を含め、[`externalize_pointer`] と同じ理由で短く保ち、`--pointer` では上書きしない。
pub fn account_pointer(label: &str, pct: u64, threshold: u64) -> String {
    format!("退避 tick: 口座 {label} {pct}% ≥ 閾値 {threshold}%・{EXTERNALIZE_SKILL} で退避してください")
}

/// tick を 1 回回す。
pub fn run(request: &Request) -> Outcome {
    let started = Instant::now();
    let Some(place) = super::state_dir_of(request.state_dir) else {
        // 置き場が無いと記録も打刻も持てない＝判定を回さない（撃たない側へ倒す）。
        return Outcome::failed_line(RC_REFUSED, render(&body_of_error(REASON_STATE_DIR)));
    };
    // host の面（`<state_dir>/host.toml`）の宣言を足す。在るが読めない周は判定を回さず記録も残さない（計測も撃たない・
    // FailClosed・account-lifecycle.md §6）。無い周は tracked の面の宣言のまま（縮退）。
    let Ok(accounts) = crate::rules::declared_labels(request.accounts, &place.path) else {
        return Outcome::failed_line(RC_REFUSED, render(&body_of_error(RuleRead::ManifestUnreadable.no_rule())));
    };
    // 逼迫度と立て直しの宣言は有効な口座の集合（宣言 − 退役中・account-lifecycle.md §3）。log を読めない周は宣言のまま
    // （口座の軸は同じ log を読む `seated` で評価されない側へ倒れる）。
    let accounts = match store::read_all(&place.path) {
        Ok(events) => replay(&events).without_retired(accounts.iter().map(String::as_str)),
        Err(_) => accounts,
    };
    let request = &Request { accounts: &accounts, ..*request };
    let dir = super::seat_dir(&place.path, request.target);
    let judged = decide(request, &place, &dir);
    let body = body(request.target, &judged, &place);
    let entry = entry_of(request.target, &body, started);
    record(&place.path, request.target, &entry);
    match judged.verdict.decision {
        TickDecision::Error(_) => Outcome::failed_line(RC_REFUSED, render(&body)),
        TickDecision::Noop(_) => Outcome::ok_line(render(&body)),
        TickDecision::Inject(..) => {
            // 注入した周は `<state_dir>/inject.jsonl`（注入の store・hook と同じ writer と lock・C6.3）にも同じ
            // 1 行を積む: 立て直しの入口 (1)「その席への直近の注入の記録」の読み先（[`last_externalize`]）。
            // 記録の失敗で rc を変えない（FR21 は推奨で、判定そのものではない）。
            let _ = crate::hook::append(&place.path, &entry);
            Outcome::ok_line(render(&body))
        }
    }
}

/// 条件を順序固定で見る（退避物の走査 → 状態の読み → pane 取得 → context → 口座 → 状態の門 → 退避物 → lock →
/// 合図の brake）。鮮度 gate は持たない（`s2-07l.109`・module doc）。
///
/// 退避物を最初に走査するのは、走査の結果を後段（context / 口座の合図の可否 / 退避物）で同じ値として
/// 使うためである（cycle を回す周だけは cycle 側が自分の入口でもう 1 度走査する＝lock の内側で
/// 確かめ直す）。読めない周を「在る」に読み替えない。
fn decide(request: &Request, place: &super::StateDir, dir: &Path) -> Judged {
    // 4 行のどれかが読めない周は判定に入らない（理由は最初に読めなかった行の variant 付き・`s2-07l.205`）。
    let rows = (state::stale_s(), cycle::ttl_s(), super::int_rule(ID_THRESHOLD), super::int_rule(ID_SIGNAL_BACKOFF));
    let (stale_s, ttl_s, threshold, backoff_s) = match rows {
        (Ok(stale_s), Ok(ttl_s), Ok(threshold), Ok(backoff_s)) => (stale_s, ttl_s, threshold, backoff_s),
        (Err(read), ..) | (_, Err(read), ..) | (_, _, Err(read), _) | (.., Err(read)) => {
            return Judged::bare(TickDecision::Error(read.no_rule().to_owned()));
        }
    };
    let wm = super::scan_wm(Path::new(request.wm_dir), request.target);
    // 状態は pane より先に読む（file 1 つ・tmux を叩かない）。読みは tick と cycle で 1 本。
    let read = state::read_last(dir, stale_s);
    let Some(pane) = pane_of(request.socket, request.target, request.capture_file) else {
        return Judged {
            state: Some(read),
            ..Judged::bare(TickDecision::Noop(NoopReason::PaneMissing))
        };
    };
    let seen = Seen {
        wm,
        context: measure_context(&pane),
        ttl_s,
        state: read,
        stale_s,
        threshold,
        backoff_s,
    };
    Judged {
        verdict: judge(request, place, dir, &seen),
        context: seen.context,
        state: Some(read),
    }
}

/// pane を取得した後の条件（context → 口座 → 状態の門 → 退避物 → lock → cycle-stamp／合図の brake）。
fn judge(request: &Request, place: &super::StateDir, dir: &Path, seen: &Seen) -> Verdict {
    // 他の cycle が走っている席（lock が live）には退避の pointer も送らない——作り直しの最中に
    // 行を queue しても、届く先は消えるか作り直された席である（排他は cycle 側と同じ 1 本の lock）。
    // 退避の合図は**状態の門の外**（FR29「idle を待たずに」・busy な席へは queue の形で届く）。
    if let Some((pct, cap)) = over_cap(seen).filter(|_| !cycle::lock_is_live(dir, seen.ttl_s)) {
        if let Some(reason) = signal_brake(place, request.target, seen.backoff_s) {
            return Verdict::of(TickDecision::Noop(reason));
        }
        let payload = externalize_pointer(pct, cap);
        let signal = Signal {
            kind: InjectKind::Externalize,
            origin: Some(SignalOrigin::Context),
            payload: &payload,
            state: seen.state,
        };
        return Verdict { origin: signal.origin, ..Verdict::of(inject_line(request, place, dir, &signal)) };
    }
    let account = match account_turn(request, place, dir, seen) {
        Turn::Settled(verdict) => return verdict,
        Turn::Pass(account) => account,
    };
    // hook 集合の軸（`s2-07l.304`・consumer-sync.md §6）: 口座の軸が評価した席だけ・逼迫の席を先に逃がした後。
    let plugin = match plugin_turn(request, place, dir, seen, &account) {
        Axis::Settled(verdict) => return Verdict { account, ..verdict },
        Axis::Pass(plugin) => plugin,
    };
    let rest = after_account(request, place, dir, seen, &account);
    Verdict { account, plugin, ..rest }
}

/// 口座の軸の後の条件（状態の門 → 退避物 → lock → 合図の brake → 打刻の合図）。
fn after_account(request: &Request, place: &super::StateDir, dir: &Path, seen: &Seen, account: &Account) -> Verdict {
    if let Some(reason) = gate_of(seen.state) {
        return Verdict::of(TickDecision::Noop(reason));
    }
    match seen.wm {
        WmScan::Unreadable => return Verdict::of(TickDecision::Noop(NoopReason::WmUnreadable)),
        WmScan::Unconsumed(_) => return parked(request, place, dir, seen),
        WmScan::None => {}
    }
    if cycle::lock_is_live(dir, seen.ttl_s) {
        return Verdict::of(TickDecision::Noop(NoopReason::CycleLive));
    }
    if let Some(reason) = pointer_brake(dir, seen.stale_s, account) {
        return Verdict::of(TickDecision::Noop(reason));
    }
    let payload = request
        .pointer
        .map_or_else(|| default_pointer(request.target), str::to_owned);
    let signal = Signal { kind: InjectKind::Pointer, origin: None, payload: &payload, state: seen.state };
    Verdict::of(inject_line(request, place, dir, &signal))
}

/// 打刻の合図の brake（`s2-07l.109`・planner 裁定 2026-09-12 案 A・account-autonomy.md §5）: tick 自身の打刻
/// （tick-stamp）が新しい周に加え、FR27 の条件「使用率が閾値未満」が立たない周も合図を送らない——閾値以上の周は
/// 同じ `pointer-recent`（閾値以上の席は退避の合図か FR29 の除外で先に返るので、ここは同じ brake の縁）、
/// 測れない周は `account-unmeasured`（注入も停止もしない）。登録 row の無い席は口座を見ない（従来のまま）。
fn pointer_brake(seat_dir: &Path, stale_s: u64, account: &Account) -> Option<NoopReason> {
    if pointer_recent(seat_dir, stale_s) {
        return Some(NoopReason::PointerRecent);
    }
    match account {
        Account::Over(..) => Some(NoopReason::PointerRecent),
        Account::Unmeasured(_) => Some(NoopReason::AccountUnmeasured),
        Account::Unevaluated | Account::Under(..) => None,
    }
}

/// 打刻の合図の brake（`s2-07l.109`・planner 裁定 2026-09-12 案 A）: tick 自身の打刻（tick-stamp）の
/// mtime が閾値**未満**なら、この周は合図を送らない。不在・読めない周は送る側（合図は可逆な
/// 1 行で、読めないことを理由に止めると合図が永久に止まる）。mtime が未来の周は送らない側
/// （経過を負に読まない）。境界は未満＝経過が閾値ちょうどの周は送る。
fn pointer_recent(seat_dir: &Path, stale_s: u64) -> bool {
    std::fs::metadata(stamp_path(seat_dir))
        .and_then(|meta| meta.modified())
        .is_ok_and(|at| at.elapsed().map_or(true, |age| age.as_secs() < stale_s))
}

/// 退避の合図の brake（`s2-07l.315`・user 裁定 2026-09-15T02:30Z・設計 seat-autonomy.md §3 / §8）: 自席への直近の注入
/// （[`last_externalize_at`]）が退避の合図で、その ts から `seat.signal_backoff_s`（`backoff_s`）**未満**なら、この周は
/// 合図を送らない（`signal-recent`）。context の軸（[`judge`]）と口座の軸（[`account::account_turn`]）の両方の
/// `Signal { kind: Externalize }` の直前で同じ 1 関数を通す。記録が無い・読めない・直近が別の注入の周は送る側
/// （合図を止める側に倒さない＝退避が遅れる方が失うものが大きい・N1）。境界は未満＝経過が back-off ちょうどの周は送る。
/// ts が未来の記録は経過 0（経過を負に読まない・[`pointer_recent`] と同じ極性）。
pub(super) fn signal_brake(place: &super::StateDir, target: &str, backoff_s: u64) -> Option<NoopReason> {
    last_externalize_at(&place.path, target)
        .filter(|signalled| state::now_secs().saturating_sub(*signalled) < backoff_s)
        .map(|_| NoopReason::SignalRecent)
}

/// `<state_dir>/inject.jsonl` の同じ席の**注入の最新行**（`who` が [`INJECT_WRITERS`]・終了の手の入口 (1) と同じ母集団・
/// hook の記録は飛ばす）が退避の合図（`decision=inject … kind=externalize`・出所は問わない）なら、その ts。記録が無い・
/// 読めない・直近が別の注入（打刻の合図・launch・終了の合図）の周は `None`。
fn last_externalize_at(state_dir: &Path, target: &str) -> Option<u64> {
    let seat = crate::hook::seat_name(target)?;
    let text = std::fs::read_to_string(crate::hook::inject_path(state_dir)).ok()?;
    let last = text
        .lines()
        .rev()
        .filter_map(|line| json_lite::parse_object(line).ok())
        .find(|pairs| {
            field(pairs, "seat").and_then(Value::as_str) == Some(seat.as_str())
                && field(pairs, "who").and_then(Value::as_str).is_some_and(|who| INJECT_WRITERS.contains(&who))
        })?;
    let what = field(&last, "what").and_then(Value::as_str)?;
    let signalled = what.starts_with("decision=inject ")
        && what
            .split_whitespace()
            .any(|token| token.strip_prefix("kind=") == Some(InjectKind::Externalize.as_str()));
    if !signalled {
        return None;
    }
    field(&last, "ts").and_then(Value::as_num)
}

/// 記録 1 行の key の値。
fn field<'a>(pairs: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    pairs.iter().find(|(found, _)| found == key).map(|(_, value)| value)
}

/// 状態の門（ADR-0015 §2.3・fail-closed）: **Idle だけが通る**。Busy・打刻なし・読めない・Busy が
/// 古い、はそれぞれ別の理由で撃たない（missing を idle に、stale を busy に読み替えない）。
fn gate_of(read: state::Read) -> Option<NoopReason> {
    match read {
        state::Read::Idle(_) => None,
        state::Read::Busy(_) => Some(NoopReason::Busy),
        state::Read::Missing => Some(NoopReason::StateMissing),
        state::Read::Unreadable => Some(NoopReason::StateUnreadable),
        state::Read::Stale(_) => Some(NoopReason::StateStale),
    }
}

/// context が cap 以上で、退避の pointer を送るべき周か（`(使用率, cap)`・cycle lock は呼び側が見る）。
///
/// **自席の未 consumed 退避物が 0 件と確認できた周に限る**（planner 裁定 2026-09-11）: 退避済みの
/// 席は `/clear` 前で cap 以上のままなので、退避物を見ずに注入すると毎周 pointer を重ねて
/// cycle に一度も落ちない（livelock）。`Unconsumed` は次の条件（idle → 退避物 → cycle）へ、
/// `Unreadable` は「0 件」と読み替えずに同じく次の条件へ（fail-closed 側＝注入しない）。
/// 測れない周も注入しない（測れないことを理由に止めもしない）。
fn over_cap(seen: &Seen) -> Option<(u64, u64)> {
    match (seen.context, &seen.wm) {
        (Context::Measured(pct, cap), WmScan::None) if pct >= cap => Some((pct, cap)),
        _ => None,
    }
}

/// pane 本文から context を評価する。cap も使用率も **meter の口**で読む（自前の literal も
/// parse も持たない＝guard / meter / tick の 3 面が同じ関数を見る）。
fn measure_context(pane: &str) -> Context {
    let cap = match meter::declared_cap() {
        Ok(found) => found,
        Err(read) => return Context::Unmeasured(read.no_rule()),
    };
    match meter::used_from_pane_pct(pane) {
        Ok((pct, _, _)) => Context::Measured(pct, cap),
        Err(reason) => Context::Unmeasured(reason),
    }
}

/// 退避して止まっている周（裁定 (b)）: lock が空いていれば cycle を**その場で**回す。
///
/// 回すのは「退避物が在る ∧ 打刻が Idle ∧ lock が空いている ∧ **直前に評価していない**」周だけで、
/// **それ以外の周は cycle を評価しない**——tick 行に `cycle=` が付かないこと自体が「評価して
/// いない」の印である。
///
/// **back-off**（`s2-07l.110`・裁定 (a)）: cycle が lock を取れた周（結果が done / failed / refused の
/// いずれでも）は cycle 側が lock の内側・`/clear` より先に `cycle-stamp` を打ち（write-ahead・
/// 打てない周は 1 key も送らず refused。lock を取れない周〔lock-held / state-dir / no-rule〕は打たない
/// ＝他の cycle が打っているか置き場が使えない）、同じ席は打刻から `seat.tick_stale_s` **未満**の間
/// cycle を評価しない（`cycle-recent`・見送った周は打ち直さない＝永久には止まらない）。stamp を
/// 読むのは tick のここ（と立て直しの入口）だけで、`seat cycle` を手で回す口は back-off を見ない（人の判断）。
/// `/clear` は不可逆の口（N1）で、復元されない退避物（`/rebrief` が走らない・
/// consume しない）へ周期ごとに繰り返してはならない。stamp を読めない周は「無い」に読み替えず
/// 評価しない（`cycle-stamp-unreadable`・読めないことを理由に不可逆の側へ倒さない）。閾値は
/// state-stale と共用し（`seat.tick_stale_s`）、新しい rules 行を足さない（C5）。境界は**未満**（経過が閾値ちょうどの周は評価
/// する）。
fn parked(request: &Request, place: &super::StateDir, dir: &Path, seen: &Seen) -> Verdict {
    let noop = TickDecision::Noop(NoopReason::WmUnconsumed);
    if cycle::lock_is_live(dir, seen.ttl_s) {
        return Verdict::of(noop);
    }
    let (stamp, blocked) = back_off(dir, cycle::STAMP_FILE, seen.stale_s);
    if let Some(reason) = blocked {
        return held(stamp, reason);
    }
    let result = cycle::run(&cycle::Request {
        target: request.target,
        wm_dir: request.wm_dir,
        socket: request.socket,
        capture_file: request.capture_file,
        state_dir: place,
        restore: request.restore,
        settle: request.settle,
        step: request.step,
    });
    Verdict { cycled: Some(cycle::summary(&result)), stamp: Some(stamp), ..Verdict::of(noop) }
}

/// 打刻の back-off（`s2-07l.110`）: 読みと、評価しない理由（読めない周は `cycle-stamp-unreadable`、
/// `seat.tick_stale_s` 未満の前に打った周は `cycle-recent`・評価してよい周は `None`）。cycle と立て直し（作り直し・
/// 起こし直しの二重を防ぐ）は cycle-stamp を、終了の手（`/exit` の二重投函を防ぐ）は exit-stamp を読む
/// （`s2-07l.252`・目的の違う 2 つを 1 本に共用しない）。語と閾値は両方で同じ（語彙も rules 行も増やさない）。
pub(super) fn back_off(seat_dir: &Path, file: &'static str, stale_s: u64) -> (Stamped, Option<NoopReason>) {
    let read = CycleStamp::read(seat_dir, file);
    let reason = match read {
        CycleStamp::Unreadable => Some(NoopReason::CycleStampUnreadable),
        CycleStamp::Age(age) if age < stale_s => Some(NoopReason::CycleRecent),
        CycleStamp::Age(_) | CycleStamp::None => None,
    };
    (Stamped { file, read }, reason)
}

/// 打刻を読んだ上で見送った周の判定。
pub(super) fn held(stamp: Stamped, reason: NoopReason) -> Verdict {
    Verdict { stamp: Some(stamp), ..Verdict::of(TickDecision::Noop(reason)) }
}

#[cfg(test)]
mod tests {
    // flip-check: moved s2-07l.279
    use super::{INJECT_KINDS, NOOP_REASONS, SIGNAL_ORIGINS};
    use crate::order::is_declaration_order;

    /// `NoopReason` / `InjectKind` / `SignalOrigin` の字面は宣言順で全数を pin する（variant を足した周はここの件数が
    /// 変わる・C2）。
    #[test]
    fn seat_account_noop_reasons_and_kinds_are_pinned_in_declaration_order() {
        let reasons: Vec<&str> = NOOP_REASONS.iter().map(|reason| reason.as_str()).collect();
        assert_eq!(
            reasons,
            [
                "pane-missing", "busy", "state-missing", "state-unreadable", "state-stale", "wm-unconsumed",
                "wm-unreadable", "cycle-live", "cycle-recent", "cycle-stamp-unreadable", "pointer-recent",
                "account-unmeasured", "account-no-candidate", "signal-recent", "restore-recent",
            ]
        );
        assert!(is_declaration_order(NOOP_REASONS, |reason| reason as usize));
        let kinds: Vec<&str> = INJECT_KINDS.iter().map(|kind| kind.as_str()).collect();
        assert_eq!(kinds, ["pointer", "externalize", "relaunch", "exit", "launch", "restore"]);
        assert!(is_declaration_order(INJECT_KINDS, |kind| kind as usize));
        let origins: Vec<&str> = SIGNAL_ORIGINS.iter().map(|origin| origin.as_str()).collect();
        assert_eq!(origins, ["context", "account", "hook"]);
        assert!(is_declaration_order(SIGNAL_ORIGINS, |origin| origin as usize));
    }
}
