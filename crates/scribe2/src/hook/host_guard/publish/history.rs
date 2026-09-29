//! publish の全履歴の段の種別（設計 docs/design/vessel-hook.md §18 行 l・ADR-0078・SRS FR80 / AC50 (d)）。
//!
//! repo の全履歴を走査せずに出す command の閉じた 3 種別と、公開の segment の読み 1 つから種別を返す pure な 1 関数を持つ。
//! host の面も GitHub も読まず、子 process を撃たない（配線は後続の行）。

use super::scan::body_of;
use super::{flag_of, Api, Published, Sort};

/// 全履歴の断りの経路。
pub const ROUTE: &str = "持ち主が手で行う（可視性を public へ変える command と public の repo を作る command は repo の全履歴を走査せずに出すので席の session からは撃たない）";
/// gh の repo の群。
const REPO: &str = "repo";
/// 可視性を変える flag。
const VISIBILITY: &str = "--visibility";
/// public の repo を作る flag。
const PUBLIC_FLAG: &str = "--public";
/// 可視性の欄の値の語。
const PUBLIC: &str = "public";
/// pflag の真の字。
const TRUE: [&str; 3] = ["1", "t", "true"];

/// 全履歴を出す command の種別（閉じた 3 値・宣言順が断りの順・設計 §18 形 1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum History {
    /// 群 repo・動詞 edit が `--visibility` の値 public を持つ。
    RepoEditPublic,
    /// 群 repo・動詞 create か new が `--public` を値なしか真の字の値で持つ。
    RepoCreatePublic,
    /// `repos/<o>/<n>` への api の書きが欄 `visibility=public` か `private=false` を持つ。
    ApiVisibilityPublic,
}

/// [`History`] の全 variant（宣言順）。
pub const HISTORIES: &[History] = &[History::RepoEditPublic, History::RepoCreatePublic, History::ApiVisibilityPublic];

impl History {
    /// hit の `full-history:` の後ろの語。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RepoEditPublic => "repo-edit-public",
            Self::RepoCreatePublic => "repo-create-public",
            Self::ApiVisibilityPublic => "api-visibility-public",
        }
    }
}

/// 値が `words` のどれかか（**1 関数**・設計 §18 形 2）: 区切りを引用した heredoc なら間の行、でなければ値そのものを、末尾の
/// 改行を落として（command 置換が落とす）ASCII の大小を畳んで比べる。
fn is_one_of(value: &str, words: &[&str]) -> bool {
    let body = body_of(value);
    let folded = body.trim_end_matches('\n');
    words.iter().any(|word| folded.eq_ignore_ascii_case(word))
}

/// 動詞の後ろの語の flag `name` の出現ごとの（続け書きの値・次の語）。
fn occurrences<'a>(rest: &'a [String], name: &str) -> Vec<(Option<&'a str>, Option<&'a str>)> {
    let found = rest.iter().enumerate().filter_map(|(at, word)| {
        let (flag, inline) = flag_of(word)?;
        (flag == name).then(|| (inline, rest.get(at.saturating_add(1)).map(String::as_str)))
    });
    found.collect()
}

/// api の書きが `repos/<o>/<n>` のちょうど 3 成分（placeholder の字面を含む）へ可視性を public にする欄を持つか。
fn api_public(api: &Api) -> bool {
    let parts: Vec<&str> = api.target.as_deref().unwrap_or_default().split('/').collect();
    let opens = |(_, key, value): &(String, String, String)| match key.as_str() {
        "visibility" => is_one_of(value, &[PUBLIC]),
        "private" => is_one_of(value, &["false"]),
        _ => false,
    };
    matches!(parts.as_slice(), ["repos", _, _]) && api.writes() && api.fields.iter().any(opens)
}

/// 公開の segment 1 つが種別を持つか（1 種別 1 arm）。
fn has(history: History, found: &Published) -> bool {
    let repo = found.sort == Sort::Gh && found.group.as_deref() == Some(REPO);
    let verb = found.verb.as_deref();
    match history {
        History::RepoEditPublic => {
            let public = |(inline, next): (Option<&str>, Option<&str>)| inline.or(next).is_some_and(|value| is_one_of(value, &[PUBLIC]));
            repo && verb == Some("edit") && occurrences(&found.rest, VISIBILITY).into_iter().any(public)
        }
        History::RepoCreatePublic => {
            let public = |(inline, _): (Option<&str>, Option<&str>)| inline.is_none_or(|value| is_one_of(value, &TRUE));
            repo && matches!(verb, Some("create" | "new")) && occurrences(&found.rest, PUBLIC_FLAG).into_iter().any(public)
        }
        History::ApiVisibilityPublic => found.sort == Sort::Api && found.api.as_ref().is_some_and(api_public),
    }
}

/// 公開の segment の読み 1 つから全履歴の種別を返す（**pure な 1 関数**・設計 §18 形 3・宣言順の先に当たった 1 つ）。
pub fn kind_of(found: &Published) -> Option<History> {
    HISTORIES.iter().copied().find(|history| has(*history, found))
}

#[cfg(test)]
mod tests {
    use super::super::read;
    use super::{kind_of, History, HISTORIES};
    use std::path::Path;

    /// 命令行の各公開の segment が返す種別の語。
    fn kinds(line: &str) -> Vec<&'static str> {
        let found = read(line, Path::new("/w"), Path::new("/root"));
        found.iter().filter_map(kind_of).map(History::as_str).collect()
    }

    /// 区切りを引用した heredoc の値（外の二重引用つき）。
    fn heredoc(body: &str) -> String {
        format!("\"$(cat <<'EOF'\n{body}\nEOF\n)\"")
    }

    /// (a) 当たる表: 3 種別がそれぞれ当たる形（続け書き・大小・引用した heredoc の値・method の無い欄を持つ api）。
    #[test]
    fn publish_history_kinds_hit_the_closed_table() {
        let edit = format!("gh repo edit o/n --visibility {}", heredoc("public"));
        let api = format!("gh api --method PATCH repos/o/n -f visibility={}", heredoc("Public"));
        let table: [(&str, Vec<&str>); 3] = [
            ("repo-edit-public", vec!["gh repo edit o/n --visibility public", "gh repo edit --visibility=PUBLIC", edit.as_str()]),
            ("repo-create-public", vec!["gh repo create x --public --source . --push", "gh repo new x --public", "gh repo create x --public=true"]),
            ("api-visibility-public", vec![
                "gh api -X PATCH repos/o/n -f visibility=public", "gh api -X PATCH repos/{owner}/{repo} -F private=false", api.as_str(),
                "gh api repos/o/n -f visibility=public",
            ]),
        ];
        for (want, lines) in table {
            for line in lines {
                assert_eq!(kinds(line), [want], "{line}");
            }
        }
    }

    /// (b) 当たらない表: 値が違う・別の flag・別の対象（4 成分）・作るだけの api・本文の語・git の ref。
    #[test]
    fn publish_history_kinds_miss_the_near_forms() {
        for line in [
            "gh repo edit o/n --visibility private", "gh repo edit o/n --description public", "gh repo create x --private",
            "gh repo create x --public=false", "gh api -X PATCH repos/o/n -f visibility=private", "gh api -X PATCH repos/o/n -f private=true",
            "gh api -X PATCH repos/o/n/pages -f visibility=public", "gh api -X POST user/repos -f name=x",
            "gh pr create --title public --body public", "git push origin public",
        ] {
            assert_eq!(kinds(line), Vec::<&str>::new(), "{line}");
        }
    }

    /// (c) 種別の const slice の語は宣言順に repo-edit-public / repo-create-public / api-visibility-public。
    #[test]
    fn publish_history_kinds_are_the_closed_three_in_order() {
        let words: Vec<&str> = HISTORIES.iter().map(|history| history.as_str()).collect();
        assert_eq!(words, ["repo-edit-public", "repo-create-public", "api-visibility-public"]);
    }
}
