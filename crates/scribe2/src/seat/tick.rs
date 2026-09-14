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
//! **退避後の終了の手**（account-autonomy.md §5「退避後の終了の手」・`s2-07l.226`・憲法 C9）: 立て直しの入口 (3)
//! 「前面 process が shell」は誰かが session を終えた後にしか立たない（AC13 実演 2026-09-13 では planner が
//! `/exit` を送った＝人手に依存）。登録 row の在る席で、直近の注入が退避の合図 ∧ その後の `Stop` ∧ 自席の未
//! consumed 退避物が在る ∧ 前面が shell でない、の周は器が `/exit` を席の入力欄の門（cycle の `/clear` と同じ
//! [`inject::guard_input`]）を通して注入し（`kind=exit`）、exit-stamp を打つ（cycle-stamp と別の 1 本・再送しない・
//! `s2-07l.252`）。送達は前面が shell になったかで確かめ、次の周は立て直しの入口が立つ（入口 (1) は「直近の注入が
//! `externalize` か `exit`」・立て直しは cycle-stamp だけを読む）。失うものが無い席（退避済み ∧ Stop）にだけ送る（N1）。
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

use super::{cycle, heartbeat, inject, meter, pane_of, role, sanitize_target, state, RuleRead, WmScan};
use crate::cli_outcome::{Outcome, RC_REFUSED};
use crate::fleet::json_lite::{self, Value};
use crate::fleet::store::{self, LockPolicy};
use crate::fleet::{cli as fleet_cli, replay, Allowance, AllowanceLatest, Registration, State, WindowKind};
use crate::hook::{inject_path, seat_name, InjectionRecord, SCHEMA};
use crate::name::NAME;
use crate::rules::manifest::Manifest;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// 自打刻 marker の名前。
pub const STAMP_FILE: &str = "tick-stamp";
/// 記録の who。
const WHO: &str = "seat-tick";
/// 記録の when。
const WHEN: &str = "tick";
/// 置き場を解けない。
const REASON_STATE_DIR: &str = "state-dir";
/// 注入は済んだが自打刻を書けない（次の周も撃つ＝storm になるので断る）。
const REASON_STAMP: &str = "stamp-unwritable";
/// `/exit` を送ったが窓の内に前面が shell にならない（`exit-` を前置きして `exit-unconfirmed`・`s2-07l.252`）。
const REASON_EXIT_UNCONFIRMED: &str = "unconfirmed";
/// 退避を促す 1 行の skill 名（席の中で打つ command）。
const EXTERNALIZE_SKILL: &str = "/ready-compaction";
/// 退避して止まった席の session を終える 1 行（Claude Code の正規の終了・SessionEnd hook が走る・account-autonomy.md
/// §5 / §10: Ctrl-C ×2 / Ctrl-D ×2 は timing と入力欄の空に依存するので採らない）。
const EXIT: &str = "/exit";
/// 口座の逼迫度の閾値（session 用・使用率の百分率）を宣言する rules 行の id（account-autonomy.md §3・値は code に
/// 焼かない・C5）。
const ID_THRESHOLD: &str = "R-C9-1";

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
/// 「退避して止まった session を終えよ」。記録に種類が残らないと、席が退避の pointer を何度受けたか
/// （storm の有無）を後から数えられない。
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
}

/// [`InjectKind`] の全 variant（宣言順）。
pub const INJECT_KINDS: &[InjectKind] = &[
    InjectKind::Pointer,
    InjectKind::Externalize,
    InjectKind::Relaunch,
    InjectKind::Exit,
];

impl InjectKind {
    /// 記録と表示に使う字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pointer => "pointer",
            Self::Externalize => "externalize",
            Self::Relaunch => "relaunch",
            Self::Exit => "exit",
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
enum Account {
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
struct Judged {
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
struct Verdict {
    /// 判定。
    decision: TickDecision,
    /// cycle を回した周の要約（回していない周は `None`）。
    cycled: Option<String>,
    /// back-off の打刻の読み（cycle・立て直しは cycle-stamp・終了の手は exit-stamp。評価まで進まなかった周は
    /// `None`＝読んでいない。進んだ周は評価した・見送った・読めなかったのいずれでも `Some`）。
    stamp: Option<Stamped>,
    /// 口座の逼迫度。
    account: Account,
    /// 立て直しを評価した周の結果（選んだ label か `none:<理由>`・評価していない周は `None`）。
    relaunched: Option<String>,
}

impl Verdict {
    /// 判定だけの周（他は評価していない）。
    fn of(decision: TickDecision) -> Self {
        Self {
            decision,
            cycled: None,
            stamp: None,
            account: Account::Unevaluated,
            relaunched: None,
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
struct Stamped {
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
struct Seen {
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
    // 3 行のどれかが読めない周は判定に入らない（理由は最初に読めなかった行の variant 付き・`s2-07l.205`）。
    let (stale_s, ttl_s, threshold) = match (state::stale_s(), cycle::ttl_s(), super::int_rule(ID_THRESHOLD)) {
        (Ok(stale_s), Ok(ttl_s), Ok(threshold)) => (stale_s, ttl_s, threshold),
        (Err(read), _, _) | (_, Err(read), _) | (_, _, Err(read)) => {
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
        let payload = externalize_pointer(pct, cap);
        let signal = Signal { kind: InjectKind::Externalize, payload: &payload, state: seen.state };
        return Verdict::of(inject_line(request, place, dir, &signal));
    }
    let account = match account_turn(request, place, dir, seen) {
        Turn::Settled(verdict) => return verdict,
        Turn::Pass(account) => account,
    };
    let rest = after_account(request, place, dir, seen, &account);
    Verdict { account, ..rest }
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
    let signal = Signal { kind: InjectKind::Pointer, payload: &payload, state: seen.state };
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
fn back_off(seat_dir: &Path, file: &'static str, stale_s: u64) -> (Stamped, Option<NoopReason>) {
    let read = CycleStamp::read(seat_dir, file);
    let reason = match read {
        CycleStamp::Unreadable => Some(NoopReason::CycleStampUnreadable),
        CycleStamp::Age(age) if age < stale_s => Some(NoopReason::CycleRecent),
        CycleStamp::Age(_) | CycleStamp::None => None,
    };
    (Stamped { file, read }, reason)
}

/// 打刻を読んだ上で見送った周の判定。
fn held(stamp: Stamped, reason: NoopReason) -> Verdict {
    Verdict { stamp: Some(stamp), ..Verdict::of(TickDecision::Noop(reason)) }
}

/// 口座の軸の結果: 判定が決まった（立て直し・退避の合図）か、逼迫度を持って次の条件へ進むか。
enum Turn {
    /// この周の判定が決まった。
    Settled(Verdict),
    /// 次の条件へ（逼迫度は判定行と合図の brake が使う）。
    Pass(Account),
}

/// 口座の軸（account-autonomy.md §5・context の直後＝状態の門の外）: 登録 row の在る席だけ評価する。実測行が
/// 古い周は計測を 1 回撃ってから逼迫度を読み（[`seated`]）、退避して止まった席は前面が shell なら立て直し
/// （[`relaunch_turn`]）・shell でなければ終了の手（[`exit_turn`]・[`parked_entry`]）、閾値以上の席へは FR29 と
/// 同じ除外の下で idle を待たずに退避の合図を注入して自打刻する（[`account_signal`]）。閾値未満・測れない・除外で
/// 注入しない周は逼迫度を持って次の条件へ（注入も停止もしない）。
fn account_turn(request: &Request, place: &super::StateDir, dir: &Path, seen: &Seen) -> Turn {
    let Some(seated) = seated(request, place, seen.stale_s) else {
        return Turn::Pass(Account::Unevaluated);
    };
    let account = reading(&seated, seen.threshold);
    match parked_entry(&place.path, request.target, request.socket, dir, &seen.wm) {
        Entry::Relaunch => {
            return Turn::Settled(Verdict { account, ..relaunch_turn(request, place, dir, seen, &seated) });
        }
        Entry::Exit => return Turn::Settled(Verdict { account, ..exit_turn(request, place, dir, seen) }),
        Entry::None => {}
    }
    let Some(payload) = account_signal(&account, seen, dir) else {
        return Turn::Pass(account);
    };
    let signal = Signal { kind: InjectKind::Externalize, payload: &payload, state: seen.state };
    Turn::Settled(Verdict { account, ..Verdict::of(inject_line(request, place, dir, &signal)) })
}

/// 閾値以上の席へ送る退避の合図の 1 行。**FR29 と同じ除外**（[`over_cap`] と同じ極性）: 自席の未 consumed
/// 退避物が 0 件と確かめられ、cycle lock が空いている周だけ（退避済み・作り直しの最中の席へ重ねない）。
fn account_signal(account: &Account, seen: &Seen, dir: &Path) -> Option<String> {
    match (account, &seen.wm) {
        (Account::Over(label, pct, threshold), WmScan::None) if !cycle::lock_is_live(dir, seen.ttl_s) => {
            Some(account_pointer(label, *pct, *threshold))
        }
        _ => None,
    }
}

/// 口座の軸の材料（登録 row と、実測行の鮮度を保った replay）。
struct Seated {
    /// 自席の登録 row（口座・`model`・起動の雛形）。
    row: Registration,
    /// replay の現在地（実測行と他の席の登録 row）。
    state: State,
}

/// 登録 row を引き、その口座の最新の実測行が `seat.tick_stale_s` より古い・無い周は FR33 の計測を 1 回撃って
/// から読み直す（account-autonomy.md §5 (1)・定期計測はこの 1 形に限る・`fleet usage` と同じ関数）。log を
/// 読めない・登録 row が無い周は `None`（軸を評価しない）。計測の失敗は行として記録されるだけで止めない
/// （fleet-usage.md §6・FailOpen）——読み直した行が Unmeasured なら逼迫度は測れない側に倒れる。
///
/// manifest の `[[account]]` に無い口座は撃たない: 計測は宣言した口座だけを読むので行が積まれず、撃つと毎周の
/// 計測に化ける（測れないまま＝逼迫度は `unmeasured`）。宣言は tick が開いた rules の label 列 + 置き場の `host.toml`
/// で、計測にも同じ `--rules` と同じ置き場を渡す（tick と `fleet usage` が別の宣言を読まない・`s2-07l.224`）。
fn seated(request: &Request, place: &super::StateDir, stale_s: u64) -> Option<Seated> {
    let state = replay(&store::read_all(&place.path).ok()?);
    let row = role::registration_of_target(&state, request.target)?.clone();
    if is_fresh(&state, &row.account, stale_s) || !request.accounts.contains(&row.account) {
        return Some(Seated { row, state });
    }
    let args: Vec<String> = request
        .rules
        .map(|path| vec!["--rules".to_owned(), path.to_owned()])
        .unwrap_or_default();
    let _ = crate::fleet::usage::run(&args, &place.path);
    let state = store::read_all(&place.path).map_or(state, |events| replay(&events));
    Some(Seated { row, state })
}

/// 口座の最新の実測行（全窓・Measured / Unmeasured）の ts が `stale_s` 以内か（行が無い周は偽＝計測する）。
fn is_fresh(state: &State, label: &str, stale_s: u64) -> bool {
    let cutoff = fleet_cli::format_utc(state::now_secs().saturating_sub(stale_s));
    state
        .allowance
        .iter()
        .any(|(key, latest)| key.account == label && latest.ts >= cutoff)
}

/// 登録 row の口座の逼迫度を閾値で分ける（閾値ちょうどは以上＝R-C9-1 は「未満なら候補」の境界）。
fn reading(seated: &Seated, threshold: u64) -> Account {
    let label = seated.row.account.clone();
    match pressure(&seated.state, &label, seated.row.model.as_deref()) {
        None => Account::Unmeasured(label),
        Some(pct) if pct >= threshold => Account::Over(label, pct, threshold),
        Some(pct) => Account::Under(label, pct),
    }
}

/// 口座の逼迫度（account-autonomy.md §3 の定義・model = 登録 row の `model`〔無い row は None＝全 model 窓の最大の
/// 保守側〕）。窓の数え方は選定（[`crate::fleet::select`]）と同じ: 口座の最新の回のうち、数える窓に Unmeasured が
/// 在れば測れない・reset を過ぎた行は数えない（reset 無しの行は古くない実測として数える・ADR-0024 §2.2）・残った窓の
/// 最大の使用率。数える窓が 1 つも無ければ `None`。
fn pressure(state: &State, label: &str, model: Option<&str>) -> Option<u64> {
    let mine: Vec<&AllowanceLatest> = state
        .allowance
        .iter()
        .filter(|(key, _)| key.account == label)
        .map(|(_, latest)| latest)
        .collect();
    let newest = mine.iter().map(|latest| latest.ts.as_str()).max()?;
    let now = fleet_cli::now_utc();
    let mut found: Option<u64> = None;
    for latest in mine.iter().filter(|latest| latest.ts == newest) {
        match &latest.allowance {
            Allowance::Unmeasured(row) if counted(model, row.window, row.model.as_deref()) => return None,
            Allowance::Measured(row)
                if counted(model, Some(row.window), row.model.as_deref())
                    && row.resets_at.as_deref().is_none_or(|resets_at| resets_at >= now.as_str()) =>
            {
                found = found.max(Some(row.used_pct));
            }
            Allowance::Unmeasured(_) | Allowance::Measured(_) => {}
        }
    }
    found
}

/// その行を逼迫度に数えるか。model が与えられた周のモデル別窓はその model の行だけを数える（model の分からない行は
/// 保守側で数える・選定と同じ）。
fn counted(model: Option<&str>, window: Option<WindowKind>, row_model: Option<&str>) -> bool {
    match (window, model, row_model) {
        (Some(WindowKind::SevenDayModel), Some(want), Some(found)) => want == found,
        _ => true,
    }
}

/// 退避して止まった席の入口（account-autonomy.md §5）。**閉じた 3 値**（憲法 C11・bool で持たない）: 立て直しと
/// 終了の手は同じ (1)(2) を共有し、前面 process の読みで分かれる。
enum Entry {
    /// 立て直し（前面が shell＝session は終わっている）。
    Relaunch,
    /// 終了の手（前面が shell でない ∧ 直近の合図が退避の合図 ∧ 自席の未 consumed 退避物が在る）。
    Exit,
    /// どちらも立たない（以後は既存の順序へ）。
    None,
}

/// 入口の条件（順序固定・tmux を撃つ前面の読みは最後）: (1) 自席への直近の注入の記録が tick の合図（退避の合図か
/// 終了の合図・[`last_signal`]）(2) 打刻の最終行が `Stop`（[`stopped_after`]・退避の合図の周はその ts が (1) より後）
/// (3) pane の前面 process が shell（[`super::pane_is_shell`]）なら立て直し。shell でない周は、直近が退避の合図
/// （終了の合図の後は再送しない・back-off は送れなかった周のためにある）∧ 自席の未 consumed 退避物が在る
/// （FR28「退避物が無い session には撃たない」と同じ極性）なら終了の手。(1) の無い停止（user の終了・crash）は
/// 起こし直しも終了もしない（器が起こしたのでない停止に器が手を出さない）。
fn parked_entry(state_dir: &Path, target: &str, socket: Option<&str>, seat_dir: &Path, wm: &WmScan) -> Entry {
    let Some((kind, signalled)) = last_signal(state_dir, target) else {
        return Entry::None;
    };
    if !stopped_after(seat_dir, kind, signalled) {
        return Entry::None;
    }
    if super::pane_is_shell(socket, target) {
        return Entry::Relaunch;
    }
    match (kind, wm) {
        (InjectKind::Externalize, WmScan::Unconsumed(_)) => Entry::Exit,
        _ => Entry::None,
    }
}

/// (1) `<state_dir>/inject.jsonl` の同じ席の `who=seat-tick` の最新行が tick の合図（`kind=externalize` か `kind=exit`）
/// なら、その種類と ts。記録の形（判定行の `kind=` の token）は同じ module の [`body`] が書く。inject.jsonl は hook の
/// 記録（`hook:pre-tool-use` 等）も共有する（vessel-hook.md §6）が、それは注入ではないので飛ばす（`s2-07l.242`・
/// 退避の後に席が Bash を撃つと最新行が hook 行になり合図が見えなくなっていた）。
fn last_signal(state_dir: &Path, target: &str) -> Option<(InjectKind, u64)> {
    let seat = seat_name(target)?;
    let text = std::fs::read_to_string(inject_path(state_dir)).ok()?;
    let last = text
        .lines()
        .rev()
        .filter_map(|line| json_lite::parse_object(line).ok())
        .find(|pairs| {
            field(pairs, "seat").and_then(Value::as_str) == Some(seat.as_str())
                && field(pairs, "who").and_then(Value::as_str) == Some(WHO)
        })?;
    let what = field(&last, "what").and_then(Value::as_str)?;
    if !what.starts_with("decision=inject ") {
        return None;
    }
    let token = what.split_whitespace().find_map(|token| token.strip_prefix("kind="))?;
    let kind = [InjectKind::Externalize, InjectKind::Exit]
        .into_iter()
        .find(|kind| kind.as_str() == token)?;
    Some((kind, field(&last, "ts").and_then(Value::as_num)?))
}

/// 記録 1 行の key の値。
fn field<'a>(pairs: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    pairs.iter().find(|(found, _)| found == key).map(|(_, value)| value)
}

/// (2) 打刻の最終行が `Stop` か（読めない・無い周は偽＝起こさない側）。退避の合図の周は、その ts が合図（`after`）
/// より後であること（合図に応えて退避し、止まった）。終了の合図の周は ts の前後を問わない: `/exit` は打刻を
/// 積まず（`Event` は 3 値・seat-state.md §6・終了の hook は無い）、`Stop` が合図より前なのは終了の合図を送った
/// 条件そのもの（退避の合図 → その後の `Stop` → `/exit`）で、席が続きを始めた周は最終行が `UserPromptSubmit` に
/// なるので偽。
fn stopped_after(seat_dir: &Path, kind: InjectKind, after: u64) -> bool {
    let text = std::fs::read_to_string(state::path(seat_dir)).unwrap_or_default();
    text.lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .and_then(|line| state::Stamp::from_line(line).ok())
        .is_some_and(|stamp| {
            stamp.event == state::Event::Stop && (matches!(kind, InjectKind::Exit) || stamp.ts > after)
        })
}

/// 退避後の終了の手（account-autonomy.md §5・`s2-07l.226`）: 入口（[`parked_entry`]）の後は cycle lock（FR29 と同じ
/// 除外・作り直しの最中の席へ送らない）→ back-off（exit-stamp・閾値は `seat.tick_stale_s`・`s2-07l.252` で cycle-stamp
/// と分けた）→ [`send_exit`]。見送った周の理由は既存の語（`cycle-live` / `cycle-recent` / `cycle-stamp-unreadable`）。
fn exit_turn(request: &Request, place: &super::StateDir, dir: &Path, seen: &Seen) -> Verdict {
    if cycle::lock_is_live(dir, seen.ttl_s) {
        return Verdict::of(TickDecision::Noop(NoopReason::CycleLive));
    }
    let (stamp, blocked) = back_off(dir, cycle::EXIT_STAMP_FILE, seen.stale_s);
    if let Some(reason) = blocked {
        return held(stamp, reason);
    }
    Verdict { stamp: Some(stamp), ..Verdict::of(send_exit(request, place, dir)) }
}

/// [`EXIT`] を 1 行送る（順序固定）: 入力欄の門（cycle の `/clear` と同じ [`inject::guard_input`]・断りは
/// `input-busy` / `input-unknown` で **1 key も送らない**）→ exit-stamp（write-ahead・打てない周は送らない＝次の周も
/// 送りうる形を作らない）→ 送る（[`cycle::send_exit`]）→ 前面 process が shell になるかで送達を確かめる（[`exited`]）。
///
/// 送達を目印の出現数（[`inject::deliver`]）で測らないのは、`/exit` を受けた席は終わって pane が shell に置き換わり
/// 目印が増えない＝成功が `absent` に倒れるためである（実地 2026-09-14・`s2-07l.252`）。Enter の修復
/// （[`inject::nudge_enter`]）も通さない: 終わった席の pane に残る古い `❯ /exit` 行を入力欄と読んで **shell へ Enter を
/// 送る**形になる。窓の内に shell にならない周は `exit-unconfirmed` の error（rc 1）で、exit-stamp は打たれたまま
/// （再送しない・次の周は back-off が見る）。
fn send_exit(request: &Request, place: &super::StateDir, dir: &Path) -> TickDecision {
    let Some(pane) = pane_of(request.socket, request.target, request.capture_file) else {
        return exit_error(cycle::REASON_PANE_MISSING);
    };
    match inject::guard_input(&pane) {
        Ok(()) => {}
        Err(inject::InputGate::Busy) => return exit_error(cycle::REASON_INPUT_BUSY),
        Err(inject::InputGate::UnknownInput) => return exit_error(cycle::REASON_INPUT_UNKNOWN),
    }
    if cycle::write_exit_stamp(dir).is_err() {
        return exit_error(cycle::REASON_STAMP);
    }
    if !cycle::send_exit(request.socket, request.target, place, EXIT) {
        return exit_error(inject::REASON_TMUX_FAILED);
    }
    if exited(request) {
        TickDecision::Inject(InjectKind::Exit, inject::Settled::Consumed)
    } else {
        exit_error(REASON_EXIT_UNCONFIRMED)
    }
}

/// `/exit` の送達の確認: 窓（[`Request::settle`]）の内に刻み（[`Request::step`]）ごとに、target の前面 process が
/// shell になったか（入口 (3) と同じ [`super::pane_is_shell`]・typed な metadata・字面を読まない）。作り直しの確認
/// （cycle の `started`）と同じ窓・同じ刻み。
fn exited(request: &Request) -> bool {
    let deadline = Instant::now().checked_add(request.settle);
    while deadline.is_some_and(|at| Instant::now() < at) {
        std::thread::sleep(request.step);
        if super::pane_is_shell(request.socket, request.target) {
            return true;
        }
    }
    false
}

/// 終了の手を送れなかった周の判定（`exit-` を前置きして注入の断りや noop の語彙と分ける・[`inject_line`] と同型）。
fn exit_error(reason: &str) -> TickDecision {
    TickDecision::Error(format!("exit-{reason}"))
}

/// 退避して止まった席の立て直し（account-autonomy.md §5）: back-off（`s2-07l.110` の cycle-stamp だけ・閾値は
/// `seat.tick_stale_s`・exit-stamp は読まない＝`/exit` の次の周に評価される・`s2-07l.252`）→ cycle lock（FR29 と同じ
/// 除外）→ [`cycle::relaunch`]（session 用の選定・雛形の穴埋め・
/// 起動と復元の注入・登録 row の口座の更新）。候補なしは `account-no-candidate` で注入せず次の tick で選び直し、
/// 立て直しが送れない・確かめられない周は `relaunch-<理由>` の error（rc 1・次の周は back-off が見る）。
fn relaunch_turn(request: &Request, place: &super::StateDir, dir: &Path, seen: &Seen, seated: &Seated) -> Verdict {
    let (stamp, blocked) = back_off(dir, cycle::STAMP_FILE, seen.stale_s);
    if let Some(reason) = blocked {
        return held(stamp, reason);
    }
    if cycle::lock_is_live(dir, seen.ttl_s) {
        return held(stamp, NoopReason::CycleLive);
    }
    let result = cycle::relaunch(&cycle::Relaunch {
        target: request.target,
        socket: request.socket,
        state_dir: place,
        restore: request.restore,
        settle: request.settle,
        step: request.step,
        row: &seated.row,
        state: &seated.state,
        labels: request.accounts,
        threshold_pct: seen.threshold,
    });
    let (decision, relaunched) = match result {
        cycle::Relaunched::Done(label, settled) => (TickDecision::Inject(InjectKind::Relaunch, settled), label),
        cycle::Relaunched::None(found) => {
            (TickDecision::Noop(NoopReason::AccountNoCandidate), format!("none:{}", found.reason.as_str()))
        }
        cycle::Relaunched::Refused(reason) | cycle::Relaunched::Failed(reason) => {
            return Verdict { stamp: Some(stamp), ..Verdict::of(TickDecision::Error(format!("relaunch-{reason}"))) };
        }
    };
    Verdict { stamp: Some(stamp), relaunched: Some(relaunched), ..Verdict::of(decision) }
}

/// 開いた manifest（tracked の面）の `[[account]]` の label 列（宣言値・`--rules` が在ればその file の宣言・
/// `s2-07l.224`）。host の面の宣言は [`run`] が置き場から足す。宣言が無い manifest は空＝選定は候補なし。
pub fn account_labels(manifest: &Manifest) -> Vec<String> {
    manifest.accounts().iter().map(|account| account.label().to_owned()).collect()
}

/// 注入する 1 行と、それを送る周の席の状態（[`inject_line`] の入力）。
///
/// 畳むのは憲法 C4 の引数上限（R-C4-4.args = 5）ゆえ: 修復の門（[`inject::repair_of`]）が席の状態を
/// 読むので、素の引数で足すと 6 になる。
struct Signal<'a> {
    /// 何を注入するか。
    kind: InjectKind,
    /// 注入する 1 行。
    payload: &'a str,
    /// 席の状態の読み（typed・pane の字面ではない）。
    state: state::Read,
}

/// 1 行を注入し、成立したら自打刻する（退避の合図も打刻の合図も同じ経路。自打刻は打刻の合図の
/// brake〔[`pointer_recent`]〕にだけ効き、退避の合図と cycle は次の周も評価する）。busy な席へは
/// queue の形で届く（`.90`）。
///
/// 送達の後に Enter だけが落ちた周を**同じ周の中で**修復する（[`inject::nudge_enter`]・`s2-07l.150`）。
/// 修復が閉じない周（`EnterLost`）は自打刻しない＝次の周の判定がそれを測れる（入力欄に目印が残る
/// 席は入力欄の門が断り、`pointer-recent` の黙った noop にならない）。再注入までは主張しない。
fn inject_line(request: &Request, place: &super::StateDir, dir: &Path, signal: &Signal) -> TickDecision {
    let sending = inject::Request {
        target: request.target,
        socket: request.socket,
        payload: signal.payload,
        state_dir: Some(place),
    };
    match inject::deliver(&sending) {
        // 注入の断り（`busy` 等）は noop の語彙と字が重なるので、**前置きで分ける**。
        inject::Delivery::Refused(reason) | inject::Delivery::Unconfirmed(reason) => {
            TickDecision::Error(format!("inject-{reason}"))
        }
        inject::Delivery::Delivered(_, settled) => {
            match inject::nudge_enter(&sending, settled, signal.state) {
                inject::Settled::EnterLost => {
                    TickDecision::Inject(signal.kind, inject::Settled::EnterLost)
                }
                repaired => match heartbeat::touch_at(&stamp_path(dir)) {
                    Ok(()) => TickDecision::Inject(signal.kind, repaired),
                    Err(_) => TickDecision::Error(REASON_STAMP.to_owned()),
                },
            }
        }
    }
}

/// 判定の本体（記録の `what` と表示で**同じ字面**を使う）。context は判定の後ろ・cycle の前
/// （評価した順）。席の状態（`state=<値> event=<出所|none>`）は既存 token の**後ろに追加**する
/// （名前・順序・書式は不変・`s2-07l.95`・`event` は C10 の出所＝置き場の `source=` と混ぜない）。
/// cycle-stamp（`s2-07l.110`）は**その後ろ**（先に land した側の token が前・後から land した側が
/// その後ろ・planner 裁定 2026-09-12）で、cycle の評価まで進んだ周だけ載る。口座（`account=<label>:<pct>`）と
/// 立て直し（`relaunch=<label|none:理由>`・`s2-07l.211`）は同じ規律で**さらに後ろ**・評価した周だけ載る。
/// **置き場と出所は最後**（置き場が解けた周は判定に依らず載せる＝席側の打刻行と並べるだけで、
/// 別の dir を見ていることを記録から弁別できる・`s2-07l.70`）。
/// 注入した周は `consumed=<値>` の**直後**に理由（`reason=<語>`・queue と消費の周は無し）を足す
/// （`seat inject` の行と同じ並び・既存 token の名前と順序は不変・`s2-07l.150`）: 測れない周と
/// Enter が落ちた周を `false` と同じ顔で流さない（憲法 C10）。
fn body(target: &str, judged: &Judged, place: &super::StateDir) -> String {
    let verdict = &judged.verdict;
    let head = match verdict.decision {
        TickDecision::Inject(kind, settled) => format!(
            "decision=inject target={} consumed={}{} kind={}",
            sanitize_target(target),
            settled.as_str(),
            settled.reason().map_or_else(String::new, |why| format!(" reason={why}")),
            kind.as_str()
        ),
        TickDecision::Noop(reason) => format!("decision=noop reason={}", reason.as_str()),
        TickDecision::Error(ref reason) => body_of_error(reason),
    };
    let with_context = format!("{head}{}", judged.context.suffix());
    let with_cycle = match verdict.cycled.as_deref() {
        Some(found) => format!("{with_context} cycle={found}"),
        None => with_context,
    };
    let with_state = judged.state.map_or(String::new(), state::Read::suffix);
    let with_stamp = verdict.stamp.map_or(String::new(), Stamped::suffix);
    let with_relaunch = verdict
        .relaunched
        .as_deref()
        .map_or_else(String::new, |found| format!(" relaunch={found}"));
    format!(
        "{with_cycle}{with_state}{with_stamp}{}{with_relaunch}{}",
        verdict.account.suffix(),
        place.suffix()
    )
}

/// 実行系が回らなかった周の本体。
fn body_of_error(reason: &str) -> String {
    format!("decision=error reason={reason}")
}

/// 宣言 rule（cycle の確認の刻み）が読めない周の 1 行（`s2-07l.151`）。**判定を回さない**
/// ＝置き場も pane も見ない（置き場を解けない周と同じ早い側の断りで、記録も残らない）。
///
/// 語彙は [`decide`] の `no-rule` と同じ 1 つ: 読めない規則を理由に `decision=error` を名乗る
/// 形を 2 つに増やさない（理由の字面で routing する口を作らない・憲法 C11）。
pub fn render_no_rule() -> String {
    render(&body_of_error(meter::REASON_NO_RULE))
}

/// stdout / stderr へ出す 1 行。
fn render(body: &str) -> String {
    format!("seat: tick {body}")
}

/// 1 回の記録（**全周 1 行**・判定行と同じ字面）。
fn entry_of(target: &str, body: &str, started: Instant) -> InjectionRecord {
    InjectionRecord {
        schema: SCHEMA,
        who: WHO.to_owned(),
        what: body.to_owned(),
        when: WHEN.to_owned(),
        // 出力の byte 数＝判定行の長さ（注入 byte は便 2 の記録が持つ）。
        bytes: body.len() as u64,
        // 数えていないことを 0 と書かない。
        tokens: None,
        wall_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        seat: seat_name(target),
        ts: state::now_secs(),
    }
}

/// 1 回を席の記録へ積む。置き場が解けない周は書かない（rc は変えない）。
fn record(state_dir: &Path, target: &str, entry: &InjectionRecord) {
    let Ok(policy) = LockPolicy::embedded() else {
        return;
    };
    let path = inject::tick_path(state_dir, target);
    let _ = store::append_line(&path, &entry.to_line(), policy);
}

#[cfg(test)]
mod tests {
    use super::{inject_path, last_signal, pressure, INJECT_KINDS, NOOP_REASONS, WHO};
    use crate::fleet::{Allowance, AllowanceLatest, Measured, State, Unmeasured, UnmeasuredReason, WindowKind};
    use crate::order::is_declaration_order;

    /// reset がどの「いま」より後の窓。
    const LATER: &str = "2099-01-01T00:00:00Z";
    /// reset を過ぎた窓。
    const PAST: &str = "2000-01-01T00:00:00Z";

    /// `NoopReason` / `InjectKind` の字面は宣言順で全数を pin する（variant を足した周はここの件数が変わる・C2）。
    #[test]
    fn seat_account_noop_reasons_and_kinds_are_pinned_in_declaration_order() {
        let reasons: Vec<&str> = NOOP_REASONS.iter().map(|reason| reason.as_str()).collect();
        assert_eq!(
            reasons,
            [
                "pane-missing", "busy", "state-missing", "state-unreadable", "state-stale", "wm-unconsumed",
                "wm-unreadable", "cycle-live", "cycle-recent", "cycle-stamp-unreadable", "pointer-recent",
                "account-unmeasured", "account-no-candidate",
            ]
        );
        assert!(is_declaration_order(NOOP_REASONS, |reason| reason as usize));
        let kinds: Vec<&str> = INJECT_KINDS.iter().map(|kind| kind.as_str()).collect();
        assert_eq!(kinds, ["pointer", "externalize", "relaunch", "exit"]);
        assert!(is_declaration_order(INJECT_KINDS, |kind| kind as usize));
    }

    /// 実測 1 行。
    fn measured(account: &str, window: WindowKind, model: Option<&str>, used_pct: u64, resets_at: &str) -> Allowance {
        Allowance::Measured(Measured {
            account: account.to_owned(),
            window,
            model: model.map(str::to_owned),
            endpoint: "oauth-usage".to_owned(),
            used_pct,
            resets_at: Some(resets_at.to_owned()),
        })
    }

    /// 回の列を物理順に replay したのと同じ表（同じ key は後が勝つ）。
    fn table(rounds: &[(&str, Vec<Allowance>)]) -> State {
        let mut state = State::default();
        for (ts, rows) in rounds {
            for row in rows {
                state.allowance.insert(row.key(), AllowanceLatest { ts: (*ts).to_owned(), allowance: row.clone() });
            }
        }
        state
    }

    /// 席の逼迫度は登録 row の model の窓だけを数え（無い row は全 model の最大）、最新の回だけを読み、reset を
    /// 過ぎた窓は数えず、数える窓が Unmeasured なら測れない（選定と同じ窓の数え方・account-autonomy.md §3）。
    #[test]
    fn seat_account_pressure_reads_the_seat_model_and_the_latest_round() {
        let ts = "2026-09-13T05:59:00Z";
        let rows = table(&[(ts, vec![
            measured("a1", WindowKind::FiveHour, None, 10, LATER),
            measured("a1", WindowKind::SevenDay, None, 12, LATER),
            measured("a1", WindowKind::SevenDayModel, Some("Fable"), 95, LATER),
            measured("a1", WindowKind::SevenDayModel, Some("Opus"), 20, LATER),
            measured("a2", WindowKind::FiveHour, None, 100, PAST),
            measured("a2", WindowKind::SevenDay, None, 40, LATER),
        ])]);
        assert_eq!(pressure(&rows, "a1", Some("Opus")), Some(20), "Opus の席は Fable の 95 を数えない");
        assert_eq!(pressure(&rows, "a1", Some("Fable")), Some(95));
        assert_eq!(pressure(&rows, "a1", None), Some(95), "model の無い row は全 model の最大（保守側）");
        assert_eq!(pressure(&rows, "a2", None), Some(40), "reset を過ぎた 100 は数えない");
        assert_eq!(pressure(&rows, "a3", None), None, "実測行なし");
        let later = table(&[
            (ts, vec![measured("a1", WindowKind::FiveHour, None, 10, LATER)]),
            ("2026-09-13T06:00:00Z", vec![Allowance::Unmeasured(Unmeasured {
                account: "a1".to_owned(),
                window: None,
                model: None,
                endpoint: "oauth-usage".to_owned(),
                reason: UnmeasuredReason::NoCredentials,
            })]),
        ]);
        assert_eq!(pressure(&later, "a1", None), None, "最新の回が Unmeasured なら前の回の実測を読まない");
        let stale = table(&[(ts, vec![measured("a1", WindowKind::FiveHour, None, 99, PAST)])]);
        assert_eq!(pressure(&stale, "a1", None), None, "reset を過ぎた行だけ＝測れない");
    }

    /// 消費の無い窓（0%・reset 無し）は古くない実測として数える＝逼迫度が `None` に倒れない（ADR-0024 §2.2）。
    #[test]
    fn seat_account_pressure_counts_idle_window_without_reset() {
        let ts = "2026-09-13T05:59:00Z";
        let idle = |window| {
            Allowance::Measured(Measured {
                account: "a1".to_owned(),
                window,
                model: None,
                endpoint: "oauth-usage".to_owned(),
                used_pct: 0,
                resets_at: None,
            })
        };
        let only_idle = table(&[(ts, vec![idle(WindowKind::FiveHour), idle(WindowKind::SevenDay)])]);
        assert_eq!(pressure(&only_idle, "a1", None), Some(0), "reset 無しだけでも測れた口座");
        let mixed = table(&[(ts, vec![idle(WindowKind::FiveHour), measured("a1", WindowKind::SevenDay, None, 30, LATER)])]);
        assert_eq!(pressure(&mixed, "a1", None), Some(30), "reset 無しの 0 は最大を動かさない");
    }

    /// Unmeasured 1 行（model 別窓）。
    fn unmeasured(account: &str, window: Option<WindowKind>, model: Option<&str>) -> Allowance {
        Allowance::Unmeasured(Unmeasured {
            account: account.to_owned(),
            window,
            model: model.map(str::to_owned),
            endpoint: "oauth-usage".to_owned(),
            reason: UnmeasuredReason::ShapeMismatch,
        })
    }

    /// 席の model と**違う** model の SevenDayModel 窓だけが Unmeasured なら、その行は数えない＝測れた口座
    /// （Unmeasured 腕の guard を `true` に落とす: 落とすと None に化ける）。
    // flip-check: retroactive s2-07l.232
    #[test]
    fn mutant_in_seat_account_pressure_ignores_unmeasured_window_of_another_model() {
        let ts = "2026-09-13T05:59:00Z";
        let rows = table(&[(ts, vec![
            measured("a1", WindowKind::FiveHour, None, 10, LATER),
            measured("a1", WindowKind::SevenDayModel, Some("Opus"), 20, LATER),
            unmeasured("a1", Some(WindowKind::SevenDayModel), Some("Fable")),
        ])]);
        assert_eq!(pressure(&rows, "a1", Some("Opus")), Some(20), "Fable 窓の Unmeasured は Opus の席に効かない");
    }

    /// 数える窓（five_hour・席の model と一致する SevenDayModel）が Unmeasured なら測れない
    /// （Unmeasured 腕の guard を `false` に落とす: 落とすと測れたことにする）。
    // flip-check: retroactive s2-07l.232
    #[test]
    fn mutant_in_seat_account_pressure_is_unmeasured_when_a_counted_window_is_unmeasured() {
        let ts = "2026-09-13T05:59:00Z";
        let five_hour = table(&[(ts, vec![
            unmeasured("a1", Some(WindowKind::FiveHour), None),
            measured("a1", WindowKind::SevenDayModel, Some("Opus"), 20, LATER),
        ])]);
        assert_eq!(pressure(&five_hour, "a1", Some("Opus")), None, "five_hour の Unmeasured は席の model に関係なく数える");
        let same_model = table(&[(ts, vec![
            measured("a1", WindowKind::FiveHour, None, 10, LATER),
            unmeasured("a1", Some(WindowKind::SevenDayModel), Some("Opus")),
        ])]);
        assert_eq!(pressure(&same_model, "a1", Some("Opus")), None, "席の model と同じ SevenDayModel 窓の Unmeasured");
        assert_eq!(pressure(&same_model, "a1", None), None, "model の無い row は全 model 窓を数える（保守側）");
    }

    /// 記録 1 行（`InjectionRecord::to_line` と同じ key の flat JSON）。
    fn inject_line(who: &str, what: &str, seat: &str, ts: u64) -> String {
        format!(
            r#"{{"schema":1,"who":"{who}","what":"{what}","when":"tick","bytes":0,"tokens":null,"wall_ms":0,"seat":"{seat}","ts":{ts}}}"#
        )
    }

    /// `inject.jsonl` を `lines` で置いた state dir で [`last_signal`] を読む（`None` の lines は file を置かない）。
    fn signal_of(name: &str, lines: Option<&[String]>) -> Option<(&'static str, u64)> {
        let dir = std::env::temp_dir().join(format!("seat-tick-signal-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok();
        if let Some(lines) = lines {
            std::fs::write(inject_path(&dir), format!("{}\n", lines.join("\n"))).ok();
        }
        let found = last_signal(&dir, "seat1").map(|(kind, ts)| (kind.as_str(), ts));
        std::fs::remove_dir_all(&dir).ok();
        found
    }

    /// (a) 退避の合図の後ろに同じ席の hook の記録（role-allow・session-start）が在っても、合図は hook 行に隠れない
    /// （hook の記録は注入ではない・vessel-hook.md §6）。別の席の tick の行も数えない。
    #[test]
    fn seat_exit_signal_skips_hook_rows_after_externalize() {
        let lines = [
            inject_line(WHO, "decision=inject account=a1:100 kind=externalize", "seat1", 100),
            inject_line("hook:pre-tool-use", "decision=role-allow capability=launch", "seat1", 200),
            inject_line("hook:session-start", "served version=2", "seat1", 300),
            inject_line(WHO, "decision=inject kind=pointer", "seat2", 400),
        ];
        assert_eq!(signal_of("hook-rows", Some(&lines)), Some(("externalize", 100)));
    }

    /// (b) 退避の合図の後ろに同じ席の tick の別の注入（heartbeat の `kind=pointer`）が在れば、合図は上書きされている。
    #[test]
    fn seat_exit_signal_is_overwritten_by_a_later_tick_pointer() {
        let lines = [
            inject_line(WHO, "decision=inject account=a1:100 kind=externalize", "seat1", 100),
            inject_line(WHO, "decision=inject kind=pointer", "seat1", 200),
            inject_line("hook:pre-tool-use", "decision=role-allow capability=launch", "seat1", 300),
        ];
        assert_eq!(signal_of("pointer", Some(&lines)), None);
    }

    /// (c) 同じ席の tick の行が無い・file が無い周は合図なし。
    #[test]
    fn seat_exit_signal_is_none_without_tick_rows() {
        let hook_only = [inject_line("hook:pre-tool-use", "decision=role-allow capability=launch", "seat1", 100)];
        assert_eq!(signal_of("hook-only", Some(&hook_only)), None, "hook の行だけ");
        assert_eq!(signal_of("empty", Some(&[])), None, "空");
        assert_eq!(signal_of("missing", None), None, "file が無い");
    }
}
