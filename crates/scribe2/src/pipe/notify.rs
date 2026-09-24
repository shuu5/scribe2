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
use super::dispatch::{Turn, WaitReason};
use crate::fleet::State;
use crate::name::NAME;
use crate::rules::manifest::Manifest;
use crate::seat::inject::Request;
use crate::seat::inject::{deliver_or_confirm, deliver_within, Confirm, Delivery, Sent};
use crate::seat::role::{registration_of_key, Role};
use crate::seat::StateDir;
use std::path::Path;
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
pub(super) struct Terminal<'a> {
    /// 便の bead id。
    pub(super) bead: &'a str,
    /// 便 id。
    pub(super) run: &'a str,
    /// 最後の段の `as_str`。
    pub(super) stage: &'a str,
    /// verdict か kind か detail の 1 語（[`head_word`] を通した字面）。
    pub(super) word: &'a str,
}

/// 終端の 1 行（§19 形 3 (a)・次の 1 手が末尾に在る 1 行・逐語も path も載せない）。
pub(super) fn terminal_line(terminal: &Terminal<'_>) -> String {
    format!(
        "{NAME} pipe: {} {} {}={} — 次の 1 手は pipe dispatch ls",
        terminal.bead, terminal.run, terminal.stage, terminal.word
    )
}

/// idle の 1 行（§19 形 3 (b)）。列の結果が「起こした便 0 ∧ 候補 1 本以上」の周だけ `Some`。
///
/// `Turn` から**読むだけ**で組む: 候補の本数 = `candidates` の長さ・先頭の候補の理由 = その `reason` の `render`。
pub(super) fn idle_line(turn: &Turn) -> Option<String> {
    if !turn.launches.is_empty() {
        return None;
    }
    let top = turn.candidates.first()?;
    let reason = top.reason.as_ref().map_or_else(|| DASH.to_owned(), WaitReason::render);
    Some(format!("{NAME} pipe: idle ready={} launched=0 reason={reason}", turn.candidates.len()))
}

/// detail の頭の 1 語（空白と `:` の手前・無ければ [`DASH`]）。値の後ろ（sha・path）は pane に載せない。
pub(super) fn head_word(detail: Option<&str>) -> &str {
    detail
        .and_then(|found| found.split(|ch: char| ch.is_whitespace() || ch == ':').find(|word| !word.is_empty()))
        .unwrap_or(DASH)
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
