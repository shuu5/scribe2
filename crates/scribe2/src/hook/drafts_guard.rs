//! 席の起草の写しを、起草の置き場の直下に限る門（設計 docs/design/vessel-hook.md §25・契約表の行 q・SRS FR42 / FR68 / NFR5・
//! [ADR-0096](../../../../design-intent/decisions/ADR-0096-seat-drafts-are-vessel-owned-and-swept-after-writes-stop.html)）。
//!
//! 席の Bash の `git worktree add` と `git clone` の行き先を、席の指示文の 12 行目と同じ 1 関数（[`crate::seat::drafts_dir`]）が解く
//! 置き場の直下の子と、anchor の `.worktrees/` の直下の子に限り、置き場の外と解けない行き先と席を解けない周を実行の前に断る。
//! 位置は bypass の門の後ろ・走っている便の行の門の前。行き先の読みは [`copy_places`]（git の segment の読み手の上の pure な 1 関数）。
//!
//! 写しの segment が 0 の周は tmux も event log も読まずに通し、`--pane` が無いか空の周と登録 row の無い席は通す（席でない）。

use super::command::BASH;
use super::live_row::{copy_places, Dest, Place};
use crate::fleet::{replay, store};
use crate::name::NAME;
use crate::pipe::worktrees_dir;
use crate::polarity::{OnFailure, Polarity, Timing};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// 記録の `what` の頭（極性一覧の語と同じ・後ろに理由の語が付く）。
pub const WHAT: &str = "drafts-deny";

/// この境界の極性: 道具の呼び出しの時点で止め、席を解けない周は写しの segment を断る（止める側へ倒れる）。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailClosed,
};

/// 断る理由の語（閉じた 3 つ）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// 行き先が置き場の直下でない（置き場そのもの・深い所・外）。
    Outside,
    /// 行き先を字面で解けない。
    Unresolved,
    /// 席（tmux の target か event log）を解けない。
    SeatUnresolved,
}

impl Reason {
    /// 断りの行と記録に出す理由の語。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Outside => "outside",
            Self::Unresolved => "unresolved",
            Self::SeatUnresolved => "seat-unresolved",
        }
    }
}

/// 判定（閉じた 2 値）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DraftsDecision {
    /// 通す。
    Pass,
    /// 断る。
    Deny {
        /// 当たった理由。
        reason: Reason,
        /// stderr の 1 行。
        line: String,
    },
}

/// 判定の場（hook が解いた入力だけ）。
pub struct Scene<'a> {
    /// tool 名。
    pub tool: &'a str,
    /// Bash の command 行。
    pub command: Option<&'a str>,
    /// payload の作業 dir。
    pub cwd: &'a Path,
    /// hook の `--project` の root（anchor）。
    pub root: &'a Path,
    /// hook の置き場。
    pub state_dir: &'a Path,
    /// 自席の pane id（無い・空は席でない）。
    pub pane: Option<&'a str>,
    /// tmux の socket。
    pub socket: Option<&'a str>,
}

/// pane から解いた席。
enum Seat {
    /// 登録 row の在る席の target。
    Registered(String),
    /// 登録 row の無い target（席でない）。
    Unregistered,
    /// target か event log を読めない。
    Unresolved,
}

/// 判定する。Bash の周で写しの segment が在り、`--pane` が在る周だけ席を解き、command の中の順に最初の断りを返す。
pub fn decide(scene: &Scene) -> DraftsDecision {
    if scene.tool != BASH {
        return DraftsDecision::Pass;
    }
    let places = copy_places(scene.command.unwrap_or_default(), scene.cwd, scene.root);
    let (Some(first), Some(pane)) = (places.first(), scene.pane.filter(|found| !found.trim().is_empty())) else {
        return DraftsDecision::Pass;
    };
    match seat_of(scene, pane) {
        Seat::Unresolved => deny(Reason::SeatUnresolved, first, &Home::default()),
        Seat::Unregistered => DraftsDecision::Pass,
        Seat::Registered(target) => {
            let drafts = crate::seat::drafts_dir(scene.state_dir, &target);
            let home = Home::of(scene.root, std::path::absolute(&drafts).unwrap_or(drafts));
            places
                .iter()
                .find_map(|place| judged(place, &home).map(|reason| deny(reason, place, &home)))
                .unwrap_or(DraftsDecision::Pass)
        }
    }
}

/// pane → target → 登録 row の順に席を解く（event log は replay する・読めない周は解けない側）。
fn seat_of(scene: &Scene, pane: &str) -> Seat {
    let socket = scene.socket.filter(|found| !found.trim().is_empty());
    let Some(target) = crate::seat::target_of_pane(socket, pane) else {
        return Seat::Unresolved;
    };
    let Ok(events) = store::read_all(scene.state_dir) else {
        return Seat::Unresolved;
    };
    match crate::seat::role::registration_of_target(&replay(&events), &target) {
        Some(_) => Seat::Registered(target),
        None => Seat::Unregistered,
    }
}

/// 通す置き場の 2 つ（席の起草の置き場と anchor の `.worktrees/`）。席を解けない周は既定値（置き場を持たない）。
#[derive(Default)]
struct Home {
    /// 席の起草の置き場（絶対 path）。
    drafts: PathBuf,
    /// anchor の `.worktrees/`（`worktrees_dir` の親・字を 2 面に持たない）。
    worktrees: PathBuf,
    /// `.worktrees/` の直下で通さない名（`worktrees_dir` の名＝便の木の置き場）。
    reserved: OsString,
}

impl Home {
    /// anchor の root と席の起草の置き場から置き場の 2 つを解く。
    fn of(root: &Path, drafts: PathBuf) -> Self {
        let tree = worktrees_dir(root);
        Self {
            drafts,
            worktrees: tree.parent().map_or_else(|| root.to_path_buf(), Path::to_path_buf),
            reserved: tree.file_name().map(OsString::from).unwrap_or_default(),
        }
    }
}

/// 写し 1 つの判定（通すなら `None`）。
fn judged(place: &Place, home: &Home) -> Option<Reason> {
    match &place.to {
        Dest::Unresolved => Some(Reason::Unresolved),
        Dest::Path(path) => (!child_of_a_place(path, home)).then_some(Reason::Outside),
        Dest::Under(dir) => (!is_a_place(dir, home)).then_some(Reason::Outside),
    }
}

/// path の親がどちらかの置き場と同じか（`.worktrees/` の直下は名が便の木の置き場の名と違う周だけ）。
fn child_of_a_place(path: &Path, home: &Home) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    same(parent, &home.drafts) || (same(parent, &home.worktrees) && path.file_name() != Some(home.reserved.as_os_str()))
}

/// dir がどちらかの置き場と同じか。
fn is_a_place(dir: &Path, home: &Home) -> bool {
    same(dir, &home.drafts) || same(dir, &home.worktrees)
}

/// 2 つの path が同じか（両方を、在る最も深い祖先を実体 path に解いて残りを足した形で比べる）。
fn same(left: &Path, right: &Path) -> bool {
    real(left) == real(right)
}

/// 在る最も深い祖先を実体 path に解き、残りを足した path（解けなければ字面のまま）。
fn real(path: &Path) -> PathBuf {
    let Some(existing) = path.ancestors().find(|dir| dir.exists()) else {
        return path.to_path_buf();
    };
    match (existing.canonicalize(), path.strip_prefix(existing)) {
        (Ok(base), Ok(rest)) => base.join(rest),
        _ => path.to_path_buf(),
    }
}

/// 断りにする。
fn deny(reason: Reason, place: &Place, home: &Home) -> DraftsDecision {
    DraftsDecision::Deny { reason, line: deny_line(reason, place, home) }
}

/// 行き先の字（断りの `to=`・作業 dir の直下の形は作業 dir・解けない形は `-`）。
fn shown(to: &Dest) -> String {
    match to {
        Dest::Path(path) | Dest::Under(path) => path.display().to_string(),
        Dest::Unresolved => "-".to_owned(),
    }
}

/// 断りの 1 行。席を解けない周は置き場を持たない。
fn deny_line(reason: Reason, place: &Place, home: &Home) -> String {
    let head = format!("{NAME}: deny drafts reason={} verb={} to={}", reason.as_str(), place.verb, shown(&place.to));
    if reason == Reason::SeatUnresolved {
        return format!(
            "{head} — 席（pane の tmux の target か置き場の event log）を解けないので、写しの行き先を確かめられない（fail-closed・\
             ADR-0096・vessel-hook.md §25）。tmux と event log を直してから撃つ"
        );
    }
    let (drafts, worktrees) = (home.drafts.display(), home.worktrees.display());
    format!(
        "{head} — 席の起草の写しは起草の置き場 {drafts} の直下に置く（器の掃除が起草の木と読むのは直下の子だけ・ADR-0096・\
         vessel-hook.md §25・行き先を字面で解けない形も断る）。docs PR の写しは anchor の .worktrees/（{worktrees}）の直下でもよい\
         （.worktrees/{} は便の木の置き場）。例: git worktree add {drafts}/<名>",
        home.reserved.to_string_lossy()
    )
}
