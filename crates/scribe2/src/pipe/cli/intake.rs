//! `pipe intake` の受付（設計 §5「subcommand」・pipeline-conflict.md §2「入口の排他」・contract-source.md §3）。
//!
//! 契約 file を読み、宣言を上限と突き合わせ、上限の余地と write-set の交差で断り、置き場へ写して run を
//! 起こす。`s2-07l.295` で `cli.rs` から純移動した（本文は不変・外から呼ぶ path は `cli` が持つ）。
//! 親の共通の材料（`need` / `refused` / `broken` / `state_dir_of` / `live` 等）は `super::` で引く。
//!
//! **write-set の弁別**（契約 (h)・contract-source.md §3「write-set の導出」「手書きの write-set の扱いと撃つ場所」）:
//! 契約の `design` が設計 pointer（`<doc>#<id>`）なら base の契約表の行を引き、行が `creates` / `tests` / `also` を
//! 1 つも持たず `write-set` を持てば [`WriteSet::Declared`]（(g) までの検査だけ）・それ以外は [`WriteSet::Derived`]
//! （導出値を作り、行に `write-set` が在れば集合一致を要り、無ければ導出値を契約の写しの write-set に書く）。
//! pointer でない `design`（(b) の前の契約 file）は従来どおり導出しない。**撃つのは受付だけ**（CI は撃たない）。
//!
//! **Promised の行**（契約表の行 ag・contract-source.md §33）: 行が約束の行（`[[promise]]`）を 1 つでも持てば
//! [`WriteSet::Promised`]。器は約束の行から write-set（[`closure::derive_promised`]＝§3 の導出の 1 本）と契約 file の
//! `verify` / `done`（[`crate::pipe::contract::promised_verify`] / [`crate::pipe::contract::promised_done`]）を生成し、
//! 行の値の代わりに写しへ載せる（設計 doc には書き戻さない）。行が導く欄を手で書いた周は `promised-field-written`・
//! `symbols` の名が base と合わない周は `promise-symbol-unresolved`・行の `verify` が生成値と集合で違う周は
//! `write-set-drift`（§3 と同じ照合）で断る。
//!
//! **judge と create**（契約表の行 u・contract-source.md §21・C2「判定関数は 1 本」）: 受付の判定は [`judge`]（run を作らない・
//! 断りを判定関数 1 本につき高々 1 件で**全部**集める）と [`create`]（run dir・写し・event）の 2 段で、`intake` = judge →
//! create（列の先頭の 1 件で断る＝従来の外形）・`pipe preflight`（[`super::preflight`]）= judge だけ。各判定関数
//! （[`freeze`] / [`settle_write_set`] / [`exclude_same_kind`] / [`exclude_unaddressed`] / [`exclude_cap_shortfall`] /
//! [`exclude_max_live`] / [`exclude_overlap`] / 重複 run）の中身と「先頭の 1 件で返す」形は不変で、Ok 値だけを事実（[`Headrooms`] / [`Crossed`]）へ
//! 広げる。
//!
//! **同型の停止と焼き直しの門**（契約表の行 w・contract-source.md §23・`s2-07l.396`）: 受付は置き場の replay から同じ bead
//! の便を新しい順に読み（[`history`]）、同じ理由の型（[`FindingKind`]）の審査 FAIL が rules 行 `review.same_kind_stop`
//! の本数続き契約 file と節の本文がともに不変の周を `same-kind-repeated` で、直前の便の指摘（`at`）に対応する差分の無い
//! 周を `finding-unaddressed` で断る。どちらも run dir も event も作らず、write-set の弁別の後・余地と交差の前に撃つ。

use super::base_run::{self, BaseRun, Early};
use super::{broken, flag, int_row, list_row, live, need, refused, repo_flag, repo_of, state_dir_of, REPO_FLAG};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_REFUSED};
use crate::fleet::store::{self, LockPolicy, StoreError};
use crate::fleet::{self, EventKind, Stage};
use crate::hook::command;
use crate::hook::host_guard::WORD_ROWS;
use crate::name::NAME;
use crate::pipe::closure::{self, ClosureError, Source};
use crate::pipe::contract::{Contract, ContractError, CLASS_ROW};
use crate::pipe::declaration::{self, Ceiling, Effective, EntranceFlip, NewFilePolicy, WriteSetItem, CEILING_ROW, DENIED_ROW};
use crate::pipe::refuse::{overlaps, Refuse, DELETE_FILE, NEW_FILE, PLACE_ONLY_FILE, SHRINK_FILE};
use crate::pipe::review::{self, FindingKind, Judgement, ROW_SAME_KIND_STOP};
use crate::pipe::table::{self, ContractRow, TableError};
use crate::pipe::{contract_path, current, emit, run_dir, run_id, vessel_path, Emit, CONTRACT_FILE};
use crate::rules::manifest::Manifest;
use crate::rules::RuleValue;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// 1 file の行数の上限を持つ rules 行（上限の余地の分子・設計 contract-source.md §3・値は読むだけ・C4）。
const ROW_FILE_LINES: &str = "R-C4-2";

/// core の総行数の上限を持つ rules 行（上限の余地・値は読むだけ・C4）。
const ROW_CORE_LINES: &str = "R-C4-1";

/// 行の数え方の幅を持つ rules 行（上限の余地の行数を xtask check と同じ式で数える・kind `LineWidth`）。
const ROW_LINE_WIDTH: &str = "R-C4.line-width";

/// host で同時に走る便（live な便）の本数の最大値を持つ rules 行（設計 gate-cost.md §24・値は読むだけ・C1）。
const ROW_MAX_LIVE: &str = "pipe.max_live";

/// 契約の `size` = S の 1 file あたりの増分の見積（行）を持つ rules 行。
const ROW_SIZE_S: &str = "pipe.size_s_lines";

/// 契約の `size` = M の見積を持つ rules 行。
const ROW_SIZE_M: &str = "pipe.size_m_lines";

/// 契約の `size` = L の見積を持つ rules 行。
const ROW_SIZE_L: &str = "pipe.size_l_lines";

/// 契約の write-set の出所（設計 contract-source.md §3 / §33・C10「導出値と宣言値を型で分ける」）。**閉じた 3 値**で、
/// 契約表の行の欄と約束の行の有無だけで決まる（散文の免除を持たない）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WriteSet {
    /// 器が導出した（`creates` / `tests` / `also` のどれかを持つ行と、新欄も `write-set` も無い行）。
    Derived,
    /// 行が手で列挙した（新欄を持たず `write-set` を持つ行・(h) の前の形）。
    Declared,
    /// 器が約束の行（`[[promise]]`）から導出した（約束の行を 1 つでも持つ行・§33）。
    Promised,
}

/// [`WriteSet`] の全 variant（宣言順・`enum-slices` が集合完全性を測る・読むのは pin の歯だけ）。
#[cfg_attr(not(test), expect(dead_code, reason = "宣言順 pin の歯だけが読む（`REFUSALS` と同じ形）"))]
pub(crate) const WRITE_SETS: &[WriteSet] = &[WriteSet::Derived, WriteSet::Declared, WriteSet::Promised];

impl WriteSet {
    /// 判定行の token の値。
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Derived => "derived",
            Self::Declared => "declared",
            Self::Promised => "promised",
        }
    }
}

/// 受付を通った便（`run` の連鎖は id だけを要る・`intake` は判定行に write-set の弁別も載せる）。
struct Intaken {
    /// run id。
    id: String,
    /// write-set の弁別と本数（設計 pointer を持たない契約は `None`＝従来の形）。
    write_set: Option<(WriteSet, usize)>,
    /// base の木で撃った周の欄（[`BaseRun::fact`]・名乗りの無い周は `None`＝1 行は従来の形・§56 形 5）。
    entrance: Option<String>,
}

/// 契約 file を読み込み、置き場へ写して run を起こし、**直後に審査の段を通す**（FR49・設計 contract-source.md
/// §4）。1 行目は受付の判定行・2 行目は審査の判定行で、rc は審査の verdict（PASS = 0 / FAIL = 1 /
/// INCONCLUSIVE = 3）＝受付は通っても PASS でない便は終端で、`run=<id>` は落ちた周も出す。
pub(super) fn intake(args: &[String], manifest: &Manifest, policy: LockPolicy) -> Outcome {
    match intake_run(args, manifest, policy) {
        Ok(found) => {
            let mut line = intake_line(args, &found.id);
            if let Some((kind, files)) = found.write_set {
                line.push_str(&format!(" write-set={} files={files}", kind.as_str()));
            }
            if let Some(fact) = found.entrance {
                line.push_str(&format!(" {fact}"));
            }
            let mut reviewed = super::step::review_run(args, &found.id, manifest, policy);
            reviewed.out.insert(0, line);
            reviewed
        }
        Err(outcome) => outcome,
    }
}

/// intake の 1 行。`--rules` で上限を差し替えて通した周は**その事実を同じ行に残す**
/// （`ceiling-overridden=<path>`・値は渡した path の字面そのもの・`s2-07l.65`）。
///
/// `--rules` は test の seam で、上限（`runner.allowed_commands`）を無条件に差し替える。
/// 差し替えた周が通常の周と同じ 1 行しか出さないと、review は「埋め込みの上限で通った便」と
/// 区別できない（`.56` lens M1）。差し替えていない周は出さない＝不在が既定。
pub(super) fn intake_line(args: &[String], id: &str) -> String {
    match flag(args, "--rules") {
        Ok(Some(path)) => format!("run={id} ceiling-overridden={path}"),
        _ => format!("run={id}"),
    }
}

/// intake の本体。**id を返す**のは `run` が続きの段へ渡すためである
/// （自分の stdout を読み直して id を取る形にすると、表示を変えた瞬間に連鎖が壊れる）。
pub(super) fn intake_id(args: &[String], manifest: &Manifest, policy: LockPolicy) -> Result<String, Outcome> {
    intake_run(args, manifest, policy).map(|found| found.id)
}

/// 受付の 1 周（id と write-set の弁別）= [`judge`] → [`create`]。
fn intake_run(args: &[String], manifest: &Manifest, policy: LockPolicy) -> Result<Intaken, Outcome> {
    let (pointer, bead, repo) = read_args(args).map_err(|denial| denial.outcome)?;
    // repo は spawn まで使わないが、**intake の時点で** git repo かを確かめる（judge も先頭で同じ検査を撃つが、intake は
    // 置き場と行を読む前に断る＝従来の順）。後段で初めて落ちると、契約は受理されたのに進めない run が残る。
    // HEAD の sha は名乗りに依らず材料の読みの前に読む（base の木で撃った後に読み直して比べる・§56 形 2）。
    let Some(sha) = super::head_of(&repo) else {
        return Err(not_a_repo(&repo).outcome);
    };
    let state_dir = state_dir_of(args).map_err(refused)?;
    // 上限・材料（設計 dispatcher.md §5 の 1 回）・行の生成と表の検査・freeze は**入口の lock の前**に 1 周に 1 回
    // （§56 形 2・base の木の実走の長さで入口を塞がない）。lock の中の judge は freeze の結果を借りる。
    let ceiling = ceiling_of(manifest).map_err(|denial| denial.outcome)?;
    let materials = Materials::read(&repo, &ceiling.borrow()).map_err(|denial| denial.outcome)?;
    let (contract, body) = generated(&repo, &pointer, &materials).map_err(|denial| denial.outcome)?;
    let early = early(&repo, manifest, &contract, Some(&state_dir), &sha);
    // **入口の排他はここから**（ADR-0019 §2.1・設計 pipeline-conflict.md §2）: [`judge`] と [`create`] を
    // 1 つの周として閉じる。持たないと、同時に来た 2 つの受付がどちらも「live な便は無い」と読んでから
    // 両方が run を作る（`s2-07l.366` の実測: 契機を同時に 2 回撃つと 20 回に 1 回 2 本作られた）。
    let _entrance = Entrance::hold(&state_dir, policy)?;
    let material = Material {
        repo: &repo, manifest, contract: &contract, state_dir: Some(&state_dir), bead: &bead, materials: &materials,
        early: Some(&early),
    };
    create(judge(&material), &material, &state_dir, &body, policy)
}

/// lock の前の 1 回の読み（§56 形 2 / 3）: freeze を撃ち、宣言が `detect` / `deny` を名乗る周だけ契約の検証行を base の木で撃つ。
pub(super) fn early(repo: &Path, manifest: &Manifest, contract: &Contract, state_dir: Option<&Path>, sha: &str) -> Early {
    let frozen = freeze(repo, manifest, contract);
    let fires = frozen.as_ref().is_ok_and(|(_, named)| named.is_some_and(EntranceFlip::fires_on_base));
    let base = fires.then(|| base_run::run(repo, state_dir, sha, &contract.verify, manifest));
    Early { frozen, base }
}

/// 受付の入口の lock file の名（置き場の直下・**run dir の側に置かない**）。
///
/// `pipe/` の下に置くと「断った周は run dir を 1 つも作らない」を測る既存の歯が、lock file を run dir と
/// 数えて落ちる（`s2-07l.366` の実測 4 本）。入口の lock は便ではないので便の置き場に混ぜない。
const ENTRANCE_LOCK: &str = "intake.lock";

/// 受付の入口の排他（**[`judge`] と [`create`] を 1 つの周として閉じる**・ADR-0019 §2.1）。
///
/// event log の lock（[`store::append_line`] が中で取る）とは**別の lock file** である——同じものを外から
/// 握ると、`create` の記帳が自分の握った lock を待って止まる。`Drop` で外すので、判定のどの断りに落ちても
/// 残らない。
struct Entrance {
    /// 握っている lock file。
    lock: PathBuf,
}

impl Entrance {
    /// 入口を 1 つだけ通す（取れない周は rc 2＝置き場が壊れている側）。
    fn hold(state_dir: &Path, policy: LockPolicy) -> Result<Self, Outcome> {
        std::fs::create_dir_all(state_dir).map_err(|err| broken(format!("受付の入口を作れない: {err}")))?;
        let lock = state_dir.join(ENTRANCE_LOCK);
        store::acquire(&lock, policy).map_err(|err| broken(format!("受付の入口の lock を取れない: {err}")))?;
        Ok(Self { lock })
    }
}

impl Drop for Entrance {
    fn drop(&mut self) {
        // 外せない lock は次の受付が所有者の生死で回収する（ここで止めない）。
        let _ = std::fs::remove_file(&self.lock);
    }
}

/// rules 行から allowlist と禁じる語を読んで [`Ceiling`] の材料を持つ（借りる側は [`Rows::borrow`]）。
pub(in crate::pipe) struct Rows {
    /// 通す語列（`runner.allowed_commands`）。
    commands: Vec<String>,
    /// 禁じる語列（`runner.denied_commands`）。
    denied: Vec<String>,
    /// クラスの語列表（[`CLASS_ROW`]・設計 contract-source.md §48 の 5）。
    classes: Vec<String>,
}

impl Rows {
    /// 借りた形の上限（`row` は同じ 1 つの rules 行 id）。
    pub(in crate::pipe) fn borrow(&self) -> Ceiling<'_> {
        Ceiling { row: CEILING_ROW, commands: &self.commands, denied: &self.denied, classes: &self.classes }
    }
}

/// 1 周ぶんの repo の材料（**読みは 1 周に 1 回**・設計 dispatcher.md §5・契約表の行 b）。
///
/// base の tree の走査（tracked の一覧・`.rs`・`.snap`）と、宣言から解いた契約表の facts は**契約 1 本ごとに
/// 変わらない**。受付は 1 便で 1 回読むだけだが、列（[`crate::pipe::dispatch`]）は候補の数だけ [`generated`] と
/// [`judge`] を撃つので、読み直すと 1 周が候補数に比例して伸びる（`s2-07l.345` の実測: 候補 1 件あたり 0.2 秒）。
/// **判定の関数は不変で、材料を受け取る引数だけを足す**（C2）。
pub(in crate::pipe) struct Materials {
    /// base の tracked file の repo 相対 path。
    tracked: Vec<String>,
    /// base の `.rs`（閉包を測る側）。
    sources: Vec<Source>,
    /// base の `.snap`（外形 pin を測る側）。
    snapshots: Vec<Source>,
    /// 宣言から解いた allowlist / 禁じる語 / 要件面の path。
    facts: declaration::TableFacts,
    /// 要件面の id の集合（読めない周は理由・表の検査がそのまま断る）。
    requirements: Result<BTreeSet<String>, String>,
    /// repo の全 doc が宣言済みの新規 file（`contracts check` と同じ 1 本 [`table::declared_files`]・読めない周は理由）。
    declared: Result<Vec<String>, String>,
    /// クラスの語列表（上限の [`Ceiling`] から借りた写し・表の検査が verify 行から 3 クラスを導く）。
    classes: Vec<String>,
}

impl Materials {
    /// repo を 1 回走査して材料を読む。
    ///
    /// 断りの**順序は従来のまま**である（tracked を読めない＝git repo でない〔rc 2〕→ 宣言が読めない・上限に
    /// 外れる〔rc 1〕）。順を替えると「宣言が壊れている」便が「表を読めない」に化ける。
    pub(in crate::pipe) fn read(repo: &Path, ceiling: &Ceiling<'_>) -> Result<Self, Denial> {
        let Some(tracked) = table::tracked_files(repo) else {
            let reason = format!("{} の tracked file を読めない（git repo でない）", repo.display());
            return Err(refuse(&Refuse::ContractTable(TableError::Unreadable { line: 0, reason }), &[]));
        };
        let facts = declaration::table_facts(repo, ceiling).map_err(|errors| {
            denied(DENIAL_DECLARATION, Outcome::failed(RC_REFUSED, errors.iter().map(ToString::to_string).collect()))
        })?;
        let sources = table::read_all(repo, &tracked, ".rs");
        let snapshots = table::read_all(repo, &tracked, ".snap");
        let requirements =
            table::read(repo, &facts.requirements).and_then(|found| table::requirement_ids(&facts.requirements, &found));
        let declared = table::declared_files(repo, &tracked);
        Ok(Self { tracked, sources, snapshots, facts, requirements, declared, classes: ceiling.classes.to_vec() })
    }

    /// rules 行の上限から材料を読む（列の入口・上限の読みと base の走査を 1 本にまとめた口）。
    pub(in crate::pipe) fn of(repo: &Path, manifest: &Manifest) -> Result<Self, Denial> {
        let rows = ceiling_of(manifest)?;
        Self::read(repo, &rows.borrow())
    }

    /// base の tracked file（交差の dir の展開が読む・**2 本目の走査を作らない**ための借り）。
    pub(in crate::pipe) fn tracked(&self) -> &[String] {
        &self.tracked
    }

    /// 閉包を測る base（`.rs` / `.snap` / tracked の 3 面を 1 つに束ねた借り）。
    fn base(&self) -> closure::Base<'_> {
        base_of(&self.sources, &self.snapshots, &self.tracked)
    }

    /// 表の検査の ctx（`contracts check` と同じ材料の形）。
    fn context(&self) -> table::Context<'_> {
        table::Context {
            allowed: &self.facts.allowed,
            denied: &self.facts.denied,
            classes: &self.classes,
            requirements: &self.requirements,
            sources: &self.sources,
            tracked: &self.tracked,
            snapshots: &self.snapshots,
            declared: &self.declared,
        }
    }
}

/// 上限の材料を rules 行から読む（読めない周は [`DENIAL_RULES`] の断り・[`freeze`] と同じ 2 行）。
pub(super) fn ceiling_of(manifest: &Manifest) -> Result<Rows, Denial> {
    let rules = |reason| denied(DENIAL_RULES, refused(reason));
    let commands = list_row(manifest, CEILING_ROW).map_err(rules)?;
    let denied_commands = denied_rows(manifest).map_err(rules)?;
    Ok(Rows { commands, denied: denied_commands, classes: list_row(manifest, CLASS_ROW).map_err(rules)? })
}

/// 禁じる語列: command guard の ∪ の読み手 1 本（[`command::denied_of`]・`runner.denied_commands` ∪ host-guard の語列の
/// 3 行・設計 vessel-hook.md §11 の形 f 3）。揃わない周は欠けた行を名指す（`runner.denied_commands` は従来の字面）。
/// 断り文の行 id は `declaration` が持つ従来の `runner.denied_commands` のまま（限界・後続の純移動で直す）。
fn denied_rows(manifest: &Manifest) -> Result<Vec<String>, String> {
    list_row(manifest, DENIED_ROW)?;
    let sources = command::denied_of(manifest).ok_or_else(|| {
        let id = WORD_ROWS.iter().find(|id| !matches!(manifest.get(id).map(|row| &row.value), Some(RuleValue::List(_))));
        format!("{} が無いか文字列の列でない", id.unwrap_or(&DENIED_ROW))
    })?;
    Ok(sources.into_iter().flat_map(|(_, sequences)| sequences).collect())
}

/// intake / preflight が同じ形で読む引数（`--design` / `--bead` / `--repo`・欠けは理由の 1 行）。
///
/// 契約 (b) 以後、受付が受けるのは**設計 pointer だけ**である（`<doc>#<id>`）。手書きの契約 file
/// （`--contract`）は使い方の誤りでなく [`Refuse::HandWrittenContract`] で断る（FR54）＝「渡し方を間違えた」
/// ではなく「契約の正本はそこに無い」と名乗る。pointer の形が壊れている周は理由の 1 行で断る。
pub(super) fn read_args(args: &[String]) -> Result<(table::Pointer, String, PathBuf), Denial> {
    if let Ok(Some(path)) = flag(args, FLAG_CONTRACT) {
        return Err(refuse(&Refuse::HandWrittenContract { path: path.to_owned() }, &[]));
    }
    let read = |name: &str| need(args, name).map_err(|reason| denied(DENIAL_ARGS, refused(reason)));
    let design = read("--design")?.to_owned();
    let bead = read("--bead")?.to_owned();
    // repo の読み手は [`repo_flag`] の 1 本（絶対 path に直す・設計 dispatcher.md §12）。必須の断りは `need` と同じ字面。
    let repo = repo_flag(args)
        .and_then(|found| found.ok_or(format!("{REPO_FLAG} が要る")))
        .map_err(|reason| denied(DENIAL_ARGS, refused(reason)))?;
    let pointer = table::parse_pointer(&design)
        .map_err(|err| denied(DENIAL_ARGS, refused(format!("--design {design} は設計 pointer の形でない（{}）", err.reason()))))?;
    Ok((pointer, bead, repo))
}

/// 廃止した手書きの契約 file の flag（字面だけ残して断る側に使う・契約 (b)）。
const FLAG_CONTRACT: &str = "--contract";

/// 引数の形が読めない周の名（[`Refuse`] を持たない断り）。
const DENIAL_ARGS: &str = "args";

/// base の設計 pointer から契約を組む（契約 (b)・設計 contract-source.md §2「生成」）。
///
/// 読む先は**作業木でなく base（`HEAD`）**である（[`crate::pipe::show_head`]）: 記録する base と同じ commit の
/// 行だけが契約の正本で、commit していない書きかけを受け付けると runner が base で見るものと食い違う。
/// 行を引いたら **(a) の [`table::check_table`] を同じ ctx でその 1 行に撃ち**（1 実装・C2）、findings が 1 件でも
/// 在れば先頭を理由に断る（run dir を作らない・FR48 / FR54）。
pub(in crate::pipe) fn generated(
    repo: &Path,
    pointer: &table::Pointer,
    materials: &Materials,
) -> Result<(Contract, String), Denial> {
    let Some(text) = crate::pipe::show_head(repo, &pointer.path) else {
        let reason = format!("{} を base（HEAD）から読めない", pointer.path);
        return Err(refuse(&Refuse::ContractTable(TableError::Unreadable { line: 0, reason }), &[]));
    };
    let row = table::find_row(&pointer.path, &text, &pointer.id).map_err(|errors| {
        let rest: Vec<String> = errors.iter().skip(1).map(|error| format!("pipe: {}", error.reason())).collect();
        let first = errors.into_iter().next().unwrap_or(TableError::RowMissing { line: 0, id: pointer.id.clone() });
        refuse(&Refuse::ContractTable(first), &rest)
    })?;
    let findings = check_row(&pointer.path, &text, &row, materials);
    if !findings.is_empty() {
        // 名は `Refuse::ContractTable` の側から取る（字面を 2 か所に書かない・C1）。findings は表の検査の
        // 描画をそのまま並べる（`contracts check` と 1 byte 同じ行＝読み手が 2 つの形を覚えない）。
        let name = Refuse::ContractTable(TableError::RowMissing { line: 0, id: String::new() }).as_str();
        let rc = findings.iter().map(table::Finding::rc).fold(RC_REFUSED, u8::max);
        let lines = findings.iter().map(|finding| finding.render(&pointer.path)).collect();
        return Err(denied(name, Outcome::failed(rc, lines)));
    }
    let design = format!("{}#{}", pointer.path, pointer.id);
    // 行が `write-set` を持たない周（Derived の行・§3「write-set の導出」）は**導出値**を写しに書く。
    // 契約 file は write-set を 1 本以上要るので、空のまま書くと器が自分の生成物を読めない。
    // 導出は行と base だけで決まるので、後段の [`settle_write_set`] と同じ 1 実装をここで撃つ（C2）。
    // Promised の行（§33）は約束の行からの生成値（write-set / verify / done）を行の値の代わりに渡す。
    let (row, write_set) = match promised(&pointer.path, &text, &row, materials)? {
        Some(found) => (generated_row(&row, &found), found.write_set),
        None if row.write_set.is_empty() => {
            let derived = derived_write_set(&row, materials)?;
            (row, derived)
        }
        None => {
            let written = row.write_set.clone();
            (row, written)
        }
    };
    let body = crate::pipe::contract::render(&row, &design, &write_set);
    let contract = Contract::parse(&body).map_err(|errors| {
        denied(DENIAL_GENERATED, unloadable(errors))
    })?;
    Ok((contract, body))
}

/// 回答後の再開が写しを取り直す本文（設計 pipeline-question.md §11・契約表の行 a）。
///
/// 写しの `design` が設計 pointer なら受付と**同じ導出**（[`Materials::of`] → [`generated`]）で本文を組み直して返す
/// （受付が写す byte と同じ形＝行が変わらなければ写しと 1 byte も違わない）。pointer でない `design` は `Ok(None)`
/// （取り直す行が無い）。行が受付を通らない周（行が消えた・doc を読めない・表の検査が 1 件でも出る）は受付の断りの
/// 字面をそのまま rc 1 で返す（起こさない・写しと event は呼び手が触らない）。
///
/// 上限の command は**便の写しの有効値**（run dir の `vessel.toml`・受付が上限と突き合わせて凍結した allowlist）で、
/// 禁じる語列は `manifest` の行（追随の [`crate::pipe::follow::stale_rows_in`] と同じ借り方＝resume は受付の `--rules`
/// を持たない）。受付の後に宣言の allowlist が広がった周は上限の外として断る側へ倒れる。
pub(super) fn regenerated(
    repo: &Path,
    state_dir: &Path,
    id: &str,
    manifest: &Manifest,
    design: &str,
) -> Result<Option<String>, Outcome> {
    let Ok(pointer) = table::parse_pointer(design) else {
        return Ok(None);
    };
    let refused_as = |denial: Denial| Outcome { rc: RC_REFUSED, ..denial.outcome };
    let frozen = Effective::load(&vessel_path(state_dir, id))
        .map_err(|errors| Outcome::failed(RC_BROKEN, errors.iter().map(ToString::to_string).collect()))?;
    let denied_commands = list_row(manifest, DENIED_ROW).map_err(refused)?;
    let classes = list_row(manifest, CLASS_ROW).map_err(refused)?;
    let ceiling = Ceiling { row: CEILING_ROW, commands: frozen.allowed(), denied: &denied_commands, classes: &classes };
    let materials = Materials::read(repo, &ceiling).map_err(refused_as)?;
    let (_, body) = generated(repo, &pointer, &materials).map_err(refused_as)?;
    Ok(Some(body))
}

/// 生成した写しを器自身が読めない周の名（生成の不備＝壊れた器・rc 2）。
const DENIAL_GENERATED: &str = "generated";

/// 行から導いた write-set（[`settle_write_set`] と同じ [`closure::derive_write_set`] を撃つ）。
fn derived_write_set(row: &ContractRow, materials: &Materials) -> Result<Vec<String>, Denial> {
    let fields = fields_of(row);
    let derived =
        closure::derive_write_set(&fields, &materials.base()).map_err(|error| refuse(&refuse_of(error, row), &[]))?;
    Ok(derived.into_iter().collect())
}

/// Promised の行（設計 contract-source.md §33）の生成値（契約 file に行の値の代わりに載せる）。
struct Generated {
    /// 約束の行から導いた write-set（§3 の導出の 1 本の値・辞書順）。
    write_set: Vec<String>,
    /// 歯を（crate・scope）で束ねた nextest 行。
    verify: Vec<String>,
    /// `n` の順の「(n) expect」の 1 文。
    done: String,
}

/// 行が約束の行を持てば（Promised）生成値を組む（持たない行は `Ok(None)`＝Declared / Derived の判定は不変）。
///
/// 断る順: 器が導く欄を行が書いた（`promised-field-written`・書かれた欄を全部名指す）→ `symbols` の名が base と合わない
/// （`promise-symbol-unresolved`）→ 導出の欄が解けない（§3 と同じ理由）→ 行の `verify` が生成値と集合で違う（§3 と同じ
/// `write-set-drift`）。約束の行は `text`（base の設計 doc の本文）を同じ parse で読み直して引く。
fn promised(path: &str, text: &str, row: &ContractRow, materials: &Materials) -> Result<Option<Generated>, Denial> {
    let (_, promises) = table::read_table(path, text).unwrap_or_default();
    let own = table::promises_of(&promises, &row.id);
    if own.is_empty() {
        return Ok(None);
    }
    let fields = written_fields(row);
    if !fields.is_empty() {
        return Err(refuse(&Refuse::PromisedFieldWritten { row: row.id.clone(), fields }, &[]));
    }
    check_symbols(&own, materials).map_err(|error| refuse(&error, &[]))?;
    let (derived, inputs) =
        closure::derive_promised(&own, &materials.base()).map_err(|error| refuse(&refuse_of(error, row), &[]))?;
    let verify = crate::pipe::contract::promised_verify(&inputs.teeth);
    if !row.verify.is_empty() {
        let wanted: BTreeSet<String> = verify.iter().cloned().collect();
        closure::check_drift(&row.verify, &wanted).map_err(|error| refuse(&refuse_of(error, row), &[]))?;
    }
    let done = crate::pipe::contract::promised_done(&own);
    Ok(Some(Generated { write_set: derived.into_iter().collect(), verify, done }))
}

/// Promised の行が書いてはならない欄のうち書かれたもの（欄の宣言順＝`FIELDS` の順）。
fn written_fields(row: &ContractRow) -> Vec<String> {
    let lists = [
        ("touches", &row.touches),
        ("surfaces", &row.surfaces),
        ("write-set", &row.write_set),
        ("creates", &row.creates),
        ("tests", &row.tests),
        ("also", &row.also),
    ];
    let mut found: Vec<String> =
        lists.iter().filter(|(_, items)| !items.is_empty()).map(|(name, _)| (*name).to_owned()).collect();
    if !row.done.is_empty() {
        found.push("done".to_owned());
    }
    found
}

/// 約束の行の `symbols` の実在（§33 項 5・[`closure::symbols_in_base`] の同じ読み手）: `+` 無しの名は base に解け、`+` 付き
/// の名は base に**無い**こと（`creates` の `MustBeAbsent` と同じ極性）。名指しの形でない字面は測れない（下界）。
fn check_symbols(own: &[&table::PromiseRow], materials: &Materials) -> Result<(), Refuse> {
    for promise in own {
        let names: Vec<&str> = promise.symbols.iter().map(|name| name.strip_prefix(NEW_FILE).unwrap_or(name)).collect();
        let found = closure::symbols_in_base(&names, &materials.tracked, &materials.sources)
            .map_err(|error| Refuse::ContractTable(TableError::Unreadable { line: promise.line, reason: error.reason() }))?;
        for (name, in_base) in promise.symbols.iter().zip(found) {
            if in_base == Some(name.starts_with(NEW_FILE)) {
                return Err(Refuse::PromiseSymbolUnresolved { of: promise.of.clone(), n: promise.n, name: name.clone() });
            }
        }
    }
    Ok(())
}

/// 生成値を行の値の代わりに持つ行（`verify` / `done` を差し替える・他の欄は行のまま）。
fn generated_row(row: &ContractRow, generated: &Generated) -> ContractRow {
    ContractRow { verify: generated.verify.clone(), done: generated.done.clone(), ..row.clone() }
}

/// 行 1 つに (a) の表の検査を撃つ（`contracts check` と**同じ 1 実装**・C2）。ctx（allowlist / 禁じる語 /
/// 要件面 / base の tree）は `contracts check` と同じ材料から組み、doc の本文は base（`HEAD`）の字面を渡す。
///
/// 材料（tracked / sources / snapshots / facts）は [`Materials::read`] が 1 周に 1 回だけ読む。base の tree を
/// 読めない周と宣言が上限に外れる周は、その読みの時点で断る（順序は従来のまま）。
///
/// 検査する行は 1 つのままだが、`depends` の解決の母集団は**同じ doc の全行の id**（§30・行 ad）: 既に読んだ
/// base の本文から全行を引き直して id だけを渡す（新しい読みは足さない・閉包と名指しは当該行にだけ撃つ）。
/// 行は [`table::find_row`] で既に読めているので、全行の読みが落ちる周は無い（落ちれば母集団は空＝相手の在る
/// `depends` も断る側に倒れる・黙って通さない）。
fn check_row(path: &str, text: &str, row: &ContractRow, materials: &Materials) -> Vec<table::Finding> {
    let rows = table::read_rows(path, text).unwrap_or_default();
    let ids: Vec<&str> = rows.iter().map(|other| other.id.as_str()).collect();
    table::check_table(text, std::slice::from_ref(row), &ids, &materials.context())
}

/// 契約 file が読めない周の断り（rc 2・理由を全件出す）。
pub(super) fn unloadable(errors: Vec<ContractError>) -> Outcome {
    Outcome::failed(RC_BROKEN, errors.iter().map(ToString::to_string).collect())
}

/// 対象が git repo でない断り。
fn not_a_repo(repo: &Path) -> Denial {
    refuse(&Refuse::NotARepo { repo: repo.display().to_string() }, &[])
}

/// 判定関数 1 本の断り（**先頭の 1 件が理由**・後続行は stderr に並ぶ・§21）。intake は [`Self::outcome`] で従来どおりの
/// rc と stderr で断り、preflight は [`Self::name`] と理由を `refuse=` の行に写す。
#[derive(Clone)]
pub(in crate::pipe) struct Denial {
    /// 理由の名（[`Refuse::as_str`]・[`Refuse`] を持たない断り〔宣言の写し / rules 行 / 置き場の読み〕は材料の名）。
    pub(in crate::pipe) name: &'static str,
    /// 従来の断り（rc は理由が持つ・stderr の行）。
    pub(super) outcome: Outcome,
}

/// 宣言の写し（`freeze`）が外れた周の名（[`Refuse`] の variant を持たない断り）。
const DENIAL_DECLARATION: &str = "declaration";

/// rules 行が読めない周の名。
const DENIAL_RULES: &str = "rules";

/// 契約の `size` が S / M / L のどれでもない周の名。
const DENIAL_SIZE: &str = "size";

/// 置き場の store が読めない周の名。
const DENIAL_STORE: &str = "store";

/// [`Refuse`] を持たない断りを [`Denial`] に写す（名は材料の側・rc と行は `outcome` のまま）。
fn denied(name: &'static str, outcome: Outcome) -> Denial {
    Denial { name, outcome }
}

/// judge が読む材料（run を作らずに揃う値・intake と preflight が同じ 1 本を撃つ・C2）。
pub(in crate::pipe) struct Material<'a> {
    /// 対象 repo（base = HEAD）。
    pub(in crate::pipe) repo: &'a Path,
    /// 規則の値（上限と余地の行）。
    pub(in crate::pipe) manifest: &'a Manifest,
    /// 読み込み済みの契約 file。
    pub(in crate::pipe) contract: &'a Contract,
    /// 置き場（交差と重複 run の 2 検査だけが読む・`None` = 撃たず overlap は unmeasured・intake は常に `Some`）。
    pub(in crate::pipe) state_dir: Option<&'a Path>,
    /// 1 周ぶんの repo の材料（**呼び手が 1 回読む**・設計 dispatcher.md §5）。
    pub(in crate::pipe) materials: &'a Materials,
    /// 契約の bead id（この秒の run id の材料）。
    pub(in crate::pipe) bead: &'a str,
    /// lock の前に撃った freeze と base の木の実走（§56 形 2・`None` = 列の候補＝judge が freeze を撃ち base は撃たない）。
    pub(in crate::pipe) early: Option<&'a Early>,
}

/// [`judge`] の結果: 事実（§21 の 1 行 1 事実の材料・関数が Err の周はその関数の事実が無い）と断りの列（判定関数 1 本
/// につき高々 1 件・撃った順＝intake が先頭で断る順）。
pub(in crate::pipe) struct Judged {
    /// 設計 pointer の字面と行の § 番号（pointer でない `design` と行の解けない周は `None`）。
    pub(super) design: Option<(String, String)>,
    /// write-set の弁別と本数（`settle_write_set` が Ok で pointer の在る周）。
    pub(super) write_set: Option<(WriteSet, usize)>,
    /// verify の nextest 行ごとの (filter 語, base の歯の file)。
    pub(super) teeth: Vec<(String, Vec<String>)>,
    /// 上限の余地（`exclude_cap_shortfall` が Ok の周）。
    pub(super) headroom: Option<Headrooms>,
    /// live との交差（`exclude_overlap` が Ok の周・置き場が無い周は撃たない）。
    pub(super) overlap: Option<Crossed>,
    /// 断りの列。
    pub(in crate::pipe) denials: Vec<Denial>,
    /// 導出値で写しの write-set を置き換える周の導出値（create が写しに書く）。
    derived: Option<Vec<String>>,
    /// 宣言の有効値（`freeze` が Ok の周・create が写す）。
    effective: Option<Effective>,
    /// この秒の run id（置き場が在る周・重複 run の検査と create が同じ id を読む）。
    run: Option<String>,
}

/// 受付の判定（run を作らない・§21）。判定関数を `freeze` → `settle_write_set` → `exclude_cap_shortfall` →
/// `exclude_max_live` → `exclude_overlap` → 重複 run の順に**全部撃ち**、各関数が返した断りを列に積む。前段の Ok 値を取るのは導出値で
/// write-set を置き換える 1 点だけで、`settle_write_set` が Err の周は契約 file の write-set のまま後段を撃つ
/// （Declared 行は元々置き換えが無い＝前段と後段の断りが同時に載る）。git repo でない対象は他の関数が撃てないので
/// `not-a-repo` の 1 件で止まる。
pub(in crate::pipe) fn judge(material: &Material<'_>) -> Judged {
    let Material { repo, manifest, contract, state_dir, bead, materials, early } = *material;
    let mut judged = Judged {
        design: None,
        write_set: None,
        teeth: Vec::new(),
        headroom: None,
        overlap: None,
        denials: Vec::new(),
        derived: None,
        effective: None,
        run: None,
    };
    // base の tracked file の一覧（交差の dir の展開と上限の余地が読む・設計 contract-source.md §3）。
    // **走査は [`Materials::read`] が 1 周に 1 回だけ撃つ**（設計 dispatcher.md §5）ので、ここでは借りるだけ。
    if super::head_of(repo).is_none() {
        judged.denials.push(not_a_repo(repo));
        return judged;
    }
    let tracked = materials.tracked.as_slice();
    // **宣言は上限と突き合わせてから**。ここで断つ周は run dir も event も作らない
    // ——撃てない契約の run が置き場に残ると、続きから引ける便に見えてしまう。lock の前に撃った周は結果を借りる（§56 形 2）。
    match early.map_or_else(|| freeze(repo, manifest, contract), |found| found.frozen.clone()) {
        Ok((found, _)) => judged.effective = Some(found),
        Err(denial) => judged.denials.push(denial),
    }
    // **write-set の弁別は余地と交差より前**（導出値が write-set になる周は、その導出値で余地と交差を測る）。
    let mut measured = contract.clone();
    match settle_write_set(repo, contract, materials) {
        Ok(settled) => {
            judged.write_set = settled.as_ref().map(|found| (found.kind, found.files));
            judged.derived = settled.and_then(|found| found.replaced);
            if let Some(files) = judged.derived.as_deref() {
                measured.write_set = files.to_vec();
            }
        }
        Err(denial) => judged.denials.push(denial),
    }
    row_facts(repo, contract, materials, &mut judged);
    // **同型の停止と焼き直しの門は余地と交差より前**（§23）: 便の履歴で断る周は、余地と交差を測るまでもない。
    exclude_repeats(material, &measured, &mut judged);
    // **上限の余地は受付だけが撃つ**（§3「撃つ場所は受付だけ」）: その便を今の base に当てたら入るか、という
    // 受付時点の事実で、CI の `contracts check` は撃たない（表は履歴を持つ）。
    match exclude_cap_shortfall(manifest, &measured, materials) {
        Ok(found) => judged.headroom = Some(found),
        Err(denial) => judged.denials.push(denial),
    }
    if let Some(denial) = exclude_entrance_not_red(early) {
        judged.denials.push(denial);
    }
    let Some(state_dir) = state_dir else {
        return judged;
    };
    // **同時本数の上限は交差の前**（設計 gate-cost.md §24）。短絡しない＝上限で断る周も交差は撃ち、組は後続に並ぶ。
    if let Err(denial) = exclude_max_live(manifest, state_dir, &measured, tracked) {
        judged.denials.push(denial);
    }
    // **入口で排他する**（ADR-0019 §2.1）。live な便と write-set が交差する契約は、
    // run dir も event も作らずに断る——後段（land の rebase）で衝突を知るより安い。
    match exclude_overlap(state_dir, &measured, tracked) {
        Ok(found) => judged.overlap = Some(found),
        Err(denial) => judged.denials.push(denial),
    }
    let id = run_id(bead, &fleet::cli::now_utc());
    // stamp は秒までなので、同じ bead を同じ秒に 2 回 intake すると id が衝突する。
    // 黙って上書きすると **前の便の契約が別物に化ける**ので、何も書かずに断る。
    if run_dir(state_dir, &id).exists() {
        judged.denials.push(refuse(&Refuse::DuplicateRun { run: id.clone() }, &[]));
    }
    judged.run = Some(id);
    judged
}

/// `deny` の名乗りで base の木で撃った結果に base で緑か測れない行が 1 本以上在る周の断り（§56 形 6・余地の判定の後・置き場の
/// 要る判定の前）。結果を持たない judge（列の候補）と他の名乗りは立てない。
fn exclude_entrance_not_red(early: Option<&Early>) -> Option<Denial> {
    let base = early.and_then(Early::denying)?;
    let count = base.not_red();
    (count > 0).then(|| refuse(&Refuse::EntranceNotRed { count, values: base.values() }, &[]))
}

/// 便を作る（run dir・写し・event）。judge の断りが 1 件でも在れば**先頭の 1 件**で断り、何も書かない（従来の外形）。
fn create(
    judged: Judged,
    material: &Material<'_>,
    state_dir: &Path,
    body: &str,
    policy: LockPolicy,
) -> Result<Intaken, Outcome> {
    let Judged { write_set, denials, derived, effective, run, .. } = judged;
    if let Some(first) = denials.into_iter().next() {
        return Err(first.outcome);
    }
    // 断り 0 の周は freeze が Ok（有効値が在る）で、置き場 `Some` の judge は run id を持つ。
    let (Some(effective), Some(id)) = (effective, run) else {
        return Err(broken("受付の判定が有効値と run id を持たない".to_owned()));
    };
    write_contract(state_dir, &id, body, derived.as_deref()).map_err(broken)?;
    copy_vessel(state_dir, &id, &effective).map_err(broken)?;
    remember_repo(state_dir, &id, material.repo).map_err(broken)?;
    let base = material.early.and_then(|found| found.base.as_ref());
    if let Some(found) = base {
        found.write(&run_dir(state_dir, &id)).map_err(broken)?;
    }
    let emitted = emit(
        state_dir,
        &Emit {
            kind: EventKind::RunCreated,
            run: &id,
            bead: material.bead,
            stage: Some(Stage::Intake),
            seat: None,
            pid: None,
            detail: Some(format!("classes:{}", material.contract.classes.join("+"))),
        },
        policy,
    );
    match emitted {
        Err(err) => Err(broken(err.to_string())),
        Ok(()) => Ok(Intaken { id, write_set, entrance: base.map(BaseRun::fact) }),
    }
}

/// 行の事実（設計 pointer と § 番号・verify の nextest 行ごとの歯の置き場）を judge に載せる（§21・判定はしない）。
/// pointer でない `design` と行の解けない周（`settle_write_set` が同じ根で断る）は載せず、読めない `.rs` が在る周は
/// 歯の置き場を測れない（Unreadable の断りが立つ）ので `teeth` を載せない。
///
/// 歯の置き場は導出 (ii) と Declared 行の門が撃つ [`closure::teeth_places`] の同じ 1 実装で、行ごとに `tests` 欄を空に
/// して読む（`tests` の file は置き場でなく write-set の側）。filter 語も同じ 1 実装から取る: 本文 0 本で撃つと nextest
/// 形の行は必ず [`ClosureError::TeethPlaceUnresolved`] で filter 語を返し、nextest 形でない行は空で通る（2 本目の
/// 読み手を作らない）。本文で 0 本の filter 語は 0 本の事実として載せる（断るかは `settle_write_set` の側）。
fn row_facts(repo: &Path, contract: &Contract, materials: &Materials, judged: &mut Judged) {
    let Ok(Some(Pointed { row, .. })) = pointed_row(repo, contract, materials) else {
        return;
    };
    judged.design = Some((contract.design.clone(), row.section.clone()));
    let texts: Option<Vec<(&str, &str)>> = materials
        .sources
        .iter()
        .map(|source| source.body.as_deref().ok().map(|text| (source.path.as_str(), text)))
        .collect();
    let Some(texts) = texts else {
        return;
    };
    let fields = fields_of(&row);
    let base = materials.base();
    for line in &row.verify {
        let one = closure::Fields { verify: std::slice::from_ref(line), tests: &[], ..fields };
        let Err(ClosureError::TeethPlaceUnresolved { filter }) = closure::teeth_places(&one, &base, &[]) else {
            continue;
        };
        let files: Vec<String> = closure::teeth_places(&one, &base, &texts).map(Vec::from_iter).unwrap_or_default();
        judged.teeth.push((filter, files));
    }
}

/// 設計 pointer の行から決めた write-set の弁別（設計 contract-source.md §3・受付だけ）。
struct Settled {
    /// 導出か手書きか。
    kind: WriteSet,
    /// 導出値を契約の写しの write-set にする周（行に `write-set` の無い `Derived`）の導出値。他は `None`。
    replaced: Option<Vec<String>>,
    /// 判定行に載せる本数（導出値か手書きの項目数）。
    files: usize,
}

/// 設計 pointer の行（Promised の行は `verify` / `done` を生成値に差し替えた形）と、Promised の行の導出した write-set。
struct Pointed {
    /// 行（Promised の行は [`generated_row`] の形＝写しと同じ値）。
    row: ContractRow,
    /// Promised の行の write-set（他の行は `None`）。
    promised: Option<Vec<String>>,
}

/// 契約の `design` が設計 pointer（`<doc>#<id>`）なら base の契約表の行を引く（[`settle_write_set`] と [`row_facts`] が
/// 同じ 1 本で読む）。pointer でない `design`（(b) の前の契約 file）は `Ok(None)`。pointer が解けない（doc を読めない・
/// 区間が無い・行が無い）周は契約表の欠陥として断る（FR54・fail-closed）。Promised の行は [`promised`] の同じ 1 本で
/// 生成値を組む（[`generated`] と同じ断り）。
fn pointed_row(repo: &Path, contract: &Contract, materials: &Materials) -> Result<Option<Pointed>, Denial> {
    let Ok(pointer) = table::parse_pointer(&contract.design) else {
        return Ok(None);
    };
    let text = table::read(repo, &pointer.path)
        .map_err(|reason| refuse(&Refuse::ContractTable(TableError::Unreadable { line: 0, reason }), &[]))?;
    let row = table::find_row(&pointer.path, &text, &pointer.id).map_err(|errors| {
        let rest: Vec<String> = errors.iter().skip(1).map(|error| format!("pipe: {}", error.reason())).collect();
        let first = errors.into_iter().next().unwrap_or(TableError::RowMissing { line: 0, id: pointer.id.clone() });
        refuse(&Refuse::ContractTable(first), &rest)
    })?;
    Ok(Some(match promised(&pointer.path, &text, &row, materials)? {
        Some(found) => Pointed { row: generated_row(&row, &found), promised: Some(found.write_set) },
        None => Pointed { row, promised: None },
    }))
}

/// 行の欄を導出の材料に写す（`settle_write_set` と `row_facts` が同じ形で組む）。
fn fields_of(row: &ContractRow) -> closure::Fields<'_> {
    closure::Fields {
        touches: &row.touches,
        surfaces: &row.surfaces,
        verify: &row.verify,
        creates: &row.creates,
        tests: &row.tests,
        also: &row.also,
        files: &[],
    }
}

/// base の tree の事実を導出の材料に写す。
fn base_of<'a>(sources: &'a [Source], snapshots: &'a [Source], tracked: &'a [String]) -> closure::Base<'a> {
    closure::Base { sources, snapshots, tracked, core_crate: NAME }
}

/// 契約の `design` が設計 pointer なら base の契約表の行を引いて write-set を弁別する（§3「手書きの write-set の扱いと
/// 撃つ場所」）。pointer でない `design` は `None`＝従来どおり導出しない。行の読みは [`pointed_row`]。
///
/// - `creates` / `tests` / `also` を 1 つも持たず `write-set` を持つ行は [`WriteSet::Declared`]（導出も drift も撃たず、
///   verify の歯の file が write-set に在るかの門〔[`closure::declared_teeth`]・§20〕だけを撃つ）。
/// - それ以外は [`WriteSet::Derived`]: 導出値を作り（解けない欄は typed に断る）、
///   行に `write-set` が在れば集合一致でなければ `write-set-drift`・無ければ導出値が
///   write-set になる。
fn settle_write_set(repo: &Path, contract: &Contract, materials: &Materials) -> Result<Option<Settled>, Denial> {
    let Some(Pointed { row, promised }) = pointed_row(repo, contract, materials)? else {
        return Ok(None);
    };
    // Promised の行（§33）は約束の行から導いた値が write-set（行は write-set を持てない＝drift も撃たない）。
    if let Some(files) = promised {
        return Ok(Some(Settled { kind: WriteSet::Promised, files: files.len(), replaced: Some(files) }));
    }
    let declared = row.creates.is_empty() && row.tests.is_empty() && row.also.is_empty() && !row.write_set.is_empty();
    let fields = fields_of(&row);
    let base = materials.base();
    if declared {
        // Declared 行は導出も drift も撃たないが、**歯の置き場の門**だけは撃つ（§20・行 t）: verify の nextest 行の
        // 歯の file が write-set の外に在る契約は、便を作らずに file を全部名指して断る（審査へ先送りしない・C16）。
        closure::declared_teeth(&fields, &base, &row.write_set).map_err(|error| refuse(&refuse_of(error, &row), &[]))?;
        let files = row.write_set.len();
        return Ok(Some(Settled { kind: WriteSet::Declared, replaced: None, files }));
    }
    let derived = closure::derive_write_set(&fields, &base).map_err(|error| refuse(&refuse_of(error, &row), &[]))?;
    let files = derived.len();
    if row.write_set.is_empty() {
        let replaced: Vec<String> = derived.into_iter().collect();
        return Ok(Some(Settled { kind: WriteSet::Derived, replaced: Some(replaced), files }));
    }
    closure::check_drift(&row.write_set, &derived).map_err(|error| refuse(&refuse_of(error, &row), &[]))?;
    Ok(Some(Settled { kind: WriteSet::Derived, replaced: None, files }))
}

/// 導出の理由を契約単位の拒否へ写す（理由は 1 対 1・型の形と読めなさは契約表の欠陥として行番号を持つ）。
fn refuse_of(error: ClosureError, row: &ContractRow) -> Refuse {
    match error {
        ClosureError::TypeForm { .. } | ClosureError::Unreadable { .. } => {
            Refuse::ContractTable(TableError::Unreadable { line: row.line, reason: error.reason() })
        }
        ClosureError::SurfaceUnknown { name } => Refuse::ContractTable(TableError::SurfaceUnknown { line: row.line, name }),
        ClosureError::WriteSetDrift { missing, extra } => Refuse::WriteSetDrift { missing, extra },
        ClosureError::TeethPlaceUnresolved { filter } => Refuse::TeethPlaceUnresolved { filter },
        ClosureError::AlsoNamesRust { item } => Refuse::AlsoNamesRust { item },
        ClosureError::TestsNotATeethFile { item } => Refuse::TestsNotATeethFile { item },
        ClosureError::ItemUnresolved { item } => Refuse::WriteSetItemUnresolved { item },
        ClosureError::FnUndeclared { module, name } => Refuse::FnUndeclared { module, name },
        ClosureError::TeethOutsideWriteSet { files } => Refuse::TeethOutsideWriteSet { files },
    }
}

/// 同じ bead の直前までの便 1 つ（新しい順の列の要素・§23）。
struct Past {
    /// 便 id。
    id: String,
    /// `review.json` の判定。
    judgement: Judgement,
}

/// 今回の材料（§23・受付が写す形の契約 file〔導出値を置いた後〕と base から読む節の本文〔審査と同じ 1 本
/// [`review::design_material`]〕と、行の `touches`）。
struct Today {
    /// 契約 file の字面（[`crate::pipe::contract::render`]＝`create` が写す形と同じ 1 本）。
    contract: String,
    /// 節の本文（`review/design.txt` と同じ形）。
    design: String,
    /// 今回の write-set（弁別済み＝導出値を置いた後）。
    write_set: Vec<String>,
    /// 設計 pointer の行。
    row: ContractRow,
    /// 行が約束の行を持つ（[`WriteSet::Promised`]・§33 行 ah: 焼き直しの門は契約 file の字面だけを見る）。
    promised: bool,
}

/// 受付の 2 門（§23・[`exclude_same_kind`] と [`exclude_unaddressed`]）を撃ち、各門の断りを列に積む。
///
/// rules 行 `review.same_kind_stop` は**置き場の有無に依らず**要る（行の無い manifest は受付を 1 byte も動かさない・
/// rc 2・`pipe.land_wait_s` と同じ極性）。置き場が無い周（preflight の `--state-dir` 無し・列の余地の判定）は便の履歴を
/// 読めないので 2 門とも撃たない（交差と同じ扱い）。pointer でない `design` は行を持たないので撃たない。
fn exclude_repeats(material: &Material<'_>, measured: &Contract, judged: &mut Judged) {
    let Material { repo, manifest, state_dir, bead, materials, .. } = *material;
    let stop = match int_row(manifest, ROW_SAME_KIND_STOP) {
        Ok(found) => found,
        Err(reason) => return judged.denials.push(denied(DENIAL_RULES, broken(reason))),
    };
    let Some(state_dir) = state_dir else {
        return;
    };
    let today = match today_of(repo, measured, materials) {
        Ok(Some(found)) => found,
        Ok(None) => return,
        Err(denial) => return judged.denials.push(denial),
    };
    let past = match history(state_dir, bead) {
        Ok(found) => found,
        Err(denial) => return judged.denials.push(denial),
    };
    if let Err(denial) = exclude_same_kind(state_dir, &past, stop, &today) {
        judged.denials.push(denial);
    }
    if let Err(denial) = exclude_unaddressed(state_dir, &past, &today, materials) {
        judged.denials.push(denial);
    }
}

/// 今回の材料を組む（行の無い pointer は `Ok(None)`・行の解けない周は [`pointed_row`] の断り）。
fn today_of(repo: &Path, measured: &Contract, materials: &Materials) -> Result<Option<Today>, Denial> {
    let Some(Pointed { row, promised }) = pointed_row(repo, measured, materials)? else {
        return Ok(None);
    };
    let contract = crate::pipe::contract::render(&row, &measured.design, &measured.write_set);
    let design = review::design_material(repo, &measured.design);
    Ok(Some(Today { contract, design, write_set: measured.write_set.clone(), row, promised: promised.is_some() }))
}

/// 置き場の replay から同じ bead の便を **id の新しい順**に読む（run id は `<bead>-<UTC の秒>`＝id の降順が時系列の
/// 逆順）。段が Reviewed 以降の便は `review.json` を要り、読めない便は `WriteSetUnreadable`（rc 2・`live` と同じ読み手・
/// 読めなさを「判定なし」に読み替えない）。段が `Intake` の便と、審査に届く前に終端した便（`review.json` の file が
/// 無い `Landed` / `Failed` / `Stopped`）は数えない。
fn history(state_dir: &Path, bead: &str) -> Result<Vec<Past>, Denial> {
    let state = current(state_dir).map_err(|errors| {
        denied(DENIAL_STORE, Outcome::failed(RC_BROKEN, errors.iter().map(StoreError::to_string).collect()))
    })?;
    let mut found = Vec::new();
    for (id, run) in state.runs.iter().rev().filter(|(_, run)| run.bead == bead) {
        if !reviewed_stage(state_dir, id, run.stage) {
            continue;
        }
        let Some(judgement) = review::judgement_of(state_dir, id) else {
            return Err(refuse(&Refuse::WriteSetUnreadable { run: id.clone() }, &[]));
        };
        found.push(Past { id: id.clone(), judgement });
    }
    Ok(found)
}

/// 便が審査の判定を持つ段か（**段の網羅 match**・段が増えたら compile で気付く）。`Intake` は審査の前。終端の 3 段は
/// 審査に届く前（lens の途中で process が消えた便を `stop` した周）にも着くので、`review.json` の file の有無で分ける。
fn reviewed_stage(state_dir: &Path, id: &str, stage: Stage) -> bool {
    match stage {
        Stage::Intake => false,
        Stage::Landed | Stage::Failed | Stage::Stopped => review::review_path(state_dir, id).exists(),
        Stage::Reviewed
        | Stage::Blocked
        | Stage::Spawned
        | Stage::Questioned
        | Stage::RateLimited
        | Stage::Implemented
        | Stage::Gated => true,
    }
}

/// 同型の停止（§23 (2)）: 先頭の便の理由の型と同じ型が verdict PASS で途切れるまで連続する本数を数え（`unparsed` の便は
/// 数えず連鎖も切らない・C10）、本数が行の値に達し、かつ先頭の便の材料（`review/` の契約の写しと `design.txt`）が
/// 今回の材料と両方とも同じ字面の周は [`Refuse::SameKindRepeated`]。契約か節のどちらかが変わっていれば通す。
/// 先頭の便の材料を読めない周は `WriteSetUnreadable`（材料は判定より前に置かれるので、無い便は壊れた store）。
fn exclude_same_kind(state_dir: &Path, past: &[Past], stop: u64, today: &Today) -> Result<(), Denial> {
    let mut chain = past.iter().filter(|found| found.judgement.kind != Some(FindingKind::Unparsed));
    let Some(head) = chain.next() else {
        return Ok(());
    };
    let Some(kind) = head.judgement.kind else {
        return Ok(());
    };
    let runs: Vec<String> = std::iter::once(head)
        .chain(chain.take_while(|found| found.judgement.kind == Some(kind)))
        .map(|found| found.id.clone())
        .collect();
    if (runs.len() as u64) < stop {
        return Ok(());
    }
    let dir = review::review_dir(state_dir, &head.id);
    let (Ok(contract), Ok(design)) =
        (std::fs::read_to_string(dir.join(CONTRACT_FILE)), std::fs::read_to_string(dir.join(review::DESIGN_FILE)))
    else {
        return Err(refuse(&Refuse::WriteSetUnreadable { run: head.id.clone() }, &[]));
    };
    if contract == today.contract && design == today.design {
        return Err(refuse(&Refuse::SameKindRepeated { kind, runs, stop }, &[]));
    }
    Ok(())
}

/// 焼き直しの門（§23 (3)）: 直前の便（新しい順の先頭）の verdict が PASS でない周、その指摘（`kind` と `at`）に対応する
/// 差分が今回の材料に在るかを kind ごとの物差し（[`review::unaddressed`]）で測り、対応の無い項目が 1 つでも在れば
/// [`Refuse::FindingUnaddressed`]（項目は辞書順）。測れない型と `at` の空な周は物差しが空を返す＝通す。
///
/// Promised の行（§33 行 ah (iv)）は `at` の物差しを撃たずに通す: 契約 file は約束の行からの生成値で、焼き直しは
/// 契約 file の字面が変わったかだけで測る（不変の N 回目は [`exclude_same_kind`] が断る）。
fn exclude_unaddressed(state_dir: &Path, past: &[Past], today: &Today, materials: &Materials) -> Result<(), Denial> {
    if today.promised {
        return Ok(());
    }
    let Some(head) = past.first() else {
        return Ok(());
    };
    let Some(kind) = head.judgement.kind else {
        return Ok(());
    };
    let Ok(previous) = std::fs::read_to_string(review::review_dir(state_dir, &head.id).join(review::DESIGN_FILE)) else {
        return Err(refuse(&Refuse::WriteSetUnreadable { run: head.id.clone() }, &[]));
    };
    let rework = review::Rework {
        write_set: &today.write_set,
        contract: &today.contract,
        design: &today.design,
        previous_design: &previous,
        touches: &today.row.touches,
        tracked: &materials.tracked,
        sources: &materials.sources,
    };
    let at = review::unaddressed(kind, &head.judgement.at, &rework).map_err(|error| refuse(&refuse_of(error, &today.row), &[]))?;
    if at.is_empty() {
        return Ok(());
    }
    Err(refuse(&Refuse::FindingUnaddressed { kind, at }, &[]))
}

/// live な便（終端でない run）と write-set が交差する契約を断る（設計 pipeline-conflict.md §2）。
///
/// **読めない側が勝つ**: live な便の写しを 1 つでも読めなければ、交差の有無に関わらず
/// `WriteSetUnreadable`（rc 2）で止まる。読めない store を「交差なし」に読み替えると、
/// 排他が黙って無効化される（fail-closed・NFR4）。
///
/// 交差した周は**全組を stderr へ並べ**、理由の 1 行は先頭の 1 組を名乗る。dir 項目は base の tracked file に
/// 展開してから数える（設計 contract-source.md §3・[`overlaps`]）。通った周は突き合わせた live な run を [`Crossed`]
/// で返す（交差は 0・§21 の `overlap=` の材料）。
fn exclude_overlap(state_dir: &Path, contract: &Contract, tracked: &[String]) -> Result<Crossed, Denial> {
    let found = crossings(state_dir, contract, tracked)?;
    match &found.first {
        None => Ok(found),
        Some(reason) => Err(refuse(reason, &found.lines)),
    }
}

/// 置き場の live な便の本数が rules 行 `pipe.max_live` の値以上の周を断る（設計 gate-cost.md §24・受付だけ）。
///
/// 数えるのは [`crossings`] が突き合わせた live な run（**交差と同じ 1 本**の live の判定・C2）で、読めない便が在る周は
/// 交差と同じ `WriteSetUnreadable`（rc 2・fail-closed）。便を作る前だけ撃つ＝走行中の便は止めない。行の無い manifest は
/// 受付を動かさない（rc 2）。
fn exclude_max_live(manifest: &Manifest, state_dir: &Path, contract: &Contract, tracked: &[String]) -> Result<(), Denial> {
    let cap = int_row(manifest, ROW_MAX_LIVE).map_err(|reason| denied(DENIAL_RULES, broken(reason)))?;
    let live = u64::try_from(crossings(state_dir, contract, tracked)?.runs.len()).unwrap_or(u64::MAX);
    if live >= cap {
        return Err(refuse(&Refuse::MaxLive { live, cap }, &[]));
    }
    Ok(())
}

/// live な便との交差を**測るだけ**の 1 本（断りは作らない・設計 dispatcher.md §3）。
///
/// [`exclude_overlap`]（受付＝交差 1 件で断る）と `pipe::dispatch`（列＝交差した相手を待ちの理由にする）が
/// **同じこの 1 本**を読む（判定が 2 か所にならない・憲法 C2）。読めない側が勝つ極性はここが持つ。
pub(in crate::pipe) fn crossings(state_dir: &Path, contract: &Contract, tracked: &[String]) -> Result<Crossed, Denial> {
    let state = current(state_dir).map_err(|errors| {
        denied(DENIAL_STORE, Outcome::failed(RC_BROKEN, errors.iter().map(StoreError::to_string).collect()))
    })?;
    let mut first: Option<Refuse> = None;
    let mut lines: Vec<String> = Vec::new();
    let mut runs: Vec<(String, Vec<String>)> = Vec::new();
    for (id, run) in &state.runs {
        let Some(alive) = live(state_dir, id, run.stage) else {
            return Err(refuse(&Refuse::WriteSetUnreadable { run: id.clone() }, &[]));
        };
        if !alive {
            continue;
        }
        let Ok(live_contract) = Contract::load(&contract_path(state_dir, id)) else {
            return Err(refuse(&Refuse::WriteSetUnreadable { run: id.clone() }, &[]));
        };
        let mut crossed: Vec<String> = Vec::new();
        for (mine, theirs) in overlaps(&contract.write_set, &live_contract.write_set, tracked) {
            if first.is_none() {
                first = Some(Refuse::WriteSetOverlap { run: id.clone(), path: mine.clone() });
            }
            lines.push(format!("pipe: overlap run={id} contract={mine} live={theirs}"));
            crossed.push(mine);
        }
        runs.push((id.clone(), crossed));
    }
    Ok(Crossed { runs, first, lines })
}

/// live との交差の事実（§21 の `overlap=` の材料）。[`exclude_overlap`] が通った周は交差が全部空である。
pub(in crate::pipe) struct Crossed {
    /// 突き合わせた live な run（run id の順）と、その便と交差した契約側の file（[`exclude_overlap`] が
    /// 通った周は全部空・列は空でない組を待ちの理由にする）。
    pub(in crate::pipe) runs: Vec<(String, Vec<String>)>,
    /// 先頭の 1 組の断り（交差 0 なら `None`・受付の理由の 1 行）。
    first: Option<Refuse>,
    /// 交差の全組の行（受付の stderr・交差 0 なら空）。
    lines: Vec<String>,
}

/// 上限の余地の事実（[`exclude_cap_shortfall`] が通った周・§21 の `headroom=` の材料）。
pub(super) struct Headrooms {
    /// write-set の `.rs`（dir は配下に展開・`+` の新規 file は 0 行・`-` / `~` / `=` は余地を求めない）ごとの余地
    /// （R-C4-2 の値 − base の行数）・余地の小さい順（同じ余地は path の辞書順）。
    pub(super) rooms: Vec<(String, u64)>,
    /// 上限の値と契約の `size` の見積（行・rules 行 `pipe.size_<s|m|l>_lines` の値）。
    caps: declaration::Caps,
    /// 契約の growth を読んだ file ごとの見込み（path と行数・設計 contract-source.md §46）。
    growth: Vec<(String, u64)>,
}

impl Headrooms {
    /// file の見込み（growth に在ればその値・無ければ `size` の見積＝受付の判定と同じ [`declaration::Caps::estimate`]）。
    pub(super) fn estimate(&self, file: &str) -> u64 {
        self.caps.estimate(file, &self.growth)
    }
}

/// [`Headrooms`] を組む（[`exclude_cap_shortfall`] が余地を測る同じ `items` / `lines` / `growth` / `caps` から・判定は
/// しない・file の余地は全体の行数から）。
fn headrooms_of(
    items: &[WriteSetItem],
    lines: &[declaration::FileLines],
    growth: Vec<(String, u64)>,
    caps: declaration::Caps,
) -> Headrooms {
    let lines_of = |path: &str| lines.iter().find(|found| found.path == path).map_or(0, |found| found.total);
    let mut rooms: Vec<(String, u64)> = items
        .iter()
        .flat_map(|item| match *item {
            WriteSetItem::File(ref path) | WriteSetItem::New(ref path) => vec![path.clone()],
            WriteSetItem::Dir(ref under) => under.clone(),
            WriteSetItem::Shrink(_) | WriteSetItem::Delete(_) | WriteSetItem::PlaceOnly(_) => Vec::new(),
        })
        .filter(|path| path.ends_with(".rs"))
        .map(|path| {
            let room = caps.file_lines.saturating_sub(lines_of(&path));
            (path, room)
        })
        .collect();
    rooms.sort_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)));
    Headrooms { rooms, caps, growth }
}

/// 上限の余地（設計 contract-source.md §3・受付だけ）: write-set の各 `.rs` の base の行数と R-C4-2 の差、core の
/// 合計と R-C4-1 の差に、契約の `size` の見積（rules 行 `pipe.size_<s|m|l>_lines`・数は manifest が持つ・C1）を
/// 当て、入らない file を名指して断る（file と core の 2 形・先頭の 1 件が理由の 1 行・残りは stderr に並ぶ）。
/// core の合計は各 file の**本体**（行頭 `#[cfg(test)]` より前）だけ＝xtask check の core-lines と同じ母集団
/// （[`declaration::FileLines`]・設計 core-boundary.md §2）で、file の余地は全体の行数から。
///
/// dir 項目は base の配下に展開し、`+` の新規 file は 0 行として数え、`-` の縮む面と `~` の消える file は余地も
/// 本数も数えない（弁別は [`declaration::headroom_shortfalls`] の中）。base に無い項目は数えない（項目の実在は
/// 契約表の行の検査〔`contracts check` / 設計 pointer の intake〕が名指す）——ただし **接頭辞付きで解けない項目は
/// 受付で断る**（`write-set-item-unresolved`）: `-` / `~` の先が base に無い項目を落として測ると「余地を求めない」
/// 宣言が静かに消え、無い file を減らす / 消す便が通る（`~` は §24）。`+` の先が base に在る項目
/// （[`NewFilePolicy::MustBeAbsent`]）も同じ＝契約表の検査は
/// land 済みの `+` を実在 file と読む（`MayBeLanded`・`s2-07l.346`）ので、入口で止めないと満杯の file を `+` で
/// 書いた便が余地を測られずに通る。通った周は file ごとの余地を [`Headrooms`] で返す（§21 の `headroom=` の材料）。
fn exclude_cap_shortfall(manifest: &Manifest, contract: &Contract, materials: &Materials) -> Result<Headrooms, Denial> {
    let (tracked, sources) = (materials.tracked.as_slice(), materials.sources.as_slice());
    let rules = |id: &str| int_row(manifest, id).map_err(|reason| denied(DENIAL_RULES, broken(reason)));
    let caps = declaration::Caps {
        file_lines: rules(ROW_FILE_LINES)?,
        core_lines: rules(ROW_CORE_LINES)?,
        size_lines: rules(size_row(&contract.size).map_err(|reason| denied(DENIAL_SIZE, refused(reason)))?)?,
    };
    let items = match declaration::read_write_set(&contract.write_set, tracked, NewFilePolicy::MustBeAbsent) {
        Ok(found) => found,
        Err(unresolved) => {
            if let Some(item) =
                unresolved.iter().find(|item| item.starts_with([NEW_FILE, SHRINK_FILE, DELETE_FILE, PLACE_ONLY_FILE]))
            {
                return Err(refuse(&Refuse::WriteSetItemUnresolved { item: item.clone() }, &[]));
            }
            let resolvable: Vec<String> =
                contract.write_set.iter().filter(|item| !unresolved.contains(item)).cloned().collect();
            declaration::read_write_set(&resolvable, tracked, NewFilePolicy::MustBeAbsent).unwrap_or_default()
        }
    };
    // 行数は幅で正規化して数える（1 行に詰め込んでも余地は増えない・rules-manifest.md §4）。読めない file は 0 行。
    let width = rules(ROW_LINE_WIDTH)?;
    let lines: Vec<declaration::FileLines> = sources
        .iter()
        .map(|source| declaration::FileLines::of(&source.path, source.body.as_deref().unwrap_or_default(), width))
        .collect();
    // file ごとの見込み（§46）は契約表の検査と同じ読み手で読む。表の検査を通った行の写しは崩れを持たないが、読めない
    // 周は `size` へ黙って戻さず表の検査と同じ語で断る（fail-closed・C10）。
    let growth = match WriteSetItem::read_growth(&contract.growth, &contract.write_set) {
        Ok(found) => found,
        Err(unfit) => {
            let named: Vec<Refuse> = unfit
                .into_iter()
                .map(|(item, reason)| Refuse::ContractTable(TableError::GrowthForm { line: 0, item, reason }))
                .collect();
            let rest = |rest: &[Refuse]| rest.iter().map(|found| format!("pipe: {}", found.reason())).collect::<Vec<String>>();
            return Err(named.split_first().map_or_else(
                || refuse(&Refuse::ContractTable(TableError::Unreadable { line: 0, reason: "growth を読めない".to_owned() }), &[]),
                |(first, others)| refuse(first, &rest(others)),
            ));
        }
    };
    let short: Vec<Refuse> = declaration::headroom_shortfalls(&items, &lines, &growth, caps)
        .into_iter()
        .map(|found| Refuse::CapHeadroom {
            file: found.file,
            headroom: found.headroom,
            size: contract.size.clone(),
            estimate: found.estimate,
        })
        .collect();
    match short.split_first() {
        None => Ok(headrooms_of(&items, &lines, growth, caps)),
        Some((first, rest)) => {
            let lines: Vec<String> = rest.iter().map(|found| format!("pipe: {}", found.reason())).collect();
            Err(refuse(first, &lines))
        }
    }
}

/// 契約の `size` に対応する rules 行の id（S / M / L の 3 段だけ・他は見積を持たない）。
fn size_row(size: &str) -> Result<&'static str, String> {
    match size {
        "S" => Ok(ROW_SIZE_S),
        "M" => Ok(ROW_SIZE_M),
        "L" => Ok(ROW_SIZE_L),
        other => Err(format!("size {other:?} は S / M / L のどれでもない（上限の余地の見積を持てない）")),
    }
}

/// 契約単位の拒否（**rc は理由の variant が持つ**・名は [`Refuse::as_str`]）。`extra` は理由の後ろに並べる行。
fn refuse(found: &Refuse, extra: &[String]) -> Denial {
    let mut err = vec![format!("pipe: {}", found.reason())];
    err.extend(extra.iter().cloned());
    Denial { name: found.as_str(), outcome: Outcome::failed(found.rc(), err) }
}

/// 対象 repo の HEAD から vessel 宣言を読み、器の上限と突き合わせて有効値にする。
///
/// **外れは rc 1**（前提違反）で、宣言が読めない周も同じ極性である——「宣言が無い」と
/// 「宣言が壊れている」で扱いを変えると、器の視野の外の verify 行が片方から入る。有効値に宣言の名乗りを添える（§56 形 2）。
fn freeze(repo: &Path, manifest: &Manifest, contract: &Contract) -> Result<(Effective, Option<EntranceFlip>), Denial> {
    let rows = ceiling_of(manifest)?;
    declaration::measure_named(repo, &rows.borrow(), &contract.verify).map_err(|errors| {
        denied(DENIAL_DECLARATION, Outcome::failed(RC_REFUSED, errors.iter().map(ToString::to_string).collect()))
    })
}

/// 有効値を便の写し面へ凍結する（以後の段は repo の宣言を読み直さない）。
fn copy_vessel(state_dir: &Path, id: &str, effective: &Effective) -> Result<(), String> {
    let path = vessel_path(state_dir, id);
    std::fs::write(&path, effective.render())
        .map_err(|err| format!("{} を書けない: {err}", path.display()))
}

/// 便の対象 repo を写し面へ書き留める（現在地を cwd に依らせない）。
fn remember_repo(state_dir: &Path, id: &str, repo: &Path) -> Result<(), String> {
    let path = super::repo_path(state_dir, id);
    std::fs::write(&path, format!("{}\n", repo.display()))
        .map_err(|err| format!("{} を書けない: {err}", path.display()))
}

/// 便の repo。`--repo` が上書きし（読み手は [`repo_flag`] の 1 本・絶対 path）、無ければ写し面から解く。写し面も
/// 無い周は [`repo_of`] の flag 不在の断り（cwd を読まない・設計 pipeline.md §15）。
pub(super) fn run_repo(args: &[String], state_dir: &Path, id: &str) -> Result<PathBuf, String> {
    if let Some(found) = repo_flag(args)? {
        return Ok(found);
    }
    match super::repo_of_run(state_dir, id) {
        Some(found) => Ok(found),
        None => repo_of(args),
    }
}

/// 契約 file を置き場へ写す（process 間で持ち越す面は event log とこの写しだけ）。導出値が write-set になる周
/// （`derived`）は写しの `write-set` の行だけをその値に差し替える（他の行は逐語・runner が読むのは写しの write-set）。
fn write_contract(state_dir: &Path, id: &str, body: &str, derived: Option<&[String]>) -> Result<(), String> {
    let dir = run_dir(state_dir, id);
    std::fs::create_dir_all(&dir).map_err(|err| format!("{} を作れない: {err}", dir.display()))?;
    let to = contract_path(state_dir, id);
    let text = match derived {
        Some(files) => with_write_set(body, files),
        None => body.to_owned(),
    };
    std::fs::write(&to, text).map_err(|err| format!("{} を書けない: {err}", to.display()))
}

/// 契約 file の本文の `write-set` の行を `files` の列に差し替える（契約 file は 1 行 1 key・配列は 1 行に収まる）。
fn with_write_set(text: &str, files: &[String]) -> String {
    let quoted: Vec<String> = files.iter().map(|item| format!("\"{item}\"")).collect();
    let line = format!("write-set = [{}]", quoted.join(", "));
    let mut out = String::new();
    for found in text.lines() {
        let key = found.trim().split_once('=').map(|(key, _)| key.trim());
        out.push_str(if key == Some("write-set") { line.as_str() } else { found });
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{
        exclude_cap_shortfall, int_row, with_write_set, Entrance, Materials, WriteSet, ENTRANCE_LOCK, ROW_FILE_LINES,
        ROW_SIZE_S, WRITE_SETS,
    };
    use crate::fleet::store::LockPolicy;
    use crate::order::is_declaration_order;
    use crate::pipe::closure::Source;
    use crate::pipe::contract::Contract;
    use crate::pipe::declaration::TableFacts;
    use crate::rules::manifest::Manifest;
    use std::collections::BTreeSet;

    /// 受付の入口は**同時に 1 つしか通さない**（[`judge`] と [`create`] を 1 周として閉じる・ADR-0019 §2.1）。
    ///
    /// 契機が重なると 2 つの受付が同時に来る（`s2-07l.366`）。入口を握れていなければ、どちらも「live な
    /// 便は無い」と読んでから両方が run を作る。**握っている間は 2 つ目が取れない・外せば取れる**を
    /// 決定的に測る（同時撃ちの e2e は競合の再現が確率的なので、不変条件はここで固定する）。
    #[test]
    fn pipe_terminal_dispatch_entrance_admits_one_holder_at_a_time() {
        let state = std::env::temp_dir().join(format!("s2-entrance-{}", std::process::id()));
        let policy = LockPolicy { retry_ms: 60, stale_ms: 60_000 };
        let first = Entrance::hold(&state, policy).map_err(|out| out.rc);
        assert!(first.is_ok(), "1 つ目は通る（rc {:?}）", first.as_ref().err());
        let lock = state.join(ENTRANCE_LOCK);
        assert!(lock.exists(), "握っている間は lock file が在る");
        let second = Entrance::hold(&state, policy).map_err(|out| out.rc);
        assert!(second.is_err(), "握っている間は 2 つ目が取れない（母集団 2 回の取得）");
        drop(first);
        assert!(!lock.exists(), "外すと lock file が消える");
        let third = Entrance::hold(&state, policy).map_err(|out| out.rc);
        assert!(third.is_ok(), "外れた後は取れる（rc {:?}）", third.as_ref().err());
        drop(third);
        std::fs::remove_dir_all(&state).ok();
    }

    /// 弁別は閉じた 3 値で、const slice は宣言順・`as_str` は判定行の token（`derived` / `declared` / `promised`・§33 の
    /// Promised は宣言順の末尾）。
    #[test]
    fn contract_derive_write_set_kinds_are_pinned_in_declaration_order() {
        assert_eq!(WRITE_SETS, [WriteSet::Derived, WriteSet::Declared, WriteSet::Promised], "母集団 3 値");
        assert!(is_declaration_order(WRITE_SETS, |kind| kind as usize), "宣言順");
        let names: Vec<&str> = WRITE_SETS.iter().map(|kind| kind.as_str()).collect();
        assert_eq!(names, ["derived", "declared", "promised"], "判定行の token");
    }

    /// 写しの差し替えは `write-set` の行だけ（他の行は逐語・key の前後の空白も同じ key と読む・新規 file の `+` は
    /// そのまま載る）。
    #[test]
    fn contract_derive_copy_replaces_only_the_write_set_line() {
        let text = "goal = \"g\"\nwrite-set = [\"src/lib.rs\"]\n  write-set  = [\"x\"]\nverify = [\"git status\"]\n";
        let files = ["+src/new.rs".to_owned(), "src/a.rs".to_owned()];
        let want = "goal = \"g\"\nwrite-set = [\"+src/new.rs\", \"src/a.rs\"]\nwrite-set = [\"+src/new.rs\", \"src/a.rs\"]\nverify = [\"git status\"]\n";
        assert_eq!(with_write_set(text, &files), want);
    }

    /// 余地の段の除外（§31 (a)）の fixture: 解ける `crates/toy/src/full.rs`（`lines` 行）と base に無い素の
    /// `crates/toy/src/none.rs` の 2 項目の write-set・S の契約・材料。
    fn short_of_room(lines: u64) -> (Contract, Materials) {
        let full = "crates/toy/src/full.rs";
        let contract = Contract {
            goal: "g".to_owned(),
            done: "d".to_owned(),
            size: "S".to_owned(),
            owner: "s2-x".to_owned(),
            disposition: "A-now".to_owned(),
            write_set: vec![full.to_owned(), "crates/toy/src/none.rs".to_owned()],
            verify: vec!["git status".to_owned()],
            req: vec!["FR1".to_owned()],
            design: "docs/design/toy.md".to_owned(),
            classes: Vec::new(),
            opens: Vec::new(),
            touches: Vec::new(),
            growth: Vec::new(),
        };
        let body = "x\n".repeat(usize::try_from(lines).unwrap_or_default());
        let materials = Materials {
            tracked: vec![full.to_owned()],
            sources: vec![Source { path: full.to_owned(), body: Ok(body) }],
            snapshots: Vec::new(),
            facts: TableFacts { allowed: Vec::new(), denied: Vec::new(), requirements: String::new() },
            requirements: Ok(BTreeSet::new()),
            declared: Ok(Vec::new()),
            classes: Vec::new(),
        };
        (contract, materials)
    }

    /// (a) 項目の解決に失敗した周は「解けない項目を**除いた**列」で数え直す: 余地の在る full.rs は通って余地の列に
    /// 1 本だけ載り、満杯の full.rs は `cap-headroom` で断られる（`!` を落とすと解けない none.rs だけで数え直し、列が
    /// 空になって満杯の file が通る）。
    #[test]
    fn contract_closure_ext_survivor_a_cap_shortfall_recounts_without_the_unresolved_items() {
        // flip-check: retroactive s2-07l.277
        let Ok(manifest) = Manifest::embedded() else {
            panic!("埋め込み manifest を読める");
        };
        let (cap, size) = (int_row(&manifest, ROW_FILE_LINES), int_row(&manifest, ROW_SIZE_S));
        let (cap, size) = (cap.unwrap_or_default(), size.unwrap_or_default());
        assert!(cap > size && size > 0, "rules 行の値が在る（R-C4-2 {cap} > S {size} > 0）");
        // 余地は境界（(c) の歯の側）から離して置く＝見積の 2 倍と 0。
        let room = size.saturating_mul(2);
        let (contract, materials) = short_of_room(cap.saturating_sub(room));
        let fits = exclude_cap_shortfall(&manifest, &contract, &materials).map(|found| found.rooms);
        let rooms = fits.as_ref().map(Vec::len).unwrap_or_default();
        assert_eq!(
            fits.as_ref().ok(),
            Some(&vec![("crates/toy/src/full.rs".to_owned(), room)]),
            "余地の在る full.rs は通り、解ける項目だけが余地の列に載る（件数 {rooms} / 母集団 write-set 2 項目・解ける 1 項目）"
        );
        let (contract, materials) = short_of_room(cap);
        let short = exclude_cap_shortfall(&manifest, &contract, &materials).err().map(|denial| denial.name);
        assert_eq!(
            short,
            Some("cap-headroom"),
            "余地 0 の full.rs は解ける項目で断る（断り {} 件 / 母集団 write-set 2 項目・解ける 1 項目）",
            usize::from(short.is_some())
        );
    }
}
