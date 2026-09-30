//! publish の細かい語の読みと、包まれた gh が読みだけかの判じ（設計 docs/design/vessel-hook.md §23 行 o・ADR-0099・FR80 / NFR4）。

use super::{flag_of, target_of, API, GRAPHQL, PUSH, TABLE};
use crate::hook::ledger_guard::is_assignment;

/// 細かい語を割る shell の区切りの字（空白と `$` の前と `:-` `:=` `:+` `:?` でも割る）。
const BREAKS: [char; 10] = [';', '&', '|', '(', ')', '{', '}', '<', '>', '`'];
/// gh の読みだけの群と動詞の閉じた表（表の外と変数の動詞は読みだけでない）。
pub const READ_ONLY: [(&str, &[&str]); 6] = [
    ("pr", &["view", "list", "status", "checks", "diff"]),
    ("issue", &["view", "list", "status"]),
    ("release", &["view", "list"]),
    ("gist", &["view", "list"]),
    ("label", &["list"]),
    ("repo", &["view", "list"]),
];
/// gh api の読みだけの flag（値を取らない）。
const READ_BARE: [&str; 6] = ["-i", "--include", "--paginate", "--slurp", "--silent", "--verbose"];
/// gh api の読みだけの flag（値を取る・`-X` / `--method` は別に読む）。
const READ_VALUED: [&str; 6] = ["-q", "--jq", "-t", "--template", "--cache", "--hostname"];

/// gh の公開の群の語か。
pub fn is_group(word: &str) -> bool {
    word == API || TABLE.iter().any(|(group, _)| *group == word)
}

/// 細かい語に `git` とその後ろの `push`、か `gh` とその後ろの公開の群の語が在り、その gh が読みだけでないか（隣でなくてよい）。
pub fn follows(fine: &[(String, String)]) -> bool {
    fine.iter().enumerate().any(|(at, (_, word))| {
        let after = fine.get(at.saturating_add(1)..).unwrap_or_default();
        match word.as_str() {
            "git" => after.iter().any(|(_, next)| next == PUSH),
            "gh" => after.iter().any(|(_, next)| is_group(next)) && !read_only(after),
            _ => false,
        }
    })
}

/// gh の後ろの片が読みだけか（形 3）: `-R` / `--repo` とその値だけを読み飛ばした群と動詞が [`READ_ONLY`] の対か、api の後ろが対象 1 つと
/// 読みの flag だけ。`$` か backtick を持つ片（api は全ての片・他は `-R` の値と群と動詞）は撃つ時に割れて読みが変わるので読みだけでない。
fn read_only(after: &[(String, String)]) -> bool {
    let mut rest = after.iter().map(|(piece, _)| piece.as_str());
    let mut picked = Vec::new();
    while picked.len() < 2 {
        let Some(piece) = rest.next() else {
            return false;
        };
        match flag_of(piece) {
            Some(("-R" | "--repo", inline)) if !inline.or_else(|| rest.next()).is_none_or(loose) => {}
            Some(_) => return false,
            None if loose(piece) => return false,
            None => {
                picked.push(piece);
                if piece == API {
                    break;
                }
            }
        }
    }
    match picked.as_slice() {
        [API] => read_api(rest),
        [group, verb] => READ_ONLY.iter().any(|(name, verbs)| name == group && verbs.contains(verb)),
        _ => false,
    }
}

/// `$` か backtick を持つ片か。
fn loose(piece: &str) -> bool {
    piece.contains(['$', '`'])
}

/// gh api の後ろの片が対象 1 つ（`graphql` でない・`-` で始まらない）と読みの flag とその値だけか（値を取る flag は空白書きと長い名の
/// `--<名>=<値>` だけ・短い flag の続け書きは知らない flag・method の値は大文字の `GET` ちょうど）。
fn read_api<'a>(mut rest: impl Iterator<Item = &'a str>) -> bool {
    let mut targets = 0_usize;
    while let Some(piece) = rest.next() {
        if loose(piece) {
            return false;
        }
        let Some((name, inline)) = flag_of(piece) else {
            targets = targets.saturating_add(1);
            if piece.starts_with('-') || target_of(piece) == GRAPHQL {
                return false;
            }
            continue;
        };
        if READ_BARE.contains(&name) && inline.is_none() {
            continue;
        }
        let method = matches!(name, "-X" | "--method");
        if !(method || READ_VALUED.contains(&name)) || (!name.starts_with("--") && inline.is_some()) {
            return false;
        }
        match inline.or_else(|| rest.next()) {
            Some(value) if !loose(value) && (!method || value == "GET") => {}
            _ => return false,
        }
    }
    targets == 1
}

/// 語の細かい語（設計 §17 形 2）を（片・basename）の対にする: `$(` か `` ` `` を持つ語と、空白を持たずに `<(` か `>(` を持つ語は
/// [`pieces`] で割り、他の語は割らずに先頭の `(` `{` と末尾の `)` `}` `;` だけを落とす。どちらも片の先頭の `NAME=` を落とした片と
/// その basename（basename が空の片は捨てる）。印は basename を、読みだけの判じは片を読む（`--input=/tmp/b` の basename は flag を失う）。
pub fn fine(word: &str) -> Vec<(String, String)> {
    let substituted = word.contains("$(") || word.contains('`');
    let process = !word.contains(char::is_whitespace) && (word.contains("<(") || word.contains(">("));
    let parts = if substituted || process {
        pieces(word)
    } else {
        vec![word.trim_start_matches(['(', '{']).trim_end_matches([')', '}', ';'])]
    };
    parts
        .into_iter()
        .filter_map(|part| {
            let bare = if is_assignment(part) { part.split_once('=').map_or(part, |(_, value)| value) } else { part };
            let base = bare.rsplit('/').next().unwrap_or(bare);
            (!base.is_empty()).then(|| (bare.to_owned(), base.to_owned()))
        })
        .collect()
}

/// 割る語を片に分ける: 空白と [`BREAKS`] と `:-` `:=` `:+` `:?` で割り、`$` の前でも割る。
fn pieces(word: &str) -> Vec<&str> {
    let (mut found, mut start) = (Vec::new(), 0_usize);
    for (at, found_char) in word.char_indices() {
        let next = at.saturating_add(found_char.len_utf8());
        let resume = if found_char.is_whitespace() || BREAKS.contains(&found_char) {
            Some(next)
        } else if found_char == '$' {
            Some(at)
        } else if found_char == ':' && word.get(next..).is_some_and(|rest| rest.starts_with(['-', '=', '+', '?'])) {
            Some(next.saturating_add(1))
        } else {
            None
        };
        if let Some(resume) = resume {
            found.extend(word.get(start..at));
            start = resume;
        }
    }
    found.extend(word.get(start..));
    found
}

#[cfg(test)]
mod tests {
    use super::READ_ONLY;
    use crate::hook::host_guard::publish::{marked, Mark};
    use std::path::Path;

    /// command 行の公開の segment ごとの印（cwd `/w`・root `/root`）を 1 列にしたもの。
    fn marks(line: &str) -> Vec<Mark> {
        marked(line, Path::new("/w"), Path::new("/root")).into_iter().flat_map(|seg| seg.marks).collect()
    }

    /// 行 o (a) 読みだけの形（loop の本体と条件・command 置換・二重引用・`-R` の後・list・issue・api の読み）は印を持たない。
    #[test]
    fn publish_read_only_gh_forms_carry_no_mark() {
        for line in [
            "x=$(gh pr view 859 --repo o/n --json state)", "for i in 1 2; do gh pr checks 859; done", "until gh pr checks 859; do sleep 30; done",
            "while ! gh pr checks 1; do sleep 5; done", "if gh pr view 1 --json state | grep -q MERGED; then echo ok; fi",
            "echo \"$(gh pr view 1)\"", "x=$(gh -R o/n pr view 1)", "x=$(gh pr list --state open)", "x=$(gh issue view 3)",
            "x=$(gh api repos/o/n/pulls/1 --jq .state)", "x=$(gh api -X GET repos/o/n/pulls/1)",
        ] {
            assert_eq!(marks(line), [], "{line}");
        }
    }

    /// 行 o (b) 書きと決められない並び（書きの動詞・書きの api・続け書き・小文字の method・変数・知らない flag・引用の中の改行）は
    /// wrapped のまま。
    #[test]
    fn publish_read_only_gh_writing_or_undecided_forms_stay_wrapped() {
        for line in [
            "x=$(gh pr create --fill)", "for i in 1; do gh pr merge 1; done", "x=$(gh api -X PATCH repos/o/n -f title=x)",
            "x=$(gh api repos/o/n/issues --input /tmp/b)", "x=$(gh api repos/o/n/issues -f title=x)", "x=$(gh api -H 'A: b' repos/o/n)",
            "x=$(gh api graphql --jq .a)", "x=$(gh api -XPATCH repos/o/n)", "x=$(gh api -fk=v repos/o/n)", "x=$(gh api -Fk=@f repos/o/n)",
            "x=$(gh api -X post repos/o/n)", "x=$(gh $v pr view 1)", "x=$(gh pr $v 1)", "x=$(gh --foo pr view 1)",
            "echo \"$(gh pr view 1\ngh pr create)\"", "x=$(gh api $T)", "x=$(gh api repos/o/n/pulls/1 --jq $Q)", "x=$(gh -R $R pr view 1)",
        ] {
            assert_eq!(marks(line).first(), Some(&Mark::Wrapped), "{line}: {:?}", marks(line));
        }
    }

    /// 行 o (c) 読みだけの表の群と動詞の対の宣言順。
    #[test]
    fn publish_read_only_gh_table_pairs_are_declared_in_order() {
        let want: [(&str, &[&str]); 6] = [
            ("pr", &["view", "list", "status", "checks", "diff"]), ("issue", &["view", "list", "status"]), ("release", &["view", "list"]),
            ("gist", &["view", "list"]), ("label", &["list"]), ("repo", &["view", "list"]),
        ];
        assert_eq!(READ_ONLY, want);
    }
}
