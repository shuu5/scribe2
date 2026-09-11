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
//! Rust の parser は足さない（字面で数える・憲法 C13）。

use crate::check::{Measured, SourceFile};

/// 判定行の tag。
const TAG: &str = "enum-slices";

/// 1 対（slice と enum）の測定。
struct Pair {
    /// slice の名前。
    slice: String,
    /// slice の型が名指す enum。
    enum_name: String,
    /// slice の要素（`Enum::` を剥がした名前）。
    elements: Vec<String>,
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
            pairs = pairs.saturating_add(1);
            match found {
                Err(reason) => violations.push(format!("{TAG}: {shown}: {reason}")),
                Ok(pair) => match variants_of(&file.text, &pair.enum_name) {
                    Err(reason) => violations.push(format!("{TAG}: {shown}: {reason}")),
                    // struct の slice は enum の形ではない＝対に数えない。
                    Ok(None) => pairs = pairs.saturating_sub(1),
                    Ok(Some(variants)) => violations.extend(
                        compare(&pair, &variants)
                            .into_iter()
                            .map(|reason| format!("{TAG}: {shown}: {reason}")),
                    ),
                },
            }
        }
    }
    Measured {
        fact: format!("{TAG}={pairs}"),
        violations,
    }
}

/// file 内の `const NAME: &[Elem] = &[…]` のうち、`Elem` が大文字で始まる裸の識別子のもの。
///
/// `&[&str]` / `&[u8]` / `&[(…)]` / `&[Box<…>]` は enum の slice の形ではないので対象外
/// （`&` や `(` や小文字始まり・`<` を含む型は拾わない）。struct の slice は [`variants_of`] が
/// `struct` 宣言を見て対象外にする。
fn slices_in(text: &str) -> Vec<Result<Pair, String>> {
    let mut found = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let Some(head) = const_head(line) else {
            continue;
        };
        let Some((name, rest)) = head.split_once(": &[") else {
            continue;
        };
        let Some((elem, after_type)) = rest.split_once(']') else {
            continue;
        };
        if !is_type_ident(elem) {
            continue;
        }
        let line_no = index.saturating_add(1);
        let body = match after_type.trim_start().strip_prefix("= &[") {
            Some(_) => slice_body(text, index),
            None => Err(format!("{name}（{line_no} 行）の右辺が `= &[` で始まらない")),
        };
        found.push(body.and_then(|body| {
            elements_of(&body, elem).map(|elements| Pair {
                slice: name.to_owned(),
                enum_name: elem.to_owned(),
                elements,
            })
        }));
    }
    found
}

/// `pub const` / `pub(crate) const` / `const` の行から `NAME: &[…] …` の部分を返す。
fn const_head(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if trimmed.len() != line.len() {
        return None; // 字下げされた const（fn / mod / test の中）は対象外。
    }
    ["pub const ", "pub(crate) const ", "const "]
        .iter()
        .find_map(|prefix| trimmed.strip_prefix(prefix))
}

/// 大文字で始まり英数字と `_` だけの識別子か（enum / struct の名前の形）。
fn is_type_ident(elem: &str) -> bool {
    let mut chars = elem.chars();
    chars.next().is_some_and(|first| first.is_ascii_uppercase())
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

/// `= &[` から対応する `];` までの本文（`index` 行から下を読む）。閉じが無ければ `Err`。
fn slice_body(text: &str, index: usize) -> Result<String, String> {
    let from = text
        .lines()
        .skip(index)
        .collect::<Vec<&str>>()
        .join("\n");
    let Some((_, after_open)) = from.split_once("= &[") else {
        return Err("右辺の `= &[` を見つけられない".to_owned());
    };
    match after_open.split_once(']') {
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
        if trimmed.starts_with("///") || trimmed.starts_with("//") {
            continue;
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
fn compare(pair: &Pair, variants: &[String]) -> Vec<String> {
    let mut reasons = Vec::new();
    for variant in variants {
        let hits = pair.elements.iter().filter(|e| *e == variant).count();
        if hits == 0 {
            reasons.push(format!(
                "{}::{variant} が {} に無い（末尾の入れ忘れ）",
                pair.enum_name, pair.slice
            ));
        } else if hits > 1 {
            reasons.push(format!("{}::{variant} が {} に {hits} 回在る", pair.enum_name, pair.slice));
        }
    }
    for element in &pair.elements {
        if !variants.iter().any(|v| v == element) {
            reasons.push(format!(
                "{} の {}::{element} は enum に無い variant である",
                pair.slice, pair.enum_name
            ));
        }
    }
    reasons
}
