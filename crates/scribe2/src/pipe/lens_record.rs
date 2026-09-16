//! lens の cmd の写し（設計 docs/design/pipeline.md §26・`s2-07l.378`・FR9 / FR10）。
//!
//! `pipe review --lens <cmd>`（run の最初に lens を受ける口）が cmd を `<run_dir>/lens.toml` に写し、`gate` /
//! `land`（と `resume` が通る同じ関数）は `--lens` が無い周にその写しを読む（`--lens` が在れば flag が勝つ）。
//! 操作役が land を後から手で撃つ周に毎回同じ cmd を渡さないと、main が動いた便の再 gate が「lens が要るのに
//! --lens が無い」で INCONCLUSIVE に倒れる（admin の実測 2026-09-16）——record の 1 面でその seam を塞ぐ。
//!
//! 形は `schema = 1` / `cmd = "<逐語>"` の 2 行（rules manifest と同じ parser の subset・[`scalar`]・run dir の
//! 一時物で跨版契約ではない）。読み手は先頭と末尾の `"` だけを剥がし中身を素通しで返すので、cmd の中の `"` と
//! `\` は escape しない（往復する）。改行を含む cmd は 1 行に書けないので、書き手が typed に断る（fail-closed）。
//!
//! 読みの結果と口への引数は同じ閉じた型 [`LensSource`] 1 つ: 写しが**無い**は従来どおりの INCONCLUSIVE、写しが
//! **読めない**は path と理由を持つ別の値（「無い」に潰さない・C10）。3 つの口は [`resolve`] の 1 関数で読む（C2）。

use crate::fleet::SCHEMA;
use crate::rules::manifest::{scalar, Scalar};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// run dir に残す lens の cmd の写し。
pub const FILE: &str = "lens.toml";

/// lens の cmd の出所（flag か写し）。**`Option<&str>` で持たない**（「無い」と「読めない」の 2 つの意味があるため・
/// C10 / C3.3）。gate / review の判定順は 3 値を match して INCONCLUSIVE の理由を分ける。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LensSource {
    /// flag か写しから得た cmd（逐語）。
    Cmd(String),
    /// flag も写しも無い。
    Absent,
    /// 写しは在るが読めない（path と読めなかった理由）。
    Unreadable {
        /// 読めなかった写し。
        path: PathBuf,
        /// 読めなかった理由。
        reason: String,
    },
}

/// 便の lens の写し。
pub fn record_path(run_dir: &Path) -> PathBuf {
    run_dir.join(FILE)
}

/// 書き手（**1 本**）: `--lens` の cmd を写す。改行を含む cmd と空の cmd は写せない（`Err`＝呼び手は段を止める）。
pub fn keep(run_dir: &Path, cmd: &str) -> Result<(), String> {
    let body = render(cmd)?;
    let path = record_path(run_dir);
    std::fs::write(&path, body).map_err(|err| format!("{} を書けない: {err}", path.display()))
}

/// 読み手（**1 本**）: `--lens` が在れば flag、無ければ写し。写しが無い周は [`LensSource::Absent`]、在って読めない周は
/// [`LensSource::Unreadable`]（読めなさを「無い」に潰さない）。
pub fn resolve(flag: Option<&str>, run_dir: &Path) -> LensSource {
    if let Some(cmd) = flag {
        return LensSource::Cmd(cmd.to_owned());
    }
    let path = record_path(run_dir);
    let text = match std::fs::read_to_string(&path) {
        Ok(found) => found,
        Err(err) if err.kind() == ErrorKind::NotFound => return LensSource::Absent,
        Err(err) => return LensSource::Unreadable { path, reason: err.to_string() },
    };
    match parse(&text) {
        Ok(cmd) => LensSource::Cmd(cmd),
        Err(reason) => LensSource::Unreadable { path, reason },
    }
}

/// 写しの本文（**pure**）。cmd は素通し（escape しない）ゆえ、1 行に収まらない cmd と空の cmd だけを断る。
fn render(cmd: &str) -> Result<String, String> {
    if cmd.is_empty() {
        return Err("lens の cmd が空である（写せない）".to_owned());
    }
    if cmd.contains(['\n', '\r']) {
        return Err("lens の cmd が改行を含む（1 行の写しに書けない）".to_owned());
    }
    Ok(format!("schema = {SCHEMA}\ncmd = \"{cmd}\"\n"))
}

/// 写しの本文から cmd を読む（**pure**）。`schema = 1` と `cmd = "…"` の 2 key だけを受理し、未知 key・重複・
/// 欠け・形の違いは理由 1 行の `Err`。
pub fn parse(text: &str) -> Result<String, String> {
    let (mut schema, mut cmd): (Option<Scalar>, Option<Scalar>) = (None, None);
    for (index, raw) in text.lines().enumerate() {
        let line = index.saturating_add(1);
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            return Err(format!("{line} 行目が key = value の形でない"));
        };
        let key = key.trim();
        let slot = match key {
            "schema" => &mut schema,
            "cmd" => &mut cmd,
            _ => return Err(format!("{line} 行目の key {key} は未知")),
        };
        if slot.is_some() {
            return Err(format!("{line} 行目の key {key} が重複する"));
        }
        let Some(read) = scalar(value.trim()) else {
            return Err(format!("{line} 行目の {key} の value を読めない"));
        };
        *slot = Some(read);
    }
    match schema {
        Some(Scalar::Int(found)) if found == SCHEMA => {}
        Some(found) => return Err(format!("schema が {SCHEMA} でない（実 {found:?}）")),
        None => return Err(format!("schema = {SCHEMA} が無い")),
    }
    match cmd {
        Some(Scalar::Str(found)) if !found.is_empty() => Ok(found),
        Some(Scalar::Str(_)) => Err("cmd が空文字である".to_owned()),
        Some(_) => Err("cmd が文字列でない".to_owned()),
        None => Err("cmd が無い".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::{keep, parse, record_path, render, resolve, LensSource};
    use std::path::PathBuf;

    /// 歯ごとの空の tmp dir。
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pipe-lens-record-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    /// 写しは escape しない（cmd の中の `"` / `\` / `=` / `'` は素通しで往復する）。改行と空は書けない。
    #[test]
    fn pipe_lens_record_render_and_parse_round_trip_without_escaping() {
        let cmd = r#"cat >/dev/null; touch 'm'; echo '{"verdict":"PASS","evidence":"a\b"}' # k=v "tail""#;
        let body = render(cmd).unwrap();
        assert_eq!(body, format!("schema = 1\ncmd = \"{cmd}\"\n"), "2 行・二重引用符・素通し");
        assert_eq!(parse(&body).as_deref(), Ok(cmd), "往復する");
        assert_eq!(parse("schema = 1\n\n# memo\ncmd = \"x\"\n").as_deref(), Ok("x"), "空行と # は読み飛ばす");
        assert!(render("a\nb").is_err(), "改行は 1 行に書けない");
        assert!(render("a\rb").is_err(), "CR も同じ");
        assert!(render("").is_err(), "空の cmd は写さない");
    }

    /// 読めない形は理由 1 行で断る（欠け・重複・未知 key・schema 違い・文字列でない・空文字・形の違い）。
    #[test]
    fn pipe_lens_record_parse_names_the_defect() {
        for (text, needle) in [
            ("cmd = \"x\"\n", "schema = 1 が無い"),
            ("schema = 1\n", "cmd が無い"),
            ("schema = 2\ncmd = \"x\"\n", "schema が 1 でない"),
            ("schema = 1\ncmd = \"x\"\ncmd = \"y\"\n", "key cmd が重複する"),
            ("schema = 1\ncmd = \"x\"\nlens = \"y\"\n", "key lens は未知"),
            ("schema = 1\ncmd = 3\n", "cmd が文字列でない"),
            ("schema = 1\ncmd = \"\"\n", "cmd が空文字である"),
            ("schema = 1\ncmd\n", "2 行目が key = value の形でない"),
            ("schema = 1\ncmd = x\n", "2 行目の cmd の value を読めない"),
        ] {
            let reason = parse(text).expect_err(text);
            assert!(reason.contains(needle), "{text:?}: {reason}");
        }
    }

    /// 読み手は flag が勝ち、写しが無い周は `Absent`、在って読めない周は path と理由を持つ `Unreadable`。
    #[test]
    fn pipe_lens_record_resolve_prefers_flag_and_keeps_absent_apart_from_unreadable() {
        let dir = scratch("resolve");
        assert_eq!(resolve(None, &dir), LensSource::Absent, "写しが無い");
        assert_eq!(resolve(Some("flag"), &dir), LensSource::Cmd("flag".to_owned()), "flag は写しを読まずに勝つ");
        assert_eq!(keep(&dir, "echo '{\"verdict\":\"PASS\"}'"), Ok(()));
        assert_eq!(resolve(None, &dir), LensSource::Cmd("echo '{\"verdict\":\"PASS\"}'".to_owned()), "写しを読む");
        assert_eq!(resolve(Some("flag"), &dir), LensSource::Cmd("flag".to_owned()), "写しが在っても flag が勝つ");
        let path = record_path(&dir);
        std::fs::write(&path, "schema = 1\ncmd = 3\n").unwrap();
        let unreadable = resolve(None, &dir);
        let LensSource::Unreadable { path: found, reason } = unreadable else {
            panic!("読めない写しは Unreadable: {unreadable:?}");
        };
        assert_eq!(found, path, "path を持つ");
        assert!(reason.contains("cmd が文字列でない"), "理由を持つ: {reason}");
        assert!(keep(&dir, "a\nb").is_err(), "改行の cmd は書かない（fail-closed）");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "schema = 1\ncmd = 3\n", "断った周は写しを触らない");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
