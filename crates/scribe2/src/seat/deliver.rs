//! 待ちの席へ裁定の指し示しを 1 行送る口 `seat deliver`（設計 seat-heartbeat.md §15・ADR-0077・SRS FR79 / AC49 / FR44）。
//!
//! 受けるのは裁定面の記帳 id 1 語だけで、送る 1 行は器が持つ雛形（[`line`]）の展開である＝送る行に入る外からの値は記帳 id
//! だけ（好きな文を送る注入の口 `seat inject` は戻さない）。登録 row・状態の打刻の最終行が Idle・入力欄が空の席にだけ
//! [`deliver_within`] を 1 回撃ち、条件の外は 1 key も送らずに [`Refusal`] の語で断る。管理 tick の判定の列・梯子・heartbeat の
//! 停止の記録は読まず、event log には書かない（送った周の記録は [`deliver_within`] の既存の経路が書く）。

use super::inject::{deliver_within, guard_input, Delivery, InputPass, Request};
use super::state::{self, SeatState, Stamp};
use super::{capture, int_rule, seat_dir, state_dir_of, InputGate};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_REFUSED};
use crate::name::NAME;
use std::fs;
use std::path::Path;
use std::time::Duration;

/// 記帳 id の上限（byte・器の定数・設計 §15 形 2）。
pub const ID_MAX: usize = 64;

/// 送達の窓の rules 行（ミリ秒・管理 tick と便の終端の周の通知と同じ行・埋め込みの manifest から読む）。
const ROW_WINDOW: &str = "pipe.stop_grace_ms";

/// 1 key も送らずに断る理由（**閉じた 13 語**・設計 §15 形 7・管理 tick の `NoopReason` を名指さない）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// 記帳 id が空。
    IdEmpty,
    /// 記帳 id に ASCII の英数字と `.` `-` `_` `:` の外の字が在る。
    IdShape,
    /// 記帳 id が [`ID_MAX`] byte を越える。
    IdLong,
    /// 置き場が解けない。
    StateDir,
    /// event log を読めない（rc 2）。
    Store,
    /// target の登録 row が無い。
    NoRow,
    /// 状態の打刻の最終行が Busy。
    Busy,
    /// 状態の打刻の file が無い。
    StateMissing,
    /// 状態の打刻を読めない（dir・読める行が 0）。
    StateUnreadable,
    /// 送達の窓の行を読めない。
    NoRule,
    /// pane を読めない。
    PaneMissing,
    /// 入力欄が非空（器自身の queue も含む）。
    InputBusy,
    /// 入力欄が見えない。
    InputUnknown,
}

/// [`Refusal`] の全部（宣言順）。
pub const REFUSALS: &[Refusal] = &[
    Refusal::IdEmpty,
    Refusal::IdShape,
    Refusal::IdLong,
    Refusal::StateDir,
    Refusal::Store,
    Refusal::NoRow,
    Refusal::Busy,
    Refusal::StateMissing,
    Refusal::StateUnreadable,
    Refusal::NoRule,
    Refusal::PaneMissing,
    Refusal::InputBusy,
    Refusal::InputUnknown,
];

impl Refusal {
    /// 断りの行の `reason=` の字面。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::IdEmpty => "id-empty",
            Self::IdShape => "id-shape",
            Self::IdLong => "id-long",
            Self::StateDir => "state-dir",
            Self::Store => "store",
            Self::NoRow => "no-row",
            Self::Busy => "busy",
            Self::StateMissing => "state-missing",
            Self::StateUnreadable => "state-unreadable",
            Self::NoRule => "no-rule",
            Self::PaneMissing => "pane-missing",
            Self::InputBusy => "input-busy",
            Self::InputUnknown => "input-unknown",
        }
    }

    /// 断りの rc（event log を読めない周だけ壊れの 2・他は 1）。
    pub const fn rc(self) -> u8 {
        match self {
            Self::Store => RC_BROKEN,
            _ => RC_REFUSED,
        }
    }
}

/// 記帳 id の形を判じる**1 関数**（設計 §15 形 2・何も読まない）: 空は [`Refusal::IdEmpty`]・ASCII の英数字と `.` `-` `_` `:` の外の
/// byte（空白・制御文字・他の記号・ASCII の外）が 1 つでも在れば [`Refusal::IdShape`]・[`ID_MAX`] byte を越えれば
/// [`Refusal::IdLong`]。
pub fn id_check(id: &str) -> Result<(), Refusal> {
    if id.is_empty() {
        return Err(Refusal::IdEmpty);
    }
    if !id.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b':')) {
        return Err(Refusal::IdShape);
    }
    if id.len() > ID_MAX {
        return Err(Refusal::IdLong);
    }
    Ok(())
}

/// 指し示しの雛形の展開（**正本はこの 1 関数**・`id` だけが外からの値・逐語と続きの指示は持たない）。
pub fn line(id: &str) -> String {
    format!("{NAME} seat: 裁定 {id} が届いた（在りかは裁定面の記帳）")
}

/// `seat deliver` の引数（読みは `cli.rs`）。
pub struct Flags<'a> {
    /// `--state-dir S`。
    pub state_dir: &'a str,
    /// `--target S:W`。
    pub target: &'a str,
    /// `--ruling ID`（形は [`id_check`] が判じる）。
    pub ruling: &'a str,
    /// `--tmux-socket PATH`（任意）。
    pub socket: Option<&'a str>,
}

/// `seat deliver` の本体: 形 2〜5 の門を順に通し、通った周だけ雛形の 1 行を [`deliver_within`] で 1 回送る。
pub fn run(flags: &Flags) -> Outcome {
    match deliver(flags) {
        Ok(Delivery::Delivered(_, settled)) => Outcome::ok_line(format!(
            "seat deliver: delivered target={} ruling={} consumed={}",
            flags.target,
            flags.ruling,
            settled.as_str()
        )),
        Ok(Delivery::Refused(reason)) => refused_line("refused", reason, flags.target, RC_REFUSED),
        Ok(Delivery::Unconfirmed(reason)) => refused_line("unconfirmed", reason, flags.target, RC_REFUSED),
        Err(refusal) => refused_line("refused", refusal.as_str(), flags.target, refusal.rc()),
    }
}

/// 断りの 1 行（stderr）。
fn refused_line(verdict: &str, reason: &str, target: &str, rc: u8) -> Outcome {
    Outcome::failed_line(rc, format!("seat deliver: {verdict} reason={reason} target={target}"))
}

/// 門（id → 置き場 → event log → 登録 row → 打刻 → 窓 → pane → 入力欄）を通した周だけ送る。
fn deliver(flags: &Flags) -> Result<Delivery, Refusal> {
    id_check(flags.ruling)?;
    let state = state_dir_of(Some(flags.state_dir)).ok_or(Refusal::StateDir)?;
    let events = crate::fleet::store::read_all(&state.path).map_err(|_| Refusal::Store)?;
    super::role::registration_of_target(&crate::fleet::replay(&events), flags.target).ok_or(Refusal::NoRow)?;
    idle(&seat_dir(&state.path, flags.target))?;
    let window_ms = int_rule(ROW_WINDOW).map_err(|_| Refusal::NoRule)?;
    let pane = capture(flags.socket, flags.target).ok_or(Refusal::PaneMissing)?;
    input_clear(&pane)?;
    let payload = line(flags.ruling);
    let request = Request { target: flags.target, socket: flags.socket, payload: &payload, state_dir: Some(&state) };
    Ok(deliver_within(&request, Duration::from_millis(window_ms)))
}

/// 状態の打刻の読めた最終行が Idle の周だけ通す（Stop を失った席の係数と黙りの門は持たない＝FR79 の「最終行が idle」だけ）。
fn idle(seat: &Path) -> Result<(), Refusal> {
    let text = match fs::read_to_string(state::path(seat)) {
        Ok(found) => found,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Err(Refusal::StateMissing),
        Err(_) => return Err(Refusal::StateUnreadable),
    };
    let last = text.lines().rev().find_map(|found| Stamp::from_line(found).ok()).ok_or(Refusal::StateUnreadable)?;
    match last.state {
        SeatState::Idle => Ok(()),
        SeatState::Busy => Err(Refusal::Busy),
    }
}

/// 入力欄の門（`own` は渡さない＝器自身の queue も非空として断り、Enter を 1 key も送らない）。
fn input_clear(pane: &str) -> Result<(), Refusal> {
    match guard_input(pane, None) {
        Ok(InputPass::Clear) => Ok(()),
        Ok(InputPass::OwnQueued) | Err(InputGate::Busy) => Err(Refusal::InputBusy),
        Err(InputGate::UnknownInput) => Err(Refusal::InputUnknown),
    }
}

#[cfg(test)]
mod tests {
    use super::{id_check, line, Refusal, ID_MAX, REFUSALS};
    use crate::cli_outcome::{RC_BROKEN, RC_REFUSED};
    use crate::order::is_declaration_order;

    /// 上限ちょうど（64 byte）は通り、1 byte 越え（65 byte）は `id-long`・空は `id-empty`。
    #[test]
    fn seat_deliver_id_length_boundary_is_64_bytes() {
        assert_eq!(ID_MAX, 64);
        assert_eq!(id_check(&"a".repeat(64)), Ok(()), "64 byte");
        assert_eq!(id_check(&"a".repeat(65)), Err(Refusal::IdLong), "65 byte");
        assert_eq!(id_check("a"), Ok(()), "1 byte");
        assert_eq!(id_check(""), Err(Refusal::IdEmpty), "空");
    }

    /// 4 つの記号と英数字は通り、他の字（空白・制御文字・他の ASCII 記号・ASCII の外）は `id-shape`。
    #[test]
    fn seat_deliver_id_shape_admits_alnum_and_four_symbols_only() {
        assert_eq!(id_check("Az09.-_:"), Ok(()), "英数字と 4 つの記号");
        for symbol in ['.', '-', '_', ':'] {
            assert_eq!(id_check(&format!("r{symbol}1")), Ok(()), "{symbol:?}");
        }
        for bad in ["a b", " ", "a\tb", "a\u{7}b", "a\nb", "a;b", "a/b", "a$b", "a`b", "a\"b", "裁定", "a\u{3000}b", "é"] {
            assert_eq!(id_check(bad), Err(Refusal::IdShape), "{bad:?}");
        }
        let other_ascii = (0x21_u8..0x7f).filter(|byte| !byte.is_ascii_alphanumeric() && !b".-_:".contains(byte));
        for byte in other_ascii {
            let id = format!("a{}b", char::from(byte));
            assert_eq!(id_check(&id), Err(Refusal::IdShape), "{id:?}");
        }
    }

    /// 上限を越える長さでも形の外の字が在れば `id-shape`（形を先に見る）・空は他の形より先に `id-empty`。
    #[test]
    fn seat_deliver_id_shape_is_judged_before_length() {
        assert_eq!(id_check(&format!("{} ", "a".repeat(70))), Err(Refusal::IdShape));
        assert_eq!(id_check(&"é".repeat(40)), Err(Refusal::IdShape), "80 byte の非 ASCII");
    }

    /// 断りの語は 13 語・宣言順・重なりなしで、rc は `store` だけ 2。
    #[test]
    fn seat_deliver_refusal_words_are_thirteen_and_distinct() {
        assert!(is_declaration_order(REFUSALS, |refusal| refusal as usize), "宣言順");
        let words: Vec<&str> = REFUSALS.iter().map(|refusal| refusal.as_str()).collect();
        assert_eq!(
            words,
            [
                "id-empty", "id-shape", "id-long", "state-dir", "store", "no-row", "busy", "state-missing", "state-unreadable", "no-rule",
                "pane-missing", "input-busy", "input-unknown",
            ]
        );
        for refusal in REFUSALS {
            let want = if *refusal == Refusal::Store { RC_BROKEN } else { RC_REFUSED };
            assert_eq!(refusal.rc(), want, "{refusal:?}");
        }
    }

    /// 雛形の外からの値は id だけ（1 回・1 行）。
    #[test]
    fn seat_deliver_line_carries_only_the_id() {
        let sent = line("r-1");
        assert_eq!(sent, format!("{} seat: 裁定 r-1 が届いた（在りかは裁定面の記帳）", crate::name::NAME));
        assert_eq!(sent.matches("r-1").count(), 1);
        assert!(!sent.contains('\n'));
    }
}
