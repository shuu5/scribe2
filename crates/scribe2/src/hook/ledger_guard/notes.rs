//! 起票の門の notes の段（設計 docs/design/ledger-form.md §18・FR81 (b)）。
//!
//! `bd` / `bdw` の update と create の `--notes` / `--append-notes`（`--flag value` と `--flag=value`・値は次の語を無条件に取る）
//! と、`note` の本文の語と `--file` の file を notes の本文として読み、本文のどれかの行が裁定の行
//! （[`is_ruling_line`]・判定はその 1 本だけが持つ）なら止める。`--stdin`・値の無い flag・開けない file・`$` か backtick を
//! 含む値は裁定の行が無いと言えないので読めない側で止める。本文を読めた分だけを読み手へ渡し、台帳の接頭辞は裁定の行が
//! 読める周にだけ引く。

use super::{deny, is_assignment, LedgerDecision, CLIENTS};
use crate::ledger::close_reason::is_ruling_line;
use crate::name::NAME;

/// 本文を書く flag（update と create・`--notes` は置き換え・`--append-notes` は足す）。
const SETS: [&str; 2] = ["--notes", "--append-notes"];

/// 本文を書く flag を持つ subcommand。
const FLAGGED: [&str; 2] = ["create", "update"];

/// 本文を語と file で渡す subcommand。
const NOTE: &str = "note";

/// `note` の本文の file の flag。
const FILE: &str = "--file";

/// 標準入力から読む flag。
const STDIN: &str = "--stdin";

/// 裁定の行を書く席の書きの記録の語。
const RULING_LINE: &str = "notes-ruling-line";

/// 本文を読めない書きの記録の語。
const UNREADABLE: &str = "notes-unreadable";

/// 本文 1 つの読み（読めた字か、読めない理由）。
type Text = Result<String, String>;

/// segment の語が notes を書く口なら、subcommand と本文の読みの列を返す（それ以外は空）。
fn texts_of(words: &[String], read: &impl Fn(&str) -> Option<String>) -> Vec<(&'static str, Text)> {
    let mut rest = words.iter().skip_while(|word| is_assignment(word));
    let Some(client) = rest.next().and_then(|word| word.rsplit('/').next()) else {
        return Vec::new();
    };
    if !CLIENTS.contains(&client) {
        return Vec::new();
    }
    let rest: Vec<&str> = rest.map(String::as_str).collect();
    let Some(at) = rest.iter().position(|word| !word.starts_with('-')) else {
        return Vec::new();
    };
    let args = rest.get(at.saturating_add(1)..).unwrap_or_default();
    let name = rest.get(at).copied().unwrap_or_default();
    if let Some(sub) = FLAGGED.into_iter().find(|sub| *sub == name) {
        return flagged(args).into_iter().map(|text| (sub, text)).collect();
    }
    if name == NOTE {
        return note(args, read).into_iter().map(|text| (NOTE, text)).collect();
    }
    Vec::new()
}

/// 語を `--flag` と `=` の後ろの値に割る（flag でない語は値なし・flag の名は語のまま）。
fn split_flag(word: &str) -> (&str, Option<&str>) {
    match word.split_once('=') {
        Some((flag, value)) if word.starts_with('-') => (flag, Some(value)),
        _ => (word, None),
    }
}

/// 展開が要る字を含む値は読めない（展開の後の字を門は知らない）。
fn plain(what: &str, value: &str) -> Text {
    if value.contains(['$', '`']) {
        return Err(format!("{what} に $ か backtick（展開の後の字を門は知らない）"));
    }
    Ok(value.to_owned())
}

/// update と create の本文を書く flag の値の列（値は次の語を無条件に取る・値の無い flag は読めない）。
fn flagged(args: &[&str]) -> Vec<Text> {
    let mut found = Vec::new();
    let mut words = args.iter().copied();
    while let Some(word) = words.next() {
        let (flag, inline) = split_flag(word);
        if !SETS.contains(&flag) {
            continue;
        }
        let value = inline.or_else(|| words.next());
        found.push(value.map_or_else(|| Err(format!("{flag} の値が無い")), |text| plain(&format!("{flag} の値"), text)));
    }
    found
}

/// `note <id> [text...]` の本文（id の後の flag でない語を空白 1 つで繋いだ字）と `--file` の file と `--stdin`。
fn note(args: &[&str], read: &impl Fn(&str) -> Option<String>) -> Vec<Text> {
    let mut found = Vec::new();
    let mut bare: Vec<&str> = Vec::new();
    let mut words = args.iter().copied();
    while let Some(word) = words.next() {
        let (flag, inline) = split_flag(word);
        if !word.starts_with('-') {
            bare.push(word);
        } else if flag == STDIN && inline != Some("false") {
            found.push(Err(format!("{STDIN}（標準入力の字を門は知らない）")));
        } else if flag == FILE {
            let path = inline.or_else(|| words.next());
            found.push(path.map_or_else(|| Err(format!("{FILE} の値が無い")), |path| file_text(path, read)));
        }
    }
    let text = bare.iter().skip(1).copied().collect::<Vec<_>>().join(" ");
    if !text.is_empty() {
        found.push(plain("note の本文", &text));
    }
    found
}

/// `--file` の file の字（`-` は標準入力・`$` か backtick を含む path・開けない file は読めない）。
fn file_text(path: &str, read: &impl Fn(&str) -> Option<String>) -> Text {
    let path = path.trim();
    if path == "-" {
        return Err(format!("{FILE} が - （標準入力の字を門は知らない）"));
    }
    plain(&format!("{FILE} の path"), path)?;
    read(path).ok_or_else(|| format!("{FILE} の file を開けない"))
}

/// notes の段（裁定の行が先・読めない書きが次・当たらなければ `None`）。`read` は cwd から解いた file の字・`prefix` は台帳の
/// 接頭辞（本文を読めた周にだけ引く）。
pub(super) fn judge(
    segments: &[Vec<String>],
    read: &impl Fn(&str) -> Option<String>,
    prefix: impl Fn() -> Option<String>,
) -> Option<LedgerDecision> {
    let found: Vec<(&str, Text)> = segments.iter().flat_map(|words| texts_of(words, read)).collect();
    let prefix = found.iter().any(|(_, text)| text.is_ok()).then(prefix).flatten();
    let ruled = found.iter().find_map(|(sub, text)| {
        let hit = text.as_ref().is_ok_and(|text| text.lines().any(|line| is_ruling_line(line, prefix.as_deref())));
        hit.then_some(sub)
    });
    if let Some(sub) = ruled {
        let why = "裁定の行を書くのは器の口（seat ruling bind・裁定面の答えの口）だけ — 発話を bind で問いへ結ぶ";
        return Some(refuse(sub, RULING_LINE, why.to_owned()));
    }
    let (sub, what) = found.iter().find_map(|(sub, text)| text.as_ref().err().map(|what| (sub, what)))?;
    let why = format!("notes の本文を読めない（{what}） — 本文を file に書いて bdw note <id> --file <path> で足す");
    Some(refuse(sub, UNREADABLE, why))
}

/// 断り文 1 行（既存の形・出所は ledger-form.md §18）。
fn refuse(sub: &str, what: &str, why: String) -> LedgerDecision {
    deny(what, format!("{NAME}: deny bd {sub} は起票の門が止める reason={what}（{why}・ledger-form.md §18）"))
}

#[cfg(test)]
mod tests {
    use super::{judge, texts_of, LedgerDecision, Text};
    use crate::hook::ledger_guard::segments;

    /// 本文の file を 1 つだけ持つ読み手（path が `f` のとき `line`）。
    fn reads(path: &str) -> Option<String> {
        (path == "f").then(|| "batch:x | s2-1 | 逐語".to_owned())
    }

    /// command 行の最初の segment が書く本文の読み。
    fn texts(line: &str) -> Vec<Text> {
        let words = segments(line).into_iter().next().unwrap_or_default();
        texts_of(&words, &reads).into_iter().map(|(_, text)| text).collect()
    }

    /// 台帳の接頭辞を持たない周の判定の語（通すなら `None`）。
    fn what(line: &str) -> Option<String> {
        match judge(&segments(line), &reads, || None) {
            Some(LedgerDecision::Deny { what, .. }) => Some(what),
            _ => None,
        }
    }

    /// 値の対: `--flag value` と `--flag=value`・何度でも・値は次の語を無条件（`-` で始まる本文も値）。
    #[test]
    fn hook_notes_ruling_reads_flag_value_pairs() {
        let ok = |text: &str| Ok(text.to_owned());
        assert_eq!(texts("bd update s2-1 --append-notes a --notes=b"), [ok("a"), ok("b")]);
        assert_eq!(texts("X=1 scripts/bdw create t --parent s2-1 --append-notes --x --notes -y"), [ok("--x"), ok("-y")]);
        assert_eq!(texts("bdw update s2-1 --append-notes="), [ok("")]);
        assert_eq!(texts("bdw update s2-1 --design --notes=d"), [ok("d")], "ほかの flag の語は読まない");
        assert_eq!(texts("bdw update s2-1 --append-notes").len(), 1, "値の無い flag は読めない 1 件");
        assert!(texts("bdw update s2-1 --append-notes").iter().all(Result::is_err));
        assert!(texts("bdw close s2-1 --notes x").is_empty() && texts("echo bd update --notes x").is_empty());
    }

    /// note の本文: id の後の flag でない語を空白 1 つで繋ぎ、`--file` の file の字を読む。
    #[test]
    fn hook_notes_ruling_joins_note_words_and_reads_the_file() {
        let ok = |text: &str| Ok(text.to_owned());
        assert_eq!(texts("bdw note s2-1 a b 'c  d'"), [ok("a b c  d")]);
        assert_eq!(texts("bd note s2-1 --file f"), [ok("batch:x | s2-1 | 逐語")]);
        assert_eq!(texts("bd note s2-1 --file=f"), [ok("batch:x | s2-1 | 逐語")]);
        assert!(texts("bd note s2-1").is_empty(), "本文の無い note は空");
    }

    /// 読めない字: `--stdin`・値の無い flag・開けない file・`-` の file・`$` か backtick の値。
    #[test]
    fn hook_notes_ruling_marks_unreadable_forms() {
        for line in [
            "bd note s2-1 --stdin",
            "bd note s2-1 --file",
            "bd note s2-1 --file nope",
            "bd note s2-1 --file -",
            "bd note s2-1 '$(cat f)'",
            "bd note s2-1 --file '$F'",
            "bdw update s2-1 --append-notes '$(cat f)'",
            "bdw create t --parent s2-1 --notes=`x`",
        ] {
            assert_eq!(what(line).as_deref(), Some("notes-unreadable"), "{line}");
        }
        assert_eq!(what("bd note s2-1 --stdin=false ok"), None, "=false の --stdin は読めない形でない");
    }

    /// 裁定の行は読めない書きより先に当たり、裁定の行を含まない本文は通る。
    #[test]
    fn hook_notes_ruling_orders_the_words_and_passes_plain_text() {
        assert_eq!(what("bdw update s2-1 --append-notes 'a | batch:x | b'").as_deref(), Some("notes-ruling-line"));
        assert_eq!(what("bdw update s2-1 --append-notes '$(x)' --append-notes 'batch:x'").as_deref(), Some("notes-ruling-line"));
        assert_eq!(what("bd note s2-1 --file f").as_deref(), Some("notes-ruling-line"));
        assert_eq!(what("bdw update s2-1 --append-notes 'plain' --design 'batch:x'"), None);
        assert_eq!(what("bdw update s2-1 --append-notes '### 出所'"), None);
        assert_eq!(what("bdw update s2-1 --append-notes '裁定 batch:x を引く'"), None);
    }
}
