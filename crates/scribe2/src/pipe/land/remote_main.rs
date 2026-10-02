//! 着地の番を取った周の remote の main の取り込み（設計 docs/design/pipeline.md §69・契約表の行 bm・FR11 / FR10）。
//!
//! remote を宣言する repo は、候補の木の前に remote の main の先端 T を読む（読みは retire の照合と同じ 1 本・
//! [`read_tip`]）。anchor の main L が T の祖先なら CAS で T へ進め、anchor を land の CAS の後と同じ形で揃える。
//! 揃えない周（remote を宣言しない・main が無い・L と T が同じ・T が L の祖先）は今の経路へ進む。読めない・分かれた・
//! CAS を git が断った周は main に載せず [`Block`] で返す。
//!
//! 子は repo の path だけを受けて閉じた結果を返す。記帳と stdout は親（land.rs）が書く＝他の行が守る型を持たない。

use super::anchor::{self, Anchored};
use super::{anchor_plan, git_line, git_ok, sync_anchor, MAIN_REF};
use crate::pipe::declaration::terminal_facts;
use crate::pipe::retire::{read_tip, RemoteTip};
use std::path::Path;

/// 取り込みの結果（**閉じた 3 値**）。
pub(super) enum Aligned {
    /// 揃えない（今の経路へ進む・stdout も記帳も 1 字も変わらない）。
    Untouched,
    /// main を remote の先端へ進めた。
    Forwarded(Forward),
    /// main に載せずに止める。
    Blocked(Block),
}

/// 進めた周の材料。
pub(super) struct Forward {
    /// 進める前の main。
    from: String,
    /// remote の main の先端（進めた先）。
    to: String,
    /// anchor の揃えの結果（§57 の印を含む）。
    anchored: Anchored,
}

impl Forward {
    /// 記帳の detail（`remote-main:ff:<from>..<to>`）。
    pub(super) fn detail(&self) -> String {
        format!("remote-main:ff:{}..{}", self.from, self.to)
    }

    /// stdout の 1 行（揃えの行・anchor の token を後置する）。
    pub(super) fn line(&self, run: &str) -> String {
        format!("run={run} remote-main=ff:{}..{} {}", self.from, self.to, self.anchored.sync.token())
    }

    /// stderr の行（anchor の warning と印を書けない行）。
    pub(super) fn notes(&self) -> Vec<String> {
        self.anchored.sync.warning().into_iter().chain(self.anchored.err.clone()).collect()
    }
}

/// 止める理由（**閉じた 3 値**・字面は stdout の `remote-main=` と記帳の `remote-main:` が同じ）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Block {
    /// 先端を読めない・取れない。
    Unread,
    /// main と先端がどちらも相手の祖先でない（祖先の判定を読めない周も同じ側）。
    Diverged,
    /// fast-forward の CAS を git が断った。
    FfFailed,
}

impl Block {
    /// 理由の語。
    pub(super) fn word(self) -> &'static str {
        match self {
            Self::Unread => "unreadable",
            Self::Diverged => "diverged",
            Self::FfFailed => "ff-failed",
        }
    }

    /// 記帳の detail（`remote-main:<語>`）。
    pub(super) fn detail(self) -> String {
        format!("remote-main:{}", self.word())
    }
}

/// `a` が `b` の祖先か（rc 0 だけを真と読む・読めない周は偽＝止める側に倒れる）。
fn is_ancestor(repo: &Path, a: &str, b: &str) -> bool {
    git_ok(repo, &["merge-base", "--is-ancestor", a, b])
}

/// remote の main の先端を読んで仕分ける（宣言を読めない周と anchor の main を読めない周は git を撃たず `Untouched`）。
pub(super) fn align(repo: &Path) -> Aligned {
    let Some(remote) = terminal_facts(repo).ok().and_then(|facts| facts.remote) else {
        return Aligned::Untouched;
    };
    let Some(local) = git_line(repo, &["rev-parse", MAIN_REF]) else {
        return Aligned::Untouched;
    };
    let tip = match read_tip(repo, &remote) {
        RemoteTip::Found(found) => found,
        RemoteTip::NoMain => return Aligned::Untouched,
        RemoteTip::Unread => return Aligned::Blocked(Block::Unread),
    };
    if tip == local || is_ancestor(repo, &tip, &local) {
        return Aligned::Untouched;
    }
    if !is_ancestor(repo, &local, &tip) {
        return Aligned::Blocked(Block::Diverged);
    }
    forward(repo, local, tip)
}

/// main を CAS で先端へ進め、anchor を land の CAS の後と同じ形（見立ては ref を進める前に読む）で揃える。
fn forward(repo: &Path, from: String, to: String) -> Aligned {
    let plan = anchor_plan(repo);
    if !git_ok(repo, &["update-ref", MAIN_REF, &to, &from]) {
        return Aligned::Blocked(Block::FfFailed);
    }
    let anchored = anchor::record(repo, sync_anchor(repo, &plan, &from, &to), &from, &to);
    Aligned::Forwarded(Forward { from, to, anchored })
}
