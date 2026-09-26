//! 群の逼迫の読み手（設計 docs/design/account-lifecycle.md §19・契約表の行 h・ADR-0055）。
//!
//! 閾値の rules 行 3 本（窓ごとに 1 行・[`Caps`]）と、口座 1 つの最新の実測から逼迫を判じる 1 本（[`pressed`]）を持つ。
//! 読み手は 2 つで、**同じこの 1 本を通る**（C2）: dispatch の 1 周の群の段（`pipe::dispatch` の `group`）と、席自身の
//! hook（[`lines`]・SessionStart と UserPromptSubmit）。
//!
//! hook は自席の登録 row の口座と群の今の口座（[`current_of`]）を読み、記録の口座が登録 row と食い違う周は逼迫を測らず移動中の
//! 1 行（`row=` / `current=`）だけを出す（設計 §21 形 2・記録が読めない周は 0 行）。それ以外は登録 row の口座 1 つだけを測り、
//! **鮮度の外は hook の中で測らない**（`hook.timeout_s` < `fleet.usage_timeout_s`）:
//! 器自身を子として `fleet usage --state-dir D --account <label> --fresh` で起こして待たず、`usage: measuring` の 1 行を出す
//! （値は次の話す番で読める）。群に属さない anchor・群 0 の host は 1 語も出さない。
//!
//! 第 3 段（§20）の host の根の 2 つの記録もここが持つ（読み手が dispatch の 1 周・席の起動・doctor・hook の 4 つ＝1 本に置く）:
//! 群の今の口座の記録（[`Record`]・書くのは 1 周の群の段だけ・[`write_current`]）とその解決の 1 関数（[`current_of`]・記録 >
//! 種）、hook が逼迫を読んだ周に置く移動を頼む記録（[`put_request`]・在れば上書きしない）。どちらも前の file を消さず履歴の
//! dir へ move する（[`to_history`]・N1.2）。
//!
//! 群の段の lock（[`Lock`]）と、移動の続きで保留の席へ送る 1 行（[`EXIT`]）と `/exit` の確認 dialog の既定の行
//! （[`exit_dialog`]）もここが持つ: 手は 2 つ（dispatch の 1 周の群の段と管理 tick の移動の周・設計 seat-heartbeat.md §4）で、
//! 同じ 1 本の lock と同じ値を読む（二重に書かない・C17）。
//!
//! 群の移動の判定も 1 本（[`judge`]・設計 seat-heartbeat.md §9 形 1）: 測る集合 → 鮮度の外の計測 → 判定 → 記録と承認 event /
//! 断りの event と頼みの履歴化。呼び手は dispatch の 1 周の群の段と管理 tick で、判定の側は席の pane に触らない（通知・退避の
//! 合図・起こし直しは呼び手が出力の値で撃つ）。移り先は群の予約（[`reserve`]・残量の鍵 [`by_key`] で並べる・設計
//! account-lifecycle.md §29）で、doctor の群の行の `next=` も同じ 1 関数を測らずに呼ぶ。

use super::{record, record_lines, Emit, Hooked};
use crate::account::seat_accounts;
use crate::fleet::cli::{host, now_utc};
use crate::fleet::select::Model;
use crate::fleet::store::{self, LockPolicy};
use crate::fleet::usage;
use crate::fleet::{epoch_of, replay, Allowance, Event, EventKind, Measured, State, WindowKind, SCHEMA};
use crate::invocation::Invocation;
use crate::name::NAME;
use crate::rules::manifest::{AccountGroup, Manifest};
use crate::rules::RuleValue;
use crate::seat::inject::Confirm;
use crate::seat::role::Role;
use crate::seat::{host_groups_dir, sanitize_target};
use std::cmp::Reverse;
use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Instant;

/// 群の段の lock の file 名（群用 dir の 1 file・`create_new`・設計 account-lifecycle.md §20 形 1 / 5）。
const LOCK_FILE: &str = "lock";

/// 移動の続きで pane が shell でない保留の席へ送る 1 行（席は自分の process を終えられない＝器が代わりに打つ・設計
/// account-lifecycle.md §21 形 1）。
pub const EXIT: &str = "/exit";

/// `/exit` の確認 dialog の既定の行（入力欄の門の tail の字面・畳んで等値で比べる・設計 account-lifecycle.md §22 形 2）。
const EXIT_DIALOG_ROW: &str = "1. Exit and stop tasks";

/// 既定の行へ Enter を送った周の記録の `what`（設計 account-lifecycle.md §22 形 2）。
const EXIT_DIALOG_WHAT: &str = "enter:exit-dialog";

/// `/exit` の確認 dialog の既定の行へ Enter を送る口の材料（値はこの 1 か所・記録の `who` は呼び手の名）。
pub fn exit_dialog(who: &str) -> Confirm<'_> {
    Confirm { who, row: EXIT_DIALOG_ROW, what: EXIT_DIALOG_WHAT }
}

/// 群の段の lock（**群の段と tick の移動の周が共有する 1 本**・握った周だけ在る・drop で外す・設計 seat-heartbeat.md §4 形 2）。
pub struct Lock(PathBuf);

impl Lock {
    /// 群用 dir に lock の file を `create_new` で置く（中身は握った process の pid＝人が残りを読む）。既に在る・置けない周は
    /// `Err`（呼び手は 1 つも撃たずに止まる）。
    pub fn take(dir: &Path) -> std::io::Result<Self> {
        let path = dir.join(LOCK_FILE);
        fs::create_dir_all(dir)?;
        let mut file = OpenOptions::new().write(true).create_new(true).open(&path)?;
        let _ = writeln!(file, "pid={}", std::process::id());
        Ok(Self(path))
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// 群の今の口座の記録の拡張子（`<群用 dir>/<群の名>.account`）。
const CURRENT_EXT: &str = "account";

/// 移動を頼む記録の拡張子（`<群用 dir>/<群の名>.request`）。
const REQUEST_EXT: &str = "request";

/// 群用 dir の下の履歴の dir の名（前の記録と応えた頼みの move 先・消さない）。
pub const HISTORY_DIR: &str = "history";

/// 記録の理由（器が書くのは移動の 1 つだけ）。
const REASON_MOVE: &str = "move";

/// 群の今の口座の記録（群ごとに高々 1 file・`account=` / `ts=` / `reason=move` / `previous=` の 4 行をこの順で）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// 今の口座 label（移り先）。
    pub account: String,
    /// 書いた時刻（UTC）。
    pub ts: String,
    /// 前の口座 label（移る前の今の口座）。
    pub previous: String,
}

impl Record {
    /// file の本文。
    pub fn render(&self) -> String {
        format!("account={}\nts={}\nreason={REASON_MOVE}\nprevious={}\n", self.account, self.ts, self.previous)
    }

    /// [`Self::render`] の形だけを読む（key の欠け・順の違い・余りの行・空白を含む値・理由が move でない形は `None`）。
    pub fn parse(text: &str) -> Option<Self> {
        let mut lines = text.lines();
        let mut value = |key: &str| -> Option<String> {
            let found = lines.next()?.strip_prefix(key)?.strip_prefix('=')?;
            (!found.is_empty() && !found.contains(char::is_whitespace)).then(|| found.to_owned())
        };
        let (account, ts, reason, previous) = (value("account")?, value("ts")?, value("reason")?, value("previous")?);
        (reason == REASON_MOVE && lines.next().is_none()).then_some(Self { account, ts, previous })
    }
}

/// 群の今の口座の出所（閉じた 2 値）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// host の根の記録（1 周の群の段が移した）。
    Record,
    /// 記録が無い＝宣言の候補の先頭（ADR-0049）。
    Seed,
}

/// 解決した群の今の口座。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Current {
    /// 口座 label。
    pub label: String,
    /// 出所。
    pub source: Source,
    /// 記録の ts（`YYYY-MM-DDTHH:MM:SSZ`・種は `None`・移動の鍵 [`signal_key`] にだけ使う・seat-heartbeat.md §14 形 2）。
    pub ts: Option<String>,
}

/// 記録が在るのに読めない（typed に止まる・種に読み替えない・C10）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordError {
    /// file を開けない・UTF-8 でない（候補の列が空の宣言もここ＝§17 形 3 が断るので現物には来ない）。
    Unreadable,
    /// 形が [`Record::render`] でない。
    Malformed,
}

impl RecordError {
    /// 断りの行の字面（便用の除外の断り・[`crate::rules::GroupedError`]）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unreadable => "unreadable",
            Self::Malformed => "malformed",
        }
    }
}

/// 群の名の file（群用 dir の直下・名は file 名に使える字面へ潰す）。
fn group_file(dir: &Path, group: &str, ext: &str) -> PathBuf {
    dir.join(format!("{}.{ext}", sanitize_target(group)))
}

/// 群の今の口座の記録の path。
pub fn current_path(dir: &Path, group: &str) -> PathBuf {
    group_file(dir, group, CURRENT_EXT)
}

/// 移動を頼む記録の path。
pub fn request_path(dir: &Path, group: &str) -> PathBuf {
    group_file(dir, group, REQUEST_EXT)
}

/// 群の判定の打刻の path（`<群用 dir>/<群の名>.judged`・書き手は管理 tick だけ・設計 seat-heartbeat.md §9 形 2）。
pub fn judged_path(dir: &Path, group: &str) -> PathBuf {
    group_file(dir, group, "judged")
}

/// 移り先の無い断りの印の path（`<群用 dir>/<群の名>.refused`・読み手は doctor だけ・設計 account-lifecycle.md §31 形 2）。
pub fn refused_path(dir: &Path, group: &str) -> PathBuf {
    group_file(dir, group, "refused")
}

/// 断りの印の本文（1 行 `ts=<UTC の秒> reason=no-candidate`）。
pub fn refused_mark(ts: &str) -> String {
    format!("ts={ts} reason={NO_CANDIDATE}\n")
}

/// 断りの印の ts（[`refused_mark`] の形だけ・ts が [`epoch_of`] の形でない・余りの field・理由の違いは `None`）。
pub fn refused_ts(text: &str) -> Option<&str> {
    let ts = text.strip_suffix(&format!(" reason={NO_CANDIDATE}\n"))?.strip_prefix("ts=")?;
    (!ts.contains(char::is_whitespace) && epoch_of(ts).is_some()).then_some(ts)
}

/// 断りの印を書く（**書き手はこの 1 本**・一時 file → rename・前の印は上書き）。
fn write_refused(dir: &Path, group: &str) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let path = refused_path(dir, group);
    let temporary = path.with_extension("refused.tmp");
    fs::write(&temporary, refused_mark(&now_utc()))?;
    fs::rename(&temporary, &path)
}

/// 断りの印が在れば履歴へ退避する（消さない・C1）。呼び手は Stay の周と記録を書く周。
fn clear_refused(dir: &Path, group: &str) {
    let path = refused_path(dir, group);
    if path.exists() {
        let _ = to_history(dir, &path);
    }
}

/// 群の今の口座（**解決の 1 関数**・設計 §20 形 2）: 記録が在ればその label・無ければ種（面の読みが埋めた種の欄・§28）・在るのに
/// 読めなければ [`RecordError`]。読み手は dispatch の 1 周・席の起動・doctor の 3 つで、種の読みはこの中だけに在る。記録の ts が
/// [`epoch_of`] の形でない記録も [`RecordError::Malformed`]（形でない記録を種に読み替えない・seat-heartbeat.md §13 形 2）。
pub fn current_of(state_dir: &Path, group: &AccountGroup) -> Result<Current, RecordError> {
    match fs::read_to_string(current_path(&host_groups_dir(state_dir), group.name())) {
        Ok(text) => Record::parse(&text)
            .filter(|found| epoch_of(&found.ts).is_some())
            .map(|found| Current { label: found.account, source: Source::Record, ts: Some(found.ts) })
            .ok_or(RecordError::Malformed),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            Ok(Current { label: group.seed().to_owned(), source: Source::Seed, ts: None })
        }
        Err(_) => Err(RecordError::Unreadable),
    }
}

/// 猶予の残りの秒（**1 関数**・設計 seat-heartbeat.md §14 形 2）: 合図の `at` + `grace_s` − `now` が正ならその秒。猶予 0・越えた
/// 周は `None`（猶予なし＝`/exit` を送ってよい）。群の記録の ts は入らない。呼び手は tick の移動の周と群の段の続きの周の 2 つ。
pub fn grace_left(at: u64, now: u64, grace_s: u64) -> Option<u64> {
    Some(at.saturating_add(grace_s).saturating_sub(now)).filter(|left| *left > 0)
}

/// 退避の合図の記録の file 名（席の置き場の直下・1 行 `to=<移り先> ts=<記録の ts か seed> at=<epoch 秒>`・seat-heartbeat.md §14）。
pub const SIGNAL_FILE: &str = "move-signal";

/// 移動の鍵（移り先・群の記録の ts か種の `seed`）。「同じ移動」はこの 2 つの等値（口座が同じでも記録が別なら別の移動）。
pub fn signal_key(current: &Current) -> (&str, &str) {
    (current.label.as_str(), current.ts.as_deref().unwrap_or("seed"))
}

/// 合図の記録（鍵 + 合図を書いた epoch 秒）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signal {
    /// 移り先の口座。
    pub to: String,
    /// 群の記録の ts か `seed`。
    pub ts: String,
    /// 合図を書いた epoch 秒（猶予の起点）。
    pub at: u64,
}

impl Signal {
    /// 1 行を読む（**読み手はこの 1 本**）: 3 field が揃わない・順が違う・`at` が整数でない・前の版の `ts=` だけの行は `None`。
    pub fn parse(text: &str) -> Option<Self> {
        let mut fields = text.strip_suffix('\n')?.split(' ');
        let mut value = |key: &str| -> Option<String> {
            let found = fields.next()?.strip_prefix(key)?.strip_prefix('=')?;
            (!found.is_empty() && !found.contains(char::is_whitespace)).then(|| found.to_owned())
        };
        let (to, ts, at) = (value("to")?, value("ts")?, value("at")?.parse().ok()?);
        fields.next().is_none().then_some(Self { to, ts, at })
    }
}

/// 席の置き場 `seat` の、鍵が `key` の移動の合図の `at`（無い・別の移動・読めない周は `None`）。
pub fn signalled(seat: &Path, key: (&str, &str)) -> Option<u64> {
    let found = Signal::parse(&fs::read_to_string(seat.join(SIGNAL_FILE)).ok()?)?;
    (found.to == key.0 && found.ts == key.1).then_some(found.at)
}

/// 合図の記録を書く（**書き手はこの 1 本**・一時 file → rename・前の記録は上書き）。呼び手は tick の合図の周と群の段の移動の周。
pub fn write_signal(seat: &Path, (to, ts): (&str, &str), now: u64) -> std::io::Result<()> {
    fs::create_dir_all(seat)?;
    let temporary = seat.join(format!("{SIGNAL_FILE}.tmp"));
    fs::write(&temporary, format!("to={to} ts={ts} at={now}\n"))?;
    fs::rename(&temporary, seat.join(SIGNAL_FILE))
}

/// 退避の合図の 1 行（**字面はこの 1 関数**・群の段と tick が同じ字面を送る・設計 seat-heartbeat.md §13 形 4）。
pub fn evacuate_line(group: &str, to: &str, left: u64) -> String {
    format!(
        "{NAME} group: evacuate group={group} to={to} — 新しい subagent を起こさず今の作業に区切りをつけ、作業記憶を台帳と git に残す（{left} 秒の後に器が /exit を送る）"
    )
}

/// 記録を書く（**1 周の群の段だけが lock の内側で撃つ**・設計 §20 形 1）: 一時 file に書き、前の記録が在れば履歴へ move して
/// から rename する＝群用 dir の記録は高々 1 file。
pub fn write_current(dir: &Path, group: &str, record: &Record) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let path = current_path(dir, group);
    let temporary = path.with_extension(format!("{CURRENT_EXT}.tmp"));
    fs::write(&temporary, record.render())?;
    if path.exists() {
        to_history(dir, &path)?;
    }
    fs::rename(&temporary, &path)
}

/// 群用 dir の file を履歴の dir へ move する（`history/<file 名>.<ts>.<n>`・消さない・同じ秒の 2 つ目は `n` で分ける）。
pub fn to_history(dir: &Path, path: &Path) -> std::io::Result<()> {
    let history = dir.join(HISTORY_DIR);
    fs::create_dir_all(&history)?;
    let name = path.file_name().map(|found| found.to_string_lossy().into_owned()).unwrap_or_default();
    let stamp = sanitize_target(&crate::fleet::cli::now_utc());
    let free = (0..1000_u32).map(|n| history.join(format!("{name}.{stamp}.{n}"))).find(|to| !to.exists());
    fs::rename(path, free.ok_or_else(|| std::io::Error::from(std::io::ErrorKind::AlreadyExists))?)
}

/// 移動を頼む記録を置く（設計 §20 形 4・`ts=` / `account=` / `window=` の 3 行）。**在れば上書きしない**（`create_new`）。
/// 置けた周だけ `true`。
pub fn put_request(dir: &Path, group: &str, account: &str, window: WindowKind) -> bool {
    let body = format!("ts={}\naccount={account}\nwindow={}\n", crate::fleet::cli::now_utc(), window.short());
    fs::create_dir_all(dir).is_ok()
        && OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(request_path(dir, group))
            .and_then(|mut file| file.write_all(body.as_bytes()))
            .is_ok()
}

/// 5 時間窓の閾値の rules 行（値は manifest だけが持つ・C1 / C5）。
const ROW_FIVE: &str = "fleet.group_pressure_5h_pct";

/// 7 日窓の閾値の rules 行。
const ROW_SEVEN: &str = "fleet.group_pressure_7d_pct";

/// モデル別 7 日窓の閾値の rules 行。
const ROW_MODEL: &str = "fleet.group_pressure_model_pct";

/// 記録の `what`（逼迫の 1 行・計測の子を起こした 1 行）。
const WHAT_GROUP: &str = "group-pressure";

/// 窓ごとの閾値（使用率の百分率・rules 行 3 本の値）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    /// 5 時間窓。
    five: u64,
    /// 7 日窓。
    seven: u64,
    /// モデル別 7 日窓。
    model: u64,
}

impl Caps {
    /// 3 行を読む。どれかが無い・不発効・整数でない周は理由つきの `Err`（既定の値を焼かない・C1）。
    pub fn of(manifest: &Manifest) -> Result<Self, String> {
        Ok(Self {
            five: crate::rules::int_row(manifest, ROW_FIVE)?,
            seven: crate::rules::int_row(manifest, ROW_SEVEN)?,
            model: crate::rules::int_row(manifest, ROW_MODEL)?,
        })
    }

    /// 窓に当てる行の値（**窓ごとに別の行**・網羅 match）。
    fn cap(self, window: WindowKind) -> u64 {
        match window {
            WindowKind::FiveHour => self.five,
            WindowKind::SevenDay => self.seven,
            WindowKind::SevenDayModel => self.model,
        }
    }
}

/// 逼迫と判じた 1 窓（閾値を越えた窓のうち使用率が最大の 1 つ）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pressed {
    /// 窓。
    pub window: WindowKind,
    /// 使用率（整数 %）。
    pub used: u64,
    /// その窓の閾値の行の値。
    pub cap: u64,
}

/// 口座 1 つの最新の実測の行から逼迫を判じる（**pure**・判定の 1 本）。実測の窓のどれかの `used_pct` が対応する行の値
/// **以上**なら、越えた窓のうち使用率が最大の 1 つ（同率は行の並び＝窓の宣言順で先の 1 つ）。測れなかった行は数えない
/// （0 に読み替えない・越えたとも言わない）。モデル別 7 日窓は `models`（役割の model の表示名の集合）に表示名が在る行だけを
/// 数え（集合が空なら数えない・model を parse できない行も数えない）、5 時間窓と 7 日窓は集合に依らない（設計 §33 形 1）。
pub fn pressed(rows: &[Allowance], caps: Caps, models: &BTreeSet<&str>) -> Option<Pressed> {
    rows.iter()
        .filter_map(|row| match row {
            Allowance::Measured(found) => Some(found),
            Allowance::Unmeasured(_) => None,
        })
        .filter(|found| {
            found.window != WindowKind::SevenDayModel
                || found.model.as_deref().and_then(Model::parse).is_some_and(|model| models.contains(model.display()))
        })
        .map(|found| Pressed { window: found.window, used: found.used_pct, cap: caps.cap(found.window) })
        .filter(|found| found.used >= found.cap)
        .fold(None, |best: Option<Pressed>, found| match best {
            Some(kept) if kept.used >= found.used => Some(kept),
            _ => Some(found),
        })
}

/// anchor の属する群（宣言順で最初の 1 つ・宣言は同じ置き場を 2 つの群に置けない）。
pub fn group_of<'a>(manifest: &'a Manifest, anchor: &str) -> Option<&'a AccountGroup> {
    manifest.groups().iter().find(|group| group.anchors().iter().any(|found| found == anchor))
}

/// 断りの理由（移り先の候補が無い＝妥協の移動を作らない・ADR-0020 §2.4）。
const NO_CANDIDATE: &str = "no-candidate";

/// 移り先の無い群の断りの 1 行（群の段の通知と tick の自席への 1 行が同じ字面・設計 seat-heartbeat.md §9 形 3）。
pub fn refused_line(group: &AccountGroup) -> String {
    format!("{NAME} group: move-refused group={} reason={NO_CANDIDATE}", group.name())
}

/// 判定の 1 本の入力（設計 seat-heartbeat.md §9 形 1）。
pub struct Judge<'a> {
    /// 置き場（event log・記録の置き場の出所・登録 row はこの置き場の log から読む）。
    pub state_dir: &'a Path,
    /// 合わせた面（tracked + host の面・鮮度の行と起こし直しの行の出所）。
    pub manifest: &'a Manifest,
    /// 判じる群。
    pub group: &'a AccountGroup,
    /// 周の頭の各群の今の口座（と記録を読めない群の候補）＝先の群の予約の導きが外す。
    pub head: &'a BTreeSet<String>,
    /// いまの各群の今の口座（同じ周で先に移った群の移り先で更新・記録を読めない群の候補も）＝判じる群の移り先にしない。
    pub taken: &'a BTreeSet<String>,
    /// 鮮度に依らず測る口座（頼みの在る群の測る集合の和・tick は空）。
    pub forced: &'a BTreeSet<String>,
    /// 窓ごとの閾値。
    pub caps: Caps,
    /// 計測の口（口座・鮮度に依らず測るか）。
    pub measure: &'a dyn Fn(&str, bool),
}

/// 候補なしの周の断り（閉じた 2 値）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// 断りの event を記した。
    Recorded,
    /// 同じ実測に既に断った（記さない）。
    Repeated,
}

/// 判定の出力（**閉じた 4 値**）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Judgement {
    /// 記録を移り先へ書き承認 event を記した。
    Moved(String),
    /// 今の口座は逼迫でない（測る集合のうち逼迫の口座と窓）。
    Stay(Vec<(String, Pressed)>),
    /// 今の口座が逼迫で移り先が無い。
    NoCandidate(Refusal),
    /// 記録・event log・移動の前提が読めない。
    Unreadable,
}

/// 測る集合: 群の今の口座 ∪ 群の置き場の席の登録 row の口座（重複は畳む）。
pub fn measured_set(group: &AccountGroup, current: &Current, state: &State) -> BTreeSet<String> {
    std::iter::once(current.label.as_str()).chain(seat_accounts(group, state)).map(str::to_owned).collect()
}

/// 群 1 つの移動の判定（**1 本**・lock は呼び手が握る）: 測る集合 ∖ `measured` を 1 回ずつ測り（`forced` の口座は鮮度に依らず）、
/// 測った口座を `measured` へ足す。今の口座が逼迫でなければ [`Judgement::Stay`]、逼迫なら移り先（[`reserve`]）へ記録を書いて
/// 承認 event を記すか、断りの event を記す（同じ実測に 2 度断らない）。頼みの在る群（測る集合が全部 `forced`）は判定の後に
/// 頼みを履歴へ move する。
pub fn judge(input: &Judge<'_>, measured: &mut BTreeSet<String>) -> Judgement {
    let dir = host_groups_dir(input.state_dir);
    let (Ok(current), Ok(events)) = (current_of(input.state_dir, input.group), store::read_all(input.state_dir)) else {
        return Judgement::Unreadable;
    };
    let set = measured_set(input.group, &current, &replay(&events));
    let request = request_path(&dir, input.group.name());
    let requested = request.is_file() && set.is_subset(input.forced);
    for label in &set {
        if measured.insert(label.clone()) {
            (input.measure)(label, input.forced.contains(label));
        }
    }
    let judged = decide(input, &current.label, &set, measured);
    if requested {
        let _ = to_history(&dir, &request);
    }
    judged
}

/// 計測の後の判定（[`judge`] の続き）。
fn decide(input: &Judge<'_>, current: &str, set: &BTreeSet<String>, measured: &mut BTreeSet<String>) -> Judgement {
    let Ok(events) = store::read_all(input.state_dir) else {
        return Judgement::Unreadable;
    };
    let state = replay(&events);
    // 役割の model の行が無い周は逼迫を測らず移らず断らない（記録 0・event 0・設計 §33 形 3）。
    let Some(models) = role_models(input.manifest, input.group, &state) else {
        return Judgement::Unreadable;
    };
    let pressed_of = |label: &str| usage::latest_of(&state, label).and_then(|rows| pressed(&rows, input.caps, &models));
    if pressed_of(current).is_none() {
        clear_refused(&host_groups_dir(input.state_dir), input.group.name());
        return Judgement::Stay(set.iter().filter_map(|label| Some((label.clone(), pressed_of(label)?))).collect());
    }
    // 移り先は自分の予約（役割の model の行が無い周・読めない周は移らず断らない＝記録 0・event 0・設計 §29 形 1 / 2）。
    match reserve(input, measured) {
        Ok(Some(target)) if approve(input, current, &target) => Judgement::Moved(target),
        Ok(None) => refuse(input, current),
        Ok(Some(_)) | Err(_) => Judgement::Unreadable,
    }
}

/// 予約を導けない周（閉じた 2 値・判定は [`Judgement::Unreadable`] に、doctor の `next=` は語に写す）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unreserved {
    /// 役割の model・鮮度の rules 行が無い（集合を空に読み替えない・既定を出さない・C1）。
    NoRule,
    /// event log を読めない。
    Unreadable,
}

/// 各群の今の口座（記録 > 種）∪ 記録を読めない群の候補の全部（今の口座が分からない＝移り先にしない・fail-closed）。
pub fn currents_of(state_dir: &Path, manifest: &Manifest) -> BTreeSet<String> {
    manifest
        .groups()
        .iter()
        .flat_map(|found| current_of(state_dir, found).map_or_else(|_| found.accounts().to_vec(), |current| vec![current.label]))
        .collect()
}

/// 役割の model の集合（設計 §29 形 1）: 群の置き場の席の登録 row の役割（重複は畳む）ごとの rules 行 `seat.model.<役割>` の
/// 表示名（[`role_model`] を登録 row ごとに呼ぶ）。どれかの行が無い・不発効・表に無い周は `None`（空に読み替えない）。席の
/// row が無い群は空の集合。
pub fn role_models(manifest: &Manifest, group: &AccountGroup, state: &State) -> Option<BTreeSet<&'static str>> {
    state
        .registrations
        .values()
        .filter(|latest| group.anchors().contains(&latest.registration.anchor))
        .map(|latest| role_model(manifest, latest.registration.role))
        .collect()
}

/// 役割 1 つの model の表示名（**読みの 1 本**・設計 §33 形 2）: rules 行 `seat.model.<役割>` の値を閉じた表で引く。行が無い・
/// 不発効・文字列でない・表に無い周は `None`。
pub fn role_model(manifest: &Manifest, role: Role) -> Option<&'static str> {
    let row = manifest.get(&role.model_row()).filter(|row| row.enabled)?;
    let RuleValue::Str(text) = &row.value else {
        return None;
    };
    Model::parse(text).map(Model::display)
}

/// 残量の鍵で候補を並べる（**pure**・設計 §29 形 1）: `rows` は宣言の候補の順の (label, 鮮度の内側の実測の行)・`models` は役割の
/// model の表示名の集合。モデル別窓の行を 1 つでも欠く口座は最後 → 残量 = min(100 − 7 日窓の使用率, model ごとの 100 − モデル別
/// 7 日窓の使用率) の大きい順（7 日窓の行が無ければ 0）→ 7 日窓の `resets_at` の早い順（無い口座は後）→ 宣言の順。5 時間窓は
/// 読まない（門にだけ使う）。
pub fn by_key<'a>(rows: &'a [(String, Vec<Allowance>)], models: &BTreeSet<&str>) -> Vec<&'a str> {
    let mut keyed: Vec<_> = rows.iter().enumerate().map(|(at, (label, found))| (key_of(found, models), at, label.as_str())).collect();
    keyed.sort();
    keyed.into_iter().map(|(_, _, label)| label).collect()
}

/// 口座 1 つの鍵（小さい方が先）。
fn key_of(rows: &[Allowance], models: &BTreeSet<&str>) -> (bool, Reverse<u64>, bool, Option<String>) {
    let measured: Vec<&Measured> = rows
        .iter()
        .filter_map(|row| match row {
            Allowance::Measured(found) => Some(found),
            Allowance::Unmeasured(_) => None,
        })
        .collect();
    let left = |found: &&Measured| 100_u64.saturating_sub(found.used_pct);
    let seven: Vec<&Measured> = measured.iter().copied().filter(|found| found.window == WindowKind::SevenDay).collect();
    let model = |name: &str| {
        measured
            .iter()
            .filter(|found| found.window == WindowKind::SevenDayModel)
            .filter(|found| found.model.as_deref().and_then(Model::parse).map(Model::display) == Some(name))
            .map(left)
            .min()
    };
    let missing = models.iter().any(|name| model(name).is_none());
    let remaining = models.iter().filter_map(|name| model(name)).fold(seven.iter().map(left).min().unwrap_or(0), u64::min);
    let reset = seven.iter().filter_map(|found| found.resets_at.clone()).min();
    (missing, Reverse(remaining), reset.is_none(), reset)
}

/// 群の予約（**1 関数**・設計 §29 形 2 / 3・§31 形 1）: 群を宣言順に `input.group` まで見て、判じる群と**今の口座が逼迫の先の群**
/// （[`pressed_now`]）ごとに門を通る候補を残量の鍵（[`by_key`]）で並べ、先の群の予約でない先頭をその群の予約とし、`input.group` の
/// 予約を返す（無ければ `None`）。逼迫でない先の群は候補を測らず予約を持たない。門は今の口座でなく・どの群の今の口座でもなく（先の
/// 群は周の頭の `head`・判じる群は `taken`）・退役中でなく・鮮度の内側の実測を持ち 3 窓とも閾値未満（鮮度の外の候補は `measure` で
/// 口座ごとに 1 周 1 回測る）。予約は記録しない（周ごとに導き直す）。役割の model の行が無い周は測らずに [`Unreserved::NoRule`]。
pub fn reserve(input: &Judge<'_>, measured: &mut BTreeSet<String>) -> Result<Option<String>, Unreserved> {
    let read = || store::read_all(input.state_dir).map(|events| replay(&events)).map_err(|_| Unreserved::Unreadable);
    let state = read()?;
    let groups = input.manifest.groups();
    let upto = groups.iter().position(|found| found.name() == input.group.name()).map_or(0, |at| at.saturating_add(1));
    let models = groups.iter().take(upto).map(|found| role_models(input.manifest, found, &state).ok_or(Unreserved::NoRule));
    let models = models.collect::<Result<Vec<_>, _>>()?;
    let mut reserved: BTreeSet<String> = BTreeSet::new();
    for (found, models) in groups.iter().zip(&models) {
        let target = found.name() == input.group.name();
        if !target && !pressed_now(input, found, models, measured)? {
            continue;
        }
        let taken = if target { input.taken } else { input.head };
        let open: Vec<&String> =
            found.accounts().iter().filter(|label| !taken.contains(*label) && !state.retired.contains_key(*label)).collect();
        for label in &open {
            if measured.insert((*label).clone()) {
                (input.measure)(label, false);
            }
        }
        let state = read()?;
        let mut rows = Vec::new();
        for label in open {
            let fresh = usage::fresh_rows(input.manifest, &state, label).map_err(|_| Unreserved::NoRule)?;
            rows.extend(fresh.filter(|found| pressed(found, input.caps, models).is_none()).map(|found| (label.clone(), found)));
        }
        let pick = by_key(&rows, models).into_iter().find(|label| !reserved.contains(*label)).map(str::to_owned);
        if target {
            return Ok(pick);
        }
        reserved.extend(pick);
    }
    Ok(None)
}

/// 先の群の今の口座が逼迫か（設計 §31 形 1）: 記録を読めない群・鮮度の内側の実測を持たない群は偽。鮮度の外は**今の口座だけ**を
/// `measure` で口座ごとに 1 周 1 回測る。`models` はその群の役割の model の表示名の集合（呼び手が読む・設計 §33 形 3）。
fn pressed_now(
    input: &Judge<'_>,
    found: &AccountGroup,
    models: &BTreeSet<&str>,
    measured: &mut BTreeSet<String>,
) -> Result<bool, Unreserved> {
    let Ok(current) = current_of(input.state_dir, found) else {
        return Ok(false);
    };
    if measured.insert(current.label.clone()) {
        (input.measure)(&current.label, false);
    }
    let state = store::read_all(input.state_dir).map(|events| replay(&events)).map_err(|_| Unreserved::Unreadable)?;
    let fresh = usage::fresh_rows(input.manifest, &state, &current.label).map_err(|_| Unreserved::NoRule)?;
    Ok(fresh.is_some_and(|rows| pressed(&rows, input.caps, models).is_some()))
}

/// 移動の記録と承認（この順）: 前提（宣言の逐語・起こし直しの刻み・役割の既定の面）が揃わない周と記録を書けない周は 1 つも
/// 書かずに `false`。
fn approve(input: &Judge<'_>, current: &str, target: &str) -> bool {
    let (Some(words), Some(_), Ok(_)) = (
        declaration(input.state_dir, input.group),
        crate::seat::cycle::pace_of(input.manifest),
        crate::seat::embedded_manifest(),
    ) else {
        return false;
    };
    let record = Record { account: target.to_owned(), ts: now_utc(), previous: current.to_owned() };
    let dir = host_groups_dir(input.state_dir);
    if write_current(&dir, input.group.name(), &record).is_err() {
        return false;
    }
    clear_refused(&dir, input.group.name());
    append(input.state_dir, input.manifest, EventKind::GroupMoved, target, words);
    true
}

/// 移り先の無い群の断りの event（前の断りより**後に**今の口座の新しい実測が無い周は記さない＝同じ実測に 2 度断らない）。
fn refuse(input: &Judge<'_>, current: &str) -> Judgement {
    let Ok(events) = store::read_all(input.state_dir) else {
        return Judgement::Unreadable;
    };
    let detail = format!("group={} reason={NO_CANDIDATE}", input.group.name());
    let refused = |event: &Event| event.kind == EventKind::GroupMoveRefused && event.detail.as_deref() == Some(detail.as_str());
    if !measured_since(&events, current, None, refused) {
        return Judgement::NoCandidate(Refusal::Repeated);
    }
    append(input.state_dir, input.manifest, EventKind::GroupMoveRefused, current, detail);
    let _ = write_refused(&host_groups_dir(input.state_dir), input.group.name());
    Judgement::NoCandidate(Refusal::Recorded)
}

/// 群の宣言の行の逐語（承認の逐語・A1）: host の面の path と行番号 + `[[account-group]]` の行から次の表の手前までの行（末尾の
/// 空行は落とす）。読めない・行番号の行が群の表の頭でない周は `None`（逐語の無い承認で移さない）。
fn declaration(state_dir: &Path, group: &AccountGroup) -> Option<String> {
    let path = crate::rules::host_manifest_path(state_dir);
    let text = fs::read_to_string(&path).ok()?;
    let start = usize::try_from(group.line()).ok()?.checked_sub(1)?;
    let mut block: Vec<&str> = text
        .lines()
        .skip(start)
        .enumerate()
        .take_while(|(at, line)| *at == 0 || !line.trim_start().starts_with('['))
        .map(|(_, line)| line)
        .collect();
    while block.last().is_some_and(|line| line.trim().is_empty()) {
        block.pop();
    }
    (block.first().map(|head| head.trim()) == Some("[[account-group]]"))
        .then(|| format!("{}:{}\n{}", path.display(), group.line(), block.join("\n")))
}

/// `mark` に当たる最後の行より**後に**、口座 `label` の実測の行（`window` が在ればその窓だけ）が在るか（印が無ければ `true`・
/// log の位置で判じ、値の比較で判じない・account-lifecycle.md §19 形 4）。
pub fn measured_since(events: &[Event], label: &str, window: Option<WindowKind>, mark: impl Fn(&Event) -> bool) -> bool {
    let marked = events.iter().rposition(mark);
    let measured = events.iter().rposition(|event| {
        event.kind == EventKind::AllowanceMeasured
            && matches!(&event.allowance, Some(Allowance::Measured(row))
                if row.account == label && window.is_none_or(|found| row.window == found))
    });
    match marked {
        None => true,
        Some(at) => measured.is_some_and(|last| last > at),
    }
}

/// 群の event を 1 件記す（run / bead を持たない・actor は machine・`account` と `detail` の 2 つが本体）。書けない周も止めない。
pub fn append(state_dir: &Path, manifest: &Manifest, kind: EventKind, account: &str, detail: String) {
    let Ok(policy) = LockPolicy::from_rules(manifest) else {
        return;
    };
    let event = Event {
        schema: SCHEMA,
        ts: now_utc(),
        kind,
        run: String::new(),
        bead: String::new(),
        host: host(),
        actor: kind.default_actor().to_owned(),
        stage: None,
        seat: None,
        pid: None,
        detail: Some(detail),
        allowance: None,
        registration: None,
        mark: None,
        account: Some(account.to_owned()),
        cost: None,
        rule: None,
    };
    let _ = store::append(state_dir, &event, policy);
}

/// hook の 1 行（`group=<名> account=<label> window=<w> used=<n> cap=<n> — …`・設計 §19 形 5 / §21 形 2 (b)）。
fn seat_line(group: &AccountGroup, account: &str, found: Pressed) -> String {
    format!(
        "group={} account={account} window={} used={} cap={} — 次の 1 周が移り先を決める",
        group.name(),
        found.window.short(),
        found.used,
        found.cap
    )
}

/// 移動中の席の 1 行（記録の口座 ≠ 登録 row の口座・設計 §21 形 2 (a)）。
fn moving_line(group: &AccountGroup, row: &str, current: &str) -> String {
    format!("group={} row={row} current={current} — 器が移動中: 作業記憶を台帳と git に残して待つ（/exit は器が送る）", group.name())
}

/// 席の hook の群の段（SessionStart の brief・UserPromptSubmit の追加文脈に足す行）。`who` / `when` は記録の欄。
///
/// 群に属さない anchor・群 0 の host・登録の無い席・読めない面は 0 行（席は止めない・rc は変えない）。鮮度の内側で逼迫なら
/// 1 行、閾値未満なら 0 行、鮮度の外なら計測の子を 1 本起こして `usage: measuring account=<label>` の 1 行。出した行は
/// 記録 1 行（`what` = [`WHAT_GROUP`]）を残す。
pub(super) fn lines(hooked: &Hooked, (who, when): (&str, &str), started: Instant) -> (Vec<String>, Vec<String>) {
    let Some(line) = line_of(hooked) else {
        return (Vec::new(), Vec::new());
    };
    let emit = Emit { who, what: WHAT_GROUP, when, line: &line };
    (vec![line.clone()], record_lines(hooked.dir, &record(&emit, hooked, started)))
}

/// 群の段の 1 行（出さない周は `None`）。
fn line_of(hooked: &Hooked) -> Option<String> {
    let tracked = hooked.rules.map_or_else(Manifest::embedded, |path| Manifest::load(Path::new(path))).ok()?;
    let manifest = crate::rules::with_state_dir(tracked, Some(hooked.dir)).ok()?;
    let group = group_of(&manifest, &hooked.root.display().to_string())?;
    let caps = Caps::of(&manifest).ok()?;
    let pane = hooked.pane.filter(|found| !found.trim().is_empty())?;
    let socket = hooked.socket.filter(|found| !found.trim().is_empty());
    let target = crate::seat::target_of_pane(socket, pane)?;
    let events = crate::fleet::store::read_all(hooked.dir).ok()?;
    let state = crate::fleet::replay(&events);
    let account = crate::seat::role::registration_of_target(&state, &target)?.account.clone();
    // 群の今の口座（記録 > 種）を読む: 記録が読めない周は 0 行・記録が登録 row と食い違う周は逼迫を測らず移動中の 1 行
    // （移動を頼む記録は置かない＝移動は既に決まっている・設計 §21 形 2）。
    let current = current_of(hooked.dir, group).ok()?;
    if current.source == Source::Record && current.label != account {
        return Some(moving_line(group, &account, &current.label));
    }
    // 群の役割の model の集合（行が無い周は 0 行・設計 §33 形 3）。
    let models = role_models(&manifest, group, &state)?;
    match usage::fresh_rows(&manifest, &state, &account).ok()? {
        Some(rows) => {
            let found = pressed(&rows, caps, &models)?;
            // 逼迫を読んだ周は移動を頼む記録を置く（§20 形 4・在れば上書きしない・判定と移動は 1 周の群の段が lock の内側で行う）。
            let _ = put_request(&host_groups_dir(hooked.dir), group.name(), &account, found.window);
            Some(seat_line(group, &account, found))
        }
        None => measure_later(hooked.dir, &account).then(|| format!("usage: measuring account={account}")),
    }
}

/// 鮮度の外の口座を**子として**測る（`fleet usage --state-dir D --account <label> --fresh` の 1 形・待たない）。子は新しい
/// process group の leader にし（hook の終わりで道連れにしない）、入出力は捨てる。起こせた周だけ `true`。
fn measure_later(state_dir: &Path, account: &str) -> bool {
    Invocation::new(crate::pipe::dispatch::myself())
        .args(["fleet", "usage", "--state-dir"])
        .arg(state_dir)
        .args(["--account", account, "--fresh"])
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::{by_key, grace_left, measure_later, pressed, refused_mark, refused_ts, Caps, Pressed, Signal};
    use crate::fleet::{Allowance, Measured, WindowKind};
    use std::collections::BTreeSet;

    /// 断りの印（account-lifecycle.md §31 形 2）: render した 1 行は parse で同じ ts に戻る。
    #[test]
    fn group_refused_render_parses_back_to_the_ts() {
        let ts = "2026-09-26T14:07:00Z";
        assert_eq!(refused_mark(ts), format!("ts={ts} reason=no-candidate\n"), "1 行の形");
        assert_eq!(refused_ts(&refused_mark(ts)), Some(ts), "同じ ts に戻る");
    }

    /// 形でない印（改行の欠け・理由の違い・余りの field・ts が時刻の形でない・空）は `None`（doctor は unreadable と読む）。
    #[test]
    fn group_refused_parse_refuses_other_forms() {
        let t = "ts=2026-09-26T14:07:00Z";
        let refused = [format!("{t} reason=no-candidate"), format!("{t} reason=other\n"), format!("{t} reason=no-candidate x=1\n")];
        let refused = refused.into_iter().chain(["ts=soon reason=no-candidate\n".to_owned(), String::new()]);
        refused.for_each(|text| assert_eq!(refused_ts(&text), None, "印でない: {text:?}"));
    }

    /// 合図の記録の parse（seat-heartbeat.md §14 形 1）: 3 field は読める・旧形・`at` が整数でない・欠け / 余り / 順違いは記録なし。
    #[test]
    fn group_signal_parse_reads_three_fields_and_refuses_the_rest() {
        let want = Signal { to: "a2".to_owned(), ts: "2026-09-25T00:00:00Z".to_owned(), at: 1_790_000_000 };
        assert_eq!(Signal::parse("to=a2 ts=2026-09-25T00:00:00Z at=1790000000\n"), Some(want), "3 field が読める");
        assert_eq!(Signal::parse("to=a2 ts=seed at=7\n").map(|found| (found.ts, found.at)), Some(("seed".to_owned(), 7)), "種の鍵");
        let t = "ts=2026-09-25T00:00:00Z";
        // 前の版の形・at が整数でない / 負 / 欠ける・余りの field・順が違う・空の値。
        let refused = [format!("{t}\n"), format!("to=a2 {t} at=soon\n"), format!("to=a2 {t} at=-1\n"), format!("to=a2 {t}\n")];
        let refused = refused.into_iter().chain([format!("to=a2 {t} at=1 x=2\n"), format!("{t} to=a2 at=1\n"), format!("to= {t} at=1\n")]);
        refused.for_each(|text| assert_eq!(Signal::parse(&text), None, "記録なし: {text:?}"));
    }

    /// 残りの秒は合図の `at` + 猶予 − 今 が正ならその秒・越えた周（境界の 0 を含む）と猶予 0 は `None`。
    #[test]
    fn group_signal_grace_left_counts_from_the_signal_at() {
        let found = [(1100, 1800), (2799, 1800), (2800, 1800), (2801, 1800), (1000, 0)].map(|(now, grace)| grace_left(1000, now, grace));
        assert_eq!(found, [Some(1700), Some(1), None, None, None], "内側・越える 1 秒前・ちょうど・越えた・猶予 0");
    }

    /// 実測の行 1 つ。
    fn measured(window: WindowKind, used_pct: u64) -> Allowance {
        Allowance::Measured(Measured {
            account: "a1".to_owned(),
            window,
            model: (window == WindowKind::SevenDayModel).then(|| "Fable".to_owned()),
            endpoint: "oauth-usage".to_owned(),
            used_pct,
            resets_at: None,
        })
    }

    /// 越えた窓が 2 つ在れば使用率の大きい方、同率は窓の宣言順で先の 1 つ。越えない窓は使用率が大きくても選ばない。
    #[test]
    fn hook_group_pressed_picks_the_largest_window_over_its_own_cap() {
        let caps = Caps { five: 85, seven: 95, model: 95 };
        let fable = BTreeSet::from(["Fable"]);
        let rows =
            [measured(WindowKind::FiveHour, 90), measured(WindowKind::SevenDay, 94), measured(WindowKind::SevenDayModel, 97)];
        assert_eq!(pressed(&rows, caps, &fable), Some(Pressed { window: WindowKind::SevenDayModel, used: 97, cap: 95 }));
        let tie = [measured(WindowKind::FiveHour, 96), measured(WindowKind::SevenDay, 96)];
        assert_eq!(pressed(&tie, caps, &fable).map(|found| found.window), Some(WindowKind::FiveHour), "同率は先の窓");
        let under = [measured(WindowKind::FiveHour, 84), measured(WindowKind::SevenDay, 94)];
        assert_eq!(pressed(&under, caps, &fable), None, "どの窓も閾値未満");
    }

    /// モデル別窓の行 1 つ（model の字面を選ぶ）。
    fn model_row(model: &str, used_pct: u64) -> Allowance {
        Allowance::Measured(Measured {
            account: "a1".to_owned(),
            window: WindowKind::SevenDayModel,
            model: Some(model.to_owned()),
            endpoint: "oauth-usage".to_owned(),
            used_pct,
            resets_at: None,
        })
    }

    /// モデル別窓は集合に表示名が在る model の行だけが逼迫にする（設計 §33 形 1）: Fable の窓 99 は集合 {Opus} では数えず、
    /// Opus の窓 96 は数える。model の字面は表示名に畳んで比べる（小文字の `opus` も Opus）。
    #[test]
    fn hook_group_pressed_counts_only_the_model_windows_in_the_set() {
        let caps = Caps { five: 85, seven: 95, model: 95 };
        let opus = BTreeSet::from(["Opus"]);
        let low = [measured(WindowKind::FiveHour, 10), measured(WindowKind::SevenDay, 10)];
        let fable_only: Vec<Allowance> = low.iter().cloned().chain([model_row("Fable", 99)]).collect();
        assert_eq!(pressed(&fable_only, caps, &opus), None, "集合に無い model の窓は数えない");
        let both: Vec<Allowance> = fable_only.iter().cloned().chain([model_row("opus", 96)]).collect();
        let want = Some(Pressed { window: WindowKind::SevenDayModel, used: 96, cap: 95 });
        assert_eq!(pressed(&both, caps, &opus), want, "集合に在る model の窓だけ（Fable の 99 でなく Opus の 96）");
        let unknown: Vec<Allowance> = low.iter().cloned().chain([model_row("no-such-model", 99)]).collect();
        assert_eq!(pressed(&unknown, caps, &opus), None, "parse できない model の行は数えない");
    }

    /// 集合が空ならモデル別窓は数えず、5 時間窓と 7 日窓は集合に依らず数える（設計 §33 形 1）。
    #[test]
    fn hook_group_pressed_empty_set_skips_the_model_window_but_not_five_and_seven() {
        let caps = Caps { five: 85, seven: 95, model: 95 };
        let (empty, opus) = (BTreeSet::new(), BTreeSet::from(["Opus"]));
        let model = [measured(WindowKind::FiveHour, 10), measured(WindowKind::SevenDay, 10), model_row("Fable", 99)];
        assert_eq!(pressed(&model, caps, &empty), None, "集合が空はモデル別窓を数えない");
        let five = [measured(WindowKind::FiveHour, 90), measured(WindowKind::SevenDay, 10), model_row("Fable", 99)];
        let seven = [measured(WindowKind::FiveHour, 10), measured(WindowKind::SevenDay, 96), model_row("Fable", 99)];
        for set in [&empty, &opus] {
            assert_eq!(pressed(&five, caps, set).map(|found| (found.window, found.used)), Some((WindowKind::FiveHour, 90)), "5h");
            assert_eq!(pressed(&seven, caps, set).map(|found| (found.window, found.used)), Some((WindowKind::SevenDay, 96)), "7d");
        }
    }

    /// 鍵の歯の口座 1 つの行（5 時間窓・7 日窓〔reset つき〕・model ごとのモデル別窓）。
    fn account(label: &str, (five, seven, reset): (u64, u64, Option<&str>), models: &[(&str, u64)]) -> (String, Vec<Allowance>) {
        let row = |window, model: Option<&str>, used_pct, resets_at: Option<&str>| {
            Allowance::Measured(Measured {
                account: label.to_owned(),
                window,
                model: model.map(str::to_owned),
                endpoint: "oauth-usage".to_owned(),
                used_pct,
                resets_at: resets_at.map(str::to_owned),
            })
        };
        let mut rows = vec![row(WindowKind::FiveHour, None, five, None), row(WindowKind::SevenDay, None, seven, reset)];
        rows.extend(models.iter().map(|(name, used)| row(WindowKind::SevenDayModel, Some(name), *used, None)));
        (label.to_owned(), rows)
    }

    /// 並びの label（役割の model の集合は表示名の列）。
    fn order(rows: &[(String, Vec<Allowance>)], models: &[&'static str]) -> Vec<String> {
        by_key(rows, &models.iter().copied().collect::<BTreeSet<&str>>()).into_iter().map(str::to_owned).collect()
    }

    /// 役割の集合が空の群は 7 日窓の残量だけで並べ（モデル別窓・5 時間窓は読まない）、同点は 7 日窓の reset の早い方・reset の
    /// 無い口座は後・reset も同じなら宣言の順。
    #[test]
    fn group_key_empty_role_set_orders_by_the_seven_day_window_then_reset_then_declaration() {
        let (early, late) = (Some("2099-01-01T00:00:00Z"), Some("2099-02-01T00:00:00Z"));
        let rows = [
            account("low", (0, 80, early), &[("Fable", 0)]),
            account("high", (99, 10, early), &[("Fable", 99)]),
            account("late", (0, 50, late), &[]),
            account("none", (0, 50, None), &[]),
            account("early", (0, 50, early), &[]),
            account("twin", (0, 50, early), &[]),
        ];
        assert_eq!(order(&rows, &[]), ["high", "early", "twin", "late", "none", "low"]);
    }

    /// 役割の model 1 つ: 残量は 7 日窓とその model の窓の小さい方。別の model の窓は読まない。
    #[test]
    fn group_key_one_role_model_takes_the_smaller_remainder() {
        let reset = Some("2099-01-01T00:00:00Z");
        let rows = [
            account("seven", (0, 10, reset), &[("Fable", 70), ("Opus", 0)]),
            account("model", (0, 40, reset), &[("Fable", 20), ("Opus", 99)]),
        ];
        assert_eq!(order(&rows, &["Fable"]), ["model", "seven"], "min(90, 30) = 30 < min(60, 80) = 60");
        assert_eq!(order(&rows, &[]), ["seven", "model"], "集合が空なら 7 日窓だけ");
    }

    /// 役割の model 2 つ: 残量は 7 日窓と 2 つの model の窓の最小。
    #[test]
    fn group_key_two_role_models_take_the_smallest_remainder() {
        let reset = Some("2099-01-01T00:00:00Z");
        let rows = [
            account("opus-full", (0, 10, reset), &[("Fable", 10), ("Opus", 90)]),
            account("even", (0, 30, reset), &[("Fable", 30), ("Opus", 30)]),
        ];
        assert_eq!(order(&rows, &["Fable", "Opus"]), ["even", "opus-full"], "min(90, 90, 10) = 10 < 70");
        assert_eq!(order(&rows, &["Fable"]), ["opus-full", "even"], "Opus を集合に入れなければ opus-full が先");
    }

    /// 役割の model のモデル別窓の行を 1 つでも欠く口座は残量に依らず最後（欠く口座どうしは残量の順）。
    #[test]
    fn group_key_account_missing_a_role_model_row_goes_last() {
        let reset = Some("2099-01-01T00:00:00Z");
        let rows = [
            account("bare", (0, 0, reset), &[]),
            account("half", (0, 5, reset), &[("Fable", 0)]),
            account("worn", (0, 90, reset), &[("Fable", 90), ("Opus", 90)]),
        ];
        assert_eq!(order(&rows, &["Fable", "Opus"]), ["worn", "bare", "half"]);
        assert_eq!(order(&rows, &["Fable"]), ["half", "worn", "bare"]);
    }

    /// 鮮度の外の口座を測る子は起動の記述を通る（設計 core-boundary.md §9 行 h）: program は自分・引数は `fleet usage
    /// --state-dir <D> --account <label> --fresh`。記録する stub は spawn を断るので、起動は偽（起こせた周だけ真）。
    #[test]
    fn invocation_hook_group_self_launch_failure_is_false() {
        use crate::pipe::fixture::{exited, Stub};
        let state = std::path::Path::new("/nonexistent-invocation-hook-group");
        let stub = Stub::install(|_| exited(0, b""));
        assert!(!measure_later(state, "a1"), "stub の断りは起動の失敗＝偽");
        let found: Vec<(String, Vec<String>)> = stub.calls().into_iter().map(|call| (call.program, call.args)).collect();
        let args = ["fleet", "usage", "--state-dir", "/nonexistent-invocation-hook-group", "--account", "a1", "--fresh"];
        let expected = vec![(crate::pipe::dispatch::myself(), args.iter().map(|arg| (*arg).to_owned()).collect())];
        assert_eq!(found, expected, "program は自分・引数は fleet usage の 1 形");
    }
}
