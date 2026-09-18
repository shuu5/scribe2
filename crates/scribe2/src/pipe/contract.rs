//! 契約 file の読み取り（設計 §3・FR1 / FR2・ADR-0004 §2.3）。
//!
//! section を持たない flat な TOML subset である。値は文字列と**文字列の配列**だけで、
//! 値そのものの受理集合は [`rules::manifest::scalar`] と共有する（第 2 の値 parser を
//! 作らない・憲法 C6）。配列は `key = ["a", "b"]` の形で **1 行に収まる**こと。
//!
//! **検査は 1 か所に閉じ、見つけた不備は全件集めて返す**（C2 / FR1）。1 件目で止めると
//! 直すたびに次の 1 件が出る形になり、契約を書き切れない。

use crate::hook::role_guard::{PathKind, PATH_KINDS};
use crate::rules::manifest::{elements, quoted_once, scalar, Scalar};
use std::path::Path;

/// 必ず在る key（この順で報告する）。
const REQUIRED: &[&str] = &[
    "goal",
    "done",
    "size",
    "owner",
    "disposition",
    "write-set",
    "verify",
    "req",
    "design",
];

/// 任意の key。`touches` は契約 (b) の生成物が行から写す欄（型の閉包の宣言・受付は読まないが
/// 写しに残す＝run dir の写しだけで行の宣言が読める）。
const OPTIONAL: &[&str] = &["classes", "opens", "touches"];

/// 3 クラスの自己申告が取れる値（FR15）。
pub const CLASSES: &[&str] = &["delete", "publish", "consume"];

/// 契約の印 `opens` の key（設計 seat-roles.md §3「契約が開く例外」・AC16）。値は path 種別
/// （[`PathKind`]）の名の列で、席が自分の手で編集してよい種別を便ごとに開く。印の無い便は従来どおり。
pub const OPENS: &str = "opens";

/// 契約 file が読めない理由。**行番号を必ず持つ**（0 は file 全体を指す）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractError {
    /// 何行目か（1 始まり・0 は file 全体）。
    pub line: u64,
    /// なぜ読めないか。
    pub reason: String,
}

impl ContractError {
    /// 1 件を組む。
    fn new(line: u64, reason: String) -> Self {
        Self { line, reason }
    }
}

impl std::fmt::Display for ContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "contract: {} line={}", self.reason, self.line)
    }
}

/// 読み込み済みの契約。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contract {
    /// 1 行の目的。
    pub goal: String,
    /// 何ができたら終わりか。
    pub done: String,
    /// 見積の目安。
    pub size: String,
    /// bead id（文字列として持つだけ・台帳は読まない）。
    pub owner: String,
    /// 処分。
    pub disposition: String,
    /// 触ってよい file / dir。
    pub write_set: Vec<String>,
    /// 検証コマンド（各要素は 1 行で完結する）。
    pub verify: Vec<String>,
    /// SRS の要件 id。
    pub req: Vec<String>,
    /// 設計 doc の repo 相対 path。
    pub design: String,
    /// 3 クラスの自己申告（既定 空）。
    pub classes: Vec<String>,
    /// 契約の印（席の編集を開く path 種別の名・既定 空＝印なし・[`OPENS`]）。
    pub opens: Vec<String>,
    /// 行が宣言した型の閉包の種（`crate::…` の path の列・既定 空）。生成の写しが行から運ぶ。
    pub touches: Vec<String>,
}

/// 走査中の 1 key の値。
enum Raw {
    /// 文字列。
    Text(String),
    /// 文字列の配列。
    List(Vec<String>),
}

impl Contract {
    /// file から読む。読めない file 自体も 1 件の error にする。
    pub fn load(path: &Path) -> Result<Self, Vec<ContractError>> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(err) => Err(vec![ContractError::new(
                0,
                format!("{} を読めない: {err}", path.display()),
            )]),
        }
    }

    /// 本文から読む。**不備は全件集めて返す**。
    pub fn parse(text: &str) -> Result<Self, Vec<ContractError>> {
        let mut errors = Vec::new();
        let (found, seen) = scan(text, &mut errors);
        check_required(&seen, &mut errors);
        let built = build(&found, &mut errors);
        match built {
            Some(contract) if errors.is_empty() => Ok(contract),
            _ => Err(errors),
        }
    }
}

/// 1 行ずつ読み、key → 値 を集める。行の形の不備はここで全件積む。
fn scan(text: &str, errors: &mut Vec<ContractError>) -> (Vec<(String, Raw, u64)>, Vec<String>) {
    let mut found: Vec<(String, Raw, u64)> = Vec::new();
    // 値が壊れていても「その key は書かれていた」ことは覚える。忘れると
    // 「値が読めない」と「key が無い」を同じ key について二重に報告してしまう。
    let mut seen: Vec<String> = Vec::new();
    for (index, raw_line) in text.lines().enumerate() {
        let line = index as u64 + 1;
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((key, raw_value)) = trimmed.split_once('=') else {
            errors.push(ContractError::new(
                line,
                format!("key = value の形でない: {trimmed}"),
            ));
            continue;
        };
        let key = key.trim().to_owned();
        if !REQUIRED.contains(&key.as_str()) && !OPTIONAL.contains(&key.as_str()) {
            errors.push(ContractError::new(line, format!("未知の key {key}")));
            continue;
        }
        if seen.contains(&key) {
            errors.push(ContractError::new(line, format!("key {key} が重複する")));
            continue;
        }
        seen.push(key.clone());
        if let Some(value) = value_of(&key, raw_value.trim(), line, errors) {
            found.push((key, value, line));
        }
    }
    (found, seen)
}

/// 1 つの値を読む。配列は 1 行に収まっていること。
fn value_of(key: &str, raw: &str, line: u64, errors: &mut Vec<ContractError>) -> Option<Raw> {
    if !raw.starts_with('[') {
        return match scalar(raw) {
            Some(Scalar::Str(text)) => Some(Raw::Text(text)),
            _ => {
                errors.push(ContractError::new(
                    line,
                    format!("{key} の value が文字列でない"),
                ));
                None
            }
        };
    }
    if !raw.ends_with(']') {
        errors.push(ContractError::new(
            line,
            format!("{key} の配列が同じ行で閉じていない（要素に改行は置けない・1 行で完結すること）"),
        ));
        return None;
    }
    let Some(parts) = elements(raw) else {
        errors.push(ContractError::new(line, format!("{key} の配列の形でない")));
        return None;
    };
    let mut list = Vec::new();
    for part in parts {
        // 引用符 1 組ちょうどでなければ受けない。`["a" "b"]` の区切り忘れを 1 本の
        // 壊れた文字列として黙って通すと、**書いた本数と通る本数が食い違う**（NFR4）。
        match scalar(part.trim()).filter(|_| quoted_once(&part)) {
            Some(Scalar::Str(text)) => list.push(text),
            _ => errors.push(ContractError::new(
                line,
                format!("{key} の要素が引用符 1 組の文字列でない: {}", part.trim()),
            )),
        }
    }
    Some(Raw::List(list))
}

/// 必須 key の欠落を全件積む。**書かれていれば値が壊れていても欠落とは言わない**。
fn check_required(seen: &[String], errors: &mut Vec<ContractError>) {
    for key in REQUIRED {
        if !seen.iter().any(|found| found == key) {
            errors.push(ContractError::new(0, format!("必須の key {key} が無い")));
        }
    }
}

/// 文字列 key を取り出す。型違いはここで積む。
fn text_of(found: &[(String, Raw, u64)], key: &str, errors: &mut Vec<ContractError>) -> String {
    match found.iter().find(|(seen, _, _)| seen == key) {
        None => String::new(),
        Some((_, Raw::Text(text), _)) => text.clone(),
        Some((_, Raw::List(_), line)) => {
            errors.push(ContractError::new(
                *line,
                format!("{key} は文字列である（配列でない）"),
            ));
            String::new()
        }
    }
}

/// 配列 key を取り出す。`least` 本に満たなければ積む。
fn list_of(
    found: &[(String, Raw, u64)],
    key: &str,
    least: usize,
    errors: &mut Vec<ContractError>,
) -> Vec<String> {
    match found.iter().find(|(seen, _, _)| seen == key) {
        None => Vec::new(),
        Some((_, Raw::Text(_), line)) => {
            errors.push(ContractError::new(
                *line,
                format!("{key} は配列である（文字列でない）"),
            ));
            Vec::new()
        }
        Some((_, Raw::List(list), line)) => {
            if list.len() < least {
                errors.push(ContractError::new(
                    *line,
                    format!("{key} は 1 本以上要る（実 {} 本）", list.len()),
                ));
            }
            list.clone()
        }
    }
}

/// 集めた key から契約を組み、値の不備を全件積む。
fn build(found: &[(String, Raw, u64)], errors: &mut Vec<ContractError>) -> Option<Contract> {
    // 「verify の要素に改行なし」は **1 行走査**で構造的に守られる: 値は 1 行から取るので
    // 改行を含む要素は作れず、行を跨いだ配列は [`value_of`] が名指して断る。ここに
    // `contains('\n')` を置いても到達しないので、届かない検査は持たない。
    let verify = list_of(found, "verify", 1, errors);
    let classes = names_of(found, "classes", CLASSES, errors);
    let kinds: Vec<&str> = PATH_KINDS.iter().map(|kind| kind.as_str()).collect();
    let opens = names_of(found, OPENS, &kinds, errors);
    Some(Contract {
        goal: text_of(found, "goal", errors),
        done: text_of(found, "done", errors),
        size: text_of(found, "size", errors),
        owner: text_of(found, "owner", errors),
        disposition: text_of(found, "disposition", errors),
        write_set: list_of(found, "write-set", 1, errors),
        verify,
        req: list_of(found, "req", 1, errors),
        design: text_of(found, "design", errors),
        classes,
        opens,
        touches: list_of(found, "touches", 0, errors),
    })
}

/// **閉じた名の列**を取る任意 key（`classes` / `opens`）。列に無い名は行番号つきで全件積む。
fn names_of(
    found: &[(String, Raw, u64)],
    key: &str,
    taken: &[&str],
    errors: &mut Vec<ContractError>,
) -> Vec<String> {
    let names = list_of(found, key, 0, errors);
    let at = found
        .iter()
        .find(|(seen, _, _)| seen == key)
        .map_or(0, |(_, _, found_at)| *found_at);
    for item in &names {
        if !taken.contains(&item.as_str()) {
            errors.push(ContractError::new(
                at,
                format!("未知の {key} 値 {item}（取るのは {}）", taken.join(" / ")),
            ));
        }
    }
    names
}

impl Contract {
    /// 印で開いた path 種別（`opens` の名を [`PathKind`] へ引いた列・未知の名は [`Contract::parse`] が拒む）。
    pub fn opened_kinds(&self) -> Vec<PathKind> {
        self.opens.iter().filter_map(|name| PathKind::parse(name)).collect()
    }
}

/// 生成の写しの `owner`（**導出値**・宣言値ではない・C10）。契約の正本は設計 doc の行で、行は owner を持たない
/// ＝器が固定の 1 語を書く（値の形は [`REQUIRED`] の text のまま）。
pub const GENERATED_OWNER: &str = "generated";

/// 生成の写しの `disposition`（同じく導出値・受付の判定は `classes` が持ち、この語は判定に使われない）。
pub const GENERATED_DISPOSITION: &str = "A-now";

/// 契約表の行 1 つから契約 file の本文を組む（契約 (b)・設計 §2「生成」）。
///
/// **手で書く口は無い**（§7: 写しは生成物）。`owner` / `disposition` は固定の導出値（C10）で、残りは行の欄と
/// pointer の逐語をそのまま写す。`goal` は**行の `title`**である（planner 裁定 2026-09-19）: 節の本文は
/// `design` pointer が指しており、写しに複製しない——契約 file の値は 1 行 1 key の TOML subset で `"` を
/// 表せず（[`quoted_once`]）、実測で設計 doc の 276 節のうち 41 節が `"` を含み最大の節は 27 KB である。
/// key の順は [`REQUIRED`] の宣言順 + 任意 key で、配列は 1 行に収める。
///
/// **値は逃がさない**（escape の仕組みが TOML subset に無い）。行の値と契約 file の値は**同じ 1 つの
/// scalar の読み**（[`scalar`]）を通るので、行が持てた字面は写しでも同じ字面として読み戻る。
/// 読み戻せない本文を書いた周は [`Contract::parse`] が Err にし、受付は rc 2 で断る（fail-closed）。
pub fn render(row: &crate::pipe::table::ContractRow, design: &str, write_set: &[String]) -> String {
    let list = |items: &[String]| {
        let quoted: Vec<String> = items.iter().map(|item| format!("\"{}\"", item)).collect();
        format!("[{}]", quoted.join(", "))
    };
    // 契約 file は `schema` の key を持たない（[`REQUIRED`] / [`OPTIONAL`] の外＝書くと自分の parser が断る）。
    let mut out = String::new();
    out.push_str(&format!("goal = \"{}\"\n", row.title));
    out.push_str(&format!("done = \"{}\"\n", row.done));
    out.push_str(&format!("size = \"{}\"\n", row.size));
    out.push_str(&format!("owner = \"{GENERATED_OWNER}\"\n"));
    out.push_str(&format!("disposition = \"{GENERATED_DISPOSITION}\"\n"));
    out.push_str(&format!("write-set = {}\n", list(write_set)));
    out.push_str(&format!("verify = {}\n", list(&row.verify)));
    out.push_str(&format!("req = {}\n", list(&row.req)));
    out.push_str(&format!("design = \"{}\"\n", design));
    if !row.classes.is_empty() {
        out.push_str(&format!("classes = {}\n", list(&row.classes)));
    }
    if !row.touches.is_empty() {
        out.push_str(&format!("touches = {}\n", list(&row.touches)));
    }
    if !row.opens.is_empty() {
        out.push_str(&format!("{OPENS} = {}\n", list(&row.opens)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{render, Contract, GENERATED_DISPOSITION, GENERATED_OWNER};
    use crate::pipe::table::ContractRow;

    /// 生成の材料になる行（欄は最小・値は行の parser が通す形）。
    fn row() -> ContractRow {
        ContractRow {
            line: 7,
            id: "b".to_owned(),
            title: "縦 1 本を通す".to_owned(),
            req: vec!["FR4".to_owned()],
            section: "2".to_owned(),
            touches: vec!["crate::pipe::refuse::Refuse".to_owned()],
            surfaces: Vec::new(),
            write_set: vec!["src/lib.rs".to_owned()],
            creates: Vec::new(),
            tests: Vec::new(),
            also: Vec::new(),
            verify: vec!["sh verify-ok.sh".to_owned()],
            size: "M".to_owned(),
            done: "run が Implemented になる".to_owned(),
            depends: Vec::new(),
            classes: Vec::new(),
            opens: Vec::new(),
        }
    }

    /// 生成した本文は**器自身が読める**（`Contract::parse` を通る）。`goal` は行の `title`・`owner` と
    /// `disposition` は導出値・`touches` は行から運ぶ（設計 contract-source.md §2「行の field」）。
    #[test]
    fn contract_render_round_trips_through_parse() {
        let row = row();
        let body = render(&row, "docs/design/contract-source.md#b", &row.write_set);
        let found = match Contract::parse(&body) {
            Ok(found) => found,
            Err(errors) => panic!("生成した本文を読めない: {errors:?}\n{body}"),
        };
        assert_eq!(found.goal, row.title, "goal は行の title（節の本文は pointer が指す）");
        assert_eq!(found.done, row.done);
        assert_eq!(found.size, row.size);
        assert_eq!(found.owner, GENERATED_OWNER, "owner は導出値");
        assert_eq!(found.disposition, GENERATED_DISPOSITION, "disposition は導出値");
        assert_eq!(found.write_set, row.write_set);
        assert_eq!(found.verify, row.verify);
        assert_eq!(found.req, row.req);
        assert_eq!(found.design, "docs/design/contract-source.md#b", "pointer の逐語");
        assert_eq!(found.touches, row.touches, "行の touches を写す");
        assert!(found.classes.is_empty() && found.opens.is_empty(), "空の任意 key は書かない");
    }

    /// 行の値が `"` を含んでも**同じ字面で読み戻る**（行と写しは同じ scalar の読みを通る＝escape を持たない
    /// 形が両側で揃っている）。逃がす実装を足すと、逃がした字面が写しに残って行と食い違う。
    #[test]
    fn contract_render_keeps_a_row_value_that_carries_a_quote() {
        let mut row = row();
        row.title = "\"引用\" を持つ題".to_owned();
        let body = render(&row, "docs/design/toy.md#b", &row.write_set);
        let found = match Contract::parse(&body) {
            Ok(found) => found,
            Err(errors) => panic!("生成した本文を読めない: {errors:?}\n{body}"),
        };
        assert_eq!(found.goal, row.title, "字面は行のまま");
    }

    /// 必須 key の欠落は**全件**返す（1 件目で止めない）。生成の不備がここで 1 回で見える。
    #[test]
    fn contract_parse_reports_every_missing_required_key() {
        let errors = Contract::parse("goal = \"g\"\n").expect_err("欠落は Err");
        let joined: Vec<String> = errors.iter().map(ToString::to_string).collect();
        let text = joined.join("\n");
        for key in ["done", "size", "owner", "disposition", "write-set", "verify", "req", "design"] {
            assert!(text.contains(key), "{key} の欠落を名乗る: {text}");
        }
    }

    /// 値が壊れているだけの key を「無い」とは言わない（同じ key について二重に報告しない）。
    #[test]
    fn contract_parse_names_a_broken_value_without_claiming_absence() {
        let errors = Contract::parse("goal = 1\n").expect_err("壊れた値は Err");
        let text: Vec<String> = errors.iter().map(ToString::to_string).collect();
        let joined = text.join("\n");
        assert!(joined.contains("goal の value が文字列でない"), "値の不備を言う: {joined}");
        assert!(!joined.contains("必須の key goal が無い"), "書かれている key を「無い」とは言わない: {joined}");
    }
}
