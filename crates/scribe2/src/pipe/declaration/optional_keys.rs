//! 宣言の任意 key の群（key の列 2 つ・key の名と既定・値の読み手・HEAD の宣言から値を外へ渡す口）（設計 pipeline.md §59）。
//!
//! 親（`declaration.rs`）の余地のために子 module へ置く（ADR-0047 の path の種別と同じ置き方）。外から呼ぶ path は親の
//! 再輸出で `crate::pipe::declaration::terminal_facts` などのまま。新しい任意 key は、key の名・読み手・key の列の 1 行ずつ・
//! 外へ渡す口をこの file へ、`Declared` の欄と `parse` の読みの 1 行を親へ足す。

use super::path_kinds;
use super::{declared_at_head, head_declaration, Ceiling, DeclError, Declared, EntranceFlip, Raw, Sourced, DETECTION_KEY, ENTRANCE_KEY};
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
    QUESTION_ROUTE_KEY,
    CLOSE_CHECK_KEY,
];

/// **close の理由の門に加わるか**の key（任意・設計 ledger-form.md §16・ADR-0097）。値は真偽だけ（既定は持たない＝
/// 書かない宣言は加わらない）。
const CLOSE_CHECK_KEY: &str = "close-check";

/// **問いの経路の 1 行**の key（任意・設計 vessel-hook.md §20 形 4・ADR-0084）。hook が選択式の問いの道具を断る 1 行の
/// 後ろに添える（既定は持たない＝書かない宣言は何も添えない）。
const QUESTION_ROUTE_KEY: &str = "question-route";

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
    QUESTION_ROUTE_KEY,
    CLOSE_CHECK_KEY,
];

/// close の理由の門に加わるか（任意）。真偽だけを受ける（文字列・整数・列は key と行番号を名指す不備）。
pub(super) fn close_check_of(found: &[(String, Raw, u64)], errors: &mut Vec<DeclError>) -> Option<bool> {
    let (_, value, line) = found.iter().find(|(seen, _, _)| seen == CLOSE_CHECK_KEY)?;
    match value {
        Raw::Bool(joins) => Some(*joins),
        _ => {
            errors.push(DeclError::new(*line, format!("{CLOSE_CHECK_KEY} は true か false の真偽だけである")));
            None
        }
    }
}

/// HEAD の宣言の close の理由の門への加わり（閉じた 3 値・設計 ledger-form.md §16）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseCheck {
    /// 加わる（key が true）。
    Joins,
    /// 加わらない（key が false か無い・宣言 file が HEAD に無い・git を撃てない）。
    Exempt,
    /// 読めない（宣言が在って不備）。「加わらない」に倒さない（C10）。
    Unreadable,
}

/// HEAD の宣言から close の理由の門への加わりを解く（読み手は [`head_declaration`] の 1 本・作業ツリーは読まない）。
pub fn close_check(repo: &Path) -> CloseCheck {
    check_of(head_declaration(repo))
}

/// HEAD の読みの結果（無い / 不備 / 値）から閉じた 3 値への写し。
fn check_of(read: Option<Result<Declared, Vec<DeclError>>>) -> CloseCheck {
    match read {
        None => CloseCheck::Exempt,
        Some(Err(_)) => CloseCheck::Unreadable,
        Some(Ok(declared)) if declared.close_check == Some(true) => CloseCheck::Joins,
        Some(Ok(_)) => CloseCheck::Exempt,
    }
}

/// 問いの経路の 1 行（任意）。前後の空白を除いて空でなく、制御文字を持たない文字列だけを受ける（列・整数・空・空白だけ・
/// 制御文字は key と行番号を名指す不備）。
pub(super) fn question_route_of(found: &[(String, Raw, u64)], errors: &mut Vec<DeclError>) -> Option<String> {
    let (_, value, line) = found.iter().find(|(seen, _, _)| seen == QUESTION_ROUTE_KEY)?;
    match value {
        Raw::Text(route) if !route.trim().is_empty() && !route.chars().any(char::is_control) => Some(route.trim().to_owned()),
        _ => {
            errors.push(DeclError::new(*line, format!("{QUESTION_ROUTE_KEY} は制御文字を持たない空でない 1 行の文字列である")));
            None
        }
    }
}

/// HEAD の宣言の問いの経路（閉じた 3 値・設計 vessel-hook.md §20 形 4）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuestionRoute {
    /// 宣言した値（前後の空白を除いた 1 行）。
    Declared(String),
    /// 無い（宣言 file が HEAD に無い・git を撃てない・key が無い）。
    Absent,
    /// 読めない（宣言が在って不備）。「無い」に倒さない（C10）。
    Unreadable,
}

/// HEAD の宣言から問いの経路を解く（読み手は [`head_declaration`] の 1 本・作業ツリーは読まない）。
pub fn question_route(repo: &Path) -> QuestionRoute {
    route_of(head_declaration(repo))
}

/// HEAD の読みの結果（無い / 不備 / 値）から閉じた 3 値への写し。
fn route_of(read: Option<Result<Declared, Vec<DeclError>>>) -> QuestionRoute {
    match read {
        None => QuestionRoute::Absent,
        Some(Err(_)) => QuestionRoute::Unreadable,
        Some(Ok(declared)) => declared.question_route.map_or(QuestionRoute::Absent, QuestionRoute::Declared),
    }
}

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

#[cfg(test)]
mod tests {
    use super::super::Declared;
    use super::{check_of, close_check, route_of, CloseCheck, QuestionRoute};

    /// 必須 key だけの宣言の本文（3 行）の後ろに `extra` を足す。
    fn with(extra: &str) -> String {
        format!("schema = 1\nallowed-commands = [\"git\"]\ncommon-verify = [\"git diff --quiet\"]\n{extra}")
    }

    /// key の無い宣言は値なし、1 行の値は前後の空白を除いた値。
    #[test]
    fn declaration_question_route_reads_one_trimmed_line_or_nothing() {
        assert_eq!(Declared::parse(&with("")).map(|found| found.question_route), Ok(None));
        let declared = Declared::parse(&with("question-route = \"  台帳の問いへ \"\n"));
        assert_eq!(declared.map(|found| found.question_route), Ok(Some("台帳の問いへ".to_owned())));
    }

    /// 空・空白だけ・tab を含む・列・整数の値は key と行番号（4 行目）を名指す不備。
    #[test]
    fn declaration_question_route_refuses_empty_blank_control_and_lists() {
        for value in ["\"\"", "\"   \"", "\"a\tb\"", "[\"a\"]", "1"] {
            let errors = Declared::parse(&with(&format!("question-route = {value}\n"))).expect_err(value);
            assert!(errors.iter().any(|error| error.line == 4 && error.reason.contains("question-route")), "{value}: {errors:?}");
        }
    }

    /// HEAD の読みの結果（無い / 不備 / 値）から閉じた 3 値への写し。key の無い宣言は「無い」。
    #[test]
    fn declaration_question_route_maps_the_head_read_to_three_values() {
        assert_eq!(route_of(None), QuestionRoute::Absent);
        assert_eq!(route_of(Some(Err(Vec::new()))), QuestionRoute::Unreadable);
        assert_eq!(route_of(Some(Declared::parse(&with("")))), QuestionRoute::Absent);
        let declared = Declared::parse(&with("question-route = \"x\"\n"));
        assert_eq!(route_of(Some(declared)), QuestionRoute::Declared("x".to_owned()));
    }

    /// key が true は加わる・false と key の無い宣言は加わらない（欄は真偽のまま）。
    #[test]
    fn declaration_close_check_reads_true_false_and_absent() {
        assert_eq!(Declared::parse(&with("")).map(|found| found.close_check), Ok(None));
        assert_eq!(Declared::parse(&with("close-check = true\n")).map(|found| found.close_check), Ok(Some(true)));
        assert_eq!(Declared::parse(&with("close-check = false\n")).map(|found| found.close_check), Ok(Some(false)));
    }

    /// 文字列・整数・列は key と行番号（4 行目）を名指す不備、重複は 5 行目を名指す不備。
    #[test]
    fn declaration_close_check_refuses_text_int_list_and_duplicates() {
        for value in ["\"true\"", "\"\"", "1", "0", "[\"true\"]"] {
            let errors = Declared::parse(&with(&format!("close-check = {value}\n"))).expect_err(value);
            assert!(errors.iter().any(|error| error.line == 4 && error.reason.contains("close-check")), "{value}: {errors:?}");
        }
        let errors = Declared::parse(&with("close-check = true\nclose-check = false\n")).expect_err("重複");
        assert!(errors.iter().any(|error| error.line == 5 && error.reason.contains("close-check")), "{errors:?}");
    }

    /// HEAD の読みの結果（無い / 不備 / 値）から閉じた 3 値への写し。true だけが加わり、型違いの宣言は読めない。
    #[test]
    fn declaration_close_check_maps_the_head_read_to_three_values() {
        assert_eq!(check_of(None), CloseCheck::Exempt);
        assert_eq!(check_of(Some(Err(Vec::new()))), CloseCheck::Unreadable);
        assert_eq!(check_of(Some(Declared::parse(&with("")))), CloseCheck::Exempt);
        assert_eq!(check_of(Some(Declared::parse(&with("close-check = false\n")))), CloseCheck::Exempt);
        assert_eq!(check_of(Some(Declared::parse(&with("close-check = true\n")))), CloseCheck::Joins);
        assert_eq!(check_of(Some(Declared::parse(&with("close-check = \"true\"\n")))), CloseCheck::Unreadable);
    }

    /// git を撃てない dir（repo でない）は宣言の無い周と同じ「加わらない」。
    #[test]
    fn declaration_close_check_treats_a_dir_without_git_as_exempt() {
        assert_eq!(close_check(std::path::Path::new("/nonexistent-close-check-dir")), CloseCheck::Exempt);
    }

    /// 真偽を書いた他の key は key ごとの型の不備になり、entrance-flip = true は 3 語の外として key と行番号を名指す。
    #[test]
    fn declaration_close_check_bool_in_other_keys_is_a_typed_refusal() {
        let errors = Declared::parse(&with("entrance-flip = true\n")).expect_err("entrance-flip");
        assert!(errors.iter().any(|error| error.line == 4 && error.reason.contains("entrance-flip")), "{errors:?}");
        let errors = Declared::parse(&with("remote = true\n")).expect_err("remote");
        assert!(errors.iter().any(|error| error.line == 4 && error.reason.contains("remote")), "{errors:?}");
        let text = "schema = true\nallowed-commands = [\"git\"]\ncommon-verify = [\"git diff --quiet\"]\n";
        let errors = Declared::parse(text).expect_err("schema");
        assert!(errors.iter().any(|error| error.line == 1 && error.reason.contains("schema")), "{errors:?}");
    }
}
