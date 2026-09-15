//! `pipe intake` の受付（設計 §5「subcommand」・pipeline-conflict.md §2「入口の排他」・contract-source.md §3）。
//!
//! 契約 file を読み、宣言を上限と突き合わせ、上限の余地と write-set の交差で断り、置き場へ写して run を
//! 起こす。`s2-07l.295` で `cli.rs` から純移動した（本文は不変・外から呼ぶ path は `cli` が持つ）。
//! 親の共通の材料（`need` / `refused` / `broken` / `state_dir_of` / `live` 等）は `super::` で引く。

use super::{broken, flag, int_row, list_row, live, need, refused, repo_of, state_dir_of};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_REFUSED};
use crate::fleet::store::{LockPolicy, StoreError};
use crate::fleet::{self, EventKind, Stage};
use crate::pipe::contract::Contract;
use crate::pipe::declaration::{self, Ceiling, Effective, CEILING_ROW, DENIED_ROW};
use crate::pipe::refuse::{overlaps, Refuse, SHRINK_FILE};
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

/// 契約 file を読み込み、置き場へ写して run を起こす。
pub(super) fn intake(args: &[String], manifest: &Manifest, policy: LockPolicy) -> Outcome {
    match intake_id(args, manifest, policy) {
        Ok(id) => Outcome::ok_line(intake_line(args, &id)),
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
    let parsed = (|| {
        Ok::<_, String>((
            PathBuf::from(need(args, "--contract")?),
            need(args, "--bead")?.to_owned(),
            PathBuf::from(need(args, "--repo")?),
        ))
    })();
    let (path, bead, repo) = parsed.map_err(refused)?;
    // repo は spawn まで使わないが、**intake の時点で** git repo かを確かめる。
    // 後段で初めて落ちると、契約は受理されたのに進めない run が残る。
    if super::head_of(&repo).is_none() {
        return Err(refuse(&Refuse::NotARepo { repo: repo.display().to_string() }, &[]));
    }
    let state_dir = state_dir_of(args).map_err(refused)?;
    let contract = Contract::load(&path).map_err(|errors| {
        Outcome::failed(RC_BROKEN, errors.iter().map(ToString::to_string).collect())
    })?;
    // **宣言は上限と突き合わせてから**。ここで断つ周は run dir も event も作らない
    // ——撃てない契約の run が置き場に残ると、続きから引ける便に見えてしまう。
    let effective = freeze(&repo, manifest, &contract)?;
    // base の tracked file の一覧（交差の dir の展開と上限の余地が読む・設計 contract-source.md §3）。
    let tracked = super::table::tracked_files(&repo)
        .ok_or_else(|| refuse(&Refuse::NotARepo { repo: repo.display().to_string() }, &[]))?;
    // **上限の余地は受付だけが撃つ**（§3「撃つ場所は受付だけ」）: その便を今の base に当てたら入るか、という
    // 受付時点の事実で、CI の `contracts check` は撃たない（表は履歴を持つ）。
    exclude_cap_shortfall(&repo, manifest, &contract, &tracked)?;
    // **入口で排他する**（ADR-0019 §2.1）。live な便と write-set が交差する契約は、
    // run dir も event も作らずに断る——後段（land の rebase）で衝突を知るより安い。
    exclude_overlap(&state_dir, &contract, &tracked)?;
    let id = run_id(&bead, &fleet::cli::now_utc());
    // stamp は秒までなので、同じ bead を同じ秒に 2 回 intake すると id が衝突する。
    // 黙って上書きすると **前の便の契約が別物に化ける**ので、何も書かずに断る。
    if run_dir(&state_dir, &id).exists() {
        return Err(refuse(&Refuse::DuplicateRun { run: id.clone() }, &[]));
    }
    copy_contract(&state_dir, &id, &path).map_err(broken)?;
    copy_vessel(&state_dir, &id, &effective).map_err(broken)?;
    remember_repo(&state_dir, &id, &repo).map_err(broken)?;
    let emitted = emit(
        &state_dir,
        &Emit {
            kind: EventKind::RunCreated,
            run: &id,
            bead: &bead,
            stage: Some(Stage::Intake),
            seat: None,
            pid: None,
            detail: Some(format!("classes:{}", contract.classes.join("+"))),
        },
        policy,
    );
    match emitted {
        Err(err) => Err(broken(err.to_string())),
        Ok(()) => Ok(id),
    }
}

/// live な便（終端でない run）と write-set が交差する契約を断る（設計 pipeline-conflict.md §2）。
///
/// **読めない側が勝つ**: live な便の写しを 1 つでも読めなければ、交差の有無に関わらず
/// `WriteSetUnreadable`（rc 2）で止まる。読めない store を「交差なし」に読み替えると、
/// 排他が黙って無効化される（fail-closed・NFR4）。
///
/// 交差した周は**全組を stderr へ並べ**、理由の 1 行は先頭の 1 組を名乗る。dir 項目は base の tracked file に
/// 展開してから数える（設計 contract-source.md §3・[`overlaps`]）。
fn exclude_overlap(state_dir: &Path, contract: &Contract, tracked: &[String]) -> Result<(), Outcome> {
    let state = current(state_dir).map_err(|errors| {
        Outcome::failed(RC_BROKEN, errors.iter().map(StoreError::to_string).collect())
    })?;
    let mut first: Option<Refuse> = None;
    let mut lines: Vec<String> = Vec::new();
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
        for (mine, theirs) in overlaps(&contract.write_set, &live_contract.write_set, tracked) {
            if first.is_none() {
                first = Some(Refuse::WriteSetOverlap { run: id.clone(), path: mine.clone() });
            }
            lines.push(format!("pipe: overlap run={id} contract={mine} live={theirs}"));
        }
    }
    match first {
        None => Ok(()),
        Some(found) => Err(refuse(&found, &lines)),
    }
}

/// 上限の余地（設計 contract-source.md §3・受付だけ）: write-set の各 `.rs` の base の行数と R-C4-2 の差、core の
/// 合計と R-C4-1 の差に、契約の `size` の見積（rules 行 `pipe.size_<s|m|l>_lines`・数は manifest が持つ・C1）を
/// 当て、入らない file を名指して断る（file と core の 2 形・先頭の 1 件が理由の 1 行・残りは stderr に並ぶ）。
///
/// dir 項目は base の配下に展開し、`+` の新規 file は 0 行として数え、`-` の縮む面は余地も本数も数えない
/// （弁別は [`declaration::headroom_shortfalls`] の中）。base に無い項目は数えない（項目の実在は契約表の行の検査
/// 〔`contracts check` / 設計 pointer の intake〕が名指す）——ただし **`-` の先が base に無い項目は受付で断る**
/// （`write-set-item-unresolved`）: 落として測ると「余地を求めない」宣言が静かに消え、無い file を減らす便が通る。
fn exclude_cap_shortfall(repo: &Path, manifest: &Manifest, contract: &Contract, tracked: &[String]) -> Result<(), Outcome> {
    let caps = declaration::Caps {
        file_lines: int_row(manifest, ROW_FILE_LINES).map_err(broken)?,
        core_lines: int_row(manifest, ROW_CORE_LINES).map_err(broken)?,
        size_lines: int_row(manifest, size_row(&contract.size).map_err(refused)?).map_err(broken)?,
    };
    let items = match declaration::read_write_set(&contract.write_set, tracked) {
        Ok(found) => found,
        Err(unresolved) => {
            if let Some(item) = unresolved.iter().find(|item| item.starts_with(SHRINK_FILE)) {
                return Err(refuse(&Refuse::WriteSetItemUnresolved { item: item.clone() }, &[]));
            }
            let resolvable: Vec<String> =
                contract.write_set.iter().filter(|item| !unresolved.contains(item)).cloned().collect();
            declaration::read_write_set(&resolvable, tracked).unwrap_or_default()
        }
    };
    // 行数は幅で正規化して数える（1 行に詰め込んでも余地は増えない・rules-manifest.md §4）。
    let width = int_row(manifest, ROW_LINE_WIDTH).map_err(broken)?;
    let lines: Vec<(String, u64)> = super::table::read_all(repo, tracked, ".rs")
        .into_iter()
        .map(|source| {
            let count = source.body.as_deref().map_or(0, |text| declaration::line_count(text, width));
            (source.path, count)
        })
        .collect();
    let short: Vec<Refuse> = declaration::headroom_shortfalls(&items, &lines, caps)
        .into_iter()
        .map(|found| Refuse::CapHeadroom { file: found.file, headroom: found.headroom, size: contract.size.clone() })
        .collect();
    match short.split_first() {
        None => Ok(()),
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

/// 契約単位の拒否（**rc は理由の variant が持つ**）。`extra` は理由の後ろに並べる行。
fn refuse(found: &Refuse, extra: &[String]) -> Outcome {
    let mut err = vec![format!("pipe: {}", found.reason())];
    err.extend(extra.iter().cloned());
    Outcome::failed(found.rc(), err)
}

/// 対象 repo の HEAD から vessel 宣言を読み、器の上限と突き合わせて有効値にする。
///
/// **外れは rc 1**（前提違反）で、宣言が読めない周も同じ極性である——「宣言が無い」と
/// 「宣言が壊れている」で扱いを変えると、器の視野の外の verify 行が片方から入る。
fn freeze(repo: &Path, manifest: &Manifest, contract: &Contract) -> Result<Effective, Outcome> {
    let commands = list_row(manifest, CEILING_ROW).map_err(refused)?;
    let denied = list_row(manifest, DENIED_ROW).map_err(refused)?;
    let ceiling = Ceiling { row: CEILING_ROW, commands: &commands, denied: &denied };
    declaration::measure(repo, &ceiling, &contract.verify).map_err(|errors| {
        Outcome::failed(RC_REFUSED, errors.iter().map(ToString::to_string).collect())
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

/// 契約 file を置き場へ写す（process 間で持ち越す面は event log とこの写しだけ）。
fn copy_contract(state_dir: &Path, id: &str, from: &Path) -> Result<(), String> {
    let dir = run_dir(state_dir, id);
    std::fs::create_dir_all(&dir).map_err(|err| format!("{} を作れない: {err}", dir.display()))?;
    let to = contract_path(state_dir, id);
    std::fs::copy(from, &to).map_err(|err| format!("{} を写せない: {err}", to.display()))?;
    Ok(())
}
