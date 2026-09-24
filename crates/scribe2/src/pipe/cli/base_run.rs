//! 名乗った消費側の契約の nextest の検証行を、受付で base の木でも撃つ（設計 pipeline.md §56・契約表の行 ay・ADR-0059）。
//!
//! 受付（[`super::intake`]）と preflight（[`super::preflight`]）は、入口の lock の前に 1 周に 1 回だけ宣言を凍結し
//! （[`Early`]）、宣言が `entrance-flip = "detect"` か `"deny"` を名乗る周だけ [`run`] を撃つ。撃つのは契約の検証行のうち
//! 既存の nextest の行の読み手（[`closure::teeth_words`] が通る 1 本）が読み、1 語 `--no-tests=fail` を持つ行だけで、
//! 撃つ木は HEAD の sha を置き場の直下に detach した一時の worktree（撃ち終えたら登録ごと畳む）。行ごとに gate と同じ
//! 遮断器（[`health::pass`]）・撃つ口（[`gate::run_line_captured`]）・箱（host の memory の予備）を通し、rc を 4 値
//! （[`Value`]）へ写す。
//!
//! 読み切れる行（[`readable`]）で、members の逐語の crate の dir に `--test` の target が無いと決まる行だけは撃たずに
//! 「不在」とする（[`absent`]）。この事前判定が守るのは「base で緑の行を不在にしない」ことだけで、読めない形は全部撃つ側。
//! 分け方と写しは純関数（[`plan_of`] / [`settle`] / [`value_of`]）で、断り（`Refuse`）を組むのは受付の側である。

use super::super::closure;
use super::super::confine;
use super::super::declaration::{Effective, EntranceFlip};
use super::super::gate::{self, Limits, UNADMITTED_JOBS};
use super::super::health;
use super::super::{git_ok, head_of};
use super::intake::Denial;
use crate::rules::manifest::Manifest;
use std::iter::Peekable;
use std::path::Path;
use std::str::Chars;

/// 通った受付が run dir に書く記録 file の名（sha と行ごとの 4 値・rc・秒・§56 形 5）。
pub(super) const RECORD_FILE: &str = "base-run.txt";

/// 一時の木の名の頭（置き場の直下＝入口の lock file と同じ階層・`pipe/` の外）。
const TREE_HEAD: &str = "base-run-";

/// 撃つ行が持つ 1 語（無い nextest の行は撃たずに測れない）。
const NO_TESTS: &str = "--no-tests=fail";

/// 読み切れる行の package の旗。
const PACKAGE_FLAG: &str = "-p";

/// 読み切れる行の target の旗。
const TEST_FLAG: &str = "--test";

/// libtest の引数の始まり（後ろは読み切れる行の条件に数えない）。
const LIBTEST: &str = "--";

/// 読み切れる行が英数字の他に持てる字（半角 space を含む）。
const READABLE_MARKS: &str = "_-.:=/ ";

/// members の項を glob と読む字（glob の項は展開しない＝その項の crate は撃つ側）。
const GLOB_MARKS: &[char] = &['*', '?', '[', ']', '{', '}', '!'];

/// manifest の file 名。
const CARGO_TOML: &str = "Cargo.toml";

/// 撃った行の scope の unit 名に載る段の名。
const STAGE: &str = "base-run";

/// 撃たなかった値の字面（記録の rc / 秒）。
const NONE: &str = "-";

/// 撃った 1 行の 4 値（§56 形 4）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Value {
    /// rc 0（base の木でも緑＝TDD の RED を名乗れない）。
    GreenOnBase,
    /// rc 4（歯が base に無い）か、撃たずに target の不在が決まった行。
    Absent,
    /// rc 100（歯が base で落ちた）。
    Failed,
    /// 他の rc・signal・起動できない・箱の中の死・遮断器が閉じた・撃たなかった行。
    Unmeasurable,
}

impl Value {
    /// 記録と断りの理由に書く語。
    fn as_str(self) -> &'static str {
        match self {
            Self::GreenOnBase => "green-on-base",
            Self::Absent => "absent",
            Self::Failed => "failed",
            Self::Unmeasurable => "unmeasurable",
        }
    }
}

/// rc と箱の中の死を 4 値へ写す（oom_kill が 1 以上の行は rc に依らず測れない）。
fn value_of(rc: i32, oom_kill: Option<u64>) -> Value {
    if oom_kill.is_some_and(|count| count >= 1) {
        return Value::Unmeasurable;
    }
    match rc {
        0 => Value::GreenOnBase,
        4 => Value::Absent,
        100 => Value::Failed,
        _ => Value::Unmeasurable,
    }
}

/// 撃つ前の 1 行の仕分け。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Plan {
    /// nextest の行でないか `--no-tests=fail` を持たない（撃たずに測れない）。
    Unfired,
    /// 読み切れる行で target の不在が決まった（撃たずに不在）。
    Absent,
    /// 撃つ。
    Fire,
}

/// 1 行を仕分ける（`tree` は sha の木）。nextest の行かは既存の読み手 1 本（[`closure::teeth_words`]）で決める。
fn plan_of(line: &str, tree: &Path) -> Plan {
    let nextest = !closure::teeth_words(&[line.to_owned()]).is_empty();
    if !nextest || !line.split_whitespace().any(|word| word == NO_TESTS) {
        return Plan::Unfired;
    }
    match readable(line) {
        Some((package, test)) if absent(tree, package, test) => Plan::Absent,
        _ => Plan::Fire,
    }
}

/// 読み切れる行の `-p` と `--test` の値（読み切れない行は `None`）: 全体が ASCII の英数字と [`READABLE_MARKS`] だけ・
/// `-p` と `--test` が半角 space で区切った旗と値で丁度 1 つずつ・値は `-` で始まらない・他の旗は `--no-tests=fail` と
/// `--` の後ろの libtest の引数だけ。
fn readable(line: &str) -> Option<(&str, &str)> {
    if !line.chars().all(|found| found.is_ascii_alphanumeric() || READABLE_MARKS.contains(found)) {
        return None;
    }
    let mut words = line.split(' ').filter(|word| !word.is_empty());
    let (mut packages, mut tests) = (Vec::new(), Vec::new());
    while let Some(word) = words.next() {
        match word {
            LIBTEST => break,
            PACKAGE_FLAG => packages.push(words.next().filter(|value| !value.starts_with('-'))?),
            TEST_FLAG => tests.push(words.next().filter(|value| !value.starts_with('-'))?),
            NO_TESTS => {}
            flag if flag.starts_with('-') => return None,
            _ => {}
        }
    }
    match (packages.as_slice(), tests.as_slice()) {
        ([package], [test]) => Some((package, test)),
        _ => None,
    }
}

/// `package` の `test` の target が sha の木に無いと決まるか（決まらない周は偽＝撃つ側）。根の Cargo.toml が `[workspace]`
/// を持ち `[package]` を持たず、glob でない members の項の dir の `[package]` の name が逐語で一致する crate が丁度 1 つで、
/// その dir に名の `.rs`・名の dir の `main.rs`・manifest の test の表の name のどれも無い周だけ真。
fn absent(tree: &Path, package: &str, test: &str) -> bool {
    let Some(root) = read_toml(&tree.join(CARGO_TOML)) else {
        return false;
    };
    if !root.workspace || root.package_table {
        return false;
    }
    let mut named = Vec::new();
    for member in root.members.iter().filter(|member| !member.contains(GLOB_MARKS)) {
        if member.starts_with('/') || member.split('/').any(|part| part == "..") {
            return false;
        }
        let dir = tree.join(member);
        let Some(found) = read_toml(&dir.join(CARGO_TOML)) else {
            return false;
        };
        if found.package.as_deref() == Some(package) {
            named.push((dir, found));
        }
    }
    let [(dir, found)] = named.as_slice() else {
        return false;
    };
    let tests = dir.join("tests");
    let present = tests.join(format!("{test}.rs")).is_file()
        || tests.join(test).join("main.rs").is_file()
        || found.tests.iter().any(|name| name == test);
    !present
}

/// 撃った 1 行の結果。
struct Row {
    /// 契約の検証行の字面。
    line: String,
    /// 4 値。
    value: Value,
    /// rc（撃たなかった行は `None`）。
    rc: Option<i32>,
    /// 壁時計の秒（撃たなかった行は `None`）。
    secs: Option<u64>,
}

/// 1 周の実走（測った sha と契約の検証行ごとの結果・検証行の順）。
pub(in crate::pipe) struct BaseRun {
    /// 撃った木の sha（受付が材料の読みの前に読んだ HEAD）。
    sha: String,
    /// 行ごとの結果。
    rows: Vec<Row>,
}

impl BaseRun {
    /// 全行が測れない周（木を用意できない・置き場が無い・実走の後の HEAD が違う）。
    fn unmeasured(sha: &str, lines: &[String]) -> Self {
        let rows = lines.iter().map(|line| Row { line: line.clone(), value: Value::Unmeasurable, rc: None, secs: None });
        Self { sha: sha.to_owned(), rows: rows.collect() }
    }

    /// `value` の行の本数。
    fn count(&self, value: Value) -> usize {
        self.rows.iter().filter(|row| row.value == value).count()
    }

    /// 受付の 1 行と preflight の行の欄（`entrance=green-on-base:<本数>/<行数>`・測れない行があれば ` unmeasurable:<本数>`）。
    pub(super) fn fact(&self) -> String {
        let mut fact = format!("entrance=green-on-base:{}/{}", self.count(Value::GreenOnBase), self.rows.len());
        let unmeasurable = self.count(Value::Unmeasurable);
        if unmeasurable > 0 {
            fact.push_str(&format!(" unmeasurable:{unmeasurable}"));
        }
        fact
    }

    /// base で緑か測れない行の本数（deny の断りの本数）。
    pub(super) fn not_red(&self) -> u64 {
        let count = self.count(Value::GreenOnBase).saturating_add(self.count(Value::Unmeasurable));
        u64::try_from(count).unwrap_or(u64::MAX)
    }

    /// 行ごとの 4 値の語（検証行の順）。
    pub(super) fn values(&self) -> Vec<String> {
        self.rows.iter().map(|row| row.value.as_str().to_owned()).collect()
    }

    /// 記録 file の本文（1 行目 `sha=`・以後 1 行 1 検証行）。
    fn render(&self) -> String {
        let dash = |found: Option<String>| found.unwrap_or_else(|| NONE.to_owned());
        let mut text = format!("sha={}\n", self.sha);
        for (n, row) in self.rows.iter().enumerate() {
            text.push_str(&format!(
                "line={} value={} rc={} secs={} cmd={}\n",
                n.saturating_add(1),
                row.value.as_str(),
                dash(row.rc.map(|rc| rc.to_string())),
                dash(row.secs.map(|secs| secs.to_string())),
                row.line
            ));
        }
        text
    }

    /// 記録 file を run dir へ書く。
    pub(super) fn write(&self, run_dir: &Path) -> Result<(), String> {
        let path = run_dir.join(RECORD_FILE);
        std::fs::write(&path, self.render()).map_err(|err| format!("{} を書けない: {err}", path.display()))
    }
}

/// lock の前に 1 周に 1 回だけ撃った読み（§56 形 2）: `freeze` の結果（有効値と名乗りの語）と、名乗りが base で撃つ周の実走。
/// lock の中の判定はこれを借りて宣言を読み直さない。
pub(in crate::pipe) struct Early {
    /// `freeze` の結果。
    pub(super) frozen: Result<(Effective, Option<EntranceFlip>), Denial>,
    /// base の木の実走（`detect` / `deny` を名乗った周だけ）。
    pub(super) base: Option<BaseRun>,
}

impl Early {
    /// `deny` を名乗った周の実走（他の周は `None`）。
    pub(super) fn denying(&self) -> Option<&BaseRun> {
        let named = self.frozen.as_ref().ok().and_then(|(_, named)| *named);
        self.base.as_ref().filter(|_| named.is_some_and(EntranceFlip::denies))
    }
}

/// 撃った結果（遮断器が閉じたか・撃てたか）。
enum Shot {
    /// 遮断器が閉じて撃たなかった。
    Closed,
    /// 撃った。
    Fired {
        /// rc。
        rc: i32,
        /// 箱の中で kernel が殺した数（読めない周は `None`）。
        oom_kill: Option<u64>,
        /// 壁時計の秒。
        secs: u64,
    },
}

/// 仕分けた行を順に撃って結果にする（`fire` は撃つ手・引数は 1 始まりの行番号と行）。遮断器が閉じた行とそれ以後の行は
/// 撃たずに測れない。
fn settle(lines: &[String], plans: &[Plan], mut fire: impl FnMut(usize, &str) -> Shot) -> Vec<Row> {
    let mut closed = false;
    let mut rows = Vec::new();
    for (n, (line, plan)) in lines.iter().zip(plans).enumerate() {
        let (value, rc, secs) = match (closed, *plan) {
            (true, _) | (false, Plan::Unfired) => (Value::Unmeasurable, None, None),
            (false, Plan::Absent) => (Value::Absent, None, None),
            (false, Plan::Fire) => match fire(n.saturating_add(1), line) {
                Shot::Closed => {
                    closed = true;
                    (Value::Unmeasurable, None, None)
                }
                Shot::Fired { rc, oom_kill, secs } => (value_of(rc, oom_kill), Some(rc), Some(secs)),
            },
        };
        rows.push(Row { line: line.clone(), value, rc, secs });
    }
    rows
}

/// 契約の検証行を sha の木で撃つ（§56 形 3 / 4）。置き場が無い周・rules 行を読めない周・木を用意できない周は撃たずに、実走の
/// 後に読み直した HEAD が `sha` と違う周は結果を捨てて、全行を測れない。撃ち終えた木は worktree の登録ごと畳む。
pub(super) fn run(repo: &Path, state_dir: Option<&Path>, sha: &str, lines: &[String], manifest: &Manifest) -> BaseRun {
    let (Some(state_dir), Ok(limits)) = (state_dir, Limits::of(manifest)) else {
        return BaseRun::unmeasured(sha, lines);
    };
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |found| found.as_nanos());
    let tree = state_dir.join(format!("{TREE_HEAD}{}-{stamp}", std::process::id()));
    let path = tree.display().to_string();
    if std::fs::create_dir_all(state_dir).is_err() || !git_ok(repo, &["worktree", "add", "--detach", &path, sha]) {
        fold(repo, &tree);
        return BaseRun::unmeasured(sha, lines);
    }
    let plans: Vec<Plan> = lines.iter().map(|line| plan_of(line, &tree)).collect();
    let caps = confine::Caps::embedded();
    let place = tree.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let rows = settle(lines, &plans, |n, line| {
        let health::Passage::Fire(_) = health::pass(limits.breaker()) else {
            return Shot::Closed;
        };
        let unit = confine::unit_name(&place, STAGE, n);
        let wrap = confine::Wrap { unit: &unit, limit: confine::limit_of(line, UNADMITTED_JOBS), caps };
        let fired = gate::run_line_captured(&tree, line, &wrap);
        Shot::Fired { rc: fired.rc, oom_kill: fired.usage.oom_kill, secs: fired.secs }
    });
    fold(repo, &tree);
    if head_of(repo).as_deref() != Some(sha) {
        return BaseRun::unmeasured(sha, lines);
    }
    BaseRun { sha: sha.to_owned(), rows }
}

/// 一時の木を worktree の登録ごと畳む（畳めない周は dir を消して登録を刈る）。
fn fold(repo: &Path, tree: &Path) {
    if !git_ok(repo, &["worktree", "remove", "--force", &tree.display().to_string()]) {
        let _ = std::fs::remove_dir_all(tree);
        let _ = git_ok(repo, &["worktree", "prune"]);
    }
}

// ---- Cargo.toml の読み（器の読み手が読める TOML の部分集合・読めない形は `None`＝撃つ側）----

/// Cargo.toml から読む事実。
#[derive(Debug, Default)]
struct CargoToml {
    /// `[package]` の表を持つ。
    package_table: bool,
    /// `[package]` の `name`（文字列の周だけ）。
    package: Option<String>,
    /// `[workspace]` の表を持つ。
    workspace: bool,
    /// `[workspace]` の `members`。
    members: Vec<String>,
    /// test の target の名（`[[test]]` の表と、`test` の key の inline の表の配列の `name`）。
    tests: Vec<String>,
}

impl CargoToml {
    /// 1 つの key と値を取り込む（`members` の要素が文字列でない周は読めない）。
    fn take(&mut self, table: &str, array: bool, key: &str, value: Toml) -> Option<()> {
        match (table, array, key, value) {
            ("package", false, "name", Toml::Text(name)) => self.package = Some(name),
            ("workspace", false, "members", Toml::List(items)) => {
                for item in items {
                    let Toml::Text(member) = item else {
                        return None;
                    };
                    self.members.push(member);
                }
            }
            ("test", true, "name", Toml::Text(name)) => self.tests.push(name),
            (_, _, "test", Toml::List(items)) => self.tests.extend(items.into_iter().filter_map(inline_name)),
            _ => {}
        }
        Some(())
    }
}

/// inline の表の `name`（文字列の周だけ）。
fn inline_name(item: Toml) -> Option<String> {
    let Toml::Table(pairs) = item else {
        return None;
    };
    pairs.into_iter().find_map(|(key, value)| match value {
        Toml::Text(name) if key == "name" => Some(name),
        _ => None,
    })
}

/// file を読んで [`cargo_toml`] に通す（読めない file は `None`・symlink は辿る）。
fn read_toml(path: &Path) -> Option<CargoToml> {
    std::fs::read_to_string(path).ok().and_then(|text| cargo_toml(&text))
}

/// Cargo.toml の本文を読む（表の見出し・key と値の行・空行とコメントだけを受ける）。
fn cargo_toml(text: &str) -> Option<CargoToml> {
    let mut parser = Parser { tokens: tokens(text)?, at: 0 };
    let mut found = CargoToml::default();
    let (mut table, mut array) = (String::new(), false);
    loop {
        parser.skip_newlines();
        match parser.next() {
            None => return Some(found),
            Some(Token::Mark('[')) => {
                array = parser.eat('[');
                table = parser.key()?;
                if !parser.eat(']') || (array && !parser.eat(']')) {
                    return None;
                }
                found.package_table |= !array && table == "package";
                found.workspace |= !array && table == "workspace";
            }
            Some(Token::Bare(key) | Token::Text(key)) => {
                if !parser.eat('=') {
                    return None;
                }
                let value = parser.value()?;
                found.take(&table, array, &key, value)?;
            }
            Some(_) => return None,
        }
        if !matches!(parser.next(), None | Some(Token::Newline)) {
            return None;
        }
    }
}

/// TOML の字句。
#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    /// 引用符の文字列。
    Text(String),
    /// 裸の語（key・数・真偽・日付）。
    Bare(String),
    /// 記号（`[` `]` `{` `}` `=` `,`）。
    Mark(char),
    /// 改行。
    Newline,
}

/// TOML の値（読み手が要る形だけを残す）。
enum Toml {
    /// 文字列。
    Text(String),
    /// 裸の値（数・真偽・日付）。
    Bare,
    /// 配列。
    List(Vec<Toml>),
    /// inline の表。
    Table(Vec<(String, Toml)>),
}

/// 裸の語を成す字か。
fn bare_char(found: char) -> bool {
    found.is_ascii_alphanumeric() || "_-.+:".contains(found)
}

/// 本文を字句に割る（複数行の文字列と未知の字は読めない）。
fn tokens(text: &str) -> Option<Vec<Token>> {
    let mut found = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(head) = chars.next() {
        match head {
            ' ' | '\t' | '\r' => {}
            '\n' => found.push(Token::Newline),
            '#' => while chars.next_if(|next| *next != '\n').is_some() {},
            '[' | ']' | '{' | '}' | '=' | ',' => found.push(Token::Mark(head)),
            '"' | '\'' => found.push(Token::Text(quoted(&mut chars, head)?)),
            _ if bare_char(head) => {
                let mut word = String::from(head);
                while let Some(next) = chars.next_if(|next| bare_char(*next)) {
                    word.push(next);
                }
                found.push(Token::Bare(word));
            }
            _ => return None,
        }
    }
    Some(found)
}

/// 引用符 `quote` の文字列の残り（開きの後から）。`"` は `\` の escape を 5 つだけ読む・複数行の形は読めない。
fn quoted(chars: &mut Peekable<Chars<'_>>, quote: char) -> Option<String> {
    if chars.next_if_eq(&quote).is_some() {
        return chars.next_if_eq(&quote).is_none().then(String::new);
    }
    let mut text = String::new();
    loop {
        match chars.next()? {
            found if found == quote => return Some(text),
            '\n' => return None,
            '\\' if quote == '"' => text.push(match chars.next()? {
                '"' => '"',
                '\\' => '\\',
                'n' => '\n',
                't' => '\t',
                'r' => '\r',
                _ => return None,
            }),
            other => text.push(other),
        }
    }
}

/// 字句の読み手。
struct Parser {
    /// 字句の列。
    tokens: Vec<Token>,
    /// 次に読む位置。
    at: usize,
}

impl Parser {
    /// 次の字句を取る。
    fn next(&mut self) -> Option<Token> {
        let found = self.tokens.get(self.at).cloned();
        self.at = self.at.saturating_add(1);
        found
    }

    /// 次が記号 `mark` なら取って真。
    fn eat(&mut self, mark: char) -> bool {
        let hit = self.tokens.get(self.at) == Some(&Token::Mark(mark));
        if hit {
            self.at = self.at.saturating_add(1);
        }
        hit
    }

    /// 改行を読み飛ばす。
    fn skip_newlines(&mut self) {
        while self.tokens.get(self.at) == Some(&Token::Newline) {
            self.at = self.at.saturating_add(1);
        }
    }

    /// key（裸の語か文字列）。
    fn key(&mut self) -> Option<String> {
        match self.next()? {
            Token::Bare(key) | Token::Text(key) => Some(key),
            Token::Mark(_) | Token::Newline => None,
        }
    }

    /// 値 1 つ（配列は改行を跨げる・inline の表は 1 行）。
    fn value(&mut self) -> Option<Toml> {
        match self.next()? {
            Token::Text(text) => Some(Toml::Text(text)),
            Token::Bare(_) => Some(Toml::Bare),
            Token::Mark('[') => self.list().map(Toml::List),
            Token::Mark('{') => self.table().map(Toml::Table),
            Token::Mark(_) | Token::Newline => None,
        }
    }

    /// 配列の残り（開きの後から閉じまで・末尾の `,` を許す）。
    fn list(&mut self) -> Option<Vec<Toml>> {
        let mut items = Vec::new();
        loop {
            self.skip_newlines();
            if self.eat(']') {
                return Some(items);
            }
            items.push(self.value()?);
            self.skip_newlines();
            if !self.eat(',') {
                self.skip_newlines();
                return self.eat(']').then_some(items);
            }
        }
    }

    /// inline の表の残り（開きの後から閉じまで）。
    fn table(&mut self) -> Option<Vec<(String, Toml)>> {
        let mut pairs = Vec::new();
        if self.eat('}') {
            return Some(pairs);
        }
        loop {
            let key = self.key()?;
            if !self.eat('=') {
                return None;
            }
            pairs.push((key, self.value()?));
            if self.eat('}') {
                return Some(pairs);
            }
            if !self.eat(',') {
                return None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{plan_of, settle, value_of, Plan, Shot, Value};
    use std::path::{Path, PathBuf};

    /// 読み切れる対照の行（b の dir に target tt が無い木では真の不在）。
    const CONTROL: &str = "cargo nextest run -p b --test tt --no-tests=fail old_tooth";

    /// 歯ごとの一時の dir（pid と名で一意）。
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("s2-base-run-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap_or_else(|err| panic!("{} を作れる: {err}", dir.display()));
        dir
    }

    /// `files`（path と本文）を `dir` に書く。
    fn put(dir: &Path, files: &[(&str, &str)]) {
        for (path, body) in files {
            let target = dir.join(path);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).unwrap_or_else(|err| panic!("{} を作れる: {err}", parent.display()));
            }
            std::fs::write(&target, body).unwrap_or_else(|err| panic!("{} を書ける: {err}", target.display()));
        }
    }

    /// members の項 a と b の 2 crate の木（a は tests/tt.rs を持ち、b は target tt を持たない）に `extra` を足す。
    fn tree(name: &str, extra: &[(&str, &str)]) -> PathBuf {
        let dir = scratch(name);
        let base = [
            ("Cargo.toml", "[workspace]\nmembers = [\n    \"a\",\n    \"b\", # 2 本目\n]\nresolver = \"2\"\n"),
            ("a/Cargo.toml", "[package]\nname = \"a\"\nversion = \"0.1.0\"\n"),
            ("a/tests/tt.rs", "#[test]\nfn old_tooth() {}\n"),
            ("b/Cargo.toml", "[package]\nname = 'b'\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n"),
            ("b/src/lib.rs", ""),
        ];
        put(&dir, &base);
        put(&dir, extra);
        dir
    }

    /// (b) rc 0 / 4 / 100 / 101 / 137 / -1 と oom_kill 1 の 7 形の写し（oom の行は rc 0 でも測れない）。
    #[test]
    fn pipe_base_run_rc_maps_to_four_values_and_oom_is_unmeasurable() {
        let cases = [
            ((0, Some(0)), Value::GreenOnBase),
            ((4, None), Value::Absent),
            ((100, Some(0)), Value::Failed),
            ((101, None), Value::Unmeasurable),
            ((137, None), Value::Unmeasurable),
            ((-1, None), Value::Unmeasurable),
            ((0, Some(1)), Value::Unmeasurable),
        ];
        for ((rc, oom), want) in cases {
            assert_eq!(value_of(rc, oom), want, "rc {rc} oom {oom:?}（母集団 {} 形）", cases.len());
        }
    }

    /// (b) 遮断器が閉じた行とそれ以後の行は撃たずに測れない（撃つ手は閉じた行までしか呼ばれない）。
    #[test]
    fn pipe_base_run_closed_breaker_makes_the_rest_unmeasurable() {
        let lines: Vec<String> = ["one", "two", "three", "four"].iter().map(|word| format!("{CONTROL} {word}")).collect();
        let plans = [Plan::Fire, Plan::Fire, Plan::Absent, Plan::Fire];
        let mut called = Vec::new();
        let rows = settle(&lines, &plans, |n, _| {
            called.push(n);
            if n == 2 {
                Shot::Closed
            } else {
                Shot::Fired { rc: 0, oom_kill: None, secs: 1 }
            }
        });
        let values: Vec<Value> = rows.iter().map(|row| row.value).collect();
        assert_eq!(values, [Value::GreenOnBase, Value::Unmeasurable, Value::Unmeasurable, Value::Unmeasurable]);
        assert_eq!(called, [1, 2], "閉じた後は撃たない");
        assert_eq!(rows.first().and_then(|row| row.rc), Some(0), "撃った行は rc を持つ");
        assert!(rows.iter().skip(1).all(|row| row.rc.is_none() && row.secs.is_none()), "撃たなかった行は rc も秒も無い");
    }

    /// (b) nextest の行でない 5 形（cargo test・clippy・前置きの行・filter 語の無い行・`--no-tests=fail` の無い行）は撃つ列に
    /// 入らない（撃つ手を 1 度も呼ばない・測れない）。
    #[test]
    fn pipe_base_run_non_nextest_lines_are_not_fired() {
        let dir = tree("unfired", &[]);
        let lines: Vec<String> = [
            "cargo test -p b --test tt old_tooth",
            "cargo clippy --all-targets",
            "timeout 60 cargo nextest run -p b --no-tests=fail old_tooth",
            "cargo nextest run -p b --no-tests=fail",
            "cargo nextest run -p b old_tooth",
        ]
        .iter()
        .map(|line| (*line).to_owned())
        .collect();
        let plans: Vec<Plan> = lines.iter().map(|line| plan_of(line, &dir)).collect();
        assert!(plans.iter().all(|plan| *plan == Plan::Unfired), "{plans:?}");
        let rows = settle(&lines, &plans, |_, line| panic!("撃たない: {line}"));
        assert!(rows.iter().all(|row| row.value == Value::Unmeasurable), "撃たない行は測れない");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// (c) 対照の 1 行だけが不在で、7 周目の 6 形・8 周目の 4 形・`=` の形・9 周目の 4 字で旗を 1 語に埋めた 4 行・値が `-` で
    /// 始まる 1 行の 16 行は撃つ側。
    #[test]
    fn pipe_base_run_only_the_readable_control_line_is_absent() {
        let dir = tree("sixteen", &[]);
        assert_eq!(plan_of(CONTROL, &dir), Plan::Absent, "対照の行は不在");
        let mut lines: Vec<String> = [
            "cargo nextest run -p a -p b --test tt --no-tests=fail old_tooth",
            "cargo nextest run --workspace -p b --test tt --no-tests=fail old_tooth",
            "cargo nextest run --manifest-path b/Cargo.toml -p b --test tt --no-tests=fail old_tooth",
            "cargo nextest run -p b --test t* --no-tests=fail old_tooth",
            "cargo nextest run -p b --test 'tt' --no-tests=fail old_tooth",
            "cargo nextest run -p b --test tt --test tt --no-tests=fail old_tooth",
            "cargo nextest run --no-tests=fail old_tooth # -p b --test tt",
            "cargo nextest run --no-tests=fail old_tooth\\ -p\\ b\\ --test\\ tt",
            "cargo nextest run --no-tests=fail \"-p b --test tt\" old_tooth",
            "cargo nextest run --no-tests=fail `-p b --test tt` old_tooth",
            "cargo nextest run -p=b --test=tt --no-tests=fail old_tooth",
            "cargo nextest run -p b --no-tests=fail --test -- old_tooth",
        ]
        .iter()
        .map(|line| (*line).to_owned())
        .collect();
        for joint in ['\u{3000}', '\u{00a0}', '\u{000c}', '\u{000d}'] {
            let word = ["x", "-p", "b", "--test", "tt"].join(&joint.to_string());
            lines.push(format!("cargo nextest run --no-tests=fail {word} old_tooth"));
        }
        assert_eq!(lines.len(), 16, "母集団 16 行");
        for line in &lines {
            assert_eq!(plan_of(line, &dir), Plan::Fire, "撃つ側: {line:?}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// (c) target が在ると読まれる 5 形（名の .rs・名の dir の main.rs・path 付きの `[[test]]`・inline の表の配列・a の tests への
    /// symlink）は対照の行でも撃つ側。
    #[test]
    fn pipe_base_run_present_targets_are_fired() {
        let manifest = |extra: &str| format!("[package]\nname = \"b\"\n{extra}");
        let forms = [
            vec![("b/tests/tt.rs", String::new())],
            vec![("b/tests/tt/main.rs", String::new())],
            vec![("b/Cargo.toml", manifest("\n[[test]]\nname = \"tt\"\npath = \"x/y.rs\"\n"))],
            vec![("b/Cargo.toml", format!("test = [{{ name = \"tt\", path = \"x/y.rs\" }}]\n{}", manifest("")))],
        ];
        for (n, form) in forms.iter().enumerate() {
            let files: Vec<(&str, &str)> = form.iter().map(|(path, body)| (*path, body.as_str())).collect();
            let dir = tree(&format!("present-{n}"), &files);
            assert_eq!(plan_of(CONTROL, &dir), Plan::Fire, "在る形 {n}: {form:?}");
            let _ = std::fs::remove_dir_all(&dir);
        }
        let dir = tree("present-link", &[]);
        std::os::unix::fs::symlink("../a/tests", dir.join("b/tests")).unwrap_or_else(|err| panic!("symlink を作れる: {err}"));
        assert_eq!(plan_of(CONTROL, &dir), Plan::Fire, "symlink の tests dir の下の名の .rs");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// (c) `-p` だけ・`--test` だけの行と、root の package・`[workspace]` の無い単一 crate・glob の members の木は撃つ側。
    #[test]
    fn pipe_base_run_unresolved_packages_and_workspaces_are_fired() {
        let dir = tree("flags", &[]);
        for line in ["cargo nextest run -p b --no-tests=fail old_tooth", "cargo nextest run --test tt --no-tests=fail old_tooth"] {
            assert_eq!(plan_of(line, &dir), Plan::Fire, "{line}");
        }
        let _ = std::fs::remove_dir_all(&dir);
        let roots = [
            "[package]\nname = \"b\"\n\n[workspace]\nmembers = [\"a\"]\n",
            "[package]\nname = \"b\"\n",
            "[workspace]\nmembers = [\"*\"]\n",
        ];
        for (n, root) in roots.iter().enumerate() {
            let dir = tree(&format!("root-{n}"), &[("Cargo.toml", root)]);
            assert_eq!(plan_of(CONTROL, &dir), Plan::Fire, "{root:?}");
            let _ = std::fs::remove_dir_all(&dir);
        }
        let dir = tree("unreadable", &[("b/Cargo.toml", "[package]\nname = \"\"\"b\"\"\"\n")]);
        assert_eq!(plan_of(CONTROL, &dir), Plan::Fire, "読めない Cargo.toml は撃つ側");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
