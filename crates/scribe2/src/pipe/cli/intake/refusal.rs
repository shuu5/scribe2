//! 受付の断りの組み立てと上限の余地の群（設計 docs/design/contract-source.md §57・契約表の行 bl・`s2-07l.736.12`）。
//!
//! 断りを組む口（[`refuse`] / [`denied`] / [`not_a_repo`] / [`refuse_of`]）・[`Refuse`] を持たない断りの名（`DENIAL_*`）・
//! 上限の余地の判定と報告（[`exclude_cap_shortfall`] / `headrooms_of` / `size_row`）と rules 行の id（`ROW_*`）の群である。
//! `pipe/cli/intake.rs` からの**純移動**で、歯は 1 本も足していない（親に残る in-file の歯と e2e が従来どおり測る）。
//! 親に残る型（[`Denial`] / [`Headrooms`] / [`Materials`]）は可視性を変えずに引き、上げた可視性は親が呼ぶ 10 名と歯が
//! 引く 2 名の `pub(super)` だけである。

// flip-check: moved s2-07l.736.12

use super::{Denial, Headrooms, Materials};
use crate::cli_outcome::Outcome;
use crate::pipe::cli::{broken, int_row, refused};
use crate::pipe::closure::ClosureError;
use crate::pipe::contract::Contract;
use crate::pipe::declaration::{self, NewFilePolicy, WriteSetItem};
use crate::pipe::refuse::{Refuse, DELETE_FILE, NEW_FILE, PLACE_ONLY_FILE, SHRINK_FILE};
use crate::pipe::table::{ContractRow, TableError};
use crate::rules::manifest::Manifest;
use std::path::Path;

/// 1 file の行数の上限を持つ rules 行（上限の余地の分子・設計 contract-source.md §3・値は読むだけ・C4）。
pub(super) const ROW_FILE_LINES: &str = "R-C4-2";

/// core の総行数の上限を持つ rules 行（上限の余地・値は読むだけ・C4）。
const ROW_CORE_LINES: &str = "R-C4-1";

/// 行の数え方の幅を持つ rules 行（上限の余地の行数を xtask check と同じ式で数える・kind `LineWidth`）。
const ROW_LINE_WIDTH: &str = "R-C4.line-width";

/// 契約の `size` = S の 1 file あたりの増分の見積（行）を持つ rules 行。
pub(super) const ROW_SIZE_S: &str = "pipe.size_s_lines";

/// 契約の `size` = M の見積を持つ rules 行。
const ROW_SIZE_M: &str = "pipe.size_m_lines";

/// 契約の `size` = L の見積を持つ rules 行。
const ROW_SIZE_L: &str = "pipe.size_l_lines";

/// 引数の形が読めない周の名（[`Refuse`] を持たない断り）。
pub(super) const DENIAL_ARGS: &str = "args";

/// 生成した写しを器自身が読めない周の名（生成の不備＝壊れた器・rc 2）。
pub(super) const DENIAL_GENERATED: &str = "generated";

/// 対象が git repo でない断り。
pub(super) fn not_a_repo(repo: &Path) -> Denial {
    refuse(&Refuse::NotARepo { repo: repo.display().to_string() }, &[])
}

/// 宣言の写し（`freeze`）が外れた周の名（[`Refuse`] の variant を持たない断り）。
pub(super) const DENIAL_DECLARATION: &str = "declaration";

/// rules 行が読めない周の名。
pub(super) const DENIAL_RULES: &str = "rules";

/// 契約の `size` が S / M / L のどれでもない周の名。
const DENIAL_SIZE: &str = "size";

/// 置き場の store が読めない周の名。
pub(super) const DENIAL_STORE: &str = "store";

/// [`Refuse`] を持たない断りを [`Denial`] に写す（名は材料の側・rc と行は `outcome` のまま）。
pub(super) fn denied(name: &'static str, outcome: Outcome) -> Denial {
    Denial { name, outcome, refusals: Vec::new() }
}

/// 導出の理由を契約単位の拒否へ写す（理由は 1 対 1・型の形と読めなさは契約表の欠陥として行番号を持つ）。
pub(super) fn refuse_of(error: ClosureError, row: &ContractRow) -> Refuse {
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
pub(super) fn exclude_cap_shortfall(manifest: &Manifest, contract: &Contract, materials: &Materials) -> Result<Headrooms, Denial> {
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
pub(super) fn refuse(found: &Refuse, extra: &[String]) -> Denial {
    let mut err = vec![format!("pipe: {}", found.reason())];
    err.extend(extra.iter().cloned());
    Denial { name: found.as_str(), outcome: Outcome::failed(found.rc(), err), refusals: vec![found.clone()] }
}
