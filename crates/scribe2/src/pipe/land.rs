//! land（設計 docs/design/pipeline.md §5.4・FR10 / FR11 / FR12・憲法 N1）。
//!
//! **PASS が無ければ 1 byte も動かさない**。前提は Gated ∧ `verdict.json` が PASS ∧
//! `refs/heads/main` が便の記録した base と同一（CAS の old）で、どれか 1 つでも欠けたら
//! rc 1 で何もしない。
//!
//! main を進めた後の実測が赤でも **auto revert はしない**（MVP 外）。main は進んだまま
//! `Failed detail=main-red` を残して loud に落ちる——黙って巻き戻すと「何が起きたか」が
//! 履歴から消え、赤い main が緑に見える瞬間が生まれるためである。
//!
//! 後始末は **可逆な move**（N1.2）。worktree は `retired/<run>` へ移すだけで削除せず、
//! branch も消さない（squash commit は branch の祖先でないので `-d` は通らず、`-D` は
//! N1 が禁じる形である）。
//!
//! main を進めた後は **anchor（`--repo` の checkout）の index と working tree も新 main に揃える**
//! （`s2-07l.120`・N1）。`update-ref` は ref しか動かさないので、揃えないと anchor の `git status` に
//! landed 変更が staged の逆向きで残り、次の `commit -a` が landed 変更を打ち消す（`.117` 実測）。
//! 揃えるのは HEAD が main を指し tracked な未 commit の変更が無く、landed tree が足す path が anchor に
//! 無い周だけ（[`AnchorPlan`]・fail-closed）。**main の実測が赤 / 測れない周も揃える**（ref は既に
//! 進んでいる＝揃えないと同じ経路が failure exit で開く・lens-120 H1）。
//!
//! **main が動いた便は追随する**（`s2-07l.119`・FR30）。記録した base が main の祖先のまま
//! 置き去りになった周は、worktree の branch を main へ rebase し（効くのは branch だけ・
//! main は 1 byte も動かさない・force 系は使わない）、段を `Implemented` へ戻して gate を
//! **同じ関数で**撃ち直し、PASS なら新しい base で CAS する。衝突は木を戻して**実装役を
//! 起こし直す**（[`super::follow`]・便は終端にしない・終端するのは上限に達した周だけ）。
//! base が main の祖先でなくても merge-base が在れば同じ経路で追随する（`s2-07l.449`・設計 §38）:
//! rebase は `--onto <main> <base>` の 1 形で、便が base の上に積んだ commit だけを運ぶ。merge-base の
//! 無い周だけ `stale base` で断る。
//!
//! **撃ち直しの間に main がさらに動いた周は同じ land の中で追随し直す**（`s2-07l.335`・設計 §5.4 (vi) /
//! §18・FR30）。`RunStage stage=Gated detail=stale:<old>..<now>` を衝突と同じ記帳の口で記し、回数は衝突と
//! **1 つの上限**（`pipe.follow_retries`）で合算して、上限に達した周だけ `Failed detail=rebase-conflict` で
//! 終端する。runner は要らず、撃つ主体が席に残らない（[`Attempt`]・周回は [`land`] の中に閉じる）。
//!
//! **着地は gate 済みの便を先に通す**（`s2-07l.147`・設計 gate-cost.md §6）。前提検査の直後・追随の
//! 前に、同じ置き場の着地待ちの列（event log の replay から導く・別の状態 file を持たない）を見て、
//! 自分より前の便が居る間は待つ。待ちは完了 enum の variant 1 つ（[`crate::fleet::Completion::LandTurn`]）で唯一の
//! wait 実装を通り、上限（rules 行 `pipe.land_wait_s`）を超えた周と列を導けない周は**待たずに進む**
//! （断らない・止めない）。どの周だったかは land の record と stdout の `order=` が残す。
//!
//! **検出線（変異検査）は gate・主実測・候補の木のどれも撃たない**（着地後の検出の口 [`detection`] だけ・設計
//! gate-cost.md §44 形 (9)）。追随は `<base>..<main>` の path を [`DETECTION_SCOPE`] と照らし、1 つも触れない周は
//! 再 gate を丸ごと省いて前周の PASS を引き継ぐ（[`regate_skippable`]・設計 §33・形 (10)）。読めない周は撃ち直す
//! （fail-closed）。
//!
//! **既に main に自分の squash が在る便は Landed で終端する**（`s2-07l.389`・設計 §29・FR50・C3 / C10）。追随の
//! rebase で commit が 0 本になった周、`rebase-empty` に倒す前に main の log を便の trailer（`run: <run id>`）で
//! 1 回だけ探す。在れば前の周が CAS の後・実測の前に死んだ形＝squash と CAS を撃たず、主実測はその sha に対して
//! 従来どおり撃ち（記録が無いものを緑と読まない）、緑なら `already-landed` の印を持って Landed にする。無ければ
//! 本当に空の便として従来どおり `rebase-empty`。

use crate::polarity::{OnFailure, Polarity, Timing};
use super::contract::Contract;
use super::follow::{self, Conflict};
use super::gate::{gate, next_number, skip_record, Gate, Limits, Skipped, Verdict};
use super::lens_record::LensSource;
use super::{emit, git_bytes, git_line, git_ok, worktree_path, Emit};
// 子 module（[`verify`] / [`finish`]）が `super::` で呼ぶ 5 本。子から見た `super::` は `land` なので、親が同じ名を
// 持たないと**移した本文の path を書き換える**ことになり、純移動の機械証明（設計 §5.3）の (名, 本文の hash)
// が動く。親自身は従来どおり `super::` で `pipe` の側を呼ぶ（本体は 1 byte も変えていない）。
use super::{base_of_run, branch_name, declaration, verify_log_path, vessel_path};
use super::queue::{await_turn, Order, Turned};
use super::retire::verdict_field;
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_OK, RC_REFUSED};
use crate::fleet::store::{self, append_line, LockPolicy};
use crate::fleet::{EventKind, Stage};
use std::path::{Path, PathBuf};

/// 主実測の群（設計 §41・`s2-07l.457` の純移動）。`land` が呼ぶ 3 本だけを借りる（`measure_main` は [`finish`] の側）。
mod verify;

use verify::{main_red, main_unmeasured, verify_main};

/// squash と finish の群（設計 §43・`s2-07l.498` の純移動）。`land` / `attempt` が呼ぶ 3 本を借り、`pipe` の中で
/// `land::` として引かれる 2 本を同じ可視性で再輸出する（歯だけが読む名は `mod tests` の直前の `use`）。
mod finish;

use finish::{finish, open_pr, squash};
pub(in crate::pipe) use finish::{land_train, landed_sha, terminal, Car};

/// 着地後の検出の口（`pipe land --detection-only`・設計 gate-cost.md §44 行 ak）。
pub(in crate::pipe) mod detection;

pub(crate) use super::queue::turn_now;
pub use super::queue::{turn_in, Queued, Turn};
pub use super::retire::{retire, retired_path, Retire};

/// 進める ref。設計 §5.4 が名指す 1 本である（追随の相手を読む [`super::follow`] も同じ字面を使う）。
pub(crate) const MAIN_REF: &str = "refs/heads/main";

/// 面 5 の export 先の file 名（ADR-0004 §2.2・**版番号に依らず固定**）。
const VERDICTS_FILE: &str = "verdicts.jsonl";

/// 追随の rebase で便の commit が 0 本になった周の終端の理由（`Failed` の `detail`）。
///
/// **書き手（[`rebase_onto`]）と読み手（`pipe retire` の入口）で字面を 2 度書かない**——
/// 片方だけを直すと、畳める便の集合が静かにずれる。
pub(crate) const REBASE_EMPTY: &str = "rebase-empty";

/// anchor を揃えない理由（判定行 `anchor=skipped:<reason>`・**閉じた enum**・憲法 C11）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnchorSkip {
    /// HEAD が main を指さない（別 branch・detached）。通常形ゆえ warning は出さない。
    NotMain,
    /// tracked な未 commit の変更が在る（成果を消さない・N1）。
    Dirty,
    /// landed tree が**足す** path が anchor の working tree に既に在る（untracked・ignored を含む）。
    /// `read-tree -m -u` は ignored な file を黙って上書きする（実測 2026-09-12・lens-120 M1）ので、
    /// 足す path の存在を先に見て 1 file も触らない。
    Collision,
    /// anchor の状態を読めない（読めないを clean に読み替えない）。
    Unreadable,
    /// 揃える git が途中で断った（index.lock 等）。**部分的に更新されている可能性がある**。
    SyncFailed,
}

impl AnchorSkip {
    /// 判定行の字面。
    fn as_str(self) -> &'static str {
        match self {
            Self::NotMain => "not-main",
            Self::Dirty => "dirty",
            Self::Collision => "collision",
            Self::Unreadable => "unreadable",
            Self::SyncFailed => "sync-failed",
        }
    }
}

/// 本文の最後に置く trailer の key（読み手が fleet の記録へ辿る鍵）。
const RUN_TRAILER: &str = "run: ";

/// 検出線（変異検査）の面（**閉じた集合**・設計 §30・`s2-07l.397`）。末尾 `/` の項目は dir の接頭辞、
/// それ以外は file の完全一致。検出線の行の出所（`.vessel.toml`）と、変異検査が読む面（crate の source・
/// Cargo の manifest / lock・rules）である。docs / design-intent / README / .github はこの外＝検出線の
/// 結果を変えない（共通 verify は従来どおり撃つので、docs を読む歯が赤になる経路は残る）。
///
/// **rules 行にしない**（値でなく閉じた path の集合・variant の領分・§30 却下案）。
pub const DETECTION_SCOPE: &[&str] = &["crates/", "Cargo.toml", "Cargo.lock", "rules/", ".vessel.toml"];

/// path の列 → 検出線の要否（**pure**・追随の再 gate の省略と着地後の検出の面の両方がこの 1 本を通す・設計 §30）。
///
/// 1 つでも [`DETECTION_SCOPE`] に触れれば真。**空の列は偽**（差分が無い周は撃たない）——読めない周を
/// 空に読み替えない責任は呼び手（[`regate_skippable`] / [`detection`]）が持つ。
pub fn detection_needed<'a>(paths: impl IntoIterator<Item = &'a str>) -> bool {
    paths.into_iter().any(|path| DETECTION_SCOPE.iter().any(|face| in_face(path, face)))
}

/// path が面の 1 項目に触れるか（dir は接頭辞・file は完全一致）。`cratesx/a.rs` は `crates/` に触れない。
fn in_face(path: &str, face: &str) -> bool {
    if face.ends_with('/') {
        return path.starts_with(face);
    }
    path == face
}

/// `-z`（NUL 区切り）の git 出力を path の列にする（空の要素は落とす）。
fn nul_paths(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| String::from_utf8_lossy(path).into_owned())
        .collect()
}

/// この境界の極性（`MainCheck`）: main を進めた後に実測し、測れなかった周は `Failed detail=main-unmeasured` で止める（緑に化けさせない）。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::PostHoc,
    on_failure: OnFailure::FailClosed,
};

/// main 実測の結果。**「赤かった」と「測れなかった」を混ぜない**。
///
/// gate が「測れなかったを通ったに化けさせない」と決めているのと同じ理由で、land も
/// 「測れなかった」を「赤かった」に化けさせない。実測を 1 行も撃てていないのに
/// `main-red` を記帳すると、event log が事実と違うものを述べる。
enum MainCheck {
    /// verify 全行が rc 0。
    Green,
    /// 1 行以上が rc≠0（**実測した上での赤**）。
    Red(String),
    /// 実測そのものができなかった（tmp worktree を切れない等）。
    Unmeasurable(String),
}

/// land 1 回の材料。
pub struct Land<'a> {
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
    /// PR を作る seam（`--pr-cmd`）。`None` なら squash して main を進める。
    pub pr_cmd: Option<&'a str>,
    /// main が動いた便の追随で gate を撃ち直す周の lens の出所（`--lens` か run dir の写し・無い / 読めないは
    /// 撃ち直しが INCONCLUSIVE へ倒れ land しない・[`super::lens_record`]・設計 §26）。land 自身は読まず再 gate へ渡す。
    pub lens: &'a LensSource,
    /// 規則から読んだ線（撃ち直しの gate へ渡す・land 自身は数値を見ない）。
    pub limits: Limits,
    /// 追随が衝突した周に runner を起こし直すコマンド（`--runner`）と、その turn の口座を選ぶ入力
    /// （[`follow::Runner`]・設計 account-autonomy.md §4）。**無い周は起こし直さない**。
    pub runner: Option<follow::Runner<'a>>,
    /// 起こし直しの上限（rules 行 `pipe.follow_retries`・land 自身は数値を見ない）。
    pub retries: u64,
    /// 着地待ちの列で自分の番を待つ上限（秒・rules 行 `pipe.land_wait_s`）。超えた周は待たずに進む。
    pub land_wait_s: u64,
    /// 終端が CI の判定を待つ上限（秒・rules 行 `pipe.ci_wait_s`）。超えた周は **close しない**。
    pub ci_wait_s: u64,
    /// 終端が bead を閉じる台帳 client（`--bd` か [`crate::ledger::DEFAULT_BD`]）。
    pub bd: &'a str,
    /// 承認 event が在るか（起こし直しも A1 の関門を通る・replay の導出値）。
    pub approved: bool,
    /// lock の待ち方。
    pub policy: LockPolicy,
    /// 着地の列を候補の木 1 つに積む本数の上限（先頭を含む・rules 行 `land.train_max`・行が無い / 読めない周は 1
    /// ＝先頭だけ・設計 §40）。land 自身は数値を見ず [`super::train`] へ渡す。
    pub train_max: u64,
    /// land が受けた `--rules` の path（着地後の検出の子へ同じ値を渡す・受けていない周は `None`＝子も埋め込みを読む・
    /// 設計 gate-cost.md §44 形 (11)）。
    pub rules: Option<&'a Path>,
}

/// main が動いた便の追随の結果（設計 §5.4・§29）。
enum Follow {
    /// rebase と gate の撃ち直しを通した。stdout に載せる行（`rebase=` と撃ち直しの判定行）。
    Ready(Vec<String>),
    /// 追随できなかった・撃ち直しが PASS でない。呼び手はこの Outcome をそのまま返す。
    Stopped(Outcome),
    /// 便の commit は 0 本で、main に**この便の trailer を持つ squash**が在る（前の周が CAS の後・実測の前に
    /// 死んだ形・設計 §29）。値はその squash の sha。呼び手は squash と CAS を撃たず、主実測をこの sha に撃つ。
    AlreadyLanded(String),
}

/// 追随の rebase の後の便の形（[`rebase_onto`] の戻り・**閉じた enum**・設計 §29）。
enum Rebased {
    /// 便の commit が main の上に残った。gate を撃ち直す。
    Pending,
    /// 便の commit は 0 本で、main の log に便の trailer を持つ squash が在る（sha）。
    AlreadyLanded(String),
}

/// 着地の形と main に載った squash の sha（[`finish`] の印・**閉じた enum**・設計 §29）。stdout の末尾と
/// `Landed` の detail に写す。
#[derive(Debug, Clone, PartialEq, Eq)]
enum Landing {
    /// この land が squash を作り CAS で main を進めた（従来の形・stdout と detail は不変）。
    Fresh(String),
    /// squash は前の周が既に main に載せていた（見つけた sha）。この land は主実測と終端だけを通した。
    AlreadyLanded(String),
}

/// `already-landed` の印の字面（stdout は `already-landed=1`・detail は `already-landed`・**書き手はこの 1 本**）。
const ALREADY_LANDED: &str = "already-landed";

impl Landing {
    /// main に載った squash の sha（`landed=` / `sha:` / 面 5 の `sha` の宣言値）。
    fn sha(&self) -> &str {
        match self {
            Self::Fresh(sha) | Self::AlreadyLanded(sha) => sha,
        }
    }

    /// stdout の 1 行の末尾に後置する token（`Fresh` は何も足さない）。
    fn stdout_suffix(&self) -> String {
        match self {
            Self::Fresh(_) => String::new(),
            Self::AlreadyLanded(_) => format!(" {ALREADY_LANDED}=1"),
        }
    }

    /// `RunDone stage=Landed` の detail の末尾（`main:<実測>` の後ろ・空白区切り・`Fresh` は何も足さない）。
    fn detail_suffix(&self) -> String {
        match self {
            Self::Fresh(_) => String::new(),
            Self::AlreadyLanded(_) => format!(" {ALREADY_LANDED}"),
        }
    }
}

/// 面 5 の export 先。
///
/// dir 名を 2 度書かず event log の隣として導く（`store` が dir を変えたら追随する）。
pub fn verdicts_path(state_dir: &Path) -> PathBuf {
    store::events_path(state_dir).with_file_name(VERDICTS_FILE)
}

/// 試行 1 回の戻り（**閉じた enum**・設計 §18・`s2-07l.335`）。自由文（stderr の `stale base`）で判定しない。
enum Attempt {
    /// 決着した（Landed・断り・終端のどれか）。呼び手はそのまま返す。
    Settled(Outcome),
    /// 撃ち直しの間に main がさらに動いた。`old` は追随した先（CAS に使うはずだった main）、`now` は
    /// 読み直した main。呼び手は記帳して (iii) から追随し直す。
    Stale { old: String, now: String },
}

/// land を 1 回通す。
///
/// **撃ち直しの間に main がさらに動いた周は同じ land の中で追随し直す**（設計 §5.4 (vi) / §18）。試行
/// （[`attempt`]）が [`Attempt::Stale`] を返した周は `RunStage stage=Gated detail=stale:<old>..<now>` を
/// 衝突と同じ記帳の口（[`follow::on_stale`]）で記し、回数が衝突と合算で上限の内なら次の試行へ戻る。上限に
/// 達した周は `Failed detail=rebase-conflict` + rc 1 で終端する（`--runner` は要らない・main は動かさない）。
/// 周回は `land` の中に閉じるので `pipe run` / `pipe resume` / `pipe land` のどの口から撃っても同じ経路を通る。
/// event 列（`rebase:` / `stale:`）が追随の回数をそのまま語る。
pub fn land(entry: &Land<'_>) -> Outcome {
    let worktree = worktree_path(entry.repo, entry.run);
    let base = match recorded_base(entry) {
        Ok(found) => found,
        Err(stopped) => return stopped,
    };
    if verdict_of(entry.state_dir, entry.run) != Some(Verdict::Pass) {
        return refused(format!("run {} の verdict が PASS でない", entry.run));
    }
    if let Some(cmd) = entry.pr_cmd {
        return open_pr(entry, &base, cmd);
    }
    // **着地の順番**（設計 gate-cost.md §6）: 前提検査の直後・追随の前に列を見て待つ。`--pr-cmd` の形は
    // 上で返っている＝main を動かさないので列を見ない（stale base を見ないのと同じ理由）。
    // 札が死んでいて列から外した便は `finish` が stdout と面 5 に名指す（設計 §36・黙らせない）。
    let turned = await_turn(entry);
    let order = turned.order;
    // **番待ちから戻った直後の 1 点で段を読む**（設計 §40）: 列の先頭が自分を候補の木に積んで着地させた便は
    // 何もせず rc 0（§29 の冪等の終端を段で先に読む・worktree の実在を要さない・待たない周も同じ点を通る）。
    if let Some(settled) = settled_in_train(entry, order) {
        return settled;
    }
    // 追随・撃ち直し・stale の判定行は周を跨いで**捨てない**（起きたことは event に残り lens も消費している＝
    // stdout だけが空だと読み手が「何もしなかった」と誤読する）。
    let mut lines = Vec::new();
    // **列の先頭の周は後ろの便を候補の木に積む**（設計 §40・[`super::train`]）。解いた周は判定行を残して先頭 1 本の
    // 既存の経路（追随 → 撃ち直し）へそのまま入る。
    match super::train::train(entry, &worktree, order) {
        super::train::Train::Landed(outcome) => return outcome,
        super::train::Train::Dissolved(line) => lines.push(line),
        super::train::Train::Solo => {}
    }
    loop {
        let (old, now) = match attempt(entry, &worktree, &turned, &mut lines) {
            Attempt::Settled(outcome) => return with_lines(lines, outcome),
            Attempt::Stale { old, now } => (old, now),
        };
        let stale = follow::on_stale(&Conflict {
            turn: turn_of(entry),
            base: &old,
            main: &now,
            limit: entry.retries,
        });
        if let Err(stopped) = stale {
            return with_lines(lines, stopped);
        }
        lines.push(format!("run={} stale={old}..{now}", entry.run));
    }
}

/// 番待ちの間に列の先頭が自分を着地させた / 終端させた便（設計 §40）。`Landed` は rc 0 で `already-landed`、
/// 主実測が赤で列ごと `Failed` になった便は rc 1（追随へ進まない）。段を読めない周と他の段は `None`（従来どおり）。
fn settled_in_train(entry: &Land<'_>, order: Order) -> Option<Outcome> {
    let state = super::current(entry.state_dir).ok()?;
    match state.runs.get(entry.run)?.stage {
        Stage::Landed => Some(Outcome::ok_line(format!(
            "run={} {ALREADY_LANDED}=1 order={}",
            entry.run,
            order.as_value()
        ))),
        Stage::Failed => Some(refused(format!("run {} は番待ちの間に終端した（段 Failed）", entry.run))),
        _ => None,
    }
}

/// 便の記録済み base（`base_of_run` の 1 本を読む・追随の `rebase:` の行が在ればその新しい側）。
///
/// **「無い」と「読めない」を分ける**（C10）: 置き場が壊れている周を前提違反に化けさせない。周回の
/// 試行ごとに読み直す（追随した周は記帳が base を進めている＝process の記憶で持たない）。
fn recorded_base(entry: &Land<'_>) -> Result<String, Outcome> {
    match super::base_of_run(entry.state_dir, entry.run) {
        super::Base::Known(found) => Ok(found),
        super::Base::Absent => Err(refused(format!("run {} に base が無い", entry.run))),
        super::Base::Unreadable => Err(broken(format!("run {} の base を読めない（置き場）", entry.run))),
    }
}

/// 着地の試行 1 回（追随 → 撃ち直し → CAS → 主実測 → 終端）。stale の周だけ [`Attempt::Stale`] で戻り、
/// 呼び手（[`land`]）が記帳して追随し直す。
fn attempt(entry: &Land<'_>, worktree: &Path, turned: &Turned, lines: &mut Vec<String>) -> Attempt {
    let base = match recorded_base(entry) {
        Ok(found) => found,
        Err(stopped) => return Attempt::Settled(stopped),
    };
    let Some(old) = git_line(entry.repo, &["rev-parse", MAIN_REF]) else {
        return Attempt::Settled(refused(format!("{MAIN_REF} を読めない")));
    };
    // 前の周が既に main に載せた squash（設計 §29）。`Some` の周は squash と CAS を撃たない。
    let mut already = None;
    if old != base {
        // **CAS の old が動いている**。base が main の祖先なら追随する（rebase → gate の
        // 撃ち直し・設計 §5.4）。追随の形が無い周はここで断る（何も書かない）。
        match follow_main(entry, worktree, &base, &old) {
            Follow::Stopped(outcome) => return Attempt::Settled(outcome),
            Follow::Ready(followed) => lines.extend(followed),
            Follow::AlreadyLanded(found) => already = Some(found),
        }
    }
    // 撃ち直しの間に main がさらに動いた周は CAS を撃たず呼び手へ戻す（同じ land の中で追随し直す・§18）。
    let Some(now) = git_line(entry.repo, &["rev-parse", MAIN_REF]) else {
        return Attempt::Settled(refused(format!("{MAIN_REF} を読めない")));
    };
    if now != old {
        return Attempt::Stale { old, now };
    }
    // anchor の見立ては **ref を進める前**に読む: 進めた後の `git status` は index の遅れを
    // 「変更」として出すので、人の未 commit と区別できない。
    let plan = anchor_plan(entry.repo);
    // **既着地の周は squash と CAS を撃たない**（設計 §29・main は 1 byte も動かさない）。anchor は
    // `old → old` の no-op を通す（同期の判定行は従来どおり出る・ref は動いていないので揃える差分も無い）。
    // 主実測は見つけた sha に対して**従来どおり撃つ**——前の周が実測の前に死んだ可能性が在り、記録が
    // 無いものを緑と読まない（C10）。木が gate と同じ周は主実測ごと省く（`verify::same_tree`・gate-cost.md §27）。
    let landing = match already {
        Some(found) => Landing::AlreadyLanded(found),
        None => match squash(entry, worktree, &old) {
            Ok(found) => Landing::Fresh(found),
            Err(reason) => return Attempt::Settled(broken(reason)),
        },
    };
    let new = landing.sha();
    // **squash の直後に揃える**（`s2-07l.131`）。ref を進めてから anchor を揃えるまでの窓——
    // `git status` に landed 変更が staged の逆向きで見える時間——は、実測の後に揃えると
    // **main 実測の長さだけ**開く（人が anchor を触れば `.117` の経路がその間ずっと開いている）。
    // 実測の前に揃えれば窓は秒単位に縮む。「**実測の結果に依らず揃える**」（`s2-07l.120`・lens-120 H1）は
    // この順序でこそ自明である——同期が先なら、そもそも結果を見ていない。
    //
    // 同期が `Skipped(SyncFailed)` の周も実測は続ける（ref は既に進んでいる＝同期の失敗で land を
    // 止めない・極性は不変）。結果は従来どおり [`finish`] / [`main_red`] / [`main_unmeasured`] へ渡す。
    let synced_to = match &landing {
        Landing::Fresh(_) => new,
        Landing::AlreadyLanded(_) => old.as_str(),
    };
    let anchor = sync_anchor(entry.repo, &plan, &old, synced_to);
    let check = verify_main(entry, new);
    Attempt::Settled(match check {
        MainCheck::Green => finish(entry, worktree, &landing, &anchor, turned),
        MainCheck::Red(reason) => main_red(entry, &reason, &anchor),
        MainCheck::Unmeasurable(reason) => main_unmeasured(entry, &reason, &anchor),
    })
}

/// anchor（`--repo` の checkout）を land の後に新 main へ揃えるかの見立て（`s2-07l.120`）。
///
/// 揃えるのは **HEAD が `refs/heads/main` を指し ∧ tracked な未 commit の変更が無い**周だけ。
/// untracked は数えない（揃える動作は tracked path しか触らず、衝突すれば git が断る＝
/// [`AnchorSkip::SyncFailed`]）。読めない周は clean に読み替えない（fail-closed）。
///
/// 極性一覧の境界（`s2-07l.124`・C11.2 / C16.2）: 隣の [`ANCHOR_POLARITY`] が値を持つ。一覧の pointer は
/// 字面（`pipe::land::AnchorPlan`）で、型は crate の外へ出さない（構築する口が private ゆえ公開しても判定させられない）。
enum AnchorPlan {
    /// 揃える。
    Sync,
    /// 触らない（理由）。
    Skip(AnchorSkip),
}

/// この境界の極性（[`AnchorPlan`]）: 同期の**前**に見立てて止め（in-loop）、読めない周は揃えない（fail-closed）。
/// `status` を読めない周は `anchor=skipped:unreadable`、`symbolic-ref` が答えない周（detached と区別しない）は
/// `skipped:not-main` に落ちる——どちらも揃えない側である（lens-124 M2）。
pub const ANCHOR_POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailClosed,
};

/// 見立てを読む。
fn anchor_plan(repo: &Path) -> AnchorPlan {
    if git_line(repo, &["symbolic-ref", "-q", "HEAD"]).is_none_or(|head| head != MAIN_REF) {
        return AnchorPlan::Skip(AnchorSkip::NotMain);
    }
    match git_bytes(repo, &["status", "--porcelain", "--untracked-files=no"]) {
        None => AnchorPlan::Skip(AnchorSkip::Unreadable),
        Some(bytes) if !String::from_utf8_lossy(&bytes).trim().is_empty() => AnchorPlan::Skip(AnchorSkip::Dirty),
        Some(_) => AnchorPlan::Sync,
    }
}

/// 揃えた結果（判定行の `anchor=` token）。
enum AnchorSync {
    /// index と working tree が新 main に揃った。
    Synced,
    /// 触っていない（理由）。
    Skipped(AnchorSkip),
}

impl AnchorSync {
    /// 判定行の token。
    fn token(&self) -> String {
        match self {
            Self::Synced => "anchor=synced".to_owned(),
            Self::Skipped(reason) => format!("anchor=skipped:{}", reason.as_str()),
        }
    }

    /// stderr に出す warning（人の注意が要る周だけ・別 branch は通常形なので黙る）。
    /// **状態を断定しない**: 途中で断られた周は部分的に更新されていることがある（lens-120 M2）。
    fn warning(&self) -> Option<String> {
        match self {
            Self::Synced | Self::Skipped(AnchorSkip::NotMain) => None,
            Self::Skipped(AnchorSkip::SyncFailed) => Some(
                "pipe: anchor を新 main に揃える途中で git が断った（sync-failed）・index と working tree は部分的に更新されている可能性がある＝`commit -a` の前に `git status` で確かめること".to_owned(),
            ),
            Self::Skipped(reason) => Some(format!(
                "pipe: anchor を新 main に揃えていない（{}）・index と working tree は旧 main のまま＝`commit -a` の前に揃えること",
                reason.as_str()
            )),
        }
    }
}

/// landed tree が `old` に対して**足す** path のうち、anchor の working tree に既に在るものが 1 つでも
/// 在るか（untracked・ignored を含む・読めない周は「在る」側＝fail-closed）。
fn anchor_has_collision(repo: &Path, old: &str, new: &str) -> bool {
    let Some(bytes) = git_bytes(repo, &["diff", "--name-only", "--diff-filter=A", "-z", old, new]) else {
        return true;
    };
    nul_paths(&bytes)
        .iter()
        .any(|path| repo.join(path).symlink_metadata().is_ok())
}

/// anchor の index と working tree を `old` の tree から `new` の tree へ揃える。
///
/// `git read-tree -m -u <old> <new>` の 2-tree merge を使う。`reset --keep <new>` は **ref が
/// 既に `new` を指している**ため差分を 0 と見て working tree を更新しない（index だけが new に
/// なり `commit -a` が landed 変更を巻き戻す形へ悪化する・実測 2026-09-12）。`read-tree -m -u` は
/// old→new で変わった path だけを更新し、局所の変更が在れば "not uptodate" で断る——ただし
/// **ignored な untracked file は黙って上書きする**ので、足す path の衝突は先に見る（[`AnchorSkip::Collision`]）。
fn sync_anchor(repo: &Path, plan: &AnchorPlan, old: &str, new: &str) -> AnchorSync {
    match plan {
        AnchorPlan::Skip(reason) => AnchorSync::Skipped(*reason),
        AnchorPlan::Sync if anchor_has_collision(repo, old, new) => AnchorSync::Skipped(AnchorSkip::Collision),
        AnchorPlan::Sync if git_ok(repo, &["read-tree", "-m", "-u", old, new]) => AnchorSync::Synced,
        AnchorPlan::Sync => AnchorSync::Skipped(AnchorSkip::SyncFailed),
    }
}

/// 追随の行（`rebase=` と撃ち直しの判定行）を Outcome の stdout に前置する。
fn with_lines(mut lines: Vec<String>, mut outcome: Outcome) -> Outcome {
    lines.append(&mut outcome.out);
    outcome.out = lines;
    outcome
}

/// 便の base が main の祖先か merge-base を持つなら worktree の branch を main へ rebase し、gate を**同じ関数で**
/// 撃ち直す（設計 §5.4・`s2-07l.119`・§38・`s2-07l.449`）。**main は 1 byte も動かさない**——rebase が効くのは
/// worktree の branch だけで、force 系は使わない（N1）。
///
/// - 祖先検査は閉じた 3 値（[`follow::Ancestry`]・起こし直しの「追随」節と同じ 1 本）。祖先でないが merge-base の
///   在る周（main が巻き戻った / 分岐した）は便が base の上に積んだ commit だけを `--onto` で main の上へ運び
///   （[`rebase_onto`]・消えた commit は運ばない）、以後は従来の追随と同じ経路（衝突・記帳・再 gate）に合流する。
/// - merge-base が無い・読めない周だけ追随の形が無いので `stale base` の rc 1 で何もしない（字面不変・fail-closed）。
/// - worktree が clean でない周も rc 1 で何もしない（汚れた木では rebase を走らせない）。
/// - 衝突は `git rebase --abort` で木を戻し `Failed detail=rebase-conflict`（終端・fail-closed）。
/// - rebase で commit が 0 本になった周（同一変更の便が先に land）は gate を撃ち直さず
///   `Failed detail=rebase-empty`（便の変更は既に main に在る＝close してよい合図）。
/// - 追随した事実は `RunStage stage=Implemented detail=rebase:<old>..<new>` で残す（段が
///   `Gated` から `Implemented` へ戻る 1 件＝撃ち直す便の記帳）。base の読み手
///   （[`super::base_of_run`]）はこの行から新しい base を読む。
/// - 撃ち直しが PASS でない周は gate の判定行と rc で止まる（FAIL は `Gated` のまま
///   land しない・INCONCLUSIVE は測り直せる側）。
/// - main が動いた差分が [`DETECTION_SCOPE`] に 1 つも触れない周（[`regate_skippable`]・`<base>..<main>` は
///   rebase で動かない）は**撃ち直しを丸ごと省き**、前周の Gated PASS を新しい base へ引き継ぐ（[`carry_gated_pass`]・
///   設計 §33 (i)）。面に触れる周と diff を読めない周は従来どおり gate の段（①②④）を撃ち直す（設計
///   gate-cost.md §44 形 (10)）。
/// - rebase の直後（上の省略と再 gate の**前**・[`rebase_onto`] の中）に契約表の検査を便の木へ撃ち、findings のすべてが
///   便の消した path を名指す write-set の項目の未解決なら [`follow::on_stale_rows`] へ委ねる（`Implemented
///   detail=rebase-stale-rows:`・runner を起こし直す・設計 §34）。他の findings と撃てない周は従来どおり。
fn follow_main(entry: &Land<'_>, worktree: &Path, base: &str, main: &str) -> Follow {
    if !follow::Ancestry::judge(entry.repo, base, main).can_follow() {
        return Follow::Stopped(refused(format!(
            "stale base（base={base} main={main}・base は main の祖先でない）"
        )));
    }
    let check = WorktreeCheck::judge(worktree);
    if !check.is_clean() {
        return Follow::Stopped(refused(format!(
            "run {} の worktree が clean でない（{}・rebase しない）",
            entry.run,
            check.as_str()
        )));
    }
    match rebase_onto(entry, worktree, base, main) {
        Err(stopped) => return Follow::Stopped(stopped),
        // 既着地（設計 §29）: gate を撃ち直さず（lens を起こさない）・追随の event も書かない
        // （便の変更は既に main に載っている＝段は実装の戻りでなく終端へ向かう・C3）。
        Ok(Rebased::AlreadyLanded(found)) => return Follow::AlreadyLanded(found),
        Ok(Rebased::Pending) => {}
    }
    let rebased = emit(
        entry.state_dir,
        &Emit {
            kind: EventKind::RunStage,
            run: entry.run,
            bead: entry.bead,
            stage: Some(Stage::Implemented),
            seat: None,
            pid: None,
            detail: Some(format!("rebase:{base}..{main}")),
        },
        entry.policy,
    );
    if let Err(err) = rebased {
        return Follow::Stopped(broken(err.to_string()));
    }
    let mut lines = vec![format!("run={} rebase={base}..{main}", entry.run)];
    // **再 gate の要否は検出線の面と同じ 1 本の判定で決める**（新しい判定関数を足さない・C2）。面の判定は
    // `<base>..<main>`（rebase で動かない 2 つの sha）を読む。
    if regate_skippable(entry.repo, base, main) {
        if let Some(carried) = carry_gated_pass(entry) {
            lines.push(carried);
            return Follow::Ready(lines);
        }
    }
    let regated = gate(&Gate {
        run: entry.run,
        bead: entry.bead,
        repo: entry.repo,
        state_dir: entry.state_dir,
        contract: entry.contract,
        lens: entry.lens,
        // 撃ち直しの lens の口座も器が選ぶ（設計 account-autonomy.md §15）。宣言は `--runner` を持つ周に
        // 1 回だけ解いて [`follow::Runner`] へ載っている＝land は借りて渡す（runner の無い周は継承）。
        pool: entry.runner.and_then(|runner| runner.pool),
        limits: entry.limits,
        policy: entry.policy,
    });
    lines.extend(regated.out);
    if regated.rc != RC_OK {
        return Follow::Stopped(Outcome { out: lines, err: regated.err, rc: regated.rc });
    }
    Follow::Ready(lines)
}

/// 追随の再 gate を丸ごと省いて前周の PASS を引き継ぐか（設計 §33 (i)・gate-cost.md §44 形 (10)）。
///
/// main が便の base から進んだ差分（`git diff --name-only -z <base>..<main>`）の path が [`DETECTION_SCOPE`] に
/// 1 つも触れない周だけ真。**diff を読めない周は偽＝撃ち直す**（読めないを「触れていない」に読み替えない・
/// fail-closed）。
fn regate_skippable(repo: &Path, base: &str, main: &str) -> bool {
    let range = format!("{base}..{main}");
    let Some(bytes) = git_bytes(repo, &["diff", "--name-only", "-z", &range]) else {
        return false;
    };
    let paths = nul_paths(&bytes);
    !detection_needed(paths.iter().map(String::as_str))
}

/// stdout の判定行で「撃ち直しを省いて引き継いだ」を名乗る token（gate が撃った周には出ない）。
///
/// `verdict=` は**引き継いだ値**（land の前提が PASS なので PASS）を従来どおり出す——読み手
/// （人・`fleet`）が段と判定を同じ形で読めるためで、撃ったか引き継いだかはこの token が弁別する。
const REGATE_SKIPPED: &str = "regate=skipped";

/// 面に触れない周の追随: 前周の Gated PASS を新しい base へ引き継ぐ（設計 §33 (i) / (2)）。
///
/// 引き継いだ事実は 2 つの面に残す:
/// - `verify.jsonl` の 1 本（`kind=gate skipped=regate reason=<理由>`・`n` は既存の record からの通し）。
/// - `RunStage stage=Gated detail=verdict:PASS`（**gate が書くのと同じ形**＝`fleet` の読み手は不変）。
///
/// **record を書けない周は引き継がない**（`None`＝呼び手は従来どおり撃ち直す・fail-closed）。record を
/// 先に書くのは、event だけが残って判定の根が無い形を作らないためである（event は「PASS だった」と
/// 名乗る面で、撃ち直しが FAIL になり得る周にそれを先に置くと嘘が残る）。
fn carry_gated_pass(entry: &Land<'_>) -> Option<String> {
    let path = super::verify_log_path(entry.state_dir, entry.run);
    let written = std::fs::read_to_string(&path).ok()?;
    let number = next_number(written.lines().filter(|line| !line.trim().is_empty()).count());
    let body = skip_record(number, Skipped::regate());
    append_line(&path, &body, entry.policy).ok()?;
    emit(
        entry.state_dir,
        &Emit {
            kind: EventKind::RunStage,
            run: entry.run,
            bead: entry.bead,
            stage: Some(Stage::Gated),
            seat: None,
            pid: None,
            detail: Some(format!("verdict:{}", Verdict::Pass.as_str())),
        },
        entry.policy,
    )
    .ok()?;
    Some(format!("run={} verdict={} {REGATE_SKIPPED}", entry.run, Verdict::Pass.as_str()))
}

/// worktree の branch を main へ rebase する（追随の (iii)・(iii′)）。**main は動かさない**。
///
/// - 撃つのは `git rebase --onto <main> <base>` の 1 形（設計 §38）: 便が記録した base の上に積んだ commit だけを
///   main の上へ運ぶ。base が main の祖先の周は `git rebase <main>` と同じ結果で、祖先でないが merge-base の在る周は
///   merge-base から base までの消えた commit を運ばない（経路を 2 本にしない）。
/// - 衝突は [`super::follow::on_conflict`] へ委ねる（設計 pipeline-conflict.md §3）。器は木を戻し、
///   衝突を `Implemented detail=rebase-conflict:<base>..<main>` で記帳して**実装役を起こし直す**
///   ——便を終端にするのは上限に達した周だけである。どの形でも land はここで止まり、続きは
///   `gate` から撃ち直す（起こし直した turn の後は、次の land の追随で再び rebase が走る）。
/// - **既着地の便**（設計 §29）: rebase で commit が 0 本になった周、`rebase-empty` に倒す**前に** main の log を
///   この便の trailer（`run: <run id>`）で 1 回だけ探す（[`landed_squash_of`]）。在れば [`Rebased::AlreadyLanded`]
///   ——前の周が CAS の後・実測の前に死んだ形で、便の squash は既に main に載っている。
/// - **同一変更の便**: trailer が無い周は便の変更が別の便で main に在る（先に land した便と同じ patch）ので
///   gate を撃ち直さず（lens を起動しない）`rebase-empty` で終端する。commit 数を読めない周は 0 に読み替えず、
///   従来どおり撃ち直しの precheck へ流す（fail-closed の向きを変えない・`s2-07l.125`）。
/// - commit が残った周は、撃ち直しの前に契約表の行が便の消した path を名指すかを見る（[`stale_rows_stop`]・設計 §34）。
fn rebase_onto(entry: &Land<'_>, worktree: &Path, base: &str, main: &str) -> Result<Rebased, Outcome> {
    if !super::follow_step::rebase(worktree, base, main) {
        return Err(follow::on_conflict(&Conflict {
            turn: turn_of(entry),
            base,
            main,
            limit: entry.retries,
        }));
    }
    if commits_after_rebase(worktree, main) != Some(0) {
        return stale_rows_stop(entry, worktree, base, main).map_or(Ok(Rebased::Pending), Err);
    }
    if let Some(found) = landed_squash_of(entry.repo, main, entry.run) {
        return Ok(Rebased::AlreadyLanded(found));
    }
    Err(follow_failed(
        entry,
        REBASE_EMPTY,
        format!(
            "run {} の変更は既に main に在る（rebase で commit が空・base={base} main={main}）・main は動かさない",
            entry.run
        ),
    ))
}

/// **追随で入った契約表の行が便の消した path を名指す周は runner を起こし直す**（設計 §34）: rebase が通った直後・§33 の
/// 省略と再 gate の前に便の木へ契約表の検査を撃ち、findings のすべてが便の消した path を名指す write-set の項目の未解決
/// なら [`follow::on_stale_rows`] の Outcome（記帳・写しの追記・起こし直し）で止まる。該当しない findings・検査を撃てない
/// 周は `None`（従来どおり・[`follow::stale_rows`]）。
fn stale_rows_stop(entry: &Land<'_>, worktree: &Path, base: &str, main: &str) -> Option<Outcome> {
    let rows = follow::stale_rows_in(entry.state_dir, entry.run, worktree, main)?;
    let entry = Conflict {
        turn: turn_of(entry),
        base,
        main,
        limit: entry.retries,
    };
    Some(follow::on_stale_rows(&entry, &rows))
}

/// main の祖先に**この便の trailer**（`run: <run id>`・[`squash_message`] が本文の末尾に置く 1 行）を持つ
/// squash が在ればその sha（設計 §29）。
///
/// 探すのは 1 回だけ（`git log <main> -n 1 --fixed-strings --grep=<trailer> --format=%H`・run id の `.` を
/// regex に読ませない・母集団は `main` の祖先）。`--grep` は行の部分一致なので、当たった commit の本文に
/// **trailer と字面が等しい行**が在ることを確かめてから返す（別の便の id が接頭辞で重なる周を自分と読まない）。
/// 読めない周・無い周はどちらも `None`（呼び手は従来どおり `rebase-empty` へ倒す＝在ると読み替えない）。
fn landed_squash_of(repo: &Path, main: &str, run: &str) -> Option<String> {
    let trailer = format!("{RUN_TRAILER}{run}");
    let grep = format!("--grep={trailer}");
    let found = git_line(repo, &["log", main, "-n", "1", "--fixed-strings", &grep, "--format=%H"])?;
    let body = git_bytes(repo, &["log", "-n", "1", "--format=%B", &found])?;
    String::from_utf8_lossy(&body)
        .lines()
        .any(|line| line == trailer)
        .then_some(found)
}

/// 起こし直しの材料（land が持つ面から組む・**組み立てはこの 1 本**）。
fn turn_of<'a>(entry: &'a Land<'a>) -> follow::Turn<'a> {
    follow::Turn {
        run: entry.run,
        bead: entry.bead,
        repo: entry.repo,
        state_dir: entry.state_dir,
        contract: entry.contract,
        runner: entry.runner,
        approved: entry.approved,
        policy: entry.policy,
    }
}

/// rebase の後に便へ残った commit の数（`<main>..HEAD`）。**読めない周は `None`**（0 に読み替えない）。
fn commits_after_rebase(worktree: &Path, main: &str) -> Option<u64> {
    let range = format!("{main}..HEAD");
    git_line(worktree, &["rev-list", "--count", &range])?.parse().ok()
}

/// 追随の途中で便が終端した周（`rebase-conflict` / `rebase-empty`）。**main は動いていない**。
/// 理由は `Failed` の `detail` に名乗り、stderr の 1 行は呼び手が組む（同じ形・run id と base / main を持つ）。
fn follow_failed(entry: &Land<'_>, detail: &str, reason: String) -> Outcome {
    let emitted = emit(
        entry.state_dir,
        &Emit {
            kind: EventKind::RunStage,
            run: entry.run,
            bead: entry.bead,
            stage: Some(Stage::Failed),
            seat: None,
            pid: None,
            detail: Some(detail.to_owned()),
        },
        entry.policy,
    );
    match emitted {
        Err(err) => broken(err.to_string()),
        Ok(()) => refused(reason),
    }
}

/// land の終端の結末（**閉じた 7 値**・設計 contract-source.md §5）。
///
/// `Closed` と `Undeclared` 以外はどれも**台帳を閉じない**（着地は取り消さない）。やり直しは
/// `pipe land --terminal-only` で終端だけを撃ち直す（冪等）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Terminal {
    /// push → CI success → close まで通った。
    Closed,
    /// **押す先が宣言されていない**（宣言を読めた上で `remote` の行が無い）＝この repo の便は終端を持たない。
    ///
    /// push は repo の外へ出す行為（A1 の「出す」）なので、宣言の無い repo に既定で押さない。
    /// 着地は成立しているので**便は落とさない**（`--pr-cmd` 形と同じ極性）。
    Undeclared,
    /// **宣言そのものを読めない**（`.vessel.toml` が HEAD に無い・parse できない）。
    ///
    /// [`Self::Undeclared`] と融合しない（C10）——あちらは「読めた上で押す先が無い」で、こちらは
    /// 「押す先が在るかを測れていない」である。[`Self::PushFailed`] とも融合しない（push を 1 度も
    /// 撃っていないので「push が失敗した」ではない）。close しない側へ倒す。
    Unreadable,
    /// push が撃てなかった / 失敗した（理由の語）。
    PushFailed(String),
    /// CI が **failure** だった。
    CiFailed,
    /// CI の判定を**測れなかった**（上限超過・run が 0 本・行を撃てない・JSON を読めない）。
    ///
    /// `CiFailed` と融合しない（C10）——failure は「測って落ちた」で、こちらは「測れていない」である。
    CiUnmeasurable,
    /// 台帳を閉じられなかった（着地は成立・[`crate::ledger::CloseError`] の 1 行）。
    CloseFailed(String),
}

/// [`Terminal`] の全 variant の字面（`terminal=` の値・宣言順）。
pub const TERMINAL_TOKENS: &[&str] =
    &["closed", "undeclared", "unreadable", "push:failed:", "ci:failure", "ci:unmeasurable", "close:failed:"];

/// 終端の境界の極性（[`Terminal`]）: **success 以外は close しない側へ倒す**（FailClosed）。
///
/// 着地の後に測るので `PostHoc` である。止めるのは close であって着地ではない——main の commit は
/// 既に在り、取り消しは N1 の外である。
pub const TERMINAL_POLARITY: Polarity = Polarity {
    timing: Timing::PostHoc,
    on_failure: OnFailure::FailClosed,
};

impl Terminal {
    /// stdout の `terminal=` と `RunDone` の detail に載る字面。
    pub fn as_token(&self) -> String {
        match self {
            Self::Closed => "closed".to_owned(),
            Self::Undeclared => "undeclared".to_owned(),
            Self::Unreadable => "unreadable".to_owned(),
            Self::PushFailed(reason) => format!("push:failed:{reason}"),
            Self::CiFailed => "ci:failure".to_owned(),
            Self::CiUnmeasurable => "ci:unmeasurable".to_owned(),
            Self::CloseFailed(reason) => reason.clone(),
        }
    }

    /// 終端が返す rc（**`Closed` だけが 0**・設計 §5）。
    pub fn rc(&self) -> u8 {
        match self {
            Self::Closed | Self::Undeclared => RC_OK,
            Self::Unreadable | Self::PushFailed(_) | Self::CiFailed | Self::CiUnmeasurable | Self::CloseFailed(_) => {
                crate::cli_outcome::RC_REFUSED
            }
        }
    }
}

/// worktree を `retired/<run>` へ move する。**削除しない・branch も消さない**（N1.2）。
///
/// 呼び手は 2 つ（squash 形の [`finish`] と `pipe retire`）で、**move の中身は 1 本**である
/// ——2 実装に割ると、一方だけが削除へ寄る余地が生まれる。失敗は stderr 行の列で返し、
/// rc は呼び手が決める（land では 0 のまま・retire では 2）。
pub(crate) fn retire_worktree(repo: &Path, run: &str, worktree: &Path) -> Vec<String> {
    let dest = retired_path(repo, run);
    let Some(parent) = dest.parent() else {
        return vec!["pipe: retired の親 dir を解けない".to_owned()];
    };
    if let Err(err) = std::fs::create_dir_all(parent) {
        return vec![format!("pipe: {} を作れない: {err}", parent.display())];
    }
    let from = worktree.display().to_string();
    let to = dest.display().to_string();
    if git_ok(repo, &["worktree", "move", &from, &to]) {
        return Vec::new();
    }
    vec![format!("pipe: {from} を {to} へ移せなかった")]
}

/// 便の worktree が clean か（**閉じた enum**・`s2-07l.124`・C11.2）。呼び手は 2 つ——
/// [`follow_main`] の rebase の前（汚れた木で rebase を走らせない・`.119`）と [`retire`] の move の前——で、
/// 極性一覧には **判定 enum 1 つ = 行 1 本**（`land-worktree-clean`）として載る（lens-124 M1・bead notes）。
///
/// 「状態を読めなかった」を「汚れていない」に化けさせない（[`Unreadable`](Self::Unreadable) は
/// [`Dirty`](Self::Dirty) と同じく止める）——move は中身ごと運ぶので、未 commit の仕事を持った
/// worktree を畳むと、その仕事の行き先が便の外から読めなくなる。untracked も数える。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreeCheck {
    /// `status --porcelain` が空。畳める。
    Clean,
    /// 未 commit の変更（untracked を含む）が在る。
    Dirty,
    /// status を読めない（git が断った・repo でない）。
    Unreadable,
}

/// この境界の極性（[`WorktreeCheck`]）: rebase / move の**前**に読んで止め（in-loop）、読めない周は止める（fail-closed）。
pub const WORKTREE_POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailClosed,
};

impl WorktreeCheck {
    /// worktree の状態を読む。
    pub fn judge(worktree: &Path) -> Self {
        match git_bytes(worktree, &["status", "--porcelain"]) {
            None => Self::Unreadable,
            Some(bytes) if String::from_utf8_lossy(&bytes).trim().is_empty() => Self::Clean,
            Some(_) => Self::Dirty,
        }
    }

    /// 進めてよいか。**bool はここ 1 本で enum から導く**（読めない周は偽）。
    pub fn is_clean(self) -> bool {
        match self {
            Self::Clean => true,
            Self::Dirty | Self::Unreadable => false,
        }
    }

    /// 断りの理由の字面。
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::Dirty => "dirty",
            Self::Unreadable => "unreadable",
        }
    }
}

/// `verdict.json` から 3 値を読む。読めない周は `None`（＝PASS ではない）。
///
/// **判定の読み手はこの 1 本だけである**。land の前提（PASS か）だけでなく、gate の
/// 測り直し（Gated ∧ INCONCLUSIVE か）と resume の行き先（land か gate か）も同じ値を
/// 見る。読み手を増やすと、同じ JSON の解釈が場所ごとに静かにずれる。
/// 共有先は兄弟 module だけなので、公開面は crate の中に留める。
pub(crate) fn verdict_of(state_dir: &Path, id: &str) -> Option<Verdict> {
    verdict_field(state_dir, id, "verdict").as_deref().and_then(Verdict::parse)
}

/// 前提違反・使い方の誤り（rc 1 + stderr 1 行・何もしない）。
pub(super) fn refused(reason: String) -> Outcome {
    Outcome::failed_line(RC_REFUSED, format!("pipe: {reason}"))
}

/// 対象そのものが壊れている（rc 2）。
pub(super) fn broken(reason: String) -> Outcome {
    Outcome::failed_line(RC_BROKEN, format!("pipe: {reason}"))
}

// 歯（`mod tests`）だけが `super::` で読む 7 名（[`finish`] へ移した群・親の本体の site は 0）。
#[cfg(test)]
use finish::{
    squash_message, subject_of, trailer_key, CONTRACT_TRAILER, REQUIREMENTS_TRAILER, SHA_PREFIX, SUBJECT_CHARS,
};

/// message の 3 部（`s2-07l.130`）を **goal に改行が在る形**で測る歯。
///
/// 契約 file の parser は 1 行 1 値で escape を解かない（`pipe::contract`）ので、e2e の
/// 契約からは改行入りの goal を作れない——「改行を保つ」の側はここで測る。
#[cfg(test)]
mod tests {
    // flip-check: moved s2-07l.253
    // flip-check: moved s2-07l.457
    // flip-check: moved s2-07l.498
    use super::super::gate::{next_number, skip_record, Skipped};
    use super::{
        detection_needed, landed_sha, squash_message, subject_of, trailer_key, Terminal, CONTRACT_TRAILER,
        REQUIREMENTS_TRAILER, SHA_PREFIX, SUBJECT_CHARS, TERMINAL_TOKENS,
    };
    use crate::cli_outcome::RC_OK;
    use crate::fleet::{EventKind, Stage};

    // flip-check: retroactive s2-07l.222
    // flip-check: retroactive s2-07l.607
    /// `next_number` は record 数の次（1 始まり）で、主実測を省いた record を挟む 2 周分でも単調に増える。
    #[test]
    fn mutant_in_pipe_land_next_number_increases_across_two_rounds() {
        assert_eq!((0..4).map(next_number).collect::<Vec<u64>>(), vec![1, 2, 3, 4], "1 始まりの通し番号");
        let skipped = Skipped::main("tree");
        for (len, n) in [(0, 1), (3, 4)] {
            let record = skip_record(next_number(len), skipped);
            assert!(record.contains(&format!("\"n\":{n}")), "{record}");
        }
    }

    // flip-check: retroactive s2-07l.397
    /// (e) 検出線の要否は**閉じた接頭辞集合**で決まる（設計 §30）: dir は接頭辞・file は完全一致・空の列は偽。
    /// `cratesx/a.rs` は `crates/` に触れない（`/` を落とす変異を外す）。
    #[test]
    fn pipe_detection_scope_needed_is_a_closed_prefix_set() {
        let table: [(&[&str], bool); 7] = [
            (&["docs/design/pipeline.md"], false),
            (&["crates/toy/src/a.rs"], true),
            (&["Cargo.lock"], true),
            (&["rules/manifest.toml"], true),
            (&[".vessel.toml"], true),
            (&["cratesx/a.rs"], false),
            (&[], false),
        ];
        for (paths, want) in table {
            assert_eq!(detection_needed(paths.iter().copied()), want, "paths={paths:?}");
        }
        // 列の中に 1 つでも触れる path が在れば真（docs と crates が混ざった周は撃つ）。
        assert!(detection_needed(["README.md", "Cargo.toml"]), "混ざった周は撃つ");
        assert!(!detection_needed(["README.md", ".github/workflows/ci.yml", "docs/toy.md"]), "面の外だけなら省く");
    }

    /// 複数行の goal は **本文に逐語**（改行ごと）で載り、件名は先頭の文だけを持つ。
    /// 最終行は `run:` の trailer である。
    #[test]
    fn pipe_land_subject_keeps_multiline_goal_verbatim_in_body() {
        let goal = "## 何を作るか\n- 1 本目の行である。ここは件名に載らない\n- 2 本目の行";
        let bare = crate::pipe::fixture::contract(&[], &[]);
        let message = squash_message("s2-07l.130", goal, "s2-07l.130-1757600000", &bare);
        assert!(message.contains(&trailer_key(CONTRACT_TRAILER)), "fixture の契約は design を持つ: {message}");
        let mut lines = message.lines();
        assert_eq!(lines.next(), Some("s2-07l.130: 何を作るか"), "件名は先頭の文（`#` と空白を除く）");
        assert_eq!(lines.next(), Some(""), "件名の次は空行");
        assert!(message.contains(goal), "goal 全文が逐語で在る: {message}");
        assert!(
            message.lines().any(|line| line == "run: s2-07l.130-1757600000"),
            "run trailer が在る: {message}"
        );
    }

    /// `landed_sha` は **自分の便の `RunDone`** だけを読む（設計 contract-source.md §5 手順 3）。
    ///
    /// 置き場には他の便の event も並ぶ。便の弁別と kind の弁別のどちらか一方でも緩むと、**別の便が
    /// 着地した sha** で CI を照合し、その sha が success なら自分の便の bead を閉じてしまう。
    /// 他の便の行を**後に**置き、kind 違いの行に `sha:` を持たせて、両方の弁別を同時に測る。
    #[test]
    fn pipe_terminal_land_landed_sha_reads_only_its_own_run_done() {
        let root = crate::pipe::fixture::scratch("landed-sha");
        let mine = "0".repeat(40);
        let other = "1".repeat(40);
        let stray = "2".repeat(40);
        crate::pipe::fixture::append_all(
            &root,
            &[
                crate::pipe::fixture::event("mine", EventKind::RunDone, Some(Stage::Landed), None, Some(&format!("{SHA_PREFIX}{mine}"))),
                // kind 違いの行が同じ便に**後から**載る（`RunDone` 以外は読まない）。
                crate::pipe::fixture::event("mine", EventKind::RunStage, Some(Stage::Landed), None, Some(&format!("{SHA_PREFIX}{stray}"))),
                // 別の便の着地が**後から**載る（便の弁別が緩むとこちらを読む）。
                crate::pipe::fixture::event("other", EventKind::RunDone, Some(Stage::Landed), None, Some(&format!("{SHA_PREFIX}{other}"))),
            ],
        );
        assert_eq!(landed_sha(&root, "mine").as_deref(), Some(mine.as_str()), "自分の便の RunDone の sha");
        assert_eq!(landed_sha(&root, "other").as_deref(), Some(other.as_str()), "別の便からは別の sha");
        assert_eq!(landed_sha(&root, "absent"), None, "居ない便は None（HEAD に読み替えない）");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 終端の結末は**閉じた 7 値**で、字面は [`TERMINAL_TOKENS`] と 1 対 1（宣言順）。
    ///
    /// **close する側は 2 値だけ**である: 通った周（`Closed`）と、そもそも終端を持たない repo の周
    /// （`Undeclared`・rc 0）。残る 5 値はどれも close せず rc 1 で止まる——`Unreadable` を
    /// `Undeclared` と同じ側に倒すと「測れていない」が「終端が無い」に化ける（C10）。
    #[test]
    fn pipe_terminal_land_outcomes_are_the_closed_seven() {
        let listed = [
            Terminal::Closed,
            Terminal::Undeclared,
            Terminal::Unreadable,
            Terminal::PushFailed("git".to_owned()),
            Terminal::CiFailed,
            Terminal::CiUnmeasurable,
            // **前置きの字面は produce する側から採る**（fixture の literal で満たすと対の assert が空虚）。
            Terminal::CloseFailed(crate::ledger::CloseError::Unlaunchable.render()),
        ];
        let tokens: Vec<String> = listed.iter().map(Terminal::as_token).collect();
        assert_eq!(tokens.len(), TERMINAL_TOKENS.len(), "母集団 {} 値: {tokens:?}", TERMINAL_TOKENS.len());
        for (token, stem) in tokens.iter().zip(TERMINAL_TOKENS) {
            assert!(token.starts_with(stem), "宣言順の {stem} と対: {tokens:?}");
        }
        let ok: Vec<&String> = tokens.iter().zip(&listed).filter(|(_, found)| found.rc() == RC_OK).map(|(token, _)| token).collect();
        assert_eq!(ok, vec!["closed", "undeclared"], "rc 0 は 2 値だけ（母集団 {} 値）", listed.len());
    }

    /// **契約と要件の trailer**（設計 contract-source.md §5 手順 5）は `run:` の後ろに並び、key は器の名から
    /// 導く（他の道具の trailer と衝突しない）。**欄が空の周は行ごと書かない**——空の trailer は「無い」と
    /// 読めず、RTM が「まだ分からない」と言えなくなる。
    #[test]
    fn pipe_terminal_land_squash_message_carries_the_contract_and_requirements_trailers() {
        let goal = "自走の goal";
        let mut bare = crate::pipe::fixture::contract(&[], &[]);
        bare.design = String::new();
        let empty = squash_message("s2-x", goal, "s2-x-1", &bare);
        assert!(!empty.contains(&trailer_key(CONTRACT_TRAILER)), "欄の無い契約は trailer を書かない: {empty}");
        assert!(!empty.contains(&trailer_key(REQUIREMENTS_TRAILER)), "req が空なら要件の trailer も無い: {empty}");
        let mut filled = crate::pipe::fixture::contract(&[], &[]);
        filled.design = "docs/design/toy.md#a".to_owned();
        filled.req = vec!["FR1".to_owned(), "FR2".to_owned()];
        let message = squash_message("s2-x", goal, "s2-x-1", &filled);
        let tail: Vec<&str> = message.lines().rev().take(3).collect();
        assert_eq!(
            tail,
            vec![
                format!("{}FR1 FR2", trailer_key(REQUIREMENTS_TRAILER)).as_str(),
                format!("{}docs/design/toy.md#a", trailer_key(CONTRACT_TRAILER)).as_str(),
                "run: s2-x-1",
            ],
            "run の後ろに契約 → 要件の順: {message}"
        );
        assert!(trailer_key(CONTRACT_TRAILER).starts_with(char::is_uppercase), "key は器の名の大文字始まり");
    }

    /// 要旨が空（goal が空・先頭の文が空白と `#` だけ）の周は **`<bead>` だけ**の件名にして
    /// 落とさない（land を止める理由ではない）。
    #[test]
    fn pipe_land_subject_falls_back_to_bead_when_gist_is_empty() {
        assert_eq!(subject_of("s2-07l.130", ""), "s2-07l.130");
        assert_eq!(subject_of("s2-07l.130", "## \n本文だけ"), "s2-07l.130");
    }

    /// 切るのは **char 単位**である（byte で切ると UTF-8 の途中で割れる）。
    #[test]
    fn pipe_land_subject_cuts_by_chars_not_bytes() {
        let goal = "あ".repeat(SUBJECT_CHARS + 1);
        let subject = subject_of("b", &goal);
        assert_eq!(
            subject.chars().count(),
            "b: ".chars().count() + SUBJECT_CHARS + 1,
            "件名 = `b: ` + 72 文字 + `…`: {subject}"
        );
        assert!(subject.ends_with('…'), "切った印が付く: {subject}");
    }
}
