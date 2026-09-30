//! 台帳 adapter（設計 docs/design/contract-source.md §6・FR50）。
//!
//! **書きは `close` と `append_notes` の 2 種だけ**である——起票・acceptance の編集は席の手番で、器が持つのは
//! 「着地した便の bead を閉じる」ことと、裁定の結び（`seat ruling bind`）が notes へ 1 行足すことに限る（憲法 C15:
//! 台帳が持つのは task と裁定だけ）。一覧の読みは席の側（[`crate::seat::ledger`]）に在るものをそのまま使い、
//! 2 本目の reader を作らない（C2）。bead 1 本の読み（`show`）だけは結びが撃つ。
//!
//! client の binary の名は既定の const（PATH 解決は子 process の起動側・**env も HOME も読まない**・C2.2）。
//!
//! 台帳の形の lint（doctor の項目 1 行・設計 docs/design/ledger-form.md §3 の 4）は子 module [`form`] に置く
//! （読むだけ・書きの口は増えない）。memo の入口（plan を標準出力に出す read-only の口・§3 の 8）は子 module
//! [`memo`] に置く（台帳を読まず書かない・起票は席の手番）。台帳 lint（doctor の項目 1 行・設計
//! contract-source.md §6・契約表の行 e）は子 module [`lint`] に置く（読むだけ・極性は増えない）。台帳のグラフの形
//! （doctor の項目 1 行・ledger-form.md §10・契約表の行 f）は子 module [`graph`] に置く（読むだけ）。案件の局面のうち台帳の側の
//! 部品（question・memo・epic と閉じた contract・case-lifecycle.md §7・行 a1）と FR93 の条件の 1 関数は子 module [`phase`] に置く
//! （純関数・I/O も時計も持たない）。

pub mod citation;
pub mod close_reason;
pub mod form;
pub mod graph;
pub mod lint;
pub mod memo;
pub mod phase;
pub mod promotion;
pub mod question;
pub mod trigger;

use crate::cli_outcome::{Outcome, RC_REFUSED};
use crate::fleet::json_tree::{self, Tree};
use crate::invocation::Invocation;
use crate::polarity::{OnFailure, Polarity, Timing};
use crate::seat::ledger::LedgerError;
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

/// `bd update <bead> --append-notes <line>` の subcommand と flag（notes の追記・裁定の結びだけが撃つ）。
const UPDATE: &str = "update";
const APPEND_NOTES: &str = "--append-notes";

/// `bd --readonly show <bead> --json`（bead 1 本の読み）。
const READONLY: &str = "--readonly";
const SHOW: &str = "show";
const JSON: &str = "--json";

/// bead を閉じる（設計 §6・FR50）。
///
/// 着地した便の終端だけが撃つ。**冪等である**ことは台帳の側が持つ（既に closed の bead を閉じ直した
/// 周に client が rc 0 を返すかは client の契約で、器はその rc をそのまま typed に運ぶ）。
///
/// **cwd は `repo` に固定する**（読みの口 `spawn_read` と同じ形・設計 pipeline.md 行 ap）: client は台帳を cwd から
/// 上へ探すので、運転手の cwd（消えた dir でも）を継ぐと同じ repo でも台帳を解けない周が出る。
pub fn close(bd: &str, repo: &Path, bead: &str, reason: &str) -> Result<(), CloseError> {
    write(bd, repo, [CLOSE, bead, REASON, reason])
}

/// `bd update <bead> --append-notes <line>`（notes の追記・**裁定の結びだけが撃つ**・設計 fleet-event-log.md §14 約束 5）。
/// cwd と失敗の型は [`close`] と同じ 1 本（[`write`]）。
pub fn append_notes(bd: &str, repo: &Path, bead: &str, line: &str) -> Result<(), CloseError> {
    write(bd, repo, [UPDATE, bead, APPEND_NOTES, line])
}

/// 書きの 1 撃ち（cwd は `repo`・rc 0 だけが成功・失敗は stderr の末尾 1 行を運ぶ）。
fn write(bd: &str, repo: &Path, args: [&str; 4]) -> Result<(), CloseError> {
    let out = Invocation::new(bd).args(args).current_dir(repo).output().map_err(|_| CloseError::Unlaunchable)?;
    if out.status.success() {
        return Ok(());
    }
    let text = String::from_utf8_lossy(&out.stderr);
    Err(CloseError::Refused {
        rc: out.status.code(),
        tail: text.lines().next_back().unwrap_or_default().trim().to_owned(),
    })
}

/// `bd --readonly show <bead> --json` の読み（bead 1 本・[`show`] が返す key だけ）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bead {
    /// status の字面。
    pub status: String,
    /// label の列（無ければ空）。
    pub labels: Vec<String>,
    /// 起票の時刻（`created_at` の字のまま）。
    pub created_at: String,
    /// metadata の asked（無い・閉じた 2 値の外なら `None`）。
    pub asked: Option<String>,
    /// notes（無ければ空）。
    pub notes: String,
}

/// bead 1 本を読む（読みだけ・cwd は `repo`）。要素が 0 件の配列は `Ok(None)`（bead が無い）で、起動できない・rc ≠ 0・JSON を読めない・
/// `status` か `created_at` の文字列が無い周は `Err(Unreadable)`（読めなさを「無い」に倒さない・[`crate::seat::ledger::POLARITY`]）。
pub fn show(bd: &str, repo: &Path, bead: &str) -> Result<Option<Bead>, LedgerError> {
    let out = Invocation::new(bd)
        .args([READONLY, SHOW, bead, JSON])
        .current_dir(repo)
        .output()
        .map_err(|_| LedgerError::Unreadable)?;
    if !out.status.success() {
        return Err(LedgerError::Unreadable);
    }
    let tree = json_tree::parse(&String::from_utf8_lossy(&out.stdout)).map_err(|_| LedgerError::Unreadable)?;
    let node = match &tree {
        Tree::Array(items) => match items.first() {
            Some(first) => first,
            None => return Ok(None),
        },
        other => other,
    };
    bead_of(node).map(Some).ok_or(LedgerError::Unreadable)
}

/// bead 1 本の JSON から [`Bead`] を読む（`status` と `created_at` の文字列が無ければ `None`）。
fn bead_of(node: &Tree) -> Option<Bead> {
    let text_of = |key: &str| node.get(key).and_then(Tree::as_str).map(str::to_owned);
    Some(Bead {
        status: text_of("status")?,
        labels: node.get("labels").and_then(Tree::as_array).unwrap_or_default().iter().filter_map(Tree::as_str).map(str::to_owned).collect(),
        created_at: text_of("created_at")?,
        asked: node
            .get("metadata")
            .and_then(|metadata| metadata.get(question::ASKED_KEY))
            .and_then(Tree::as_str)
            .filter(|found| question::ASKED.contains(found))
            .map(str::to_owned),
        notes: text_of("notes").unwrap_or_default(),
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

    /// FR93 の fixture（memo `s2-m`・契約 2 本・子の問い 1 本）の台帳を JSON の字から作る。5 つの条件を満たす値が既定で、引数が欠けを 1 つ入れる。
    fn fr93_ledger(notes_line: &str, second_reason: &str, child_status: &str) -> Vec<crate::seat::ledger::Issue> {
        let sha = "0123456789abcdef0123456789abcdef01234567";
        let contract = |id: &str, reason: &str| {
            format!(
                r#"{{"id":"{id}","status":"closed","acceptance_criteria":"design = docs/design/x.md#a","close_reason":"{reason}","dependencies":[{{"depends_on_id":"s2-m","type":"discovered-from"}}]}}"#
            )
        };
        let memo = format!(
            r####"{{"id":"s2-m","status":"open","labels":["intake:memo"],"description":"### 出所\n### 観測\n### 候補\n### 昇格条件\n- 引き金: 再発 5\n","notes":"{notes_line}"}}"####
        );
        let child = format!(
            r#"{{"id":"s2-q","status":"{child_status}","labels":["intake:question"],"dependencies":[{{"depends_on_id":"s2-m","type":"parent-child"}}]}}"#
        );
        let text = format!("[{memo},{},{},{child}]", contract("s2-c1", &format!("landed {sha} ci=success")), contract("s2-c2", second_reason));
        crate::seat::ledger::issues_of(&text).expect("fixture の JSON を読める")
    }

    /// FR93 の関数を直に呼び、同じ台帳を局面の関数に通して memo の (局面・手番・理由) を返す。
    fn fr93_probe(issues: &[crate::seat::ledger::Issue], unjudged: &[String]) -> (bool, (String, String, Option<String>)) {
        use super::phase::{close_due, derive, Input, Lines};
        let memo = issues.iter().find(|issue| issue.id == "s2-m").expect("memo が在る");
        let input = Input {
            issues,
            prefix: Some("s2"),
            now: 0,
            window_s: 0,
            lines: Lines { cutover: None, close_check: None },
            unreflected: &[],
            unjudged,
            write_set: &[],
        };
        let part = derive(&input).parts.into_iter().find(|part| part.id == "s2-m").expect("memo の部品が在る");
        (close_due(memo, issues, Some("s2"), unjudged), (part.phase.as_str().to_owned(), part.turn.as_str().to_owned(), part.reason))
    }

    /// 5 つの条件を満たす既定の値。
    const FR93_LINE: &str = "昇格: 全部 s2-c1 s2-c2";
    const FR93_LANDED: &str = "landed 0123456789abcdef0123456789abcdef01234567 ci=success";

    /// 5 つの条件を全部満たす memo は真・局面の関数では close-due の promoting（手番 vessel）。
    #[test]
    fn phase_ledger_fr93_met_when_all_five_conditions_hold() {
        let issues = fr93_ledger(FR93_LINE, FR93_LANDED, "closed");
        let (due, (phase, turn, reason)) = fr93_probe(&issues, &[]);
        assert!(due, "5 つを満たす memo は FR93 を満たす");
        assert_eq!((phase.as_str(), turn.as_str(), reason.as_deref()), ("memo-promoting", "vessel", Some("close-due")));
    }

    /// 条件 (1) 最後の昇格の行が `全部` でない（`一部`）と偽・close-due にならない。
    #[test]
    fn phase_ledger_fr93_unmet_when_the_last_line_is_partial() {
        let issues = fr93_ledger("昇格: 一部 s2-c1 s2-c2", FR93_LANDED, "closed");
        let (due, (phase, turn, reason)) = fr93_probe(&issues, &[]);
        assert!(!due, "一部の行は FR93 を満たさない");
        assert_eq!((phase.as_str(), turn.as_str(), reason.as_deref()), ("memo-actionable", "seat", Some("promotion-unmet")));
    }

    /// 条件 (2) 行の列と辿れる契約の集合が違う（行が s2-c2 を挙げない）と偽・close-due にならない。
    #[test]
    fn phase_ledger_fr93_unmet_when_the_list_differs_from_the_traced_contracts() {
        let issues = fr93_ledger("昇格: 全部 s2-c1", FR93_LANDED, "closed");
        let (due, (phase, _, reason)) = fr93_probe(&issues, &[]);
        assert!(!due, "列が違う memo は FR93 を満たさない");
        assert_eq!((phase.as_str(), reason.as_deref()), ("memo-actionable", Some("promotion-unmet")));
    }

    /// 条件 (3) 辿れる契約の 1 本が着地の形でない（取り下げ）と偽・close-due にならない。
    #[test]
    fn phase_ledger_fr93_unmet_when_a_traced_one_closed_without_landing() {
        let issues = fr93_ledger(FR93_LINE, "取り下げ 不要になった", "closed");
        let (due, (phase, _, reason)) = fr93_probe(&issues, &[]);
        assert!(!due, "着地でない閉じの契約が在る memo は FR93 を満たさない");
        assert_eq!((phase.as_str(), reason.as_deref()), ("memo-actionable", Some("promotion-unmet")));
    }

    /// 条件 (4) 子の開いた問いが在ると偽・close-due にならない（memo-asking）。
    #[test]
    fn phase_ledger_fr93_unmet_when_a_child_question_is_open() {
        let issues = fr93_ledger(FR93_LINE, FR93_LANDED, "open");
        let (due, (phase, turn, reason)) = fr93_probe(&issues, &[]);
        assert!(!due, "子の開いた問いが在る memo は FR93 を満たさない");
        assert_eq!((phase.as_str(), turn.as_str(), reason), ("memo-asking", "user", None));
    }

    /// 条件 (5) 処置の無い判定が在ると偽・close-due にならない（actionable の verdict）。
    #[test]
    fn phase_ledger_fr93_unmet_when_a_verdict_awaits_action() {
        let issues = fr93_ledger(FR93_LINE, FR93_LANDED, "closed");
        let (due, (phase, _, reason)) = fr93_probe(&issues, &["s2-m".to_owned()]);
        assert!(!due, "処置の無い判定が在る memo は FR93 を満たさない");
        assert_eq!((phase.as_str(), reason.as_deref()), ("memo-actionable", Some("verdict")));
    }
}
