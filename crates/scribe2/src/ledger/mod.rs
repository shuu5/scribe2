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
//! （純関数・I/O も時計も持たない）。裁定と見送りの閉じの misfit 3 語（行 a2）は子 module [`phase_ruling`] に置く（純関数）。
//! main の側の部品（commit・row・requirement）と着地の commit の misfit 3 語（行 b1）は子 module [`phase_main`] に置く（純関数）。
//!
//! 先読みの口（`ledger prefetch`・設計 ledger-form.md §20 行 p）はこの file の末尾の区間に置く: store の内容の鍵（[`store_key`]）と
//! 台帳の形の写し（[`write_copy`]・[`read_copy`]）と、鍵を読みの前後で測って写しを state dir へ置く口。写しは
//! [`crate::seat::ledger::read_ledger`] の出力からしか作らない（2 本目の読み手を足さない・FR51）。

pub mod citation;
pub mod close_reason;
pub mod form;
pub mod graph;
pub mod lint;
pub mod memo;
pub mod phase;
pub mod phase_main;
pub mod phase_ruling;
pub mod promotion;
pub mod question;
pub mod trigger;

use crate::cli_args::{self, Allowed};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_REFUSED};
use crate::fleet::json_lite::quote;
use crate::fleet::json_tree::{self, Tree};
use crate::fleet::lifecycle_mark::{self, Ledger as Mark};
use crate::hook::vessel::digest::fnv1a_64;
use crate::hook::vessel::{repo_root, state_dir};
use crate::invocation::Invocation;
use crate::polarity::{OnFailure, Polarity, Timing};
use crate::rules::manifest::Manifest;
use crate::seat::ledger::{Issue, LedgerError};
use std::path::{Path, PathBuf};

/// `ledger` に続く引数を捌く（verb は `memo` の 1 つ）。
pub fn dispatch(args: &[String]) -> Outcome {
    match args.first().map(String::as_str) {
        Some("memo") => memo::dispatch(args.get(1..).unwrap_or_default()),
        Some("prefetch") => prefetch(args.get(1..).unwrap_or_default()),
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

/// 写しの形の版（鍵の頭の欄・形を替える便が上げる）。
const COPY_FORM: &str = "ledger-copy-1";

/// 写しの file 名の接頭辞（名は接頭辞と root の path の digest）。
const COPY_PREFIX: &str = "ledger-copy-";

/// 鍵の欄の数（版・root の path・印の root・gen・chunks・journal の長さ）。
const KEY_FIELDS: usize = 6;

/// 写しが運ぶ辺の種別（親は parent-child の最初の 1 本）。
const PARENT_CHILD: &str = "parent-child";

/// store の内容の鍵（設計 ledger-form.md §20 約束 1）: hook と同じ読みで解いた `root` の台帳の印が noms の形で、journal の長さを読める周だけ
/// 在る。files の形・印を読めない・journal が無い周は `None`（写しを読まず書かない）。1 行で、欄はタブで区切る。
pub fn store_key(root: &Path) -> Option<String> {
    let Mark::Noms { root: store_root, generation, chunks } = lifecycle_mark::read_ledger(root)? else {
        return None;
    };
    let journal = lifecycle_mark::read_journal_len(root)?;
    let key = format!("{COPY_FORM}\t{}\t{store_root}\t{generation}\t{chunks}\t{journal}", root.to_str()?);
    (key.split('\t').count() == KEY_FIELDS && !key.contains('\n')).then_some(key)
}

/// `root` の写しの path（`state_dir` の直下・root ごとに 1 つ）。
pub fn copy_path(state_dir: &Path, root: &Path) -> PathBuf {
    state_dir.join(format!("{COPY_PREFIX}{}", fnv1a_64(root.as_os_str().as_encoded_bytes())))
}

/// 写しの 2 行目（bd の list と同じ key の名の 4 欄・辺は parent-child だけを辺の順のまま・空白を持たない）。
fn copy_body(issues: &[Issue]) -> String {
    let one = |issue: &Issue| {
        let edges: Vec<String> = issue
            .deps
            .iter()
            .filter(|dep| dep.kind == PARENT_CHILD)
            .map(|dep| format!("{{\"depends_on_id\":{},\"type\":{}}}", quote(&dep.on), quote(PARENT_CHILD)))
            .collect();
        format!(
            "{{\"id\":{},\"status\":{},\"issue_type\":{},\"dependencies\":[{}]}}",
            quote(&issue.id),
            quote(&issue.status),
            quote(&issue.kind),
            edges.join(",")
        )
    };
    format!("[{}]", issues.iter().map(one).collect::<Vec<_>>().join(","))
}

/// 写しを置く（1 行目が `key`・2 行目が [`copy_body`]・同じ dir の一時 file からの rename で置き換える・fsync はしない）。
pub fn write_copy(state_dir: &Path, root: &Path, key: &str, issues: &[Issue]) -> std::io::Result<()> {
    let tmp = state_dir.join(format!(".{COPY_PREFIX}{}.tmp", std::process::id()));
    let written = std::fs::write(&tmp, format!("{key}\n{}\n", copy_body(issues)));
    let renamed = written.and_then(|()| std::fs::rename(&tmp, copy_path(state_dir, root)));
    if renamed.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    renamed
}

/// 写しを読む。1 行目が今の鍵 `key` と字で等しい周だけ 2 行目を [`crate::seat::ledger::issues_of`] で読む。鍵が違う・2 行目が崩れた・
/// 1 行だけ・file が無い周は `None`（写しが無い扱い）。
pub fn read_copy(state_dir: &Path, root: &Path, key: &str) -> Option<Vec<Issue>> {
    let text = std::fs::read_to_string(copy_path(state_dir, root)).ok()?;
    let (head, rest) = text.split_once('\n')?;
    let body = rest.strip_suffix('\n').unwrap_or(rest);
    if head != key || body.contains('\n') {
        return None;
    }
    crate::seat::ledger::issues_of(body)
}

/// 先読みの口の使い方（引数の誤りの周にだけ出る）。
fn prefetch_usage() -> String {
    "usage: ledger prefetch --repo R [--state-dir S] [--bd B] [--rules F]".to_owned()
}

/// 先読みの口が受ける flag。
const ALLOWED_PREFETCH: &[cli_args::Allowed] = &[Allowed::value("--repo"), Allowed::value("--state-dir"), Allowed::value("--bd"), Allowed::value("--rules")];

/// 先読みを断る閉じた 7 語（判じる順）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reason {
    NoStateDir,
    NoRule,
    NoMark,
    LedgerUnreadable,
    LedgerTimeout,
    Moved,
    Unwritable,
}

impl Reason {
    /// stderr の 1 行の語。
    fn word(self) -> &'static str {
        match self {
            Self::NoStateDir => "no-state-dir",
            Self::NoRule => "no-rule",
            Self::NoMark => "no-mark",
            Self::LedgerUnreadable => "ledger-unreadable",
            Self::LedgerTimeout => "ledger-timeout",
            Self::Moved => "moved",
            Self::Unwritable => "unwritable",
        }
    }

    /// rc（台帳か state dir が壊れている 2 語が 2・ほかは 1）。
    fn rc(self) -> u8 {
        match self {
            Self::LedgerUnreadable | Self::Unwritable => RC_BROKEN,
            _ => RC_REFUSED,
        }
    }
}

/// 先読みの口の flag の値（`--repo` は root へ解いた後）。
struct Fetch<'a> {
    root: &'a Path,
    state_dir: Option<&'a str>,
    bd: Option<&'a str>,
    rules: Option<&'a str>,
}

/// `ledger prefetch --repo R [--state-dir S] [--bd B] [--rules F]`（設計 ledger-form.md §20 約束 3・4）。
fn prefetch(args: &[String]) -> Outcome {
    let usage = || Outcome::failed(RC_REFUSED, vec![prefetch_usage()]);
    let Ok(parsed) = cli_args::parse(args, ALLOWED_PREFETCH) else { return usage() };
    let Ok(repo) = parsed.need("--repo") else { return usage() };
    let Some(root) = repo_root(Path::new(repo)).filter(|_| parsed.positionals().is_empty()) else { return usage() };
    let fetch = Fetch { root: &root, state_dir: parsed.value("--state-dir"), bd: parsed.value("--bd"), rules: parsed.value("--rules") };
    match fetch_copy(&fetch) {
        Ok(count) => Outcome::ok_line(format!("ledger-prefetch: beads={count}")),
        Err(reason) => Outcome::failed_line(reason.rc(), format!("ledger-prefetch: refused reason={}", reason.word())),
    }
}

/// 鍵を測る → 待ち上限で台帳を 1 回読む → 鍵を測り直す → 等しければ写しを置く（置いた件数を返す）。
fn fetch_copy(fetch: &Fetch) -> Result<usize, Reason> {
    let dir = match fetch.state_dir {
        Some(found) => PathBuf::from(found),
        None => state_dir(fetch.root).ok_or(Reason::NoStateDir)?,
    };
    let manifest = fetch.rules.map_or_else(Manifest::embedded, |path| Manifest::load(Path::new(path))).map_err(|_| Reason::NoRule)?;
    let timeout = crate::seat::ledger::timeout_of(&manifest).ok_or(Reason::NoRule)?;
    let before = store_key(fetch.root).ok_or(Reason::NoMark)?;
    let bd = fetch.bd.filter(|found| !found.trim().is_empty()).unwrap_or(DEFAULT_BD);
    let issues = crate::seat::ledger::read_ledger(bd, fetch.root, timeout).map_err(|error| match error {
        LedgerError::Unreadable => Reason::LedgerUnreadable,
        LedgerError::Timeout => Reason::LedgerTimeout,
    })?;
    if store_key(fetch.root).as_deref() != Some(before.as_str()) {
        return Err(Reason::Moved);
    }
    write_copy(&dir, fetch.root, &before, &issues).map_err(|_| Reason::Unwritable)?;
    Ok(issues.len())
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

    /// 裁定の閉じの misfit の関数に、線の後の閉じた問いと memo を通して (bead id・語) の列を返す（JSON の字から `issues_of` で作る）。
    fn ruling_misfits(question_reason: &str, memo_reason: &str) -> Vec<(String, &'static str)> {
        use super::phase::Lines;
        use super::phase_ruling::{derive, Input};
        let closed = |id: &str, label: &str, reason: &str| {
            format!(r#"{{"id":"{id}","status":"closed","labels":["{label}"],"close_reason":"{reason}","closed_at":"2026-09-28T00:00:00Z"}}"#)
        };
        let text = format!("[{},{}]", closed("s2-q", "intake:question", question_reason), closed("s2-m", "intake:memo", memo_reason));
        let issues = crate::seat::ledger::issues_of(&text).expect("fixture の JSON を読める");
        let (cutover, check) = (crate::fleet::epoch_of("2026-09-20T00:00:00Z"), crate::fleet::epoch_of("2026-09-25T00:00:00Z"));
        let input = Input { issues: &issues, prefix: Some("s2"), lines: Lines { cutover, close_check: check }, bound: &[] };
        derive(&input).into_iter().map(|(id, word)| (id, word.as_str())).collect()
    }

    /// 解けず結ばれてもいない問いの裁定は close-ruling-unresolved の 1 語だけ（not-bound を足さない・表の順）。
    #[test]
    fn phase_ruling_question_unresolved_returns_one_word() {
        assert_eq!(ruling_misfits("裁定 nonsense", "完了"), [("s2-q".to_owned(), "close-ruling-unresolved")]);
    }

    /// 解けず子の問いの裁定でもない見送りは close-ruling-unresolved の 1 語だけ（deferred-not-child-ruling を足さない）。
    #[test]
    fn phase_ruling_memo_unresolved_returns_one_word() {
        let got = ruling_misfits("完了", "見送り nonsense");
        assert_eq!(got, [("s2-m".to_owned(), "close-ruling-unresolved")]);
    }

    /// main の側の部品の関数は入力の不足（commit・台帳・表・要件が全部空か読めない）で落ちず、読めない 2 面を名指す（行 b1）。
    #[test]
    fn phase_main_missing_inputs_do_not_panic() {
        use super::phase_main::{derive, Input};
        let input = Input { commits: &[], cutover: 0, runs: &[], issues: &[], rows: None, requirements: None };
        let out = derive(&input);
        assert!(out.parts.is_empty() && out.ties.runs.is_empty() && out.ties.beads.is_empty(), "部品も結びも無い");
        assert_eq!(out.unmeasured.len(), 2, "表と要件の 2 面を測れないと名指す");
        let empty = derive(&Input { rows: Some(&[]), requirements: Some(&[]), ..input });
        assert!(empty.parts.is_empty() && empty.unmeasured.is_empty(), "読めて空なら 0 件で unmeasured は空");
    }

    // ─── 先読みの口の鍵と写し（設計 ledger-form.md §20 行 p・接頭辞 `ledger_prefetch_`） ───

    /// 書く（親 dir も作る）。
    fn put(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().expect("親 dir が在る")).expect("dir を作れる");
        std::fs::write(path, text).expect("書ける");
    }

    /// journal の名（全部 `v`）。
    fn journal_name() -> String {
        "v".repeat(32)
    }

    /// 実物の形の store（metadata.json・頭の欄 5 つと table の組 1 つと journal の組 1 つの manifest・journal の file）を `repo` の `.beads` に置く。
    fn real_store(repo: &Path, shape: Shape, journal: &str) {
        let (root, collected, table, chunks) = shape;
        let noms = repo.join(".beads/embeddeddolt/beads/.dolt/noms");
        put(&repo.join(".beads/metadata.json"), r#"{"dolt_mode":"embedded","dolt_database":"beads"}"#);
        let manifest = format!("5:__DOLT__:{}:{root}:{collected}:{}:{table}:{}:{chunks}\n", "l".repeat(32), "a".repeat(32), journal_name());
        put(&noms.join("manifest"), &manifest);
        put(&noms.join(journal_name()), journal);
    }

    /// store の形（root・gc の世代・table の組の chunk 数・journal の組の chunk 数）。
    type Shape = (&'static str, &'static str, u64, u64);

    /// 元の store の形。
    const BASE: Shape = ("0123456789abcdef0123456789abcdef", "00000000000000000000000000000000", 10, 7);

    /// 鍵の欄の数。
    const FIELDS: usize = 6;

    /// 実物の形の store の鍵が在り、manifest の root だけ・gc の世代だけ・table の組の chunk 数だけを替えた store と journal に byte を足すだけの store の
    /// 4 つで元と違う欄が替えた 1 欄だけで、同じ store を別の root の path に置くと違う。鍵の 1 行は 6 欄で頭が形の版。journal の file を消した store・
    /// metadata.json の無い（files の形の）`.beads`・metadata.json が JSON でない `.beads` は鍵が無い。
    #[test]
    fn ledger_prefetch_key_moves_only_on_the_field_that_changed() {
        use crate::pipe::fixture::scratch;
        let fields = |key: &str| key.split('\t').map(str::to_owned).collect::<Vec<_>>();
        let base = scratch("prefetch-key-base");
        real_store(&base, BASE, "journal");
        let key = super::store_key(&base).expect("実物の形の store に鍵が在る");
        let key_fields = fields(&key);
        assert_eq!(key_fields.len(), FIELDS, "鍵の 1 行はタブで割ると 6 欄: {key}");
        assert_eq!(key_fields.first().map(String::as_str), Some("ledger-copy-1"), "頭の欄は形の版");
        assert_eq!(key_fields.get(1).map(String::as_str), base.to_str(), "次の欄は root の path");
        let differing = |repo: &Path| {
            let found = fields(&super::store_key(repo).expect("鍵が在る"));
            found.iter().zip(&key_fields).enumerate().filter(|(_, (new, old))| new != old).map(|(at, _)| at).collect::<Vec<_>>()
        };
        // 4 つの替えは元と同じ root の path に置く（同じ dir へ書き直して測り、元の形へ戻す）。
        let rewrite = |shape: Shape, journal: &str| {
            real_store(&base, shape, journal);
            let found = differing(&base);
            real_store(&base, BASE, "journal");
            found
        };
        let (root, collected, table, chunks) = BASE;
        assert_eq!(rewrite(("fedcba9876543210fedcba9876543210", collected, table, chunks), "journal"), [2], "root だけ");
        assert_eq!(rewrite((root, "11111111111111111111111111111111", table, chunks), "journal"), [3], "gc の世代だけ（gen の欄）");
        assert_eq!(rewrite((root, collected, table + 1, chunks), "journal"), [4], "table の組の chunk 数だけ（chunks の欄）");
        assert_eq!(rewrite(BASE, "journal+appended"), [5], "journal に byte を足すだけ（長さの欄）");
        let elsewhere = scratch("prefetch-key-elsewhere");
        real_store(&elsewhere, BASE, "journal");
        assert_eq!(differing(&elsewhere), [1], "同じ store を別の root の path に置くと path の欄だけが違う");
        let gone = scratch("prefetch-key-no-journal");
        real_store(&gone, BASE, "journal");
        std::fs::remove_file(gone.join(".beads/embeddeddolt/beads/.dolt/noms").join(journal_name())).expect("journal を消せる");
        assert_eq!(super::store_key(&gone), None, "journal の file が無い");
        let files = scratch("prefetch-key-files");
        put(&files.join(".beads/issues.jsonl"), "{}\n");
        assert_eq!(super::store_key(&files), None, "metadata.json の無い files の形");
        let broken = scratch("prefetch-key-broken");
        real_store(&broken, BASE, "journal");
        put(&broken.join(".beads/metadata.json"), "{");
        assert_eq!(super::store_key(&broken), None, "metadata.json が JSON でない");
    }

    /// bd の JSON の字（親 2 つの bead の辺の順は x → y・blocks の辺と label と notes を持つ）。
    const TWO_PARENTS: &str = r#"[{"id":"s2-a","status":"open","issue_type":"task","labels":["intake:memo"],"notes":"n","dependencies":[{"issue_id":"s2-a","depends_on_id":"x","type":"parent-child"},{"issue_id":"s2-a","depends_on_id":"w","type":"blocks"},{"issue_id":"s2-a","depends_on_id":"y","type":"parent-child"}]},{"id":"s2-b","status":"closed"}]"#;

    /// 往復: id・status・型と parent-child の辺が辺の順のまま戻り、blocks の辺と label と notes は戻らない。2 行目は空白を持たない。
    #[test]
    fn ledger_prefetch_copy_round_trips_the_four_fields_only() {
        let (state, root) = (crate::pipe::fixture::scratch("prefetch-copy-state"), crate::pipe::fixture::scratch("prefetch-copy-root"));
        let issues = crate::seat::ledger::issues_of(TWO_PARENTS).expect("fixture の JSON を読める");
        super::write_copy(&state, &root, "the-key", &issues).expect("写しを置ける");
        let text = std::fs::read_to_string(super::copy_path(&state, &root)).expect("写しを読める");
        let want = r#"[{"id":"s2-a","status":"open","issue_type":"task","dependencies":[{"depends_on_id":"x","type":"parent-child"},{"depends_on_id":"y","type":"parent-child"}]},{"id":"s2-b","status":"closed","issue_type":"","dependencies":[]}]"#;
        assert_eq!(text, format!("the-key\n{want}\n"), "1 行目が鍵・2 行目が 4 欄の compact な配列");
        let back = super::read_copy(&state, &root, "the-key").expect("同じ鍵で読める");
        let first = back.first().expect("1 件目");
        assert_eq!((first.id.as_str(), first.status.as_str(), first.kind.as_str()), ("s2-a", "open", "task"), "id・status・型");
        let edges: Vec<(&str, &str)> = first.deps.iter().map(|dep| (dep.on.as_str(), dep.kind.as_str())).collect();
        assert_eq!(edges, [("x", "parent-child"), ("y", "parent-child")], "辺の順のまま・blocks は戻らない");
        assert!(first.labels.is_empty() && first.notes.is_empty(), "label と notes は戻らない");
        assert_eq!(back.len(), 2, "件数");
    }

    /// 鍵の欄を 1 つずつ違えた写し・2 行目の崩れ・1 行だけ・無い file は写しが無い扱い（欄を違えない写しは読める）。
    #[test]
    fn ledger_prefetch_copy_is_absent_unless_the_key_equals_and_the_body_reads() {
        let (state, root) = (crate::pipe::fixture::scratch("prefetch-absent-state"), crate::pipe::fixture::scratch("prefetch-absent-root"));
        let key = ["ledger-copy-1", root.to_str().expect("path"), "r", "g", "5", "9"].join("\t");
        let issues = crate::seat::ledger::issues_of(TWO_PARENTS).expect("fixture の JSON を読める");
        assert!(super::read_copy(&state, &root, &key).is_none(), "無い file");
        super::write_copy(&state, &root, &key, &issues).expect("写しを置ける");
        assert!(super::read_copy(&state, &root, &key).is_some(), "対照: 欄を違えない写しは読める");
        for at in 0..FIELDS {
            let changed: Vec<String> =
                key.split('\t').enumerate().map(|(index, field)| if index == at { format!("{field}x") } else { field.to_owned() }).collect();
            super::write_copy(&state, &root, &changed.join("\t"), &issues).expect("写しを置ける");
            assert!(super::read_copy(&state, &root, &key).is_none(), "{at} 番目の欄が違う写しは無い扱い");
        }
        let path = super::copy_path(&state, &root);
        put(&path, &format!("{key}\n{{broken\n"));
        assert!(super::read_copy(&state, &root, &key).is_none(), "2 行目の崩れ");
        put(&path, &format!("{key}\n"));
        assert!(super::read_copy(&state, &root, &key).is_none(), "鍵の後が空");
        put(&path, &key);
        assert!(super::read_copy(&state, &root, &key).is_none(), "1 行だけ");
    }
}
