//! 宣言の任意 key の群（key の列 2 つ・key の名と既定・値の読み手・HEAD の宣言から値を外へ渡す口）（設計 pipeline.md §59）。
//!
//! 親（`declaration.rs`）の余地のために子 module へ置く（ADR-0047 の path の種別と同じ置き方）。外から呼ぶ path は親の
//! 再輸出で `crate::pipe::declaration::terminal_facts` などのまま。新しい任意 key は、key の名・読み手・key の列の 1 行ずつ・
//! 外へ渡す口をこの file へ、`Declared` の欄と `parse` の読みの 1 行を親へ足す。

use super::path_kinds;
use super::{declared_at_head, Ceiling, DeclError, EntranceFlip, Raw, Sourced, DETECTION_KEY, ENTRANCE_KEY};
use std::path::Path;

/// 宣言が持つ key（この順で報告する）。path の種別の任意 key 3 本（[`path_kinds::KEYS`]・ADR-0047）は
/// 既存の任意 key と同じ読み口で読む（schema は 1 のまま・key の追加と不在＝既定は版を上げない）。
pub(super) const DECLARED_KEYS: &[&str] = &[
    "schema",
    "allowed-commands",
    "common-verify",
    DETECTION_KEY,
    REQUIREMENTS_KEY,
    REMOTE_KEY,
    CI_CMD_KEY,
    path_kinds::DESIGN_INTENT_KEY,
    path_kinds::DESIGN_DOC_KEY,
    path_kinds::TESTS_KEY,
    ENTRANCE_KEY,
];

/// **push 先の remote の名**の key（任意・設計 contract-source.md §5「land の終端」）。
///
/// **既定は持たない**。push は repo の外へ出す行為（憲法 A1 の「出す」）なので、押す先を宣言していない
/// repo に器が勝手な既定で押すことはしない——宣言の無い repo の便は終端を持たない（`--pr-cmd` 形と同じ）。
const REMOTE_KEY: &str = "remote";

/// **CI の判定を読む 1 行**の key（任意・設計 contract-source.md §5）。書かない宣言は [`DEFAULT_CI_CMD`] を撃つ。
const CI_CMD_KEY: &str = "ci-cmd";

/// CI の判定を読む行の既定（forge の CLI・`{sha}` に着地した sha が入る）。`event` は読み手が
/// `schedule` の run を母集団から外すための欄（設計 pipeline.md §46）。
pub const DEFAULT_CI_CMD: &str = "gh run list --commit {sha} --json status,conclusion,event";

/// CI の行が必ず持つ穴（**着地した commit を名指さない行は撃てない**・別の commit の判定を読むことになる）。
pub const CI_SHA_HOLE: &str = "{sha}";

/// **要件面の path** の key（任意・設計 contract-source.md §2「表の検査」）。契約表の `req` の id をこの file で
/// 測る。書かない宣言は [`DEFAULT_REQUIREMENTS`] を読む（既存の宣言を 1 行も変えさせない）。
const REQUIREMENTS_KEY: &str = "requirements";

/// 要件面の既定 path（宣言 `requirements` が無い周）。
pub const DEFAULT_REQUIREMENTS: &str = "design-intent/spec/srs.html";

/// **書かなくてよい** key（無ければ空）。書いた周の空配列は従来どおり不備である（ADR-0010 §2.1）。
///
/// 任意にするのは、検出線を持たない consumer（toy repo 等）の宣言を 1 行も変えさせないためである。
pub(super) const OPTIONAL_KEYS: &[&str] = &[
    DETECTION_KEY,
    REQUIREMENTS_KEY,
    REMOTE_KEY,
    CI_CMD_KEY,
    path_kinds::DESIGN_INTENT_KEY,
    path_kinds::DESIGN_DOC_KEY,
    path_kinds::TESTS_KEY,
    ENTRANCE_KEY,
];

/// 要件面の path（任意）。書いた周は repo 相対の path の文字列だけを受ける（repo の外を読まない）。
pub(super) fn requirements_of(found: &[(String, Raw, u64)], errors: &mut Vec<DeclError>) -> Option<String> {
    let (_, value, line) = found.iter().find(|(seen, _, _)| seen == REQUIREMENTS_KEY)?;
    match value {
        Raw::Text(path) if repo_relative(path) => Some(path.clone()),
        _ => {
            errors.push(DeclError::new(
                *line,
                format!("{REQUIREMENTS_KEY} は repo 相対の path の文字列である（空・絶対 path・home の短縮記号・.. は書けない）"),
            ));
            None
        }
    }
}

/// push 先の remote の名（任意）。**1 語だけ**を受ける——空白を含む値は `git push <remote> main:main` の
/// 引数が 2 つに割れ、別の ref を押すことになる。
pub(super) fn remote_of(found: &[(String, Raw, u64)], errors: &mut Vec<DeclError>) -> Option<String> {
    let (_, value, line) = found.iter().find(|(seen, _, _)| seen == REMOTE_KEY)?;
    match value {
        Raw::Text(name) if !name.trim().is_empty() && !name.split_whitespace().nth(1).is_some() => {
            Some(name.trim().to_owned())
        }
        _ => {
            errors.push(DeclError::new(*line, format!("{REMOTE_KEY} は空白を含まない 1 語の remote の名である")));
            None
        }
    }
}

/// CI の判定を読む 1 行（任意）。**`{sha}` の穴を必ず持つ**——穴の無い行は着地した commit を名指さず、
/// 別の commit の判定を読んで success と言いうる（測っていないものを測ったことにしない・C10）。
pub(super) fn ci_cmd_of(found: &[(String, Raw, u64)], errors: &mut Vec<DeclError>) -> Option<String> {
    let (_, value, line) = found.iter().find(|(seen, _, _)| seen == CI_CMD_KEY)?;
    match value {
        Raw::Text(cmd) if !cmd.trim().is_empty() && cmd.contains(CI_SHA_HOLE) => Some(cmd.trim().to_owned()),
        _ => {
            errors.push(DeclError::new(
                *line,
                format!("{CI_CMD_KEY} は {CI_SHA_HOLE} の穴を持つ 1 行である（着地した commit を名指さない行は撃てない）"),
            ));
            None
        }
    }
}

/// repo 相対の path か（空でない・絶対 path でない・home の短縮記号も `..` の段も持たない）。
fn repo_relative(path: &str) -> bool {
    !path.trim().is_empty() && !path.starts_with('/') && !path.contains('~') && !path.split('/').any(|part| part == "..")
}

/// 契約表の検査（`contracts check`）が読む宣言の事実（設計 contract-source.md §2「表の検査」）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableFacts {
    /// 上限と突き合わせた allowlist（verify 行の先頭語の基準）。
    pub allowed: Vec<String>,
    /// 禁じる語列（rules 行 [`DENIED_ROW`]・verify 行に intake と同じ判定を掛ける基準）。
    pub denied: Vec<String>,
    /// 要件面の repo 相対 path（宣言 `requirements`・無ければ [`DEFAULT_REQUIREMENTS`]）。
    pub requirements: String,
}

/// HEAD の宣言を読み、上限と突き合わせて契約表の検査の事実にする（intake と同じ読み口・作業ツリーは読まない）。
pub fn table_facts(repo: &Path, ceiling: &Ceiling<'_>) -> Result<TableFacts, Vec<DeclError>> {
    table_facts_named(repo, ceiling).map(|(facts, _)| facts)
}

/// [`table_facts`] に宣言の名乗り `entrance-flip` を添えた形（契約表の検査の判定行が名乗りの欄を出す・§54 形 5）。
pub fn table_facts_named(
    repo: &Path,
    ceiling: &Ceiling<'_>,
) -> Result<(TableFacts, Option<EntranceFlip>), Vec<DeclError>> {
    let sourced = Sourced::read(repo, ceiling)?;
    let requirements = sourced.declared.requirements.clone().unwrap_or_else(|| DEFAULT_REQUIREMENTS.to_owned());
    let entrance = sourced.declared.entrance_flip;
    let effective = sourced.measure(ceiling, &[])?;
    Ok((TableFacts { allowed: effective.allowed, denied: ceiling.denied.to_vec(), requirements }, entrance))
}

/// land の終端が読む宣言の事実（設計 contract-source.md §5・push 先と CI の行）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalFacts {
    /// push 先の remote の名（宣言 `remote`・**無ければ `None`＝終端を持たない**）。
    pub remote: Option<String>,
    /// CI の判定を読む 1 行（宣言 `ci-cmd`・無ければ [`DEFAULT_CI_CMD`]・`{sha}` の穴を持つ）。
    pub ci_cmd: String,
}

/// HEAD の宣言から終端の事実を解く（上限は読まない＝終端は allowlist と突き合わせない）。
///
/// **宣言が無い周も断る**（`Err`）。終端は「どこへ push し、どの行で CI を読むか」を宣言から受ける口で、
/// 宣言そのものが無い repo に既定で push するのは「測っていない先へ出す」ことになる（A1 の「出す」・C10）。
pub fn terminal_facts(repo: &Path) -> Result<TerminalFacts, Vec<DeclError>> {
    let declared = declared_at_head(repo)?;
    Ok(TerminalFacts {
        remote: declared.remote,
        ci_cmd: declared.ci_cmd.unwrap_or_else(|| DEFAULT_CI_CMD.to_owned()),
    })
}
