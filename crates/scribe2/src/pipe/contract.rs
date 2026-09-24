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
/// 写しに残す＝run dir の写しだけで行の宣言が読める）。[`TARGETS`] は検出線の的（gate が [`targets_of`] で読む）。
/// [`GROWTH`] は file ごとの見込み行数（受付の上限の余地が読む・設計 contract-source.md §46）。
const OPTIONAL: &[&str] = &["classes", "opens", "touches", TARGETS, GROWTH];

/// 行の file ごとの見込み行数の key（`<path>:<行数>` の列・設計 contract-source.md §46・行 ax）。項目の形は受付が
/// write-set と突き合わせて読む（契約表の検査と同じ読み手の 1 本）。
pub const GROWTH: &str = "growth";

/// 契約が名指す生存行（変異の的）の key（設計 gate-cost.md §16・行 g）。値は `<file>:<行>:<変異の名>` の列で、
/// 形は [`target_unfit`] の 1 本が決める。[`Contract`] の field にはしない（読むのは gate の検出線だけ・[`targets_of`]）。
pub const TARGETS: &str = "targets";

/// 3 クラスの自己申告が取れる値（FR15）。受理集合は閉じた型 [`Class`] の名の列（宣言順・字面を 2 面に書かない）。
pub const CLASSES: &[&str] = &[Class::Delete.as_str(), Class::Publish.as_str(), Class::Consume.as_str()];

/// 3 クラス（消す / 出す / 使う・FR15・設計 contract-source.md §48 の 1・ADR-0061）。宣言順は delete / publish / consume。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// 消す（戻せない削除）。
    Delete,
    /// 出す（repo の外へ出す）。
    Publish,
    /// 使う（追加課金の外部資源）。
    Consume,
}

/// [`Class`] の全 variant（宣言順）。
pub const CLASS_ALL: &[Class] = &[Class::Delete, Class::Publish, Class::Consume];

impl Class {
    /// 名の字面（契約 file の `classes` と語列表の要素の先頭語）。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Delete => "delete",
            Self::Publish => "publish",
            Self::Consume => "consume",
        }
    }

    /// 名の字面から引く。未知なら `None`。
    pub fn parse(text: &str) -> Option<Self> {
        CLASS_ALL.iter().copied().find(|class| class.as_str() == text)
    }
}

/// クラスの語列表を持つ rules 行の id（値は要素「名 + 語列」の列・設計 contract-source.md §48 の 2）。
pub const CLASS_ROW: &str = "runner.class_commands";

/// 語列表の要素 1 つの読み（§48 の 1）。rules の読みと表の検査がこの 1 本（[`class_element`]）を呼ぶ（C2）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClassElement {
    /// 先頭語がクラスの名で、残りが 1 語以上の語列（語は空白 1 つで継ぐ）。
    Pair(Class, String),
    /// 先頭語がクラスの名でない（先頭語の字面・語を持たない要素は空）。
    Unknown(String),
    /// 名だけで語列が空。
    Bare(Class),
}

/// 語列表の要素 1 つを 3 形（名 + 語列・未知の名・名だけ）に分ける（pure）。
pub fn class_element(text: &str) -> ClassElement {
    let mut words = text.split_whitespace();
    let head = words.next().unwrap_or_default();
    let Some(class) = Class::parse(head) else {
        return ClassElement::Unknown(head.to_owned());
    };
    let sequence: Vec<&str> = words.collect();
    if sequence.is_empty() {
        return ClassElement::Bare(class);
    }
    ClassElement::Pair(class, sequence.join(" "))
}

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
    /// file ごとの見込み行数（`<path>:<行数>` の列・既定 空＝全 file が `size` の見込み・[`GROWTH`]）。生成の写しが行から運ぶ。
    pub growth: Vec<String>,
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
    let at = found.iter().find(|(seen, _, _)| seen == TARGETS).map_or(0, |(_, _, line)| *line);
    for target in list_of(found, TARGETS, 0, errors) {
        if let Some(reason) = target_unfit(&target) {
            errors.push(ContractError::new(at, format!("{TARGETS} の値 {target} が的の形でない: {reason}")));
        }
    }
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
        growth: list_of(found, GROWTH, 0, errors),
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

/// 的 1 本の字面が形に合わない理由（合えば `None`・設計 gate-cost.md §16）。形は `<file>:<行>:<変異の名>` で、
/// file は空白と `:` を持たない `.rs`・行は 1 以上の十進・名は空でない。行の後ろに `<桁>:` の桁を 1 つ挟んでよい
/// （cargo-mutants の一覧の行 `file:行:桁: 名` をそのまま貼れる・桁は照合に使わない）。表の検査（受付と CI）と
/// 契約 file の読みが同じこの 1 本を通る（C2）。
pub fn target_unfit(target: &str) -> Option<String> {
    if target.chars().any(char::is_control) {
        return Some("制御文字を含む".to_owned());
    }
    let Some((file, rest)) = target.split_once(':') else {
        return Some("<file>:<行>:<変異の名> の形でない（: が無い）".to_owned());
    };
    if file.is_empty() || file.chars().any(char::is_whitespace) || !file.ends_with(".rs") {
        return Some(format!("file {file:?} が空白を持たない .rs の path でない"));
    }
    let Some((line, name)) = rest.split_once(':') else {
        return Some("<file>:<行>:<変異の名> の形でない（行の後ろの : が無い）".to_owned());
    };
    let number = line.parse::<u64>().ok().filter(|n| *n >= 1);
    if !line.chars().all(|found| found.is_ascii_digit()) || number.is_none() {
        return Some(format!("行 {line:?} が 1 以上の十進でない"));
    }
    let name = match name.split_once(':') {
        Some((column, after)) if !column.is_empty() && column.chars().all(|found| found.is_ascii_digit()) => after,
        _ => name,
    };
    name.trim().is_empty().then(|| "変異の名が空".to_owned())
}

/// 契約 file の的の列（key が無ければ空・設計 gate-cost.md §16）。本文は [`Contract::parse`] と同じ読みを通す
/// （読めない file は理由を返す＝的を空に倒して従来の経路へ黙って戻さない・C10）。
pub fn targets_of(path: &Path) -> Result<Vec<String>, String> {
    let text = std::fs::read_to_string(path).map_err(|err| format!("{} を読めない: {err}", path.display()))?;
    Contract::parse(&text).map_err(|errors| {
        let lines: Vec<String> = errors.iter().map(ToString::to_string).collect();
        format!("{} を読めない: {}", path.display(), lines.join(" / "))
    })?;
    let mut errors = Vec::new();
    let (found, _) = scan(&text, &mut errors);
    Ok(list_of(&found, TARGETS, 0, &mut errors))
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
/// key の順は [`REQUIRED`] の宣言順 + 任意 key で、配列は 1 行に収める。導出物（`.toml`）の行が節の本文の逐語 goal を
/// 運ぶ周だけは `goal` がその逐語である（空の行は `title` のまま・設計 contract-source.md §47 の 7）。
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
    let goal = if row.goal.is_empty() { &row.title } else { &row.goal };
    out.push_str(&format!("goal = \"{goal}\"\n"));
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
    if !row.targets.is_empty() {
        out.push_str(&format!("{TARGETS} = {}\n", list(&row.targets)));
    }
    if !row.growth.is_empty() {
        out.push_str(&format!("{GROWTH} = {}\n", list(&row.growth)));
    }
    out
}

/// Promised の行（設計 contract-source.md §33 項 4）の契約 file の `verify`: 歯を置き場の（crate・scope）で束ね、束ごとに
/// nextest 行 1 本（filter は歯の**完全名**を空白で並べる・束の順は歯の初出の順＝`n` の順）。行の形は §28 の scope を
/// 置き場の path から読む [`crate::pipe::closure::nextest_line`] の 1 本。`teeth` は (完全名, 置き場の file)。
pub fn promised_verify(teeth: &[(String, String)]) -> Vec<String> {
    let mut bundles: Vec<(String, &str, Vec<&str>)> = Vec::new();
    for (name, file) in teeth {
        let head = crate::pipe::closure::nextest_line(file, &[]);
        match bundles.iter_mut().find(|(found, _, _)| *found == head) {
            Some((_, _, names)) if names.contains(&name.as_str()) => {}
            Some((_, _, names)) => names.push(name),
            None => bundles.push((head, file, vec![name])),
        }
    }
    bundles.into_iter().map(|(_, file, names)| crate::pipe::closure::nextest_line(file, &names)).collect()
}

/// Promised の行の契約 file の `done`: 約束の行を `n` の順に「(n) `expect`」で並べ、空白で繋いだ 1 文（§33 項 4・設計
/// doc には書き戻さない）。
pub fn promised_done(promises: &[&crate::pipe::table::PromiseRow]) -> String {
    let mut ordered = promises.to_vec();
    ordered.sort_by_key(|promise| promise.n);
    let parts: Vec<String> = ordered.iter().map(|promise| format!("({}) {}", promise.n, promise.expect)).collect();
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::{
        class_element, promised_done, promised_verify, render, target_unfit, Class, ClassElement, Contract, CLASSES,
        CLASS_ALL, GENERATED_DISPOSITION, GENERATED_OWNER,
    };
    use crate::order::is_declaration_order;
    use crate::pipe::table::{ContractRow, PromiseRow};

    /// §48 の 1: 3 クラスの閉じた型は宣言順 delete / publish / consume の 3 値で、契約 file の `classes` の受理集合がその
    /// 型の名の列と一致する（型の名に無い字面は `Contract::parse` が断り、名の 3 つは通る）。
    #[test]
    fn class_derive_accepted_set_is_the_closed_type_names() {
        assert!(is_declaration_order(CLASS_ALL, |class| class as usize), "CLASS_ALL は宣言順");
        let names: Vec<&str> = CLASS_ALL.iter().map(|class| class.as_str()).collect();
        assert_eq!(names, ["delete", "publish", "consume"], "3 値・宣言順");
        assert_eq!(CLASSES, names.as_slice(), "受理集合は型の名の列");
        for class in CLASS_ALL {
            assert_eq!(Class::parse(class.as_str()), Some(*class), "名から引ける: {}", class.as_str());
        }
        let base = render(&row(), "docs/design/toy.md#b", &row().write_set);
        let all = Contract::parse(&format!("{base}classes = [\"delete\", \"publish\", \"consume\"]\n"));
        assert_eq!(all.map(|found| found.classes.len()), Ok(3), "型の名の 3 つは通る");
        let errors = Contract::parse(&format!("{base}classes = [\"Publish\"]\n")).expect_err("型の名に無い字面は断る");
        assert!(errors.iter().any(|error| error.reason.contains("Publish")), "{errors:?}");
    }

    /// §48 の 1: 要素の読み手 1 本が 3 形を分ける（名 + 語列は語を空白 1 つで継ぐ・先頭語が名でない要素と語の無い要素は未知の名・
    /// 名だけの要素は語列が空）。
    #[test]
    fn class_derive_element_reader_splits_pair_unknown_and_bare() {
        let pair = |class: Class, sequence: &str| ClassElement::Pair(class, sequence.to_owned());
        assert_eq!(class_element("publish git push"), pair(Class::Publish, "git push"));
        assert_eq!(class_element("  delete   git  push -d "), pair(Class::Delete, "git push -d"), "語は空白 1 つで継ぐ");
        assert_eq!(class_element("git push"), ClassElement::Unknown("git".to_owned()), "先頭語が名でない");
        assert_eq!(class_element("Publish git push"), ClassElement::Unknown("Publish".to_owned()), "綴り違いは名でない");
        assert_eq!(class_element("   "), ClassElement::Unknown(String::new()), "語を持たない要素");
        assert_eq!(class_element("consume"), ClassElement::Bare(Class::Consume), "名だけ");
        assert_eq!(class_element(" delete  "), ClassElement::Bare(Class::Delete), "名だけ（空白つき）");
    }

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
            targets: Vec::new(),
            growth: Vec::new(),
            goal: String::new(),
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
        assert!(!body.contains("targets ="), "的の無い行は targets を書かない: {body}");
        assert!(!body.contains("growth =") && found.growth.is_empty(), "見込みの無い行は growth を書かない: {body}");
    }

    /// 的の形（設計 gate-cost.md §16）: `<file>:<行>:<変異の名>`（桁 1 つを挟んでよい）は通り、file・行・名のどれかが
    /// 形を外れた字面は理由を返す。
    #[test]
    fn contract_target_form_takes_file_line_name_and_names_the_broken_part() {
        for good in ["src/a.rs:3:replace f -> bool with true", "src/a.rs:3:9: replace f with ()"] {
            assert_eq!(target_unfit(good), None, "{good}");
        }
        for (bad, want) in [
            ("src/a.rs", ": が無い"),
            ("src/a.txt:3:x", ".rs"),
            ("src/a b.rs:3:x", ".rs"),
            ("src/a.rs:3", "行の後ろ"),
            ("src/a.rs:0:x", "1 以上"),
            ("src/a.rs:x:y", "1 以上"),
            ("src/a.rs:3:  ", "名が空"),
            ("src/a.rs:3:9:", "名が空"),
        ] {
            let reason = target_unfit(bad).unwrap_or_default();
            assert!(reason.contains(want), "{bad} は {want} を名乗る: {reason}");
        }
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

    /// §47 の 7: goal を持つ行（導出物の行）の写しは goal を運び（二重引用符と backtick を含む単一行が `Contract::parse` で
    /// 逐語に読み戻る）、goal の無い行は `title` のまま。写しの key 集合は同じ。
    #[test]
    fn contract_whole_goal_render_carries_the_goal_and_falls_back_to_the_title() {
        let mut goaled = row();
        goaled.goal = "節の \"本文\" と `crate::pipe::table::ContractRow` の逐語".to_owned();
        let body = render(&goaled, "docs/design/toy.toml#b", &goaled.write_set);
        let found = Contract::parse(&body).unwrap_or_else(|errors| panic!("生成した本文を読めない: {errors:?}\n{body}"));
        assert_eq!(found.goal, goaled.goal, "goal は行の goal の逐語");
        let keys = |text: &str| -> Vec<String> {
            text.lines().filter_map(|line| line.split_once(" = ")).map(|(key, _)| key.to_owned()).collect()
        };
        let plain = row();
        let titled = render(&plain, "docs/design/toy.toml#b", &plain.write_set);
        let back = Contract::parse(&titled).unwrap_or_else(|errors| panic!("生成した本文を読めない: {errors:?}\n{titled}"));
        assert_eq!(back.goal, plain.title, "goal の無い行は title のまま");
        assert_eq!(keys(&body), keys(&titled), "写しの key 集合は変わらない");
    }

    // flip-check: s2-07l.512

    /// 約束の行 1 つ（`n` と `expect` だけを振る・他の欄は固定）。
    fn promise(n: u64, expect: &str) -> PromiseRow {
        PromiseRow {
            line: n,
            of: "b".to_owned(),
            n,
            text: format!("約束 {n}"),
            files: vec!["docs/a.md".to_owned()],
            symbols: Vec::new(),
            teeth: vec![format!("t_{n}")],
            place: String::new(),
            fixture: "fixture".to_owned(),
            expect: expect.to_owned(),
        }
    }

    /// §33 (c): 歯は置き場の（crate・scope）ごとに 1 本の nextest 行に束ねられ（`src/` = `--lib`・`tests/<name>` =
    /// `--test <name>`・crate の他の file と crate の外は scope 旗なし）、完全名が全部・初出の順に載る（同じ名は 1 回）。
    /// 母集団 = 歯 6 本（重複 1）→ 行 4 本。
    #[test]
    fn contract_promise_render_bundles_teeth_by_crate_and_scope_with_full_names() {
        let teeth: Vec<(String, String)> = [
            ("a_one", "crates/c/src/x.rs"),
            ("pipe::intake::pipe_intake_promise_b", "crates/c/tests/e2e/pipe/intake.rs"),
            ("a_two", "crates/c/src/sub/y.rs"),
            ("c_three", "crates/d/tests/e2e.rs"),
            ("a_one", "crates/c/src/x.rs"),
            ("bench_four", "crates/c/benches/z.rs"),
        ]
        .iter()
        .map(|(name, file)| ((*name).to_owned(), (*file).to_owned()))
        .collect();
        assert_eq!(
            promised_verify(&teeth),
            [
                "cargo nextest run -p c --lib --no-tests=fail a_one a_two",
                "cargo nextest run -p c --test e2e --no-tests=fail pipe::intake::pipe_intake_promise_b",
                "cargo nextest run -p d --test e2e --no-tests=fail c_three",
                "cargo nextest run -p c --no-tests=fail bench_four",
            ],
            "（crate・scope）ごとに 1 行"
        );
        assert!(promised_verify(&[]).is_empty(), "歯 0 本は行 0 本");
    }

    /// §33 (c): `done` は約束の行を `n` の順に「(n) expect」で空白で繋いだ 1 文（書かれた順ではない）で、生成値を行の値の
    /// 代わりに `render` へ渡した本文は器自身が読め（`Contract::parse`）、`verify` / `done` が生成値のまま載る。
    #[test]
    fn contract_promise_render_orders_done_by_n_and_the_body_round_trips() {
        let (second, first) = (promise(2, "rc 1 で断る"), promise(1, "rc 0 で通る"));
        let done = promised_done(&[&second, &first]);
        assert_eq!(done, "(1) rc 0 で通る (2) rc 1 で断る", "n の順");
        let verify = promised_verify(&[("t_1".to_owned(), "crates/c/src/x.rs".to_owned())]);
        let mut row = row();
        row.done = done.clone();
        row.verify = verify.clone();
        let body = render(&row, "docs/design/toy.md#b", &["crates/c/src/x.rs".to_owned()]);
        let found = Contract::parse(&body).unwrap_or_else(|errors| panic!("生成した本文を読めない: {errors:?}\n{body}"));
        assert_eq!((found.done, found.verify), (done, verify), "生成値が契約 file に載る");
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
