//! 便の終端と「起こす便 0 ∧ 候補あり」を登録 row の席の pane へ 1 行で知らせる（設計 dispatcher.md §19・契約表の行 p）。
//!
//! 送るのは運転手（終端の周の `pipe` の process）で、送達は既存の 1 関数（[`deliver_within`]）を 1 回撃つだけ。
//! 結果は stdout の `notify=<delivered consumed=<true|false|unknown[:理由]>|refused:<理由>|unconfirmed|no-seat>` の
//! 1 行に残す（C10・消費の添えは §21）。送達の失敗で便の
//! rc は変えない（通知は副作用・便の終端は既に記帳済み）。event は足さない（pane の行と stdout の 1 行だけ）。
//!
//! **閉じた型の variant をここで名指さない**（§19 形 5）: 段の 1 語は呼び手が `as_str` の字面で渡し、どの段を送るかの
//! 判定も呼び手が持つ。ここが知るのは字面と宛先と送達だけである。

use super::cli::int_row;
use super::dispatch::facts::{self, Facts};
use super::dispatch::{Turn, WaitReason};
use crate::fleet::State;
use crate::name::NAME;
use crate::rules::manifest::Manifest;
use crate::seat::inject::Request;
use crate::seat::inject::{deliver_or_confirm, deliver_within, Confirm, Delivery, Sent};
use crate::seat::role::{registration_of_key, Role};
use crate::seat::StateDir;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// 送達の窓を持つ rules 行（§19 形 4「`pipe.stop_grace_ms` と同じ桁の窓」・値はこの file に焼かない・C1）。
const ROW_WINDOW: &str = "pipe.stop_grace_ms";

/// stdout の行の頭。
const NOTIFY: &str = "notify=";

/// 登録 row が無い周の理由（送らない）。
const NO_SEAT: &str = "no-seat";

/// 窓の rules 行を読めない周の理由（1 key も送らない）。
const NO_RULE: &str = "no-rule";

/// 既定の行へ Enter を 1 回送った周の結果の語（[`send_or_confirm`]）。
const CONFIRMED: &str = "confirmed";

/// 値を持たない欄の字面（detail も理由も無い）。
const DASH: &str = "-";

/// 終端の 1 行の材料（字面だけ・段の判定は呼び手が済ませている）。
pub(crate) struct Terminal<'a> {
    /// 便の bead id。
    pub(crate) bead: &'a str,
    /// 便 id。
    pub(crate) run: &'a str,
    /// 最後の段の `as_str`。
    pub(crate) stage: &'a str,
    /// 局面の出力の便の部品の理由の語（古い周の `:stale` は呼び手が添えた字面）。
    pub(crate) word: &'a str,
}

/// 終端の 1 行（§19 形 3 (a)・次の 1 手が末尾に在る 1 行・逐語も path も載せない）。
pub(crate) fn terminal_line(terminal: &Terminal<'_>) -> String {
    format!(
        "{NAME} pipe: {} {} {}={} — 次の 1 手は pipe dispatch ls",
        terminal.bead, terminal.run, terminal.stage, terminal.word
    )
}

/// idle の行に載せる memo の要約（字面だけ・数えと語は呼び手が済ませている・設計 §43 行 au）。
#[derive(Debug, Default)]
pub(crate) struct Memos {
    /// ` reason=` の直後に足す字面（先頭の空白を含む・key を出さない周は空）。
    pub(crate) line: String,
    /// memo-actionable が 1 以上か（候補 0 の周に idle の行を送る引き金）。
    pub(crate) actionable: bool,
}

/// idle の 1 行（§19 形 3 (b)）。列の結果が「起こした便 0 ∧ 候補 1 本以上」の周と、「起こした便 0 ∧ 候補 0 ∧ 列を測れた ∧ memo-actionable」の周だけ `Some`。
///
/// `Turn` から**読むだけ**で組む: 候補の本数 = `candidates` の長さ・先頭の候補の理由 = その `reason` の `render`
/// （候補 0 の周は `ready=0 reason=-`）。既存の key と順は変えず、` reason=` の直後に memo の要約（`memos`・設計 §43 行 au）、
/// 末尾に同じ周の並列の実測の字面（[`facts::line`]・設計 §26 形 4）を足す。
///
/// その後ろに未処置の終端（`pending`・呼び手が候補の順に判じた字面・`run` は載せない）を
/// ` pending=<k>:<bead>/<段>=<語>,…` で足す。0 本の周は key を出さない（設計 §29 形 2）。`None` は局面の出力を読めない周で、
/// 呼び手が候補を 1 本以上数えた周だけ渡し ` pending=unreadable` を足す（設計 §43 行 ar）。
pub(crate) fn idle_line(turn: &Turn, facts: &Facts, pending: Option<&[Terminal<'_>]>, memos: &Memos) -> Option<String> {
    if !turn.launches.is_empty() {
        return None;
    }
    let reason = match turn.candidates.first() {
        Some(top) => top.reason.as_ref().map_or_else(|| DASH.to_owned(), WaitReason::render),
        None if turn.unmeasured.is_none() && memos.actionable => DASH.to_owned(),
        None => return None,
    };
    let tail = facts::line(facts);
    let pending = match pending {
        None => " pending=unreadable".to_owned(),
        Some([]) => String::new(),
        Some(found) => {
            let listed: Vec<String> = found.iter().map(|each| format!("{}/{}={}", each.bead, each.stage, each.word)).collect();
            format!(" pending={}:{}", listed.len(), listed.join(","))
        }
    };
    Some(format!("{NAME} pipe: idle ready={} launched=0 reason={reason}{}{tail}{pending}", turn.candidates.len(), memos.line))
}

/// 直しの束の 1 行（設計 dispatcher.md §27 形 2・行 y）: `precheck bundles=<n> rows=<m>` の後ろに束ごとの ` <束の id>=<束の file の
/// path>`。載せるのは束の一覧と在り処だけで、作法の散文は載せない（N2）。束が 0 本の周は `bundles=0 rows=0` で終わる。
pub(crate) fn precheck_line((bundles, rows): &(Vec<(String, PathBuf)>, usize)) -> String {
    let listed: String = bundles.iter().map(|(id, path)| format!(" {id}={}", path.display())).collect();
    format!("{NAME} pipe: precheck bundles={} rows={rows}{listed}", bundles.len())
}

/// `payload` を `repo` を anchor に持つ orchestrator の登録 row の席へ 1 回送り、結果の 1 行を返す。
///
/// row が無い周は送らず `notify=no-seat`。窓の rules 行を読めない周も送らず `notify=refused:no-rule`
/// （既定の窓を焼かない・C1）。
///
/// `place` は運転手の置き場（`--state-dir`・[`Provenance::Flag`](crate::seat::Provenance::Flag)）で、送達の記録
/// （`tick.jsonl`）と消費の証拠（席の打刻）はここを読む（設計 dispatcher.md §21 形 1）。届いた周は消費を
/// `consumed=<true|false|unknown[:理由]>` で添える（`Settled` の既存の字面・tick の `consumed=` と同じ語彙・C10）。
pub(super) fn send(state: &State, place: &StateDir, repo: &Path, manifest: &Manifest, payload: &str) -> String {
    match route(state, repo, manifest) {
        Ok((target, window)) => {
            let request = Request { target: &target, socket: None, payload, state_dir: Some(place) };
            line_of(deliver_within(&request, window))
        }
        Err(line) => line,
    }
}

/// [`send`] と同じ宛先・窓で `payload` を撃ち、送った周は届いても未確認でも `confirm.who` の名で inject の記録に 1 行残し、門が
/// `Foreign` で直に断り入力欄の残りが `confirm` の既定の行に等しい周は payload を送らず Enter を 1 回だけ送る
/// （[`deliver_or_confirm`]・設計 account-lifecycle.md §22 形 1 / 2）。Enter を送った周の行は `notify=confirmed`（送れない周は
/// `notify=unconfirmed`）。
pub(super) fn send_or_confirm(
    state: &State,
    place: &StateDir,
    repo: &Path,
    manifest: &Manifest,
    (payload, confirm): (&str, &Confirm<'_>),
) -> String {
    match route(state, repo, manifest) {
        Ok((target, window)) => {
            let request = Request { target: &target, socket: None, payload, state_dir: Some(place) };
            match deliver_or_confirm(&request, window, confirm) {
                Sent::Payload(delivery) => line_of(delivery),
                Sent::Confirmed(true) => format!("{NOTIFY}{CONFIRMED}"),
                Sent::Confirmed(false) => format!("{NOTIFY}unconfirmed"),
            }
        }
        Err(line) => line,
    }
}

/// 宛先（`repo` を anchor に持つ orchestrator の登録 row の target）と窓。解けない周は送らずに返す行（`Err`）。
fn route(state: &State, repo: &Path, manifest: &Manifest) -> Result<(String, Duration), String> {
    let anchor = repo.to_string_lossy();
    let Some(row) = registration_of_key(state, Role::Orchestrator, &anchor) else {
        return Err(format!("{NOTIFY}{NO_SEAT}"));
    };
    let Ok(ms) = int_row(manifest, ROW_WINDOW) else {
        return Err(format!("{NOTIFY}refused:{NO_RULE}"));
    };
    Ok((row.target.clone(), Duration::from_millis(ms)))
}

/// 送達の結果の 1 行。
fn line_of(delivery: Delivery) -> String {
    match delivery {
        Delivery::Delivered(_, settled) => match settled.reason() {
            Some(why) => format!("{NOTIFY}delivered consumed={}:{why}", settled.as_str()),
            None => format!("{NOTIFY}delivered consumed={}", settled.as_str()),
        },
        Delivery::Refused(reason) => format!("{NOTIFY}refused:{reason}"),
        Delivery::Unconfirmed(_) => format!("{NOTIFY}unconfirmed"),
    }
}
