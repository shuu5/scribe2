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

pub use launch::{
    derive_launch, fill_launch, launch, render_launched, single_model, with_agent_view_off, with_defaults, with_flags, Holes,
    Launch, Launched, HOLES,
};

use super::{state, tmux_ok};
use crate::rules::manifest::Manifest;
use crate::rules::RuleValue;
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

/// 作り直しと復元の確認上限（秒）を宣言する rules 行の id（`s2-07l.151`・裁定 id は manifest 行）。
const ID_SETTLE_S: &str = "seat.cycle_settle_s";
/// 確認を見に行く周期（ミリ秒）を宣言する rules 行の id。
const ID_POLL_MS: &str = "seat.cycle_poll_ms";
/// 復元の既定 command。
pub const DEFAULT_RESTORE: &str = "/rebrief";
/// 記録の who。
pub(super) const WHO: &str = "seat-cycle";
/// 席の起動（`seat launch`）の記録の when。
pub(super) const WHEN_LAUNCH: &str = "launch";
/// 同じ記録の `kind=` の字面。**種類は 1 つだけである**（ADR-0045 §2 (2) で合図の種類〔打刻・退避・
/// 立て直し・終了・復元〕は送り手ごと消えた）＝分岐の無い値なので enum を持たない。
pub(super) const KIND_LAUNCH: &str = "launch";
/// 席の起動の `inject.jsonl` の記録の who。
pub const WHO_LAUNCH: &str = "seat-launch";
/// 口座の credential dir の置き場（`<state_dir>/accounts/<label>/`・ADR-0017 §2.3）。
pub(super) const ACCOUNTS_DIR: &str = "accounts";
/// 起動の雛形の穴（設計 account-autonomy.md §5・seat-roles.md §2）: 選んだ口座の credential dir で埋める 1 つ。
pub const HOLE: &str = "{account_dir}";
/// claude CLI の model の flag（起動行が row の `model` を運ぶ語・値は [`crate::fleet::select::Model::alias`]）。
pub(super) const MODEL_FLAG: &str = "--model";
/// claude CLI の effort の flag（起動行が役割の既定の effort を運ぶ語・値は [`crate::headless::Effort::alias`]・設計 seat-roles.md §20）。
pub(super) const EFFORT_FLAG: &str = "--effort";

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
/// 器自身が queue した文が入力欄に残り、Enter を 1 回送っても消えない（`s2-07l.288`・注入と同じ門・
/// 人間の打ちかけの [`REASON_INPUT_BUSY`] と弁別する＝断りの記録から「誰の文で止まったか」が読める）。
pub const REASON_INPUT_OWN_QUEUED: &str = "input-own-queued";
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
/// `seat launch` の既存 window の前面 process が shell でない（走っている席へ起動行を送らない・**row を書かない**）。
pub const REASON_NOT_SHELL: &str = "not-a-shell";
/// [`REASON_NOT_SHELL`] の断りに足す**次の 1 手**（設計 seat-roles.md §26 の約束 9）: その窓には生きた席が在るので、
/// 器は殺さない——人が選ぶ 2 つの手を断りの行そのものに載せる（断りが直し方を言わない形は `s2-07l.488` の実測）。
/// **値に空白を持たない**（判定行の `key=value` の読みを壊さない）。載せるのは `seat launch` の行だけで、
/// 同じ理由を返す口座の delivery の面は従来の字面のまま。
pub const NEXT_AFTER_NOT_SHELL: &str = "その窓の席を終わらせてから同じ窓で打つ／別の名の窓を--targetで名指す";
/// 呼び手の pane が target の pane そのものの周の `--restore`（設計 seat-roles.md §26 の約束 8）: 起動行で自分を
/// 置き換えるので、立ち上がった後に合図を送る process が残らない＝送れない約束をせず**登録 row を書く前に**断る。
pub const REASON_RESTORE_SAME_WINDOW: &str = "restore-in-the-same-window";
/// 同じ窓の周に自分の process を起動行へ置き換えられない（`sh -c <起動行>` の exec が返ってきた・row は書き終えている）。
pub const REASON_REPLACE: &str = "launch-replace-failed";
/// `seat launch` が event log を読めない（選定の除外＝他の席の登録 row を取れない・row も key も書かない）。
pub const REASON_LOG_UNREADABLE: &str = "log-unreadable";
/// `seat launch` の `--anchor` 無しで cwd の repo root を解けない（`seat register` の `input-unreadable` と同じ形）。
pub const REASON_ANCHOR: &str = "anchor-unresolvable";
/// `seat launch` の `--model` が表（[`crate::fleet::select::Model::parse`]・表示名か別名）に無い（row も key も書かない）。
pub const REASON_MODEL_UNKNOWN: &str = "launch-model-unknown";
/// 起動行に `--model` が 2 つ載る（雛形の literal と器の 1 つ・後勝ちにせず断る・立て直しは `relaunch-` を前置く）。
pub const REASON_MODEL_DUPLICATED: &str = "launch-model-duplicated";
/// 起動行に `--effort` が 2 つ載る（雛形の literal と器の 1 つ・`--model` の二重とは別の理由・後勝ちにせず断る・設計 seat-roles.md §20 の約束 2）。
pub const REASON_EFFORT_DUPLICATED: &str = "launch-effort-duplicated";
/// `seat launch` の `--model` が役割の既定の行（`seat.model.<役割名>`）と食い違う（`--model` は照合であって宣言ではない・
/// row も key も書かない・設計 seat-roles.md §20 の約束 3）。
pub const REASON_MODEL_MISMATCH: &str = "launch-model-mismatch";
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

/// `target` へ 1 行を literal で送り、Enter を送る（cycle の `/clear` と立て直しの起動の 1 本）。
pub(super) fn send_to(socket: Option<&str>, target: &str, text: &str) -> bool {
    tmux_ok(socket, &["send-keys", "-t", target, "-l", text]) && tmux_ok(socket, &["send-keys", "-t", target, "Enter"])
}
