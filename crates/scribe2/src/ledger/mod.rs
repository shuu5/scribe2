//! 台帳 adapter（設計 docs/design/contract-source.md §6・FR50）。
//!
//! **書きは `close` の 1 種だけ**である——起票・acceptance の編集・裁定の記帳は席の手番で、器が持つのは
//! 「着地した便の bead を閉じる」1 つに限る（憲法 C15: 台帳が持つのは task と裁定だけ）。読みは席の側
//! （[`crate::seat::ledger`]）に在るものをそのまま使い、2 本目の reader を作らない（C2）。
//!
//! client の binary の名は既定の const（PATH 解決は子 process の起動側・**env も HOME も読まない**・C2.2）。

use crate::polarity::{OnFailure, Polarity, Timing};
use std::process::Command;

/// 台帳 client の既定（読みの側と**同じ 1 つ**を借りる＝名の宣言は 1 か所）。
pub use crate::seat::ledger::DEFAULT_BD;

/// この境界の極性（[`CloseError`]）: 閉じられない周は**着地を取り消さない**が、**台帳も閉じない**。
///
/// 着地（main の commit）は既に成立していて取り消せないので、失敗しても便は落とさない。一方で
/// 「閉じたことにする」側へは倒さない——open のまま残れば次の契機が拾えるが、閉じたと記帳して
/// しまうと誰も拾わない（やり直しは `pipe land --terminal-only <run>`・冪等）。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::PostHoc,
    on_failure: OnFailure::FailClosed,
};

/// 台帳を閉じられない理由（**境界の enum**・[`POLARITY`]・憲法 C11.2）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseError {
    /// client を起動できない（PATH に無い・実行権が無い）。
    Unlaunchable,
    /// client が rc ≠ 0 で終わった（stderr の末尾 1 行を運ぶ）。
    Refused {
        /// 子 process の rc（signal で落ちた周は `None`）。
        rc: Option<i32>,
        /// stderr の末尾 1 行（空なら空文字）。
        tail: String,
    },
}

impl CloseError {
    /// 記録と stderr に出す 1 行。
    pub fn render(&self) -> String {
        match self {
            Self::Unlaunchable => "close:failed:unlaunchable".to_owned(),
            Self::Refused { rc, tail } => {
                let code = rc.map_or_else(|| "signal".to_owned(), |found| found.to_string());
                format!("close:failed:rc={code} {tail}").trim_end().to_owned()
            }
        }
    }
}

/// `bd close <bead> --reason <text>` の subcommand（**書きはこの 1 種だけ**）。
const CLOSE: &str = "close";

/// 理由を渡す flag。
const REASON: &str = "--reason";

/// bead を閉じる（設計 §6・FR50）。
///
/// 着地した便の終端だけが撃つ。**冪等である**ことは台帳の側が持つ（既に closed の bead を閉じ直した
/// 周に client が rc 0 を返すかは client の契約で、器はその rc をそのまま typed に運ぶ）。
pub fn close(bd: &str, bead: &str, reason: &str) -> Result<(), CloseError> {
    let out = Command::new(bd).args([CLOSE, bead, REASON, reason]).output().map_err(|_| CloseError::Unlaunchable)?;
    if out.status.success() {
        return Ok(());
    }
    let text = String::from_utf8_lossy(&out.stderr);
    Err(CloseError::Refused {
        rc: out.status.code(),
        tail: text.lines().next_back().unwrap_or_default().trim().to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::{close, CloseError};

    /// 起動できない client は [`CloseError::Unlaunchable`]（「閉じた」に倒さない・C10）。
    #[test]
    fn pipe_terminal_land_close_refuses_when_the_client_cannot_launch() {
        let err = close("scribe2-no-such-ledger-client", "s2-x", "landed").expect_err("起動できない");
        assert_eq!(err, CloseError::Unlaunchable, "起動できない周の理由");
        assert_eq!(err.render(), "close:failed:unlaunchable", "記録の 1 行");
    }

    /// rc ≠ 0 の client は **rc と stderr の末尾の両方**を運ぶ（黙って「閉じた」にしない）。
    ///
    /// rc も末尾も**呼び手が選んだ値**で測る（`sh` の断り文に賭けると、文言が変わった周に歯が
    /// 静かに空虚化する）。末尾は 2 行目である＝1 行目を取る実装では落ちる。
    #[test]
    fn pipe_terminal_land_close_carries_the_rc_and_the_stderr_tail() {
        use std::os::unix::fs::PermissionsExt;
        let dir = crate::pipe::fixture::scratch("ledger-close");
        let client = dir.join("bd");
        std::fs::write(&client, "#!/bin/sh\nprintf 'first line\\nlast line\\n' >&2\nexit 7\n")
            .expect("偽の client を書ける");
        std::fs::set_permissions(&client, std::fs::Permissions::from_mode(0o755)).expect("実行権を付ける");
        let err = close(&client.display().to_string(), "s2-x", "landed").expect_err("rc 7 で断られる");
        let CloseError::Refused { rc, tail } = &err else {
            panic!("rc ≠ 0 の形: {err:?}");
        };
        assert_eq!(*rc, Some(7), "client の rc をそのまま運ぶ: {err:?}");
        assert_eq!(tail, "last line", "stderr の**末尾**の 1 行を運ぶ（1 行目ではない）: {err:?}");
        assert_eq!(err.render(), "close:failed:rc=7 last line", "記録の 1 行");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
