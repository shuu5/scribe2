//! 閉じた enum と、その全 variant を並べる const slice の**集合完全性**を測る（enum-slices）。
//!
//! ADR-0013 §2.2 の C2 充足形（閉じた enum + 全 variant の const slice + 網羅 match + 昇順の歯）は、
//! enum の末尾に足した variant を const slice へ入れ忘れた形をどの面も受けていなかった（§2.3・
//! 網羅 match は arm を書かせるが slice への登録は強制しない）。ここは**字面**で、
//! `pub const NAME: &[Enum] = &[…]` の型が名指す enum を同じ file で探し、variant の名前の集合と
//! slice の要素の集合が一致することを測る。対応表は持たない（slice の型が enum を名指す）。
//!
//! **読めない形は違反に倒す**（fail-closed・bd `s2-07l.88`）: payload 付き variant・判別子指定・
//! 属性行・1 行に複数・enum を同じ file に見つけられない、のどれも「黙って一致」にしない。
//! 型の側も同じ極性で、要素の型が大文字で始まるのに裸の識別子でない形（`&[&Enum]` / `&[Box<Enum>]`）
//! は違反にする（lens-88 MEDIUM-1: 黙って母集団から消える形を作らない）。`&'static [Enum]` と
//! 字下げされた const（impl / mod の中）は対に数える。小文字始まり・tuple・`&str` は enum の
//! slice ではないので対象外。Rust の parser は足さない（字面で数える・憲法 C13）。
//!
//! **意図した部分集合の const は書けない**（lens-88 MEDIUM-5）: `&[Enum]` 型の const は全 variant
//! を並べる形しか通らず、逃がしは無い。部分集合が要るなら関数か別の型（`&[&str]` 等）で表す。

use crate::check::{Measured, SourceFile};

/// 判定行の tag。
const TAG: &str = "enum-slices";

/// 1 対（slice と enum）の材料。要素の解析は enum と確定した後（struct の slice を先に外す）。
struct Pair {
    /// slice の名前。
    slice: String,
    /// slice の型が名指す enum。
    enum_name: String,
    /// `= &[` と `]` の間の本文。
    body: String,
}

/// `crates/*/src` の全 `.rs` から `const NAME: &[Enum] = &[…]` を拾い、同じ file の enum と突き合わせる。
///
/// 母集団 0（対が 1 つも無い木）は `enum-slices=0` で通す（測る対象が無い＝測れなかった、ではない・
/// 擬似 workspace の歯がこの形）。
pub(crate) fn measure(files: &[SourceFile]) -> Measured {
    let mut violations = Vec::new();
    let mut pairs = 0_usize;
    for file in files {
        let shown = file.path.display();
        for found in slices_in(&file.text) {
            match found {
                Err(reason) => {
                    pairs = pairs.saturating_add(1);
                    violations.push(format!("{TAG}: {shown}: {reason}"));
                }
                Ok(pair) => match variants_of(&file.text, &pair.enum_name) {
                    Err(reason) => {
                        pairs = pairs.saturating_add(1);
                        violations.push(format!("{TAG}: {shown}: {reason}"));
                    }
                    // struct の slice は enum の形ではない＝対に数えない。
                    Ok(None) => {}
                    Ok(Some(variants)) => {
                        pairs = pairs.saturating_add(1);
                        let judged = match elements_of(&pair.body, &pair.enum_name) {
                            Err(reason) => vec![reason],
                            Ok(elements) => compare(&pair, &elements, &variants),
                        };
                        violations.extend(
                            judged
                                .into_iter()
                                .map(|reason| format!("{TAG}: {shown}: {reason}")),
                        );
                    }
                },
            }
        }
    }
    Measured {
        fact: format!("{TAG}={pairs}"),
        violations,
    }
}

/// file 内の `const NAME: &[Elem] = &[…]`（`&'static [Elem]` も同じ・字下げも可）のうち、
/// `Elem` が大文字で始まるもの。裸の識別子なら対、大文字で始まるのに裸でない形
/// （`&Enum` / `Box<Enum>` / `Enum<T>`）は読めない型として `Err`。小文字始まり・tuple・
/// `&str` は enum の slice ではないので拾わない。struct の slice は [`variants_of`] が
/// `struct` 宣言を見て対象外にする。
fn slices_in(text: &str) -> Vec<Result<Pair, String>> {
    let mut found = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        let Some(head) = const_head(line) else {
            continue;
        };
        let Some((name, after_name)) = head.split_once(':') else {
            continue;
        };
        let Some(rest) = ["&'static [", "&["]
            .iter()
            .find_map(|prefix| after_name.trim_start().strip_prefix(prefix))
        else {
            continue;
        };
        let Some((elem, after_type)) = rest.split_once(']') else {
            continue;
        };
        if !starts_upper(elem) {
            continue;
        }
        let line_no = index.saturating_add(1);
        if !is_type_ident(elem) {
            found.push(Err(format!(
                "{name}（{line_no} 行）の要素の型 `{elem}` を読めない（enum の slice は `&[Enum]` の形）"
            )));
            continue;
        }
        let tail = lines.get(index.saturating_add(1)..).unwrap_or_default();
        let body = match after_type.trim_start().strip_prefix("= &[") {
            Some(open) => slice_body(open, tail),
            None => Err(format!("{name}（{line_no} 行）の右辺が `= &[` で始まらない")),
        };
        found.push(body.map(|body| Pair {
            slice: name.to_owned(),
            enum_name: elem.to_owned(),
            body,
        }));
    }
    found
}

/// `pub const` / `pub(crate) const` / `const` の行（字下げ可）から `NAME: &[…] …` の部分を返す。
fn const_head(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    ["pub const ", "pub(crate) const ", "const "]
        .iter()
        .find_map(|prefix| trimmed.strip_prefix(prefix))
}

/// 要素の型が大文字で始まるか（`&Enum` / `&'static Enum` のように `&` とライフタイムを挟む形も
/// 含めて見る＝読めない型として違反にする側へ倒すため）。
fn starts_upper(elem: &str) -> bool {
    let mut rest = elem.trim_start();
    loop {
        if let Some(after) = rest.strip_prefix('&') {
            rest = after.trim_start();
        } else if let Some(after) = rest.strip_prefix('\'') {
            rest = after
                .trim_start_matches(|ch: char| ch.is_ascii_alphanumeric() || ch == '_')
                .trim_start();
        } else {
            break;
        }
    }
    rest.chars().next().is_some_and(|first| first.is_ascii_uppercase())
}

/// 大文字で始まり英数字と `_` だけの識別子か（enum / struct の名前の形）。
fn is_type_ident(elem: &str) -> bool {
    let mut chars = elem.chars();
    chars.next().is_some_and(|first| first.is_ascii_uppercase())
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

/// `= &[` の直後（`open`）から、続く行も含めて最初の `]` までの本文。閉じが無ければ `Err`。
fn slice_body(open: &str, tail: &[&str]) -> Result<String, String> {
    let mut from = open.to_owned();
    for line in tail {
        from.push('\n');
        from.push_str(line);
    }
    match from.split_once(']') {
        Some((body, _)) => Ok(body.to_owned()),
        None => Err("slice の閉じ `]` を見つけられない".to_owned()),
    }
}

/// slice 本文を `,` で割り、各要素が `Enum::Ident` ちょうどであることを求める（fail-closed）。
fn elements_of(body: &str, enum_name: &str) -> Result<Vec<String>, String> {
    let prefix = format!("{enum_name}::");
    let mut elements = Vec::new();
    for raw in body.split(',') {
        let item = raw.trim();
        if item.is_empty() {
            continue; // 末尾の `,` の後ろ。
        }
        if item.contains("//") {
            return Err(format!("slice の中にコメントが在る（読めない形）: `{item}`"));
        }
        match item.strip_prefix(&prefix) {
            Some(ident) if is_variant_ident(ident) => elements.push(ident.to_owned()),
            _ => {
                return Err(format!(
                    "slice の要素が `{enum_name}::<Variant>` の形でない（読めない形）: `{item}`"
                ))
            }
        }
    }
    Ok(elements)
}

/// 同じ file の `enum <name> {` から `}` までの variant の名前。読めない行が 1 つでも在れば `Err`。
/// `struct <name>` の slice（enum の形ではない）は `Ok(None)`。
///
/// 認める行は「4 空白 + 識別子 + `,`」と doc / 行コメントだけ。payload 付き（`Foo(u8),`）・
/// 判別子指定（`Foo = 3,`）・属性行（`#[…]`）・末尾 `,` 無し・1 行に複数、は読めない形として
/// 違反に倒す（黙って数えない）。
fn variants_of(text: &str, name: &str) -> Result<Option<Vec<String>>, String> {
    let heads = [
        format!("pub enum {name} {{"),
        format!("pub(crate) enum {name} {{"),
        format!("enum {name} {{"),
    ];
    let lines: Vec<&str> = text.lines().collect();
    let Some(start) = lines
        .iter()
        .position(|line| heads.iter().any(|head| *line == head))
    else {
        if lines.iter().any(|line| is_struct_head(line, name)) {
            return Ok(None);
        }
        return Err(format!("`enum {name}` の宣言を同じ file に見つけられない（読めない形）"));
    };
    let mut variants = Vec::new();
    for line in lines.iter().skip(start.saturating_add(1)) {
        if *line == "}" {
            return Ok(Some(variants));
        }
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with("///") || trimmed.starts_with("//") {
            continue; // 空行と doc / 行コメントは形の一部ではない。
        }
        let Some(item) = line.strip_prefix("    ") else {
            return Err(format!("enum {name} の中の行を読めない（4 空白の字下げでない）: `{line}`"));
        };
        match item.strip_suffix(',') {
            Some(ident) if is_variant_ident(ident) => variants.push(ident.to_owned()),
            _ => {
                return Err(format!(
                    "enum {name} の variant を読めない形（payload / 判別子 / 属性 / 末尾 `,` 無し）: `{item}`"
                ))
            }
        }
    }
    Err(format!("enum {name} の閉じ `}}` を見つけられない"))
}

/// `struct <name>` の宣言行か（名前の直後が識別子の続きでないこと）。
fn is_struct_head(line: &str, name: &str) -> bool {
    ["pub struct ", "pub(crate) struct ", "struct "]
        .iter()
        .any(|head| {
            line.strip_prefix(head)
                .and_then(|rest| rest.strip_prefix(name))
                .is_some_and(|after| {
                    !after
                        .chars()
                        .next()
                        .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_')
                })
        })
}

/// 識別子（英数字と `_`・先頭は大文字）。
fn is_variant_ident(ident: &str) -> bool {
    is_type_ident(ident)
}

/// variant の集合と slice の要素の集合を突き合わせる（欠け・余り・重複を全部出す）。
fn compare(pair: &Pair, elements: &[String], variants: &[String]) -> Vec<String> {
    let mut reasons = Vec::new();
    for variant in variants {
        let hits = elements.iter().filter(|e| *e == variant).count();
        if hits == 0 {
            reasons.push(format!(
                "{}::{variant} が {} に無い（末尾の入れ忘れ）",
                pair.enum_name, pair.slice
            ));
        } else if hits > 1 {
            reasons.push(format!("{}::{variant} が {} に {hits} 回在る", pair.enum_name, pair.slice));
        }
    }
    for element in elements {
        if !variants.iter().any(|v| v == element) {
            reasons.push(format!(
                "{} の {}::{element} は enum に無い variant である",
                pair.slice, pair.enum_name
            ));
        }
    }
    reasons
}
