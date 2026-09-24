//! 台帳 adapter（設計 docs/design/contract-source.md §6・FR50）。
//!
//! **書きは `close` の 1 種だけ**である——起票・acceptance の編集・裁定の記帳は席の手番で、器が持つのは
//! 「着地した便の bead を閉じる」1 つに限る（憲法 C15: 台帳が持つのは task と裁定だけ）。読みは席の側
//! （[`crate::seat::ledger`]）に在るものをそのまま使い、2 本目の reader を作らない（C2）。
//!
//! client の binary の名は既定の const（PATH 解決は子 process の起動側・**env も HOME も読まない**・C2.2）。
//!
//! 台帳の形の lint（doctor の項目 1 行・設計 docs/design/ledger-form.md §3 の 4）は子 module [`form`] に置く
//! （読むだけ・書きの口は増えない）。memo の入口（plan を標準出力に出す read-only の口・§3 の 8）は子 module
//! [`memo`] に置く（台帳を読まず書かない・起票は席の手番）。台帳 lint（doctor の項目 1 行・設計
//! contract-source.md §6・契約表の行 e）は子 module [`lint`] に置く（読むだけ・極性は増えない）。

pub mod form;
pub mod lint;
pub mod memo;

use crate::cli_outcome::{Outcome, RC_REFUSED};
use crate::invocation::Invocation;
use crate::polarity::{OnFailure, Polarity, Timing};
use std::path::Path;

/// `ledger` に続く引数を捌く（verb は `memo` の 1 つ）。
pub fn dispatch(args: &[String]) -> Outcome {
    match args.first().map(String::as_str) {
        Some("memo") => memo::dispatch(args.get(1..).unwrap_or_default()),
        _ => Outcome::failed(RC_REFUSED, vec![memo::usage()]),
    }
}

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
///
/// **cwd は `repo` に固定する**（読みの口 `spawn_read` と同じ形・設計 pipeline.md 行 ap）: client は台帳を cwd から
/// 上へ探すので、運転手の cwd（消えた dir でも）を継ぐと同じ repo でも台帳を解けない周が出る。
pub fn close(bd: &str, repo: &Path, bead: &str, reason: &str) -> Result<(), CloseError> {
    let out = Invocation::new(bd)
        .args([CLOSE, bead, REASON, reason])
        .current_dir(repo)
        .output()
        .map_err(|_| CloseError::Unlaunchable)?;
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
    use crate::pipe::fixture::{exited, Call, Stub};
    use std::path::{Path, PathBuf};

    /// ledger の bd の起動は起動の記述を通る（設計 core-boundary.md §9 行 f）: program は client の名・引数は
    /// `close <bead> --reason <text>`・cwd は repo。起動の失敗は `Unlaunchable`・rc 非 0 は `Refused`（rc を運ぶ）・
    /// rc 0 は閉じた。
    #[test]
    fn invocation_seat_bd_output_failure_is_typed() {
        let stub = Stub::install(|call| match call.args.get(1).map(String::as_str) {
            Some("s2-ok") => exited(0, b""),
            Some("s2-refused") => exited(3, b"ignored\n"),
            _ => Err(std::io::Error::other("gone")),
        });
        let repo = Path::new("/nonexistent-invocation-seat-bd");
        assert_eq!(close("bd-stub", repo, "s2-gone", "landed"), Err(CloseError::Unlaunchable), "起動の失敗");
        assert_eq!(
            close("bd-stub", repo, "s2-refused", "landed"),
            Err(CloseError::Refused { rc: Some(3), tail: String::new() }),
            "rc 非 0 は rc を運ぶ（stdout は読まない）"
        );
        assert_eq!(close("bd-stub", repo, "s2-ok", "landed"), Ok(()), "rc 0 は閉じた");
        let bd = |bead: &str| Call {
            program: "bd-stub".to_owned(),
            args: ["close", bead, "--reason", "landed"].iter().map(|arg| (*arg).to_owned()).collect(),
            cwd: Some(PathBuf::from("/nonexistent-invocation-seat-bd")),
            envs: Vec::new(),
        };
        assert_eq!(stub.calls(), [bd("s2-gone"), bd("s2-refused"), bd("s2-ok")], "client の名と引数と cwd");
    }

    /// 起動できない client は [`CloseError::Unlaunchable`]（「閉じた」に倒さない・C10）。
    #[test]
    fn pipe_terminal_land_close_refuses_when_the_client_cannot_launch() {
        let err =
            close("scribe2-no-such-ledger-client", &std::env::temp_dir(), "s2-x", "landed").expect_err("起動できない");
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
        let err = close(&client.display().to_string(), &dir, "s2-x", "landed").expect_err("rc 7 で断られる");
        let CloseError::Refused { rc, tail } = &err else {
            panic!("rc ≠ 0 の形: {err:?}");
        };
        assert_eq!(*rc, Some(7), "client の rc をそのまま運ぶ: {err:?}");
        assert_eq!(tail, "last line", "stderr の**末尾**の 1 行を運ぶ（1 行目ではない）: {err:?}");
        assert_eq!(err.render(), "close:failed:rc=7 last line", "記録の 1 行");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 実行権つきの偽 client を `dir/bd` に書き、その path を返す。
    fn fake_client(dir: &std::path::Path, body: &str) -> String {
        use std::os::unix::fs::PermissionsExt;
        let client = dir.join("bd");
        std::fs::write(&client, format!("#!/bin/sh\n{body}")).expect("偽の client を書ける");
        std::fs::set_permissions(&client, std::fs::Permissions::from_mode(0o755)).expect("実行権を付ける");
        client.display().to_string()
    }

    /// client は **repo を cwd にして**撃たれる（設計 pipeline.md 行 ap）。repo は test の cwd と**別の** dir で測る
    /// ——cwd を継ぐ実装では、書かれた path が test の cwd になって落ちる。
    #[test]
    fn pipe_terminal_land_close_cwd_is_the_repo() {
        let dir = crate::pipe::fixture::scratch("ledger-close-cwd");
        let repo = dir.join("repo");
        std::fs::create_dir_all(&repo).expect("repo の dir を作れる");
        let repo = repo.canonicalize().expect("repo の path を解ける");
        let log = dir.join("cwd.txt");
        let client = fake_client(&dir, &format!("pwd -P > '{}'\n", log.display()));
        let here = std::env::current_dir().expect("test の cwd を読める");
        assert_ne!(here, repo, "fixture: repo は test の cwd と別の dir");
        close(&client, &repo, "s2-x", "landed").expect("rc 0 の client は閉じる");
        let written = std::fs::read_to_string(&log).expect("偽 client が撃たれた");
        assert_eq!(written.trim_end(), repo.display().to_string(), "client の cwd は repo");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// cwd を固定しても**断りの形は変わらない**: rc 1 と stderr 2 行の client は `Refused { rc: Some(1), tail: 末尾 }`。
    #[test]
    fn pipe_terminal_land_close_cwd_keeps_the_refusal_shape() {
        let dir = crate::pipe::fixture::scratch("ledger-close-cwd-refused");
        let client = fake_client(&dir, "printf 'no beads here\\nlast word\\n' >&2\nexit 1\n");
        let err = close(&client, &dir, "s2-x", "landed").expect_err("rc 1 で断られる");
        assert_eq!(err, CloseError::Refused { rc: Some(1), tail: "last word".to_owned() }, "断りの形");
        assert_eq!(err.render(), "close:failed:rc=1 last word", "記録の 1 行");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
