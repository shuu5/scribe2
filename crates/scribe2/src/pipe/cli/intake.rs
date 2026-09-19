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
//! **judge と create**（契約表の行 u・contract-source.md §21・C2「判定関数は 1 本」）: 受付の判定は [`judge`]（run を作らない・
//! 断りを判定関数 1 本につき高々 1 件で**全部**集める）と [`create`]（run dir・写し・event）の 2 段で、`intake` = judge →
//! create（列の先頭の 1 件で断る＝従来の外形）・`pipe preflight`（[`super::preflight`]）= judge だけ。各判定関数
//! （[`freeze`] / [`settle_write_set`] / [`exclude_cap_shortfall`] / [`exclude_overlap`] / 重複 run）の中身と「先頭の 1 件で
//! 返す」形は不変で、Ok 値だけを事実（[`Headrooms`] / [`Crossed`]）へ広げる。

use super::{broken, flag, int_row, list_row, live, need, refused, repo_of, state_dir_of};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_REFUSED};
use crate::fleet::store::{LockPolicy, StoreError};
use crate::fleet::{self, EventKind, Stage};
use crate::name::NAME;
use crate::pipe::closure::{self, ClosureError, Source};
use crate::pipe::contract::{Contract, ContractError};
use crate::pipe::declaration::{self, Ceiling, Effective, NewFilePolicy, WriteSetItem, CEILING_ROW, DENIED_ROW};
use crate::pipe::refuse::{overlaps, Refuse, DELETE_FILE, NEW_FILE, SHRINK_FILE};
use crate::pipe::table::{self, ContractRow, TableError};
use crate::pipe::{contract_path, current, emit, run_dir, run_id, vessel_path, Emit};
use crate::rules::manifest::Manifest;
use std::path::{Path, PathBuf};

/// 1 file の行数の上限を持つ rules 行（上限の余地の分子・設計 contract-source.md §3・値は読むだけ・C4）。
const ROW_FILE_LINES: &str = "R-C4-2";

/// core の総行数の上限を持つ rules 行（上限の余地・値は読むだけ・C4）。
const ROW_CORE_LINES: &str = "R-C4-1";

/// 行の数え方の幅を持つ rules 行（上限の余地の行数を xtask check と同じ式で数える・kind `LineWidth`）。
const ROW_LINE_WIDTH: &str = "R-C4.line-width";

/// 契約の `size` = S の 1 file あたりの増分の見積（行）を持つ rules 行。
const ROW_SIZE_S: &str = "pipe.size_s_lines";

/// 契約の `size` = M の見積を持つ rules 行。
const ROW_SIZE_M: &str = "pipe.size_m_lines";

/// 契約の `size` = L の見積を持つ rules 行。
const ROW_SIZE_L: &str = "pipe.size_l_lines";

/// 契約の write-set の出所（設計 contract-source.md §3・C10「導出値と宣言値を型で分ける」）。**閉じた 2 値**で、
/// 契約表の行の欄の有無だけで決まる（散文の免除を持たない）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WriteSet {
    /// 器が導出した（`creates` / `tests` / `also` のどれかを持つ行と、新欄も `write-set` も無い行）。
    Derived,
    /// 行が手で列挙した（新欄を持たず `write-set` を持つ行・(h) の前の形）。
    Declared,
}

/// [`WriteSet`] の全 variant（宣言順・`enum-slices` が集合完全性を測る・読むのは pin の歯だけ）。
#[cfg_attr(not(test), expect(dead_code, reason = "宣言順 pin の歯だけが読む（`REFUSALS` と同じ形）"))]
pub(crate) const WRITE_SETS: &[WriteSet] = &[WriteSet::Derived, WriteSet::Declared];

impl WriteSet {
    /// 判定行の token の値。
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Derived => "derived",
            Self::Declared => "declared",
        }
    }
}

/// 受付を通った便（`run` の連鎖は id だけを要る・`intake` は判定行に write-set の弁別も載せる）。
struct Intaken {
    /// run id。
    id: String,
    /// write-set の弁別と本数（設計 pointer を持たない契約は `None`＝従来の形）。
    write_set: Option<(WriteSet, usize)>,
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
    if super::head_of(&repo).is_none() {
        return Err(not_a_repo(&repo).outcome);
    }
    let state_dir = state_dir_of(args).map_err(refused)?;
    let ceiling = ceiling_of(manifest).map_err(|denial| denial.outcome)?;
    let (contract, body) = generated(&repo, &pointer, &ceiling.borrow()).map_err(|denial| denial.outcome)?;
    let material = Material { repo: &repo, manifest, contract: &contract, state_dir: Some(&state_dir), bead: &bead };
    create(judge(&material), &material, &state_dir, &body, policy)
}

/// rules 行から allowlist と禁じる語を読んで [`Ceiling`] の材料を持つ（借りる側は [`Rows::borrow`]）。
pub(in crate::pipe) struct Rows {
    /// 通す語列（`runner.allowed_commands`）。
    commands: Vec<String>,
    /// 禁じる語列（`runner.denied_commands`）。
    denied: Vec<String>,
}

impl Rows {
    /// 借りた形の上限（`row` は同じ 1 つの rules 行 id）。
    pub(in crate::pipe) fn borrow(&self) -> Ceiling<'_> {
        Ceiling { row: CEILING_ROW, commands: &self.commands, denied: &self.denied }
    }
}

/// 上限の材料を rules 行から読む（読めない周は [`DENIAL_RULES`] の断り・[`freeze`] と同じ 2 行）。
pub(in crate::pipe) fn ceiling_of(manifest: &Manifest) -> Result<Rows, Denial> {
    let rows = |id: &str| list_row(manifest, id).map_err(|reason| denied(DENIAL_RULES, refused(reason)));
    Ok(Rows { commands: rows(CEILING_ROW)?, denied: rows(DENIED_ROW)? })
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
    let repo = PathBuf::from(read("--repo")?);
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
pub(in crate::pipe) fn generated(repo: &Path, pointer: &table::Pointer, ceiling: &Ceiling<'_>) -> Result<(Contract, String), Denial> {
    let Some(text) = crate::pipe::show_head(repo, &pointer.path) else {
        let reason = format!("{} を base（HEAD）から読めない", pointer.path);
        return Err(refuse(&Refuse::ContractTable(TableError::Unreadable { line: 0, reason }), &[]));
    };
    let row = table::find_row(&pointer.path, &text, &pointer.id).map_err(|errors| {
        let rest: Vec<String> = errors.iter().skip(1).map(|error| format!("pipe: {}", error.reason())).collect();
        let first = errors.into_iter().next().unwrap_or(TableError::RowMissing { line: 0, id: pointer.id.clone() });
        refuse(&Refuse::ContractTable(first), &rest)
    })?;
    let findings = check_row(repo, &text, &row, ceiling)?;
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
    let write_set = if row.write_set.is_empty() { derived_write_set(repo, &row)? } else { row.write_set.clone() };
    let body = crate::pipe::contract::render(&row, &design, &write_set);
    let contract = Contract::parse(&body).map_err(|errors| {
        denied(DENIAL_GENERATED, unloadable(errors))
    })?;
    Ok((contract, body))
}

/// 生成した写しを器自身が読めない周の名（生成の不備＝壊れた器・rc 2）。
const DENIAL_GENERATED: &str = "generated";

/// 行から導いた write-set（[`settle_write_set`] と同じ [`closure::derive_write_set`] を撃つ）。
fn derived_write_set(repo: &Path, row: &ContractRow) -> Result<Vec<String>, Denial> {
    let Some(tracked) = table::tracked_files(repo) else {
        let reason = format!("{} の tracked file を読めない（git repo でない）", repo.display());
        return Err(refuse(&Refuse::ContractTable(TableError::Unreadable { line: 0, reason }), &[]));
    };
    let sources = table::read_all(repo, &tracked, ".rs");
    let snapshots = table::read_all(repo, &tracked, ".snap");
    let fields = fields_of(row);
    let base = base_of(&sources, &snapshots, &tracked);
    let derived = closure::derive_write_set(&fields, &base).map_err(|error| refuse(&refuse_of(error, row), &[]))?;
    Ok(derived.into_iter().collect())
}

/// 行 1 つに (a) の表の検査を撃つ（`contracts check` と**同じ 1 実装**・C2）。ctx（allowlist / 禁じる語 /
/// 要件面 / base の tree）は `contracts check` と同じ材料から組み、doc の本文は base（`HEAD`）の字面を渡す。
///
/// base の tree を読めない（git repo でない）周は理由の 1 行を返す＝呼び側が `Unreadable` で断る。
fn check_row(repo: &Path, text: &str, row: &ContractRow, ceiling: &Ceiling<'_>) -> Result<Vec<table::Finding>, Denial> {
    let Some(tracked) = table::tracked_files(repo) else {
        let reason = format!("{} の tracked file を読めない（git repo でない）", repo.display());
        return Err(refuse(&Refuse::ContractTable(TableError::Unreadable { line: 0, reason }), &[]));
    };
    // 宣言が読めない・上限に外れる周は **rc 1**（前提違反）である（[`freeze`] と同じ極性・同じ名）。
    // 表の検査の前に宣言を読むのは要件面の path が宣言から来るからで、ここで rc 2 に倒すと
    // 「宣言が壊れている」便が「表を読めない」に化ける。
    let facts = declaration::table_facts(repo, ceiling).map_err(|errors| {
        denied(DENIAL_DECLARATION, Outcome::failed(RC_REFUSED, errors.iter().map(ToString::to_string).collect()))
    })?;
    let sources = table::read_all(repo, &tracked, ".rs");
    let snapshots = table::read_all(repo, &tracked, ".snap");
    let requirements = table::read(repo, &facts.requirements)
        .and_then(|found| table::requirement_ids(&facts.requirements, &found));
    let ctx = table::Context {
        allowed: &facts.allowed,
        denied: &facts.denied,
        requirements: &requirements,
        sources: &sources,
        tracked: &tracked,
        snapshots: &snapshots,
    };
    Ok(table::check_table(text, std::slice::from_ref(row), &ctx))
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
    /// 契約の bead id（この秒の run id の材料）。
    pub(in crate::pipe) bead: &'a str,
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
/// `exclude_overlap` → 重複 run の順に**全部撃ち**、各関数が返した断りを列に積む。前段の Ok 値を取るのは導出値で
/// write-set を置き換える 1 点だけで、`settle_write_set` が Err の周は契約 file の write-set のまま後段を撃つ
/// （Declared 行は元々置き換えが無い＝前段と後段の断りが同時に載る）。git repo でない対象は他の関数が撃てないので
/// `not-a-repo` の 1 件で止まる。
pub(in crate::pipe) fn judge(material: &Material<'_>) -> Judged {
    let Material { repo, manifest, contract, state_dir, bead } = *material;
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
    let Some(tracked) = super::head_of(repo).and_then(|_| table::tracked_files(repo)) else {
        judged.denials.push(not_a_repo(repo));
        return judged;
    };
    // **宣言は上限と突き合わせてから**。ここで断つ周は run dir も event も作らない
    // ——撃てない契約の run が置き場に残ると、続きから引ける便に見えてしまう。
    match freeze(repo, manifest, contract) {
        Ok(found) => judged.effective = Some(found),
        Err(denial) => judged.denials.push(denial),
    }
    let sources = table::read_all(repo, &tracked, ".rs");
    // **write-set の弁別は余地と交差より前**（導出値が write-set になる周は、その導出値で余地と交差を測る）。
    let mut measured = contract.clone();
    match settle_write_set(repo, contract, &tracked, &sources) {
        Ok(settled) => {
            judged.write_set = settled.as_ref().map(|found| (found.kind, found.files));
            judged.derived = settled.and_then(|found| found.replaced);
            if let Some(files) = judged.derived.as_deref() {
                measured.write_set = files.to_vec();
            }
        }
        Err(denial) => judged.denials.push(denial),
    }
    row_facts(repo, contract, &tracked, &sources, &mut judged);
    // **上限の余地は受付だけが撃つ**（§3「撃つ場所は受付だけ」）: その便を今の base に当てたら入るか、という
    // 受付時点の事実で、CI の `contracts check` は撃たない（表は履歴を持つ）。
    match exclude_cap_shortfall(manifest, &measured, &tracked, &sources) {
        Ok(found) => judged.headroom = Some(found),
        Err(denial) => judged.denials.push(denial),
    }
    let Some(state_dir) = state_dir else {
        return judged;
    };
    // **入口で排他する**（ADR-0019 §2.1）。live な便と write-set が交差する契約は、
    // run dir も event も作らずに断る——後段（land の rebase）で衝突を知るより安い。
    match exclude_overlap(state_dir, &measured, &tracked) {
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
        Ok(()) => Ok(Intaken { id, write_set }),
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
fn row_facts(repo: &Path, contract: &Contract, tracked: &[String], sources: &[Source], judged: &mut Judged) {
    let Ok(Some(row)) = pointed_row(repo, contract) else {
        return;
    };
    judged.design = Some((contract.design.clone(), row.section.clone()));
    let texts: Option<Vec<(&str, &str)>> =
        sources.iter().map(|source| source.body.as_deref().ok().map(|text| (source.path.as_str(), text))).collect();
    let Some(texts) = texts else {
        return;
    };
    let snapshots = table::read_all(repo, tracked, ".snap");
    let fields = fields_of(&row);
    let base = base_of(sources, &snapshots, tracked);
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

/// 契約の `design` が設計 pointer（`<doc>#<id>`）なら base の契約表の行を引く（[`settle_write_set`] と [`row_facts`] が
/// 同じ 1 本で読む）。pointer でない `design`（(b) の前の契約 file）は `Ok(None)`。pointer が解けない（doc を読めない・
/// 区間が無い・行が無い）周は契約表の欠陥として断る（FR54・fail-closed）。
fn pointed_row(repo: &Path, contract: &Contract) -> Result<Option<ContractRow>, Denial> {
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
    Ok(Some(row))
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
fn settle_write_set(
    repo: &Path,
    contract: &Contract,
    tracked: &[String],
    sources: &[Source],
) -> Result<Option<Settled>, Denial> {
    let Some(row) = pointed_row(repo, contract)? else {
        return Ok(None);
    };
    let declared = row.creates.is_empty() && row.tests.is_empty() && row.also.is_empty() && !row.write_set.is_empty();
    let snapshots = table::read_all(repo, tracked, ".snap");
    let fields = fields_of(&row);
    let base = base_of(sources, &snapshots, tracked);
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
    /// write-set の `.rs`（dir は配下に展開・`+` の新規 file は 0 行・`-` の縮む面と `~` の消える file は余地を求めない）ごとの余地
    /// （R-C4-2 の値 − base の行数）・余地の小さい順（同じ余地は path の辞書順）。
    pub(super) rooms: Vec<(String, u64)>,
    /// 契約の `size` の見積（行・rules 行 `pipe.size_<s|m|l>_lines` の値）。
    pub(super) size_lines: u64,
}

/// [`Headrooms`] を組む（[`exclude_cap_shortfall`] が余地を測る同じ `items` / `lines` / `caps` から・判定はしない）。
fn headrooms_of(items: &[WriteSetItem], lines: &[(String, u64)], caps: declaration::Caps) -> Headrooms {
    let lines_of = |path: &str| lines.iter().find(|(found, _)| found == path).map_or(0, |(_, count)| *count);
    let mut rooms: Vec<(String, u64)> = items
        .iter()
        .flat_map(|item| match *item {
            WriteSetItem::File(ref path) | WriteSetItem::New(ref path) => vec![path.clone()],
            WriteSetItem::Dir(ref under) => under.clone(),
            WriteSetItem::Shrink(_) | WriteSetItem::Delete(_) => Vec::new(),
        })
        .filter(|path| path.ends_with(".rs"))
        .map(|path| {
            let room = caps.file_lines.saturating_sub(lines_of(&path));
            (path, room)
        })
        .collect();
    rooms.sort_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)));
    Headrooms { rooms, size_lines: caps.size_lines }
}

/// 上限の余地（設計 contract-source.md §3・受付だけ）: write-set の各 `.rs` の base の行数と R-C4-2 の差、core の
/// 合計と R-C4-1 の差に、契約の `size` の見積（rules 行 `pipe.size_<s|m|l>_lines`・数は manifest が持つ・C1）を
/// 当て、入らない file を名指して断る（file と core の 2 形・先頭の 1 件が理由の 1 行・残りは stderr に並ぶ）。
///
/// dir 項目は base の配下に展開し、`+` の新規 file は 0 行として数え、`-` の縮む面と `~` の消える file は余地も
/// 本数も数えない（弁別は [`declaration::headroom_shortfalls`] の中）。base に無い項目は数えない（項目の実在は
/// 契約表の行の検査〔`contracts check` / 設計 pointer の intake〕が名指す）——ただし **接頭辞付きで解けない項目は
/// 受付で断る**（`write-set-item-unresolved`）: `-` / `~` の先が base に無い項目を落として測ると「余地を求めない」
/// 宣言が静かに消え、無い file を減らす / 消す便が通る（`~` は §24）。`+` の先が base に在る項目
/// （[`NewFilePolicy::MustBeAbsent`]）も同じ＝契約表の検査は
/// land 済みの `+` を実在 file と読む（`MayBeLanded`・`s2-07l.346`）ので、入口で止めないと満杯の file を `+` で
/// 書いた便が余地を測られずに通る。通った周は file ごとの余地を [`Headrooms`] で返す（§21 の `headroom=` の材料）。
fn exclude_cap_shortfall(
    manifest: &Manifest,
    contract: &Contract,
    tracked: &[String],
    sources: &[Source],
) -> Result<Headrooms, Denial> {
    let rules = |id: &str| int_row(manifest, id).map_err(|reason| denied(DENIAL_RULES, broken(reason)));
    let caps = declaration::Caps {
        file_lines: rules(ROW_FILE_LINES)?,
        core_lines: rules(ROW_CORE_LINES)?,
        size_lines: rules(size_row(&contract.size).map_err(|reason| denied(DENIAL_SIZE, refused(reason)))?)?,
    };
    let items = match declaration::read_write_set(&contract.write_set, tracked, NewFilePolicy::MustBeAbsent) {
        Ok(found) => found,
        Err(unresolved) => {
            if let Some(item) = unresolved.iter().find(|item| item.starts_with([NEW_FILE, SHRINK_FILE, DELETE_FILE])) {
                return Err(refuse(&Refuse::WriteSetItemUnresolved { item: item.clone() }, &[]));
            }
            let resolvable: Vec<String> =
                contract.write_set.iter().filter(|item| !unresolved.contains(item)).cloned().collect();
            declaration::read_write_set(&resolvable, tracked, NewFilePolicy::MustBeAbsent).unwrap_or_default()
        }
    };
    // 行数は幅で正規化して数える（1 行に詰め込んでも余地は増えない・rules-manifest.md §4）。
    let width = rules(ROW_LINE_WIDTH)?;
    let lines: Vec<(String, u64)> = sources
        .iter()
        .map(|source| {
            let count = source.body.as_deref().map_or(0, |text| declaration::line_count(text, width));
            (source.path.clone(), count)
        })
        .collect();
    let short: Vec<Refuse> = declaration::headroom_shortfalls(&items, &lines, caps)
        .into_iter()
        .map(|found| Refuse::CapHeadroom { file: found.file, headroom: found.headroom, size: contract.size.clone() })
        .collect();
    match short.split_first() {
        None => Ok(headrooms_of(&items, &lines, caps)),
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
/// 「宣言が壊れている」で扱いを変えると、器の視野の外の verify 行が片方から入る。
fn freeze(repo: &Path, manifest: &Manifest, contract: &Contract) -> Result<Effective, Denial> {
    let rows = |id: &str| list_row(manifest, id).map_err(|reason| denied(DENIAL_RULES, refused(reason)));
    let (commands, denied_commands) = (rows(CEILING_ROW)?, rows(DENIED_ROW)?);
    let ceiling = Ceiling { row: CEILING_ROW, commands: &commands, denied: &denied_commands };
    declaration::measure(repo, &ceiling, &contract.verify).map_err(|errors| {
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

/// 便の repo。`--repo` が上書きし、無ければ写し面 → cwd の順で解く。
pub(super) fn run_repo(args: &[String], state_dir: &Path, id: &str) -> Result<PathBuf, String> {
    if let Some(found) = flag(args, "--repo")? {
        return Ok(PathBuf::from(found));
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
    use super::{with_write_set, WriteSet, WRITE_SETS};
    use crate::order::is_declaration_order;

    /// 弁別は閉じた 2 値で、const slice は宣言順・`as_str` は判定行の token（`derived` / `declared`）。
    #[test]
    fn contract_derive_write_set_kinds_are_pinned_in_declaration_order() {
        assert_eq!(WRITE_SETS, [WriteSet::Derived, WriteSet::Declared], "母集団 2 値");
        assert!(is_declaration_order(WRITE_SETS, |kind| kind as usize), "宣言順");
        let names: Vec<&str> = WRITE_SETS.iter().map(|kind| kind.as_str()).collect();
        assert_eq!(names, ["derived", "declared"], "判定行の token");
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
}
