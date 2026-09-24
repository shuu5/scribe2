//! 追随（main が動いた便の rebase）の**衝突を runner が解く**段（設計
//! docs/design/pipeline-conflict.md §3 / §5・ADR-0019 §2.2 / §2.4 / §2.6・FR34 / FR14・
//! 憲法 C2 / C3 / C6・NFR4）。
//!
//! 衝突しても**便を終端にしない**。木を `git rebase --abort` で戻し、`RunStage
//! stage=Implemented detail=rebase-conflict:<base>..<main>` を 1 件記帳して、実装役を
//! もう 1 turn 起こす（「main を取り込んで直してから続けて」）。何回まで起こし直すかは
//! rules 行 `pipe.follow_retries` が持ち、**回数は replay から導く**（別の状態 file を
//! 持たない・C3）。
//!
//! **撃ち直しの間に main がさらに動いた周も同じ上限を分け合う**（設計 pipeline.md §5.4 (vi) / §18・
//! `s2-07l.335`）。land は `RunStage stage=Gated detail=stale:<base>..<main>` を同じ記帳の口
//! （[`on_stale`]）で 1 件記し、同じ land の中で追随し直す（runner は要らない）。回数は
//! `rebase-conflict:` と `stale:` の行を合算し（[`retried`]）、上限で `Failed detail=rebase-conflict`。
//!
//! **追随で入った契約表の行が便の消した path を名指す周も同じ経路で起こし直す**（設計 pipeline.md §34・`s2-07l.400`）。
//! land は rebase の直後に [`stale_rows_in`] を撃ち、該当の周は [`on_stale_rows`] が `RunStage stage=Implemented
//! detail=rebase-stale-rows:<base>..<main>` を記帳し、写しの write-set に行の設計 doc を追記して runner を起こす。回数は
//! 衝突と同じ上限（[`is_conflict`] が両方の接頭辞を数える）で、上限で `Failed detail=rebase-stale-rows`。
//!
//! **追随の形が無い便（base が main の祖先でない）も merge-base が在れば追随する**（設計 pipeline.md §38・`s2-07l.449`）。
//! 祖先検査は閉じた 3 値 [`Ancestry`]（祖先／祖先でないが merge-base が在る／merge-base が無い）で、land の追随
//! （`follow_main`）と起こし直しの stdin の「追随」節（[`section`]）が**同じ 1 本**を読む。2 つ目の周は
//! `git rebase --onto <main> <base>` で便が base の上に積んだ commit だけを main の上へ運び、以後は従来の追随の経路
//! （衝突の起こし直し・`rebase:` の記帳・再 gate）に合流する。merge-base の無い周だけ `stale base` で断る。
//!
//! **runner を起こす経路はこの module の [`spawn_turn`] ただ 1 本**である（起動そのものは
//! [`super::spawn::spawn`]＝C6 の 1 口）。起こし直しと通常の起動で turn の後始末（[`settle`]）が
//! 分かれると、追随の base 記帳が片方の経路から静かに抜ける——`resume` で起こし直した turn が
//! 新しい base を記帳できないと、次の gate が**古い base の 2 点 diff**を測り、先着便の file を
//! write-set の外と誤る。

use super::contract::{Contract, CLASS_ROW};
use super::declaration::{is_under, Ceiling, Effective, CEILING_ROW, DENIED_ROW};
use super::gate::RC_INCONCLUSIVE;
use super::land::MAIN_REF;
use super::ratelimit::{choose_account, Pool};
use super::refuse::SHRINK_FILE;
use super::spawn::{spawn, Account, Launch};
use super::table::{repo_findings, Located};
use super::{
    base_of_run, contract_path, emit, git_line, git_ok, question_of_run, vessel_path, worktree_path, Emit, Precheck,
};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_OK, RC_REFUSED};
use crate::fleet::store::{self, LockPolicy};
use crate::fleet::{EventKind, Stage};
use crate::polarity::{OnFailure, Polarity, Timing};
use crate::rules::manifest::Manifest;
use crate::rules::RuleValue;
use std::path::Path;

/// この境界の極性（[`FollowCheck`]）: 起こし直す**前**に回数を測って止め（in-loop）、
/// 回数を読めない周は起こし直さない（fail-closed・NFR4）。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailClosed,
};

/// 上限に達した周の終端の理由（`Failed` の `detail`）。
///
/// 衝突を**記帳する**周の `detail` は同じ語に `:<base>..<main>` を付けた形で、判定は
/// [`is_conflict`] ただ 1 本が持つ（字面を 2 度書かない——片方だけを直すと、起こし直せる
/// 便の集合が静かにずれる）。
pub(crate) const EXHAUSTED: &str = "rebase-conflict";

/// 撃ち直しの間に main がさらに動いた周の記帳の語（`RunStage stage=Gated detail=stale:<base>..<main>`・
/// 設計 pipeline.md §5.4 (vi) / §18・`s2-07l.335`）。
///
/// 便は終端にせず、同じ land の中で追随し直す（rebase → 再 gate → land）。回数は衝突の記帳と
/// **同じ 1 つの上限**（`pipe.follow_retries`）に合算し（[`retried`]）、上限で `Failed detail=rebase-conflict`
/// （新しい終端の理由を増やさない）。判定は [`is_stale`] ただ 1 本が持つ。
pub(crate) const STALE: &str = "stale";

/// 追随で入った契約表の行が便の消した path を名指す周の語（設計 pipeline.md §34・`s2-07l.400`）。
///
/// 記帳は `RunStage stage=Implemented detail=rebase-stale-rows:<base>..<main>`（衝突と同じ形・終端にしない）、上限に
/// 達した周の終端は `Failed detail=rebase-stale-rows`。回数は衝突と**同じ 1 つの上限**（`pipe.follow_retries`）で、
/// 判定は [`is_conflict`] の 1 本が両方の接頭辞を数える（resume の弁別も同じ 1 本）。
pub(crate) const STALE_ROWS: &str = "rebase-stale-rows";

/// 回数を読めなかった周の終端の理由（**上限到達とは分ける**・rc 2）。
const UNMEASURED: &str = "follow-unmeasured";

/// runner が rebase の途中で turn を終えた周の終端の理由。
const DIRTY: &str = "rebase-dirty";

/// 衝突した木を戻せなかった周の終端の理由。
const ABORT_FAILED: &str = "rebase-abort-failed";

/// 木が rebase の途中であることを示す git の作業 dir（どちらの形も「途中」である）。
const REBASE_DIRS: [&str; 2] = ["rebase-merge", "rebase-apply"];

/// 起こし直しの回数の判定（**閉じた 3 値**・設計 §5 の guard）。
///
/// 終端形は variant ごとに固定である: [`Exhausted`](Self::Exhausted) は
/// `Failed detail=rebase-conflict` + rc 1、[`Unreadable`](Self::Unreadable) は
/// `Failed detail=follow-unmeasured` + rc 2（読めない store は「上限に達した」ではない）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FollowCheck {
    /// 上限の内。もう 1 turn 起こす。
    Retry,
    /// 上限に達した。終端する。
    Exhausted,
    /// 回数を読めない。**起こし直さない**（0 に読み替えない）。
    Unreadable,
}

impl FollowCheck {
    /// 起こし直した回数（replay の導出値・読めない周は `None`）と上限から決める。
    ///
    /// 値 N = **最大 N 回起こし直す**（N+1 回目の衝突で終端する）。
    pub fn judge(retried: Option<u64>, limit: u64) -> Self {
        match retried {
            None => Self::Unreadable,
            Some(found) if found < limit => Self::Retry,
            Some(_) => Self::Exhausted,
        }
    }
}

/// runner の起こし直しを記帳した `detail` か（接頭辞 `rebase-conflict:` か `rebase-stale-rows:`・設計 §34）。
///
/// **読み手はこの 1 本だけ**である——回数の導出（[`retried`]）と `resume` の弁別
/// （`pipe::cli`）が同じ判定を見る。終端の理由（語だけ・`:` 無し）は数えない。
pub(crate) fn is_conflict(detail: &str) -> bool {
    [EXHAUSTED, STALE_ROWS]
        .iter()
        .any(|word| detail.strip_prefix(word).is_some_and(|rest| rest.starts_with(':')))
}

/// 契約表の行の起こし直しを記帳した `detail` か（接頭辞 `rebase-stale-rows:`・「追随」節に行の一覧を載せる周の判定）。
fn is_stale_rows(detail: &str) -> bool {
    detail.strip_prefix(STALE_ROWS).is_some_and(|rest| rest.starts_with(':'))
}

/// 撃ち直しの間に main が動いたことを記帳した `detail` か（接頭辞 `stale:`・設計 §18）。
///
/// [`is_conflict`] とは**別の判定**である——`resume` の弁別（`pipe::cli`）は衝突だけを起こし直しの続きと
/// 読む（stale の便は `Gated` のまま land へ流れる）。回数の合算は [`is_retry`] が 2 本を束ねる。
pub(crate) fn is_stale(detail: &str) -> bool {
    detail.strip_prefix(STALE).is_some_and(|rest| rest.starts_with(':'))
}

/// 起こし直しの回数に数える `detail` か（衝突 ∪ stale・**合算の読み手はこの 1 本**・設計 §18）。
fn is_retry(detail: &str) -> bool {
    is_conflict(detail) || is_stale(detail)
}

/// runner を 1 turn 起こす材料（**通常の起動も起こし直しも同じ形**）。
pub(crate) struct Turn<'a> {
    /// 便 id。
    pub run: &'a str,
    /// 契約の bead id。
    pub bead: &'a str,
    /// 対象 repo。
    pub repo: &'a Path,
    /// 置き場。
    pub state_dir: &'a Path,
    /// 読み込み済みの契約。
    pub contract: &'a Contract,
    /// runner のコマンドと、その turn の口座を選ぶ入力。**無い周は起こせない**（起こし直しには `--runner` が要る）。
    pub runner: Option<Runner<'a>>,
    /// 承認 event が在るか（replay の導出値）。
    pub approved: bool,
    /// lock の待ち方。
    pub policy: LockPolicy,
}

/// runner を起こす材料のうち `--runner` に紐づくもの（設計 account-autonomy.md §4・`s2-07l.285`）。
///
/// 口座の選定の入力（[`Pool`]）は runner と**対**で運ぶ——runner を持たない口（`pipe land` の素の形）は
/// 起こし直さないので選定も要らず、runner を持つ口（`pipe run` / `resume` / `--runner` 付きの `land`）は
/// 宣言を 1 回解いてここへ載せる。`pool` が `None` の周は口座の宣言が 0＝親の環境を継承する。
#[derive(Clone, Copy)]
pub struct Runner<'a> {
    /// runner のコマンド（placeholder を含む）。
    pub cmd: &'a str,
    /// 便用の選定の入力（宣言した口座が 1 つ以上在る周だけ・[`Pool::declared`]）。
    pub pool: Option<&'a Pool>,
}

/// 追随が止まった 1 回分の材料（land の追随が渡す・衝突 [`on_conflict`] と stale [`on_stale`] で同じ形）。
pub(crate) struct Conflict<'a> {
    /// 起こし直しの材料。
    pub turn: Turn<'a>,
    /// 便の記録済み base（stale の周は追随した先＝CAS の old）。
    pub base: &'a str,
    /// 追随の相手（land が読んだ main・stale の周は撃ち直しの後に読み直した main）。
    pub main: &'a str,
    /// 起こし直しの上限（rules 行 `pipe.follow_retries`）。
    pub limit: u64,
}

/// 追随の rebase が衝突した周（設計 §3 の手順 1〜4）。**返り値は必ず land を止める**
/// ——起こし直した turn の後は `gate` から撃ち直す（`land の追随で再び rebase が走る`）。
pub(crate) fn on_conflict(entry: &Conflict<'_>) -> Outcome {
    let worktree = worktree_path(entry.turn.repo, entry.turn.run);
    if !git_ok(&worktree, &["rebase", "--abort"]) {
        return terminate(
            entry,
            ABORT_FAILED,
            RC_BROKEN,
            format!(
                "run {} は rebase の途中のまま（--abort も失敗・base={} main={}）",
                entry.turn.run, entry.base, entry.main
            ),
        );
    }
    let recorded = record(
        &entry.turn,
        Stage::Implemented,
        format!("{EXHAUSTED}:{}..{}", entry.base, entry.main),
    );
    if let Err(reason) = recorded {
        return broken(reason);
    }
    match FollowCheck::judge(retried(entry.turn.state_dir, entry.turn.run), entry.limit) {
        FollowCheck::Unreadable => terminate(
            entry,
            UNMEASURED,
            RC_BROKEN,
            format!("run {} の起こし直しの回数を読めない", entry.turn.run),
        ),
        FollowCheck::Exhausted => terminate(
            entry,
            EXHAUSTED,
            RC_REFUSED,
            format!(
                "run {} の rebase が衝突した（base={} main={}・起こし直しの上限 {} に達した）",
                entry.turn.run, entry.base, entry.main, entry.limit
            ),
        ),
        FollowCheck::Retry => retry(entry, "衝突"),
    }
}

/// 撃ち直しの間に main がさらに動いた周（設計 pipeline.md §5.4 (vi) / §18）。**runner は起こさない**
/// ——同じ land の中で追随し直すのは呼び手（`land`）で、ここは記帳と回数の判定だけを持つ。
///
/// 1. `RunStage stage=Gated detail=stale:<base>..<main>` を 1 件記帳する（衝突と同じ記帳の口
///    [`record`]・段は `Gated` のまま＝列の鍵〔最初の `Gated` の ts〕は動かない・verdict は前の周の PASS）。
/// 2. 回数を衝突と**同じ 1 つの上限**で判定する（[`FollowCheck`]・`rebase-conflict:` と `stale:` の合算）。
///    上限の内は `Ok(())`（呼び手が (iii) から追随し直す）。上限に達した周は `Failed detail=rebase-conflict`
///    + rc 1（既存の終端の型・新しい理由の variant を増やさない）、読めない周は `Failed detail=follow-unmeasured`
///    + rc 2（[`terminate`]・衝突と同じ終端形）。
pub(crate) fn on_stale(entry: &Conflict<'_>) -> Result<(), Outcome> {
    let recorded = record(
        &entry.turn,
        Stage::Gated,
        format!("{STALE}:{}..{}", entry.base, entry.main),
    );
    if let Err(reason) = recorded {
        return Err(broken(reason));
    }
    match FollowCheck::judge(retried(entry.turn.state_dir, entry.turn.run), entry.limit) {
        FollowCheck::Unreadable => Err(terminate(
            entry,
            UNMEASURED,
            RC_BROKEN,
            format!("run {} の追随し直しの回数を読めない", entry.turn.run),
        )),
        FollowCheck::Exhausted => Err(terminate(
            entry,
            EXHAUSTED,
            RC_REFUSED,
            format!(
                "run {} の撃ち直しの間に main が動いた（base={} main={}・追随し直しの上限 {} に達した）",
                entry.turn.run, entry.base, entry.main, entry.limit
            ),
        )),
        FollowCheck::Retry => Ok(()),
    }
}

/// 追随で入った契約表の行が便の消した path を名指す 1 行（設計 pipeline.md §34）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleRow {
    /// 行を持つ設計 doc（repo 相対・`docs/design/<doc>.md`）。
    pub doc: String,
    /// 行 id。
    pub id: String,
    /// 未解決の write-set の項目（契約が書いた字面）。
    pub item: String,
}

/// 便の base と main の関係（**閉じた 3 値**・宣言順・設計 pipeline.md §38・`s2-07l.449`）。
///
/// 読み手は 2 つ——land の追随（`follow_main`）と起こし直しの stdin の「追随」節（[`section`]）——で、**判定は
/// [`Ancestry::judge`] の 1 本**である（片方だけが祖先検査を持つと、`--onto` の周の起こし直しに追随の指示が
/// 渡らない穴が残る）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ancestry {
    /// base が main の祖先（従来の追随の形）。`git rebase --onto <main> <base>` の結果は `git rebase <main>` と同じ。
    Ancestor,
    /// 祖先でないが merge-base が 1 つ在る（main が巻き戻った / 分岐した・base は消えた commit の上に居る）。
    /// 便が base の上に積んだ commit **だけ**を `--onto` で main の上へ運ぶ（merge-base から base までの消えた
    /// commit は運ばない）。値は merge-base の sha（記帳には使わない・新しい base は main）。
    Diverged(String),
    /// merge-base が無い・読めない（無関係な歴史）。追随の形が無い＝`stale base` で断る（fail-closed）。
    Unrelated,
}

impl Ancestry {
    /// `git merge-base --is-ancestor` と `git merge-base` の 2 本で読む（祖先なら後者は撃たない）。
    pub fn judge(repo: &Path, base: &str, main: &str) -> Self {
        if git_ok(repo, &["merge-base", "--is-ancestor", base, main]) {
            return Self::Ancestor;
        }
        match git_line(repo, &["merge-base", base, main]) {
            Some(found) => Self::Diverged(found),
            None => Self::Unrelated,
        }
    }

    /// 追随の形が在るか（`Ancestor` / `Diverged`）。**bool はここ 1 本で enum から導く**（読めない周は偽）。
    pub fn can_follow(&self) -> bool {
        match self {
            Self::Ancestor | Self::Diverged(_) => true,
            Self::Unrelated => false,
        }
    }
}

/// runner の stdin の「追随」節の材料（出所は [`section`] の 1 本・[`super::spawn`] が描く）。
///
/// 追随の相手は **main と便の base の 2 sha**（設計 pipeline.md §38）。runner への指示は祖先の周も `--onto` の周も
/// 同じ `git rebase --onto <main> <base>` の形（経路を 2 本にしない・runner が消えた commit を運ばない）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// 追随の相手（main の sha）。
    pub main: String,
    /// 便の記録済み base（`--onto` の upstream＝この sha から先の commit だけを main の上へ運ぶ）。
    pub base: String,
    /// 便の消した path を名指す行（`rebase-stale-rows:` で起こし直した周だけ・他は空）。
    pub stale: Vec<StaleRow>,
}

/// 追随の rebase の直後に契約表の行が便の消した path を名指すかの判定（**pure**・設計 pipeline.md §34 (2) / (4)）。
///
/// `Some` になるのは、検査を撃てて（`found` が `Some`）findings が 1 件以上在り、**そのすべて**が行 id を持つ write-set の
/// 項目の未解決で、名指された path が便の消した・改名した path（`deleted`）に含まれる周だけ。それ以外の findings が 1 件
/// でも在る周・検査を撃てない周は `None`＝従来どおり再 gate（読めないを「行なし」に読み替えない・NFR4）。
pub(crate) fn stale_rows(found: Option<&[Located]>, deleted: &[String]) -> Option<Vec<StaleRow>> {
    let found = found.filter(|found| !found.is_empty())?;
    found
        .iter()
        .map(|located| {
            let item = located.unresolved.as_deref()?;
            let id = located.id.as_deref()?;
            names_deleted(item, deleted).then(|| StaleRow { doc: located.doc.clone(), id: id.to_owned(), item: item.to_owned() })
        })
        .collect()
}

/// write-set の項目が便の消した path を名指すか（縮む面の `-` は落として読む・末尾 `/` の dir は配下の 1 本で足りる）。
fn names_deleted(item: &str, deleted: &[String]) -> bool {
    let path = item.strip_prefix(SHRINK_FILE).unwrap_or(item);
    match path.strip_suffix('/') {
        Some(dir) => deleted.iter().any(|gone| is_under(gone, dir)),
        None => deleted.iter().any(|gone| gone == path),
    }
}

/// 便の木（rebase 済み）で [`stale_rows`] を測る: 契約表の検査（`contracts check` と同じ 1 本）と、便自身の diff
/// （`git diff --name-status -M <main> HEAD`）の D / R の旧 path。どれかを読めない周は `None`。
///
/// 上限の command は**便の写しの有効値**（run dir の `vessel.toml`・受付が上限と突き合わせて凍結した allowlist）で、
/// 禁じる語列とクラスの語列表（設計 contract-source.md §48 の 5）は埋め込みの manifest の行（land は `--rules` の manifest を
/// 持たない・受付の上限を写しから借りる）。追随で宣言の allowlist が広がった周と語列表の行を読めない周は読めない側（`None`
/// ＝従来どおり再 gate）へ倒れる。
pub(crate) fn stale_rows_in(state_dir: &Path, run: &str, worktree: &Path, main: &str) -> Option<Vec<StaleRow>> {
    let deleted = removed_paths(&super::git_bytes(worktree, &["diff", "--name-status", "-z", "-M", main, "HEAD"])?);
    let frozen = Effective::load(&vessel_path(state_dir, run)).ok()?;
    let embedded = Manifest::embedded().ok()?;
    let (denied, classes) = (list_row(&embedded, DENIED_ROW)?, list_row(&embedded, CLASS_ROW)?);
    let ceiling = Ceiling { row: CEILING_ROW, commands: frozen.allowed(), denied: &denied, classes: &classes };
    let found = repo_findings(worktree, &ceiling);
    stale_rows(found.as_deref(), &deleted)
}

/// rules 行の文字列の列（無い・不発効・型違いは `None`）。
fn list_row(manifest: &Manifest, id: &str) -> Option<Vec<String>> {
    let row = manifest.get(id).filter(|row| row.enabled)?;
    match row.value {
        RuleValue::List(ref found) => Some(found.clone()),
        _ => None,
    }
}

/// `git diff --name-status -z` の出力から消えた path（`D` の path と `R` の旧 path）を取る（**pure**）。
fn removed_paths(bytes: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(bytes);
    let mut fields = text.split('\0').filter(|field| !field.is_empty());
    let mut found = Vec::new();
    while let Some(status) = fields.next() {
        let Some(path) = fields.next() else {
            break;
        };
        match status.chars().next() {
            Some('D') => found.push(path.to_owned()),
            Some('R') => {
                found.push(path.to_owned());
                fields.next();
            }
            Some('C') => {
                fields.next();
            }
            _ => {}
        }
    }
    found
}

/// 追随で入った契約表の行が便の消した path を名指す周（設計 pipeline.md §34 (2) / (3)）。**返り値は必ず land を止める**
/// ——起こし直した turn の後は `gate` から撃ち直す（衝突 [`on_conflict`] と同じ経路・木は rebase 済みのまま）。
///
/// 1. `RunStage stage=Implemented detail=rebase-stale-rows:<base>..<main>` を 1 件記帳する（終端にしない）。
/// 2. 回数を衝突と**同じ 1 つの上限**で判定する。上限に達した周は `Failed detail=rebase-stale-rows` + rc 1、読めない周は
///    `Failed detail=follow-unmeasured` + rc 2。
/// 3. 上限の内は写しの write-set に行を持つ設計 doc を**末尾へ追記**し（既存の項目は動かさない・追記した項目は stderr の
///    1 行に写す）、runner をもう 1 turn 起こす（`--runner` の無い周は記帳と追記を残して rc 1＝resume で続く）。
pub(crate) fn on_stale_rows(entry: &Conflict<'_>, rows: &[StaleRow]) -> Outcome {
    let recorded = record(
        &entry.turn,
        Stage::Implemented,
        format!("{STALE_ROWS}:{}..{}", entry.base, entry.main),
    );
    if let Err(reason) = recorded {
        return broken(reason);
    }
    let named: Vec<String> = rows.iter().map(|row| format!("{}#{} {}", row.doc, row.id, row.item)).collect();
    match FollowCheck::judge(retried(entry.turn.state_dir, entry.turn.run), entry.limit) {
        FollowCheck::Unreadable => terminate(
            entry,
            UNMEASURED,
            RC_BROKEN,
            format!("run {} の起こし直しの回数を読めない", entry.turn.run),
        ),
        FollowCheck::Exhausted => terminate(
            entry,
            STALE_ROWS,
            RC_REFUSED,
            format!(
                "run {} の追随で入った契約表の行が便の消した path を名指す（{}・base={} main={}・起こし直しの上限 {} に達した）",
                entry.turn.run,
                named.join(" / "),
                entry.base,
                entry.main,
                entry.limit
            ),
        ),
        FollowCheck::Retry => {
            let appended = match widen_write_set(&contract_path(entry.turn.state_dir, entry.turn.run), rows) {
                Ok(found) => found,
                Err(reason) => return broken(reason),
            };
            let note = format!(
                "pipe: run {} の追随で入った契約表の行が便の消した path を名指す（{}）・写しの write-set に追記: {}",
                entry.turn.run,
                named.join(" / "),
                if appended.is_empty() { "なし".to_owned() } else { appended.join(", ") }
            );
            let mut outcome = retry(entry, "契約表の行");
            outcome.err.insert(0, note);
            outcome
        }
    }
}

/// 写しの `write-set` の行の末尾へ行を持つ設計 doc を足す（**追記だけ**・既存の項目は順序も字面も不変・既に在る doc は
/// 足さない）。足した項目の列を返す。写しを読めない・`write-set` の行が無い・書けない周は理由。
fn widen_write_set(path: &Path, rows: &[StaleRow]) -> Result<Vec<String>, String> {
    let text = std::fs::read_to_string(path).map_err(|err| format!("{} を読めない: {err}", path.display()))?;
    let mut appended: Vec<String> = Vec::new();
    let mut seen = false;
    let mut out = String::new();
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches('\n');
        let key = body.trim().split_once('=').map(|(key, _)| key.trim());
        if key != Some("write-set") || seen {
            out.push_str(line);
            continue;
        }
        seen = true;
        let Some(open) = body.trim_end().strip_suffix(']') else {
            return Err(format!("{} の write-set の行を読めない", path.display()));
        };
        let mut widened = open.to_owned();
        for row in rows {
            let quoted = format!("\"{}\"", row.doc);
            if body.contains(&quoted) || appended.contains(&row.doc) {
                continue;
            }
            if !widened.trim_end().ends_with('[') {
                widened.push_str(", ");
            }
            widened.push_str(&quoted);
            appended.push(row.doc.clone());
        }
        widened.push(']');
        out.push_str(&widened);
        if line.ends_with('\n') {
            out.push('\n');
        }
    }
    if !seen {
        return Err(format!("{} に write-set の行が無い", path.display()));
    }
    if !appended.is_empty() {
        std::fs::write(path, out).map_err(|err| format!("{} を書けない: {err}", path.display()))?;
    }
    Ok(appended)
}

/// 起こし直した回数 = 同じ run の `rebase-conflict:` / `rebase-stale-rows:` と `stale:` の `RunStage` の行数の**合算** − 1
/// （**いま記帳した分を除く**・設計 §18 / §34＝衝突と契約表の行の起こし直しと stale の追随し直しは 1 つの上限を分け合う）。
///
/// **replay の導出値**で、別の状態 file を持たない（C3・設計 §9 の却下案）。store を
/// 読めない周は `None`＝「0 回起こした」に読み替えない（fail-closed）。
fn retried(state_dir: &Path, run: &str) -> Option<u64> {
    let events = store::read_all(state_dir).ok()?;
    let seen = events
        .iter()
        .filter(|event| {
            event.run == run
                && event.kind == EventKind::RunStage
                && event.detail.as_deref().is_some_and(is_retry)
        })
        .count();
    Some(u64::try_from(seen).unwrap_or(u64::MAX).saturating_sub(1))
}

/// 上限の内の周: runner をもう 1 turn 起こし、**次に撃つ段（gate）を名乗って止まる**。
///
/// 口座は初回の起動と同じ選定（[`spawn_selected`]・設計 account-autonomy.md §4「初回の起動も同じ選定を通す」の
/// 列挙 = 衝突の起こし直し）で選ぶ。待ちの間に便が居るはずの段は、いま記帳した `Implemented`。
/// `cause` は stderr の 1 行が名乗る起こし直しの理由（`衝突` / `契約表の行`）。
fn retry(entry: &Conflict<'_>, cause: &str) -> Outcome {
    let mut outcome = spawn_selected(&entry.turn, Stage::Implemented);
    if outcome.rc != RC_OK {
        return outcome;
    }
    // turn は通った。段が `Implemented` でなければ（runner が commit 無しで落ちた等）
    // その事実は spawn が既に記帳しているので、ここは rc で名乗るだけである。
    let stage = super::current(entry.turn.state_dir)
        .ok()
        .and_then(|state| state.runs.get(entry.turn.run).map(|run| run.stage));
    if stage != Some(Stage::Implemented) {
        outcome.err.push(format!(
            "pipe: run {} の起こし直しは {} で終わった",
            entry.turn.run,
            stage.map_or("読めない", Stage::as_str)
        ));
        outcome.rc = RC_REFUSED;
        return outcome;
    }
    outcome.out.push(format!("run={} next=gate", entry.turn.run));
    outcome.err.push(format!(
        "pipe: run {} の{cause}を runner が解いた（次は gate）",
        entry.turn.run
    ));
    outcome.rc = RC_INCONCLUSIVE;
    outcome
}

/// 便用の選定を通した口座で runner を 1 turn 起こす（設計 account-autonomy.md §4「初回の起動も同じ選定を通す」・
/// FR36）。**初回の起動（Intake の審査後・承認後の `Blocked`・回答後の `Questioned`・`Reviewed` の起こし直し）と
/// 衝突の起こし直しはすべてこの 1 本を通る**——操作役の口座で起きる周を残さない。
///
/// 宣言した口座が 1 つ以上在る周（[`Runner::pool`] が `Some`）は [`choose_account`]（計測 → 便用の規則 → 候補なしの
/// 待ち）で label を選んで [`Account::Chosen`]、0 の周は [`Account::Inherit`]（親の環境を継承・stderr に 1 行）。
/// 候補なしの止まり方（rc 3 `run=<id> next=wait reset=…`）は `RateLimited` の再開と同じ字面。初回の起動は
/// `next=spawn account=` の判定行を持たない（名乗るのは再開だけ）。`expected` は待ちの間に便が居るはずの段。
pub(crate) fn spawn_selected(entry: &Turn<'_>, expected: Stage) -> Outcome {
    let Some(runner) = entry.runner else {
        return missing_runner(entry);
    };
    let mut outcome = Outcome::ok(Vec::new());
    let chosen = match runner.pool {
        None => {
            outcome.err.push("pipe: 口座の宣言が無い＝親の環境を継承".to_owned());
            None
        }
        // 除外は便の repo（`entry.repo`）を anchor に持つ席の口座だけ（設計 account-autonomy.md §14）。
        Some(pool) => match choose_account(pool, entry, expected, &mut outcome) {
            Ok(label) => Some(label),
            // 止まる周も、それまでの判定行（`next=wait …`）と計測の行は残す。
            Err(stopped) => {
                outcome.out.extend(stopped.out);
                outcome.err.extend(stopped.err);
                outcome.rc = stopped.rc;
                return outcome;
            }
        },
    };
    let account = chosen.as_deref().map_or(Account::Inherit, Account::Chosen);
    let turn = spawn_turn(entry, account);
    outcome.out.extend(turn.out);
    outcome.err.extend(turn.err);
    outcome.rc = turn.rc;
    outcome
}

/// `--runner` を持たない周の断り（1 行も書かない・[`spawn_selected`] と [`spawn_turn`] が同じ字面で断る）。
fn missing_runner(entry: &Turn<'_>) -> Outcome {
    refused(format!("run {} の起こし直しに --runner が要る", entry.run))
}

/// runner を 1 turn 起こし、終わったら追随の後始末まで見る。
///
/// **[`super::spawn::spawn`] への呼び手はこの 1 本だけ**である（起動口そのものは spawn で、
/// ここはその唯一の経路＝C6 の形を崩さない）。`--runner` を持たない周は 1 行も書かずに断る
/// ——起こし直しの口（`pipe land` / `pipe resume`）は runner を渡す責務を持つ。
///
/// `account` は runner を起こす口座（閉じた 3 値・設計 account-autonomy.md §4）: 初回の起動と衝突の起こし直しは
/// [`spawn_selected`] が選んだ [`Account::Chosen`]（宣言 0 なら [`Account::Inherit`]）、上限で止まった便の
/// 別口座での起こし直しは [`Account::Resumed`]。
pub(crate) fn spawn_turn(entry: &Turn<'_>, account: Account<'_>) -> Outcome {
    let Some(runner) = entry.runner else {
        return missing_runner(entry);
    };
    let budget = match Precheck::measure(entry.contract, entry.repo) {
        Ok(found) => found.into_budget(),
        Err(reason) => return refused(reason),
    };
    // 回答済みの質問が在る周だけ「回答」節が付く（`Questioned` 以外の段では質問が無く `None`）。
    let answered =
        question_of_run(entry.state_dir, entry.run).filter(|question| question.answer.is_some());
    // 「追随」節の有無は **stdin の組立にだけ**効く。turn の後始末（[`settle`]）は節の有無に
    // 依らず同じ 1 本である（設計 §3 手順 5）。
    let follow = section(entry.state_dir, entry.repo, entry.run);
    // 「途中再開」節も同じく stdin の組立にだけ効く（上限で止まった便と runner が死んだ便だけが持つ）。
    let resumed = resumption(entry.state_dir, entry.repo, entry.run);
    let mut outcome = spawn(
        budget,
        &Launch {
            run: entry.run,
            bead: entry.bead,
            repo: entry.repo,
            state_dir: entry.state_dir,
            contract: entry.contract,
            runner: runner.cmd,
            approved: entry.approved,
            answered,
            follow,
            resumed,
            account,
            policy: entry.policy,
        },
    );
    if outcome.rc != RC_OK {
        return outcome;
    }
    let settled = settle(entry);
    outcome.out.extend(settled.out);
    outcome.err.extend(settled.err);
    if settled.rc != RC_OK {
        outcome.rc = settled.rc;
    }
    outcome
}

/// 便が追随すべき相手（main の sha と便の base）。追随の要らない周は `None`。
///
/// **「便の base が main と違い、追随の形が在る」**（[`Ancestry::can_follow`]＝祖先か merge-base が在る・land の
/// `follow_main` と同じ 1 本の判定）で決める。決めるのは runner の stdin の「追随」節（[`super::spawn`]）
/// **だけ**である——turn の後始末（[`settle`]）はこの値を見ない（節を渡さなかった turn で runner が自ら
/// rebase した周も同じ 1 本で測る）。merge-base の無い周は節を渡さない（land も `stale base` で断る側）。
///
/// 最後の `RunStage` の detail が `rebase-stale-rows:` の周（設計 pipeline.md §34）だけ、便の木で [`stale_rows_in`] を測り直して
/// 行の一覧を節に載せる（replay と木の導出値・別の状態 file を持たない・測れない周は一覧が空）。
pub(crate) fn section(state_dir: &Path, repo: &Path, run: &str) -> Option<Section> {
    // base が無い周も読めない周も**節を渡さない**側へ倒す（追随は base が分かった周だけ書ける）。
    let base = base_of_run(state_dir, run).known()?;
    let main = git_line(repo, &["rev-parse", MAIN_REF])?;
    if main == base || !Ancestry::judge(repo, &base, &main).can_follow() {
        return None;
    }
    let stale = match last_stage_detail(state_dir, run).is_some_and(|detail| is_stale_rows(&detail)) {
        true => stale_rows_in(state_dir, run, &worktree_path(repo, run), &main).unwrap_or_default(),
        false => Vec::new(),
    };
    Some(Section { main, base, stale })
}

/// 便の最後の `RunStage` の detail（無い・読めない周は `None`）。
fn last_stage_detail(state_dir: &Path, run: &str) -> Option<String> {
    let events = store::read_all(state_dir).ok()?;
    events
        .into_iter()
        .rev()
        .find(|event| event.run == run && event.kind == EventKind::RunStage)
        .and_then(|event| event.detail)
}

/// runner が死んだ便に `pipe resume` が記帳する `SeatStopped` の理由（設計 account-autonomy.md §4「runner が
/// 死んだ便の起こし直し」）。書き手は `pipe::cli::resume`、読み手は [`resumption`] の 1 本。
pub(crate) const RUNNER_DEAD: &str = "runner-dead";

/// runner が API に届かず止まった便に `pipe spawn` が記帳する `SeatStopped` の理由（設計 account-autonomy.md §17
/// (2)・[`RUNNER_DEAD`] と同じ形）。書き手は `pipe::spawn`、読み手は [`resumption`] と `pipe::cli::resume`。
pub(crate) const RUNNER_UNREACHABLE: &str = "runner-unreachable";

/// 前の turn が止まった理由（**閉じた 3 値**・設計 account-autonomy.md §4 / §17）。「途中再開」節の理由の行と
/// `Spawned` の detail の印（`resume:rate-limit` / `resume:runner-dead` / `resume:unreachable`）はこの値で分かれる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Halt {
    /// 口座の上限で止まった（`RunStage(RateLimited)`）。別口座で続く。
    RateLimit,
    /// runner の process が消えた（host の再起動・OOM・kill で `SeatStopped` が書かれないまま死んだ形・
    /// `pipe resume` が `SeatStopped detail=runner-dead` を記帳した周）。
    RunnerDead,
    /// runner が API に届かず止まった（`pipe spawn` が `SeatStopped detail=runner-unreachable` を記帳した周）。
    /// 同じ契約で続く。
    Unreachable,
}

impl Halt {
    /// 「途中再開」節の理由の行の述部（`- 前の turn は <ts> ` に続く字面・理由の語を runner が読む）。
    pub fn as_stop_clause(self) -> &'static str {
        match self {
            Self::RateLimit => "に口座の上限で止まった",
            Self::RunnerDead => "に runner の死亡で止まった（process が消えた）",
            Self::Unreachable => "に API に届かず止まった（ネットの断）",
        }
    }

    /// `Spawned` の段で最後の席の event が `SeatStopped` の周に、その detail から理由を読む（閉じた 2 つ）。
    fn of_seat_stop(detail: Option<&str>) -> Option<Self> {
        match detail {
            Some(RUNNER_DEAD) => Some(Self::RunnerDead),
            Some(RUNNER_UNREACHABLE) => Some(Self::Unreachable),
            _ => None,
        }
    }
}

/// 「途中再開」節の材料（設計 account-autonomy.md §4）: 止まった理由と時刻・base からの commit の一覧・前の turn が
/// worktree に残した未 commit の変更の一覧。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resumption {
    /// 止まった理由。
    pub halt: Halt,
    /// 止まった時刻（上限なら `RunStage(RateLimited)`・runner の死亡 / 到達不能なら `SeatStopped` の理由つきの event の `ts`）。
    pub stopped_at: String,
    /// base から便が積んだ commit（`git log --oneline <base>..HEAD` の行・古い順）。
    pub commits: Vec<String>,
    /// 未 commit の変更（`git status --porcelain` の行・消さない＝runner が続きから commit する・N1）。
    pub uncommitted: Vec<String>,
}

/// 便が途中再開すべきか（[`section`] と同型の組み立て）。`Some` になるのは 2 つの周だけ——`RateLimited` の段に
/// 在る周（[`Halt::RateLimit`]）と、`Spawned` の段で最後の席の event が `SeatStopped detail=runner-dead` の周
/// （[`Halt::RunnerDead`]・`pipe resume` が生死を測って記帳した後）か `SeatStopped detail=runner-unreachable` の周
/// （[`Halt::Unreachable`]・`pipe spawn` が runner の rc で記帳した後・設計 account-autonomy.md §17 (3)）。
///
/// 決めるのは runner の stdin の「途中再開」節（[`super::spawn`]）**だけ**である。止まった時刻は
/// 追記だけの log の原本から読む（最後の該当の行・replay の `updated` は後の event で動く）。
/// commit / 未 commit の一覧を読めない周は空（節は付く・一覧だけ無い）。どちらの理由でも一覧の読み手は同じ 1 本。
pub(crate) fn resumption(state_dir: &Path, repo: &Path, run: &str) -> Option<Resumption> {
    let events = store::read_all(state_dir).ok()?;
    let stage = crate::fleet::replay(&events).runs.get(run).map(|found| found.stage);
    let mut own = events.iter().rev().filter(|event| event.run == run);
    let (halt, stopped_at) = if stage == Some(Stage::RateLimited) {
        let stopped = own.find(|event| event.kind == EventKind::RunStage && event.stage == Some(Stage::RateLimited))?;
        (Halt::RateLimit, stopped.ts.clone())
    } else if stage == Some(Stage::Spawned) {
        let last = own.find(|event| event.kind == EventKind::SeatSpawned || event.kind == EventKind::SeatStopped)?;
        let seat_stopped = last.kind == EventKind::SeatStopped;
        let halt = seat_stopped.then(|| Halt::of_seat_stop(last.detail.as_deref())).flatten()?;
        (halt, last.ts.clone())
    } else {
        return None;
    };
    let worktree = worktree_path(repo, run);
    // base が無い周も読めない周も**commit を数えない**側へ倒す（質問 record の材料は既定で空）。
    let commits = base_of_run(state_dir, run)
        .known()
        .map(|base| git_lines(&worktree, &["log", "--oneline", "--reverse", &format!("{base}..HEAD")]))
        .unwrap_or_default();
    let uncommitted = git_lines(&worktree, &["status", "--porcelain"]);
    Some(Resumption { halt, stopped_at, commits, uncommitted })
}

/// git を 1 回撃って stdout の非空行を得る（失敗は空・[`super::git_bytes`] の上で行に切る）。
fn git_lines(dir: &Path, args: &[&str]) -> Vec<String> {
    super::git_bytes(dir, args)
        .map(|bytes| {
            String::from_utf8_lossy(&bytes)
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// **すべての turn** の後始末（設計 §3 の手順 5 / 6）。「追随」節を渡したかは見ない。
///
/// 1. 木が rebase の途中（`rebase-merge` / `rebase-apply` 在り）で終わった周は
///    `Failed detail=rebase-dirty` で終端する（clean 前提を守る・fail-closed）。
/// 2. **器が base を進める**: worktree の HEAD と main の merge-base を実測し、記録済みの
///    base より進んでいれば `RunStage stage=Implemented detail=rebase:<old>..<merge-base>` を
///    記帳する。**書く値は実測した merge-base で、現在の main ではない**——turn の間に main が
///    さらに進んでいても、2 点 diff に main の新しい commit の逆向きが載る穴を作らない。
///    節を渡していない turn で runner が頼まれずに `git rebase` を撃った周も**同じ 1 本**で測る
///    ——節の有無で経路を分けると、記帳の無い便の gate が古い base の 2 点 diff を測り、
///    main 側の commit を write-set の外と誤る（.203 の実測）。
///
/// 2 は **turn が `Implemented` で終わった周だけ**である。`Failed` / `RateLimited` で終わった
/// turn の後に `Implemented` を記帳すると、段が静かに `Implemented` へ戻る（終端した便が
/// gate へ進む・止まった便が再開されない）。段を読めない周も記帳しない（fail-closed）。
fn settle(entry: &Turn<'_>) -> Outcome {
    let worktree = worktree_path(entry.repo, entry.run);
    if mid_rebase(&worktree) {
        let recorded = record(entry, Stage::Failed, DIRTY.to_owned());
        return match recorded {
            Err(reason) => broken(reason),
            Ok(()) => refused(format!(
                "run {} の worktree が rebase の途中で turn が終わった",
                entry.run
            )),
        };
    }
    if !ended_implemented(entry) {
        return Outcome::ok(Vec::new());
    }
    let Some((old, advanced)) = advanced(entry) else {
        return Outcome::ok(Vec::new());
    };
    match record(entry, Stage::Implemented, format!("rebase:{old}..{advanced}")) {
        Err(reason) => broken(reason),
        Ok(()) => Outcome::ok_line(format!("run={} rebase={old}..{advanced}", entry.run)),
    }
}

/// turn が `Implemented` で終わったか（replay の段・読めない周は `false`＝base を進めない）。
fn ended_implemented(entry: &Turn<'_>) -> bool {
    super::current(entry.state_dir)
        .ok()
        .and_then(|state| state.runs.get(entry.run).map(|run| run.stage))
        == Some(Stage::Implemented)
}

/// 木が rebase の途中か。**状態を読めない周は「途中」側へ倒す**（fail-closed）。
fn mid_rebase(worktree: &Path) -> bool {
    let Some(dir) = git_line(worktree, &["rev-parse", "--absolute-git-dir"]) else {
        return true;
    };
    REBASE_DIRS
        .iter()
        .any(|name| Path::new(&dir).join(name).exists())
}

/// 記録済みの base と、turn の後に実測した merge-base（**進んでいる周だけ** `Some`）。
///
/// 進んだと読むのは 2 つの形（[`Ancestry`] の同じ 1 本で弁別する）: 記録済みの base が実測の merge-base の祖先
/// （従来の追随）と、祖先でないが merge-base が在り、実測の merge-base が記録済みの base の手前**ではない**周
/// （`--onto` で運ばれた木・設計 pipeline.md §38＝実測の merge-base が記録済みの base の祖先なら、木は
/// まだ運ばれておらず base を手前へ戻す形なので進んでいない・却下案「merge-base を新しい base として記帳する」）。
fn advanced(entry: &Turn<'_>) -> Option<(String, String)> {
    // base が無い周も読めない周も**進んでいないと読む**側へ倒す（追随の記帳を増やさない）。
    let old = base_of_run(entry.state_dir, entry.run).known()?;
    let head = git_line(&worktree_path(entry.repo, entry.run), &["rev-parse", "HEAD"])?;
    let main = git_line(entry.repo, &["rev-parse", MAIN_REF])?;
    let merged = git_line(entry.repo, &["merge-base", &head, &main])?;
    if merged == old {
        return None;
    }
    match Ancestry::judge(entry.repo, &old, &merged) {
        Ancestry::Ancestor => Some((old, merged)),
        Ancestry::Diverged(fork) if fork != merged => Some((old, merged)),
        Ancestry::Diverged(_) | Ancestry::Unrelated => None,
    }
}

/// 終端の 1 件を記帳して断る（**終端形は [`FollowCheck`] の variant ごとに固定**・設計 §5）。
fn terminate(entry: &Conflict<'_>, detail: &str, rc: u8, reason: String) -> Outcome {
    match record(&entry.turn, Stage::Failed, detail.to_owned()) {
        Err(broke) => broken(broke),
        Ok(()) => Outcome::failed_line(rc, format!("pipe: {reason}・main は動かさない")),
    }
}

/// 段を 1 件記帳する。
fn record(entry: &Turn<'_>, stage: Stage, detail: String) -> Result<(), String> {
    emit(
        entry.state_dir,
        &Emit {
            kind: EventKind::RunStage,
            run: entry.run,
            bead: entry.bead,
            stage: Some(stage),
            seat: None,
            pid: None,
            detail: Some(detail),
        },
        entry.policy,
    )
    .map_err(|err| err.to_string())
}

/// 前提違反・使い方の誤り（rc 1 + stderr 1 行・何もしない）。
fn refused(reason: String) -> Outcome {
    Outcome::failed_line(RC_REFUSED, format!("pipe: {reason}"))
}

/// 対象そのものが壊れている（rc 2）。
fn broken(reason: String) -> Outcome {
    Outcome::failed_line(RC_BROKEN, format!("pipe: {reason}"))
}

#[cfg(test)]
mod tests {
    use super::{
        is_conflict, is_stale, removed_paths, retried, spawn_turn, stale_rows, widen_write_set, Ancestry, FollowCheck, Located,
        Runner, StaleRow, Turn, DIRTY, EXHAUSTED, STALE, STALE_ROWS,
    };
    use crate::cli_outcome::{RC_OK, RC_REFUSED};
    use crate::fleet::store::{self, LockPolicy};
    use crate::fleet::{EventKind, Stage};
    use crate::pipe::approve::RC_BLOCKED;
    use crate::pipe::fixture::{append_all, contract, event, scratch};
    use crate::pipe::spawn::Account;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    /// git を 1 回撃つ（失敗は読み手の assert が落とす）。
    fn git(dir: &Path, args: &[&str]) {
        let _ = Command::new("git").arg("-C").arg(dir).args(args).output();
    }

    /// commit を 1 つ持つ tmp の git repo（`<root>/repo`）と置き場（`<root>/state`）。
    fn repo_with_state(name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let root = scratch(name);
        let (repo, state) = (root.join("repo"), root.join("state"));
        let _ = std::fs::create_dir_all(&repo);
        let _ = std::fs::create_dir_all(&state);
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.name", "mutant"]);
        git(&repo, &["config", "user.email", "mutant@example.invalid"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "seed"]);
        (root, repo, state)
    }

    /// 置き場の `RunStage` の段と detail（物理順）。
    fn stages(state: &Path, run: &str) -> Vec<(Option<Stage>, Option<String>)> {
        store::read_all(state)
            .unwrap_or_default()
            .into_iter()
            .filter(|event| event.run == run && event.stage.is_some())
            .map(|event| (event.stage, event.detail))
            .collect()
    }

    // flip-check: retroactive s2-07l.222
    /// `spawn_turn` の 2 つの `!=` を、spawn の段が turn の後始末の段と一致する周と違う周の 2 fixture で撃つ。
    ///
    /// 1. spawn が起こさずに断った周（承認の関門で `Blocked`・rc 3）は、その rc のまま返り後始末を撃たない
    ///    （1 つ目の `!=` を `==` にすると、無い worktree を「rebase の途中」と読んで `Failed rebase-dirty` を足す）。
    /// 2. spawn は通った（`Implemented`・rc 0）が runner が木を rebase の途中で残した周は、後始末の rc 1 と
    ///    `Failed rebase-dirty` が勝つ（2 つ目の `!=` を `==` にすると rc 0 のまま返る・1 つ目を `==` にすると
    ///    後始末を撃たずに返る）。
    #[test]
    fn mutant_in_pipe_spawn_turn_returns_spawn_or_settle_rc() {
        let (root, repo, state) = repo_with_state("spawn-turn");
        let policy = LockPolicy::embedded().expect("埋め込みの lock 規則を読める");

        let gated = contract(&["src/lib.rs"], &["C9"]);
        let blocked = spawn_turn(
            &Turn {
                run: "blocked",
                bead: "s2-mutant",
                repo: &repo,
                state_dir: &state,
                contract: &gated,
                runner: Some(Runner { cmd: "true", pool: None }),
                approved: false,
                policy,
            },
            Account::Inherit,
        );
        assert_eq!(blocked.rc, RC_BLOCKED, "承認の関門の rc のまま: {:?}", blocked.err);
        assert_eq!(stages(&state, "blocked"), vec![(Some(Stage::Blocked), Some("C9".to_owned()))], "後始末の段を足さない");

        let open = contract(&["src/lib.rs"], &[]);
        let runner = "git commit -q --allow-empty -m runner && mkdir \"$(git rev-parse --absolute-git-dir)/rebase-merge\"";
        let dirty = spawn_turn(
            &Turn {
                run: "dirty",
                bead: "s2-mutant",
                repo: &repo,
                state_dir: &state,
                contract: &open,
                runner: Some(Runner { cmd: runner, pool: None }),
                approved: false,
                policy,
            },
            Account::Inherit,
        );
        assert_ne!(dirty.rc, RC_OK, "後始末の rc が勝つ: {:?}", dirty.err);
        assert_eq!(dirty.rc, RC_REFUSED, "rebase の途中は rc 1: {:?}", dirty.err);
        let trail = stages(&state, "dirty");
        assert!(trail.contains(&(Some(Stage::Implemented), None)), "spawn は Implemented で終わった: {trail:?}");
        assert_eq!(trail.last(), Some(&(Some(Stage::Failed), Some(DIRTY.to_owned()))), "後始末の終端: {trail:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 祖先検査は閉じた 3 値（設計 pipeline.md §38）: 祖先／祖先でないが merge-base が在る（値は merge-base）／merge-base が
    /// 無い（親を持たない commit・読めない sha）。追随の形が在るのは前 2 者だけで、`Unrelated` は偽（fail-closed）。
    #[test]
    fn pipe_land_onto_ancestry_is_a_closed_three_value() {
        let (root, repo, _state) = repo_with_state("ancestry");
        let seed = super::git_line(&repo, &["rev-parse", "HEAD"]).unwrap_or_default();
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "base"]);
        let base = super::git_line(&repo, &["rev-parse", "HEAD"]).unwrap_or_default();
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "ahead"]);
        let ahead = super::git_line(&repo, &["rev-parse", "HEAD"]).unwrap_or_default();
        assert_eq!(Ancestry::judge(&repo, &base, &ahead), Ancestry::Ancestor, "base は ahead の祖先");
        assert_eq!(Ancestry::judge(&repo, &base, &base), Ancestry::Ancestor, "同じ sha は祖先（要否は呼び手が先に見る）");
        // seed から分岐した commit: base は祖先でなく merge-base は seed。
        git(&repo, &["checkout", "-q", "-b", "side", &seed]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "side"]);
        let side = super::git_line(&repo, &["rev-parse", "HEAD"]).unwrap_or_default();
        assert_eq!(Ancestry::judge(&repo, &base, &side), Ancestry::Diverged(seed.clone()), "分岐は merge-base を持つ");
        assert_eq!(Ancestry::judge(&repo, &ahead, &seed), Ancestry::Diverged(seed.clone()), "巻き戻りの merge-base は main 自身");
        // 親を持たない commit: merge-base が無い。読めない sha も同じ側。
        let tree = super::git_line(&repo, &["rev-parse", "HEAD^{tree}"]).unwrap_or_default();
        let orphan = super::git_line(&repo, &["commit-tree", &tree, "-m", "orphan"]).unwrap_or_default();
        assert_eq!(Ancestry::judge(&repo, &base, &orphan), Ancestry::Unrelated, "無関係な歴史");
        assert_eq!(Ancestry::judge(&repo, &base, "0000000000000000000000000000000000000000"), Ancestry::Unrelated, "読めない sha");
        assert!(Ancestry::Ancestor.can_follow() && Ancestry::Diverged(seed).can_follow() && !Ancestry::Unrelated.can_follow());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 上限 N は「**最大 N 回起こし直す**」である（N 回目までは `Retry`・N 回目からは
    /// `Exhausted`）。回数を読めない周は `Unreadable` で、**0 回に読み替えない**。
    #[test]
    fn pipe_follow_judge_pins_the_boundary_of_the_limit() {
        for (retried, limit, want) in [
            (0_u64, 2_u64, FollowCheck::Retry),
            (1, 2, FollowCheck::Retry),
            (2, 2, FollowCheck::Exhausted),
            (3, 2, FollowCheck::Exhausted),
            (0, 0, FollowCheck::Exhausted),
            (0, 1, FollowCheck::Retry),
            (1, 1, FollowCheck::Exhausted),
        ] {
            assert_eq!(
                FollowCheck::judge(Some(retried), limit),
                want,
                "起こし直し {retried} 回・上限 {limit}"
            );
        }
        assert_eq!(
            FollowCheck::judge(None, 2),
            FollowCheck::Unreadable,
            "読めない回数は Unreadable（上限の内へ倒さない）"
        );
        assert_eq!(
            FollowCheck::judge(None, 0),
            FollowCheck::Unreadable,
            "上限 0 でも「読めない」は別の値である"
        );
    }

    /// 衝突の記帳（`rebase-conflict:<base>..<main>`）と終端の理由（`rebase-conflict`）を
    /// **同じ判定で混ぜない**。retire と resume はこの弁別の上に乗っている。
    #[test]
    fn pipe_follow_conflict_detail_is_distinguished_from_the_terminal_reason() {
        assert!(is_conflict("rebase-conflict:abc..def"), "記帳した衝突");
        assert!(is_conflict(&format!("{EXHAUSTED}:a..b")), "接頭辞は 1 本から組む");
        assert!(!is_conflict(EXHAUSTED), "終端の理由は衝突の記帳ではない");
        assert!(!is_conflict("rebase-empty"), "同一変更の終端は別の理由");
        assert!(!is_conflict("rebase:abc..def"), "追随の記帳は別の理由");
        assert!(!is_conflict("rebase-conflicted:x"), "接頭辞は `:` まで見る");
    }

    /// stale の記帳（`stale:<base>..<main>`）は衝突の記帳と**別の判定**である（設計 §18）: `resume` は衝突だけを
    /// 起こし直しの続きと読み、stale の便は `Gated` のまま land へ流れる。接頭辞は `:` まで見る（`stale-x:` は違う）。
    #[test]
    fn follow_stale_detail_is_distinguished_from_the_conflict_detail() {
        assert!(is_stale("stale:abc..def"), "stale の記帳");
        assert!(is_stale(&format!("{STALE}:a..b")), "接頭辞は 1 本から組む");
        assert!(!is_stale(STALE), "語だけでは記帳ではない");
        assert!(!is_stale("stale-rows:a..b"), "接頭辞は `:` まで見る");
        assert!(!is_stale("rebase-conflict:abc..def"), "衝突の記帳は stale ではない");
        assert!(!is_conflict("stale:abc..def"), "stale の記帳は衝突ではない（resume の弁別を動かさない）");
        assert!(!is_stale("rebase:abc..def"), "追随の記帳は別の理由");
    }

    /// 回数は `rebase-conflict:` と `stale:` の行を**1 つに合算**する（設計 §18・いま記帳した分を除く −1）。
    /// 衝突 1 件 + stale 1 件 = 起こし直し 1 回＝上限 1 で `Exhausted`（別々に数えると両方 0 回で `Retry` に化ける）。
    /// 他の便の行・`RunStage` 以外の行・終端の理由（`rebase-conflict` の語だけ）は数えない。
    #[test]
    fn follow_stale_rows_add_to_the_conflict_count_under_one_limit() {
        let state = scratch("follow-stale-count");
        append_all(
            &state,
            &[
                event("me", EventKind::RunStage, Some(Stage::Implemented), None, Some("rebase-conflict:a..b")),
                event("me", EventKind::RunStage, Some(Stage::Gated), None, Some("verdict:PASS")),
                event("me", EventKind::RunStage, Some(Stage::Gated), None, Some("stale:b..c")),
                // 数えない側: 別の便・段でない kind・終端の理由の語だけ。
                event("other", EventKind::RunStage, Some(Stage::Gated), None, Some("stale:b..c")),
                event("me", EventKind::RunDone, Some(Stage::Failed), None, Some("stale:x..y")),
                event("me", EventKind::RunStage, Some(Stage::Failed), None, Some(EXHAUSTED)),
            ],
        );
        assert_eq!(retried(&state, "me"), Some(1), "衝突 1 + stale 1 − 1 = 1 回");
        assert_eq!(FollowCheck::judge(retried(&state, "me"), 1), FollowCheck::Exhausted, "上限 1 は合算で尽きる");
        assert_eq!(FollowCheck::judge(retried(&state, "me"), 2), FollowCheck::Retry, "上限 2 なら残り 1 回");
        assert_eq!(retried(&state, "other"), Some(0), "別の便は自分の 1 件だけ（−1 で 0）");
        assert_eq!(retried(&state, "absent"), Some(0), "行の無い便は 0（読めないではない）");
        let _ = std::fs::remove_dir_all(&state);
    }

    /// 1 件の findings（doc・行 id・未解決の項目）。
    fn located(id: Option<&str>, unresolved: Option<&str>) -> Located {
        Located { doc: "docs/design/other.md".to_owned(), id: id.map(str::to_owned), unresolved: unresolved.map(str::to_owned) }
    }

    /// 契約表の行の起こし直しは衝突と**同じ判定の 1 本**（[`is_conflict`]）に載り、回数も 1 つの上限に合算する（設計 §34）。
    /// 終端の理由（語だけ）は記帳ではない。
    #[test]
    fn pipe_follow_stale_rows_restarts_count_under_the_conflict_predicate() {
        assert!(is_conflict(&format!("{STALE_ROWS}:a..b")), "契約表の行の記帳は起こし直しの続き");
        assert!(!is_conflict(STALE_ROWS), "終端の理由は記帳ではない");
        assert!(!is_conflict("rebase-stale-rowsx:a..b"), "接頭辞は `:` まで見る");
        assert!(!is_stale(&format!("{STALE_ROWS}:a..b")), "stale（main が動いた周）とは別");
        let state = scratch("follow-stale-rows-count");
        append_all(
            &state,
            &[
                event("me", EventKind::RunStage, Some(Stage::Implemented), None, Some("rebase-conflict:a..b")),
                event("me", EventKind::RunStage, Some(Stage::Implemented), None, Some("rebase-stale-rows:b..c")),
                event("me", EventKind::RunStage, Some(Stage::Failed), None, Some(STALE_ROWS)),
            ],
        );
        assert_eq!(retried(&state, "me"), Some(1), "衝突 1 + 契約表の行 1 − 1 = 1 回");
        let _ = std::fs::remove_dir_all(&state);
    }

    /// 起こし直すのは findings の**すべて**が行 id を持つ未解決の項目で、便の消した path を名指す周だけ（設計 §34 (2) / (4)）。
    /// 便と無関係の path・他の理由の findings が 1 件でも在る周・findings 0・検査を撃てない周（`None`）は起こし直さない。
    #[test]
    fn pipe_follow_stale_rows_unrelated_findings_and_unreadable_check_do_not_restart() {
        let deleted = vec!["src/gone.rs".to_owned(), "src/old/a.rs".to_owned()];
        let stale = located(Some("z"), Some("src/gone.rs"));
        let want = StaleRow { doc: "docs/design/other.md".to_owned(), id: "z".to_owned(), item: "src/gone.rs".to_owned() };
        assert_eq!(stale_rows(Some(std::slice::from_ref(&stale)), &deleted), Some(vec![want]), "便の消した path を名指す行");
        let shrink = located(Some("y"), Some("-src/gone.rs"));
        let dir = located(Some("x"), Some("src/old/"));
        assert_eq!(stale_rows(Some(&[shrink, dir]), &deleted).map(|rows| rows.len()), Some(2), "`-` と dir の項目も読む");
        let unrelated = located(Some("w"), Some("src/never.rs"));
        assert_eq!(stale_rows(Some(std::slice::from_ref(&unrelated)), &deleted), None, "便と無関係の path");
        assert_eq!(stale_rows(Some(&[stale.clone(), unrelated]), &deleted), None, "1 件でも無関係なら従来どおり");
        let other_reason = located(Some("v"), None);
        assert_eq!(stale_rows(Some(&[stale.clone(), other_reason]), &deleted), None, "他の理由の findings が在る");
        let whole_doc = located(None, Some("src/gone.rs"));
        assert_eq!(stale_rows(Some(&[whole_doc]), &deleted), None, "行 id の無い 1 件");
        assert_eq!(stale_rows(Some(&[]), &deleted), None, "findings 0 は従来どおり");
        assert_eq!(stale_rows(None, &deleted), None, "検査を撃てない周は「行なし」に読み替えず従来どおり");
        assert_eq!(stale_rows(Some(&[stale]), &[]), None, "便が何も消していない");
    }

    /// 便の消した path は `D` の path と `R` の旧 path（`C` の元・`M` / `A` は数えない）。
    #[test]
    fn pipe_follow_stale_rows_restarts_read_removed_paths_from_name_status() {
        let bytes = b"M\0src/lib.rs\0D\0src/gone.rs\0R100\0src/old.rs\0src/new.rs\0C75\0src/a.rs\0src/b.rs\0A\0src/c.rs\0";
        assert_eq!(removed_paths(bytes), vec!["src/gone.rs".to_owned(), "src/old.rs".to_owned()]);
        assert_eq!(removed_paths(b""), Vec::<String>::new());
    }

    /// 写しの write-set への追記は**末尾に**足すだけ（既存の項目と他の行は逐語・同じ doc を 2 度足さない・空の列にも足せる）。
    #[test]
    fn pipe_follow_stale_rows_restarts_widen_appends_only_at_the_end() {
        let root = scratch("follow-widen");
        let _ = std::fs::create_dir_all(&root);
        let path = root.join("contract.toml");
        let row = |doc: &str| StaleRow { doc: doc.to_owned(), id: "z".to_owned(), item: "src/gone.rs".to_owned() };
        let _ = std::fs::write(&path, "goal = \"g\"\nwrite-set = [\"src/lib.rs\", \"~src/gone.rs\"]\nverify = [\"v\"]\n");
        let rows = [row("docs/design/other.md"), row("docs/design/other.md")];
        assert_eq!(widen_write_set(&path, &rows), Ok(vec!["docs/design/other.md".to_owned()]), "同じ doc は 1 回");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap_or_default(),
            "goal = \"g\"\nwrite-set = [\"src/lib.rs\", \"~src/gone.rs\", \"docs/design/other.md\"]\nverify = [\"v\"]\n"
        );
        assert_eq!(widen_write_set(&path, &rows), Ok(Vec::new()), "既に在る doc は足さない");
        let _ = std::fs::write(&path, "write-set = []\n");
        assert_eq!(widen_write_set(&path, &[row("docs/design/a.md")]), Ok(vec!["docs/design/a.md".to_owned()]));
        assert_eq!(std::fs::read_to_string(&path).unwrap_or_default(), "write-set = [\"docs/design/a.md\"]\n");
        let _ = std::fs::write(&path, "goal = \"g\"\n");
        assert!(widen_write_set(&path, &[row("docs/design/a.md")]).is_err(), "write-set の行が無い写しは断る");
        let _ = std::fs::remove_dir_all(&root);
    }
}
