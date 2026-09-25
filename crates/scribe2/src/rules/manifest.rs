//! `rules/manifest.toml` を std だけで読む面（ADR-0004 §2.3・SRS NFR3）。
//!
//! 受理するのは TOML の部分集合である: 先頭の `schema = 1`・`[[rule]]` / `[[account]]` / `[[plugin]]` /
//! `[[launch-arg]]` / `[[vessel]]` の array-of-tables・値は string / integer / bool と**文字列の配列**（1 行で閉じる）。
//! **最初の 1 件で止めず**違反を全件集めて返す（silent drop 禁止・SRS NFR4）。
//!
//! `[[account]]` は**規則の値ではなく宣言値**である（口座の列挙・設計 fleet-usage.md §2・
//! ADR-0017 §2.3）。ゆえに裁定 id を行ごとに持たず、持てる key は `label` 1 つだけで、
//! `[[rule]]` 行の検査（裁定 id 必須・`enabled` 必須・kind と値の形の一致）は一切変わらない。
//!
//! **host の面**（`<state_dir>/host.toml`・設計 account-lifecycle.md §2・ADR-0026 §2.1）も同じ reader で読む:
//! 持てる表は `[[account]]` / `[[plugin]]` / `[[launch-arg]]` / `[[vessel]]`（器自身の checkout・最大 1 行・
//! 設計 consumer-sync.md §4）/ `[[account-group]]`（席の口座を持つ project の群・設計 account-lifecycle.md §17・
//! ADR-0049）/ `[[tick]]` の 6 種だけで、`[[rule]]` は置けない（規則の行は tracked の面だけ・C1）。無い周は
//! 0 宣言（縮退）・在るが読めない周は欠陥の全件（FailClosed）。
//!
//! `[[account-group]]` は**host の面にだけ**置ける最初の表である（`[[rule]]` が tracked の面にだけ置けるのと
//! 対称・設計 account-lifecycle.md §17 の約束 2）。`[[tick]]`（席の起動が入れる tick の unit の置き場と binary・最大 1 行・
//! 設計 seat-heartbeat.md §5・ADR-0064）も host の面にだけ置ける。
//!
//! **契約表の面**（設計 doc の区間・導出の `.toml`・設計 contract-source.md §2・ADR-0023 §2.1）も同じ reader で読む:
//! 持てる表は `[[contract]]` 1 種だけで（key 集合は `pipe::table::FIELDS`）、rules manifest と host の面は
//! `[[contract]]` を置けない。値の受理集合と**空の配列の拒否**は他の面と同じ（空の列は key の省略で表す）。

use super::{Rule, RuleError, RuleKind, RuleRow, RuleValue, ValueShape, HOST_MANIFEST};
use crate::hook::command::{denied_in, denied_of};
use crate::pipe::contract::{class_element, ClassElement};
use crate::pipe::declaration::CEILING_ROW;
use crate::pipe::table::{Need, DERIVED_GOAL, FIELDS};
use std::path::Path;

/// build 時に binary へ埋め込む manifest の本文。
///
/// 別 repo の worktree で走る便でも path に依存せず同じ規則を読むための形である
/// （憲法 C1・単一 static binary の向き）。
const EMBEDDED: &str = include_str!("../../../../rules/manifest.toml");

/// manifest が要求する schema 版（契約表の導出物の版の宣言 `pipe::table::WHOLE_HEAD` も同じ値・歯が pin する）。
pub(crate) const SCHEMA: u64 = 1;

/// `[[rule]]` 行が持てる key の全体。ここに無い key は拒む。
const KNOWN_KEYS: &[&str] = &["id", "kind", "value", "enabled", "ruling", "ruled_at"];

/// `[[account]]` 行が持てる key の全体。**必須もこれと同じ 1 つ**である。
///
/// label しか持たせないのは、口座の識別に使える形（host 名・path・本当の口座 id）を
/// 公開面へ載せないためである（CON2・設計 fleet-usage.md §2 の「不透明」）。
const ACCOUNT_KEYS: &[&str] = &["label"];

/// `[[plugin]]` 行が持てる key の全体（必須も同じ 1 つ）。値は席に積む plugin dir。
const PLUGIN_KEYS: &[&str] = &["dir"];

/// `[[launch-arg]]` 行が持てる key の全体（必須も同じ 1 つ）。値は席の起動行に足す引数 1 つ。
const LAUNCH_ARG_KEYS: &[&str] = &["value"];

/// `[[vessel]]` 行が持てる key の全体（必須も同じ 1 つ）。値は器自身の checkout の dir（host 固有・host の面にだけ・
/// **最大 1 行**・設計 consumer-sync.md §4）。
const VESSEL_KEYS: &[&str] = &["repo"];

/// `[[account-group]]` 1 行が持てる key の全体（**必須もこれと同じ 3 つ**・設計 account-lifecycle.md §17 の約束 1）。
///
/// `name` は host で一意な群の名、`anchors` は群に属する置き場（席の登録 row の anchor）の列、`accounts` は
/// 候補の口座 label の列で**宣言順が候補の順**である。どちらの列も空は受けない（空の列は [`list`] が断る）。
const GROUP_KEYS: &[&str] = &["name", "anchors", "accounts"];

/// `[[tick]]` 行が持てる key の全体（**必須もこれと同じ 2 つ**・設計 seat-heartbeat.md §5 形 1）。`unit-dir` は tick の unit を置く
/// dir・`binary` は unit が撃つ器（どちらも絶対 path・host 固有・host の面にだけ・**最大 1 行**）。
const TICK_KEYS: &[&str] = &["unit-dir", "binary"];

/// 行に必ず要る key。
///
/// **`enabled` も必須である**（`s2-07l.80`・裁定 id `user 2026-09-11T23:59Z`）。省略を
/// `true` で埋めていた間は、書き忘れた行が「効く」側へ黙って倒れていた——規則の発効は
/// 書かれた事実であって既定ではない（C1「規則はデータ」・C5）。同じ理由で `ruled_at` の
/// 欠落も空文字で埋めない。
const REQUIRED_KEYS: &[&str] = &["id", "kind", "value", "enabled", "ruling", "ruled_at"];

/// TOML subset が受理する値。
///
/// `pipe` の契約 file も同じ subset の値を持つので、この型と [`scalar`] を器の中で
/// 共有する（第 2 の値 parser を作らない・憲法 C6）。**受理集合はここが唯一の定義**で、
/// 配列の層も同じ module の [`list`] / [`elements`] / [`quoted_once`] が持つ
/// （rules manifest も契約 file も同じ切り方で読む・s2-07l.55）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scalar {
    /// 非負整数。
    Int(u64),
    /// 文字列。
    Str(String),
    /// 真偽。
    Bool(bool),
}

/// 1 つの key が持てる生の値。**配列の層はここが唯一の定義**である。
///
/// `pipe` の契約 file も同じ切り方（[`elements`] / [`quoted_once`]）を使う
/// ——配列を読む実装が 2 本あると、書いた本数と通る本数の食い違いが片側だけ直る。
#[derive(Debug, Clone, PartialEq, Eq)]
enum RawValue {
    /// 単一の値。
    One(Scalar),
    /// 文字列の列。
    List(Vec<String>),
    /// 読めなかった値。**scan の時点で 1 件報告済み**なので、以降の段はこの値に
    /// ついて何も言わない——同じ欠陥を 2 行にしないためであり、とりわけ
    /// 「必須 key value が無い」と**嘘をつかない**ため（key は在って値が壊れている）。
    Broken,
}

/// 受理する array-of-tables の種類。
///
/// 字面と key 集合の対応は [`Section::header`] / [`Section::known_keys`] /
/// [`Section::required_keys`] の網羅 `match` が持つ（種類を足したら compile error）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    /// 規則 1 行（値 + 裁定）。
    Rule,
    /// 口座 1 件の宣言（label だけ）。
    Account,
    /// 席に積む plugin dir 1 つの宣言（dir だけ）。
    Plugin,
    /// 席の起動行に足す引数 1 つの宣言（value だけ）。
    LaunchArg,
    /// 契約表の 1 行（key 集合は `pipe::table::FIELDS`・契約表の面にだけ置く）。
    Contract,
    /// 器自身の checkout の宣言（repo だけ・最大 1 行・doctor の `head=` と `vessel update` が読む）。
    Vessel,
    /// 席の口座を持つ project の群 1 つの宣言（名・置き場の列・候補の口座 label の列・**host の面にだけ**・
    /// 設計 account-lifecycle.md §17・ADR-0049）。
    AccountGroup,
    /// 席の起動が入れる tick の unit の置き場と binary の宣言（unit-dir と binary・最大 1 行・**host の面にだけ**・
    /// 設計 seat-heartbeat.md §5・ADR-0064）。
    Tick,
}

/// [`Section`] の全 variant（宣言順）。
const SECTIONS: &[Section] = &[
    Section::Rule,
    Section::Account,
    Section::Plugin,
    Section::LaunchArg,
    Section::Contract,
    Section::Vessel,
    Section::AccountGroup,
    Section::Tick,
];

impl Section {
    /// TOML の section header の字面。
    fn header(self) -> &'static str {
        match self {
            Self::Rule => "[[rule]]",
            Self::Account => "[[account]]",
            Self::Plugin => "[[plugin]]",
            Self::LaunchArg => "[[launch-arg]]",
            Self::Contract => "[[contract]]",
            Self::Vessel => "[[vessel]]",
            Self::AccountGroup => "[[account-group]]",
            Self::Tick => "[[tick]]",
        }
    }

    /// header の字面から引く。未知なら `None`。
    fn parse(text: &str) -> Option<Self> {
        SECTIONS.iter().copied().find(|found| found.header() == text)
    }

    /// この section が持てる key の全体（契約表の行は欄の正本 `FIELDS` から引く＝欄の列を 2 面に書かない・導出物だけの
    /// 欄 `DERIVED_GOAL` の分だけ広い〔`.md` の区間の行が持てば `pipe::table` の parse の段が断る〕）。
    fn known_keys(self) -> Vec<&'static str> {
        match self {
            Self::Rule => KNOWN_KEYS.to_vec(),
            Self::Account => ACCOUNT_KEYS.to_vec(),
            Self::Plugin => PLUGIN_KEYS.to_vec(),
            Self::LaunchArg => LAUNCH_ARG_KEYS.to_vec(),
            Self::Contract => FIELDS.iter().map(|field| field.name).chain([DERIVED_GOAL]).collect(),
            Self::Vessel => VESSEL_KEYS.to_vec(),
            Self::AccountGroup => GROUP_KEYS.to_vec(),
            Self::Tick => TICK_KEYS.to_vec(),
        }
    }

    /// この section に必ず要る key。
    fn required_keys(self) -> Vec<&'static str> {
        match self {
            Self::Rule => REQUIRED_KEYS.to_vec(),
            Self::Account => ACCOUNT_KEYS.to_vec(),
            Self::Plugin => PLUGIN_KEYS.to_vec(),
            Self::LaunchArg => LAUNCH_ARG_KEYS.to_vec(),
            Self::Contract => FIELDS.iter().filter(|field| field.need == Need::Required).map(|field| field.name).collect(),
            Self::Vessel => VESSEL_KEYS.to_vec(),
            Self::AccountGroup => GROUP_KEYS.to_vec(),
            Self::Tick => TICK_KEYS.to_vec(),
        }
    }
}

/// manifest の面（どの file を読んでいるか）。受ける section の集合だけが違う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Face {
    /// tracked の manifest（埋め込みか `--rules PATH`）。
    Tracked,
    /// host の manifest（`<state_dir>/host.toml`）。`[[rule]]` を受けない。
    Host,
    /// 契約表（設計 doc の区間・導出の `.toml`）。`[[contract]]` だけを受ける。
    Table,
}

/// section 1 つ分の生の key/value。
struct RawRow {
    section: Section,
    line: u64,
    fields: Vec<(String, RawValue, u64)>,
}

/// `[[account]]` 1 行が名乗る**不透明な** label（設計 fleet-usage.md §2）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountLabel {
    label: String,
    line: u64,
}

impl AccountLabel {
    /// label の字面。
    pub fn label(&self) -> &str {
        &self.label
    }

    /// manifest の中でこの行が始まる物理行番号。
    pub fn line(&self) -> u64 {
        self.line
    }
}

/// `[[plugin]]` 1 行が名乗る plugin dir（host 固有の場所・host の面にだけ書く・設計 account-lifecycle.md §2）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginDir {
    dir: String,
    line: u64,
}

impl PluginDir {
    /// dir の字面。
    pub fn dir(&self) -> &str {
        &self.dir
    }

    /// manifest の中でこの行が始まる物理行番号。
    pub fn line(&self) -> u64 {
        self.line
    }
}

/// `[[launch-arg]]` 1 行が名乗る起動引数 1 つ（順序 = 宣言順・設計 account-lifecycle.md §2）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchArg {
    value: String,
    line: u64,
}

impl LaunchArg {
    /// 引数の字面。
    pub fn value(&self) -> &str {
        &self.value
    }

    /// manifest の中でこの行が始まる物理行番号。
    pub fn line(&self) -> u64 {
        self.line
    }
}

/// `[[vessel]]` 1 行が名乗る器自身の checkout（host 固有の場所・host の面にだけ書く・設計 consumer-sync.md §4）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VesselRepo {
    repo: String,
    line: u64,
}

impl VesselRepo {
    /// checkout の dir の字面。
    pub fn repo(&self) -> &str {
        &self.repo
    }

    /// manifest の中でこの行が始まる物理行番号。
    pub fn line(&self) -> u64 {
        self.line
    }
}

/// `[[account-group]]` 1 行が宣言する群（席の口座を持つ project の群・設計 account-lifecycle.md §17・ADR-0049）。
///
/// **宣言値と種だけを持つ**（群の「今の口座」は第 3 段の記録で、この型も reader も書かない・種は面の読みの導出＝§28 形 1）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountGroup {
    name: String,
    anchors: Vec<String>,
    accounts: Vec<String>,
    seed: String,
    line: u64,
}

impl AccountGroup {
    /// 群の名（host で一意）。
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 群に属する置き場（席の登録 row の anchor）の列を**宣言順**で。
    pub fn anchors(&self) -> &[String] {
        &self.anchors
    }

    /// 候補の口座 label の列を**宣言順**（= 候補の順）で。
    pub fn accounts(&self) -> &[String] {
        &self.accounts
    }

    /// 種（記録の無い周の今の口座）: 宣言順で前の群の種でない最初の候補（面の読みが埋める・設計 account-lifecycle.md §28）。
    pub fn seed(&self) -> &str {
        &self.seed
    }

    /// manifest の中でこの行が始まる物理行番号。
    pub fn line(&self) -> u64 {
        self.line
    }
}

/// `[[tick]]` 1 行が名乗る tick の unit の置き場と binary（host 固有の場所・host の面にだけ書く・設計 seat-heartbeat.md §5 形 1・
/// ADR-0064）。どちらも絶対 path（unit は `WorkingDirectory=` を持たないので相対 path は解けない）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TickUnit {
    unit_dir: String,
    binary: String,
    line: u64,
}

impl TickUnit {
    /// unit を置く dir の字面（`unit-dir`）。
    pub fn unit_dir(&self) -> &str {
        &self.unit_dir
    }

    /// unit が撃つ器の字面（`binary`）。
    pub fn binary(&self) -> &str {
        &self.binary
    }

    /// manifest の中でこの行が始まる物理行番号。
    pub fn line(&self) -> u64 {
        self.line
    }
}

/// 読み込み済みの manifest。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Manifest {
    rows: Vec<RuleRow>,
    accounts: Vec<AccountLabel>,
    plugins: Vec<PluginDir>,
    launch_args: Vec<LaunchArg>,
    contracts: Vec<TableRow>,
    vessel: Option<VesselRepo>,
    groups: Vec<AccountGroup>,
    // Box は `HostManifest::Present` の大きさを抑えるため（clippy large_enum_variant）。
    tick: Option<Box<TickUnit>>,
}

/// `[[contract]]` 1 行の値（key 集合は検査済み・値の形の検査は欄の形を持つ `pipe::table` が行う）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRow {
    line: u64,
    fields: Vec<(String, TableValue, u64)>,
}

impl TableRow {
    /// 本文の中でこの行（`[[contract]]`）が始まる物理行番号。
    pub fn line(&self) -> u64 {
        self.line
    }

    /// key の値と、その key が書かれていた物理行番号。
    pub fn value(&self, key: &str) -> Option<(&TableValue, u64)> {
        self.fields.iter().find(|(found, _, _)| found == key).map(|(_, value, line)| (value, *line))
    }
}

/// 契約表の 1 つの key が持てる値（単一の値か文字列の列）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableValue {
    /// 単一の値。
    One(Scalar),
    /// 文字列の列（空は reader が断る）。
    List(Vec<String>),
}

/// 契約表の本文を読む（区間の抜き出しは呼び手の `pipe::table`・設計 contract-source.md §2）。受ける表は
/// `[[contract]]` だけで、先頭の `schema = 1` も要る。欠陥は他の面と同じく全件・行番号付き。
pub fn contract_rows(text: &str) -> Result<Vec<TableRow>, Vec<RuleError>> {
    finish(collect(text, Face::Table)).map(|found| found.contracts)
}

/// host の面（`<state_dir>/host.toml`）の読み（設計 account-lifecycle.md §7 `HostManifest`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostManifest {
    /// file が無い（縮退・0 宣言・止めない）。
    Absent,
    /// 読めた（host の面の宣言だけを持つ manifest・`rows()` は空）。
    Present(Manifest),
    /// 在るが読めない・壊れている（FailClosed）。欠陥は全件・行番号付き・`host.toml:` の接頭辞で面を名指す。
    Unreadable(Vec<RuleError>),
}

impl HostManifest {
    /// file を読む。**無い**（NotFound）だけが [`Self::Absent`] で、権限・dir・UTF-8 でない等は
    /// [`Self::Unreadable`]（無いに潰さない・NFR4）。
    pub fn read(path: &Path) -> Self {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Self::Absent,
            Err(err) => {
                return Self::Unreadable(vec![on_host(RuleError::new(
                    0,
                    format!("{} を読めない: {err}", path.display()),
                ))])
            }
        };
        match finish(collect(&text, Face::Host)) {
            Ok(face) => Self::Present(face),
            Err(errors) => Self::Unreadable(errors.into_iter().map(on_host).collect()),
        }
    }

    /// doctor の `host-manifest=` の値。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::Present(_) => "present",
            Self::Unreadable(_) => "unreadable",
        }
    }

    /// tracked の面の label 列（宣言順）に host の面の label を足す（面をまたぐ重複は拒む）。
    ///
    /// 呼び手が tracked の面を label 列でしか持たない周（`seat tick` は cli が開いた manifest の label を受け取る）の
    /// 口で、[`Manifest::joined`] と同じ規則で合わせる。
    pub fn labels_over(self, tracked: &[String]) -> Result<Vec<String>, Vec<RuleError>> {
        let face = match self {
            Self::Absent => return Ok(tracked.to_vec()),
            Self::Unreadable(errors) => return Err(errors),
            Self::Present(face) => face,
        };
        let mut errors = crossed(&face, |label| tracked.iter().any(|found| found == label));
        errors.extend(unknown_candidates(&face, |label| {
            tracked.iter().any(|found| found == label) || face.accounts.iter().any(|found| found.label == label)
        }));
        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(tracked.iter().cloned().chain(face.accounts.into_iter().map(|account| account.label)).collect())
    }
}

/// 雛形の host の面の本文から `[[account-group]]` の表を除いた写し（`init` の 2 段目・host-init.md §4 の 2）。他の表と
/// 注釈は 1 字も変えない（群は `--group` だけが足す）。
pub fn without_groups(text: &str) -> String {
    let mut inside = false;
    let mut kept = String::new();
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            inside = trimmed == Section::AccountGroup.header();
        }
        if !inside {
            kept.push_str(line);
        }
    }
    kept
}

/// 群 `name` の `anchors` に `anchor` を足した本文と、足した後の群の表の本文（`init` の 6 段目・host-init.md §4 の 6）。
///
/// 群の表が無い・`anchors` の行が 1 行の配列として読めない周は `None`。既に在れば本文は不変（呼び手が比べる）。
pub fn with_anchor(text: &str, name: &str, anchor: &str) -> Option<(String, String)> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let named = |line: &&str| line.split_once('=').is_some_and(|(key, value)| key.trim() == "name" && scalar(value.trim()) == Some(Scalar::Str(name.to_owned())));
    let starts: Vec<usize> = lines.iter().enumerate().filter(|(_, line)| line.trim().starts_with('[')).map(|(at, _)| at).collect();
    let (start, end) = starts.iter().enumerate().find_map(|(index, &start)| {
        let end = starts.get(index + 1).copied().unwrap_or(lines.len());
        let block = lines.get(start..end)?;
        (lines.get(start)?.trim() == Section::AccountGroup.header() && block.iter().any(named)).then_some((start, end))
    })?;
    let mut edited: Vec<String> = lines.iter().map(|line| (*line).to_owned()).collect();
    let at = (start..end).find(|&at| lines.get(at).and_then(|line| line.split_once('=')).is_some_and(|(key, _)| key.trim() == "anchors"))?;
    let mut anchors = list(lines.get(at)?.split_once('=')?.1.trim()).ok()?;
    if !anchors.iter().any(|found| found == anchor) {
        anchors.push(anchor.to_owned());
        let quoted: Vec<String> = anchors.iter().map(|found| format!("\"{found}\"")).collect();
        *edited.get_mut(at)? = format!("anchors = [{}]\n", quoted.join(", "));
    }
    Some((edited.concat(), edited.get(start..end)?.concat()))
}

/// 欠陥 1 件に host の面の接頭辞を付ける（どの file の行番号かを行の中で名指す）。
fn on_host(error: RuleError) -> RuleError {
    RuleError::new(error.line, format!("{HOST_MANIFEST}: {}", error.message))
}

/// host の面の label のうち、tracked の面にも在るもの（`tracked` が真を返す label）を 1 件ずつ拒む。
///
/// 重複を拒む理由は面の中の重複と同じ（同じ口座を 2 度読んで同じ枠へ 2 行書く形を塞ぐ）。
fn crossed(face: &Manifest, tracked: impl Fn(&str) -> bool) -> Vec<RuleError> {
    face.accounts
        .iter()
        .filter(|account| tracked(&account.label))
        .map(|account| {
            on_host(RuleError::new(
                account.line,
                format!("label {} が面をまたいで重複する（tracked の manifest にも在る）", account.label),
            ))
        })
        .collect()
}

impl Manifest {
    /// binary に埋め込んだ manifest を読む。
    pub fn embedded() -> Result<Self, Vec<RuleError>> {
        Self::parse(EMBEDDED)
    }

    /// tracked の面（`self`）に host の面（`host` の file）を合わせる（設計 account-lifecycle.md §2）。
    ///
    /// file が**無い**周はそのまま返す（0 宣言）。**在るが読めない・壊れている**周は欠陥の全件で `Err`
    /// （`host.toml:` の接頭辞・行番号付き）。`[[rule]]` の混入・面をまたぐ label の重複も拒む。
    pub fn with_host(self, host: &Path) -> Result<Self, Vec<RuleError>> {
        self.joined(HostManifest::read(host))
    }

    /// 読み済みの host の面を合わせる（[`Self::with_host`] の本体・doctor は読みの 3 値を先に取ってから渡す）。
    pub fn joined(mut self, host: HostManifest) -> Result<Self, Vec<RuleError>> {
        let face = match host {
            HostManifest::Absent => return Ok(self),
            HostManifest::Unreadable(errors) => return Err(errors),
            HostManifest::Present(face) => face,
        };
        let mut errors = crossed(&face, |label| self.accounts.iter().any(|found| found.label == label));
        // `[[vessel]]` は最大 1 行（面をまたいでも同じ）。
        if let (Some(_), Some(host)) = (&self.vessel, &face.vessel) {
            errors.push(on_host(RuleError::new(host.line, format!("{} が面をまたいで重複する（最大 1 行）", Section::Vessel.header()))));
        }
        // 群の候補は**合わせた**口座の表（tracked + host）に在ること（設計 account-lifecycle.md §17 の約束 3）。
        errors.extend(unknown_candidates(&face, |label| {
            self.accounts.iter().any(|found| found.label == label) || face.accounts.iter().any(|found| found.label == label)
        }));
        if !errors.is_empty() {
            return Err(errors);
        }
        self.accounts.extend(face.accounts);
        self.plugins.extend(face.plugins);
        self.launch_args.extend(face.launch_args);
        self.groups.extend(face.groups);
        self.vessel = self.vessel.take().or(face.vessel);
        // `[[tick]]` は host の面にだけ在る（tracked の面は `collect` が断る）＝面をまたぐ重複は起きない。
        self.tick = face.tick;
        Ok(self)
    }

    /// file から読む（`--rules PATH` の override）。
    pub fn load(path: &Path) -> Result<Self, Vec<RuleError>> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(err) => Err(vec![RuleError::new(
                0,
                format!("{} を読めない: {err}", path.display()),
            )]),
        }
    }

    /// 本文を読む。違反は全件集めて返す。
    pub fn parse(text: &str) -> Result<Self, Vec<RuleError>> {
        finish(collect(text, Face::Tracked))
    }

    /// 行 id で引く。
    pub fn get(&self, id: &str) -> Option<&RuleRow> {
        self.rows.iter().find(|row| row.id == id)
    }

    /// 全行。
    pub fn rows(&self) -> &[RuleRow] {
        &self.rows
    }

    /// 宣言した口座の label を**宣言順**で返す（設計 fleet-usage.md §2）。
    pub fn accounts(&self) -> &[AccountLabel] {
        &self.accounts
    }

    /// 宣言した plugin dir を**宣言順**で返す（host の面・設計 account-lifecycle.md §2）。
    pub fn plugins(&self) -> &[PluginDir] {
        &self.plugins
    }

    /// 宣言した起動引数を**宣言順**で返す（host の面・設計 account-lifecycle.md §2）。
    pub fn launch_args(&self) -> &[LaunchArg] {
        &self.launch_args
    }

    /// 宣言した器自身の checkout（host の面・最大 1 行・無ければ `None`＝doctor は `head=undeclared`・設計 consumer-sync.md §4）。
    pub fn vessel(&self) -> Option<&VesselRepo> {
        self.vessel.as_ref()
    }

    /// 宣言した群を**宣言順**で返す（host の面・群を宣言しない host は空・設計 account-lifecycle.md §17）。
    pub fn groups(&self) -> &[AccountGroup] {
        &self.groups
    }

    /// 宣言した tick の unit の置き場と binary（host の面・最大 1 行・無ければ `None`＝席の起動は unit を入れず行も変えない・
    /// 設計 seat-heartbeat.md §5 形 1）。
    pub fn tick(&self) -> Option<&TickUnit> {
        self.tick.as_deref()
    }
}

/// 本文を面の規則で読み、組めた宣言と欠陥の全件を返す（`parse` と host の面の共通の本体）。
fn collect(text: &str, face: Face) -> (Manifest, Vec<RuleError>) {
    let mut errors = Vec::new();
    let (schema, raws) = scan(text, &mut errors);
    check_schema(schema, &mut errors);
    let mut found = Manifest::default();
    let (mut vessel_rows, mut tick_rows) = (0_usize, 0_usize);
    for raw in &raws {
        match (raw.section, face) {
            (Section::Contract, Face::Table) => found.contracts.extend(build_table(raw, &mut errors)),
            // 置けない表は中身を検査しない（1 表 1 件）。規則の面と契約表を混ぜない。
            (Section::Contract, _) => errors.push(RuleError::new(
                raw.line,
                format!("{} は rules manifest に置けない（契約表は設計 doc の区間だけ）", Section::Contract.header()),
            )),
            (_, Face::Table) => errors.push(RuleError::new(
                raw.line,
                format!("{} は契約表に置けない（契約表は {} だけ）", raw.section.header(), Section::Contract.header()),
            )),
            (Section::Rule, Face::Tracked) => found.rows.extend(build_row(raw, &mut errors)),
            // 行の中身は検査しない（置けない表の欠陥を重ねて報告しない＝1 表 1 件）。
            (Section::Rule, Face::Host) => errors.push(RuleError::new(
                raw.line,
                format!("{} は host の面に置けない（規則の行は tracked の manifest だけ）", Section::Rule.header()),
            )),
            (Section::AccountGroup, Face::Host) => found.groups.extend(build_group(raw, &mut errors)),
            // **host の面にだけ在る表**（`[[rule]]` の対称形・設計 account-lifecycle.md §17 の約束 2）。行の中身は
            // 検査しない（置けない表の欠陥を重ねて報告しない＝1 表 1 件）。
            (Section::AccountGroup, Face::Tracked) => errors.push(host_only(raw, "群の宣言は host の面だけ")),
            // unit の置き場と binary は host 固有の path（tracked の面は PUBLIC repo に載る＝CON2）。行の中身は検査しない（1 表 1 件）。
            (Section::Tick, Face::Tracked) => errors.push(host_only(raw, "unit の置き場は host の面だけ")),
            // 2 行目以降は重複として拒む（`[[vessel]]` と同じ形・1 行目の欠陥は build_tick が別件で報告する）。
            (Section::Tick, Face::Host) if tick_rows > 0 => errors.push(RuleError::new(
                raw.line,
                format!("{} が重複する（最大 1 行）", Section::Tick.header()),
            )),
            (Section::Tick, Face::Host) => {
                tick_rows = tick_rows.saturating_add(1);
                found.tick = build_tick(raw, &mut errors).map(Box::new);
            }
            (Section::Account, _) => found.accounts.extend(
                build_single(raw, "label", &mut errors).map(|(label, line)| AccountLabel { label, line }),
            ),
            (Section::Plugin, _) => found
                .plugins
                .extend(build_single(raw, "dir", &mut errors).map(|(dir, line)| PluginDir { dir, line })),
            (Section::LaunchArg, _) => found.launch_args.extend(
                build_single(raw, "value", &mut errors).map(|(value, line)| LaunchArg { value, line }),
            ),
            // 2 行目以降は重複として拒む（行番号付き・1 行目の欠陥は build_single が別件で報告する）。
            (Section::Vessel, _) if vessel_rows > 0 => errors.push(RuleError::new(
                raw.line,
                format!("{} が重複する（最大 1 行）", Section::Vessel.header()),
            )),
            (Section::Vessel, _) => {
                vessel_rows = vessel_rows.saturating_add(1);
                found.vessel = build_single(raw, "repo", &mut errors).map(|(repo, line)| VesselRepo { repo, line });
            }
        }
    }
    check_duplicate_ids(&found.rows, &mut errors);
    check_class_commands(&found, &mut errors);
    check_duplicate_labels(&found.accounts, &mut errors);
    check_duplicate_groups(&found.groups, &mut errors);
    seed_groups(&mut found.groups, &mut errors);
    (found, errors)
}

/// host の面にだけ在る表（`[[account-group]]` / `[[tick]]`）を tracked の面に置いた周の 1 件（見出しの行番号・`why` は置き場の理由）。
fn host_only(raw: &RawRow, why: &str) -> RuleError {
    RuleError::new(raw.line, format!("{} は tracked の manifest に置けない（{why}）", raw.section.header()))
}

/// 欠陥が 0 件なら宣言を、在れば行番号の順に並べた欠陥の全件を返す。
fn finish((found, mut errors): (Manifest, Vec<RuleError>)) -> Result<Manifest, Vec<RuleError>> {
    if errors.is_empty() {
        Ok(found)
    } else {
        errors.sort_by_key(|error| error.line);
        Err(errors)
    }
}

/// 本文を走査して top-level の `schema` と各 section の生 field を集める。
fn scan(text: &str, errors: &mut Vec<RuleError>) -> (Option<(u64, Scalar)>, Vec<RawRow>) {
    let mut schema = None;
    let mut raws: Vec<RawRow> = Vec::new();
    for (index, raw_line) in text.lines().enumerate() {
        let line = index as u64 + 1;
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with('[') {
            match Section::parse(trimmed) {
                Some(section) => raws.push(RawRow {
                    section,
                    line,
                    fields: Vec::new(),
                }),
                None => {
                    let taken: Vec<&str> = SECTIONS.iter().map(|found| found.header()).collect();
                    errors.push(RuleError::new(
                        line,
                        format!("未知の section {trimmed}（受理するのは {} だけ）", taken.join(" / ")),
                    ));
                }
            }
            continue;
        }
        scan_pair(trimmed, line, &mut schema, raws.last_mut(), errors);
    }
    (schema, raws)
}

/// `key = value` 1 行を、直前の section に応じて振り分ける。
fn scan_pair(
    trimmed: &str,
    line: u64,
    schema: &mut Option<(u64, Scalar)>,
    current: Option<&mut RawRow>,
    errors: &mut Vec<RuleError>,
) {
    let Some((key, raw_value)) = trimmed.split_once('=') else {
        errors.push(RuleError::new(line, format!("key = value の形でない: {trimmed}")));
        return;
    };
    let key = key.trim().to_owned();
    let raw = raw_value.trim();
    let value = if raw.starts_with('[') {
        match list(raw) {
            Ok(items) => RawValue::List(items),
            Err(reason) => {
                errors.push(RuleError::new(line, format!("{key} の {reason}")));
                RawValue::Broken
            }
        }
    } else {
        match scalar(raw) {
            Some(found) => RawValue::One(found),
            None => {
                errors.push(RuleError::new(
                    line,
                    format!(
                        "{key} の value が TOML subset の形でない（string / integer / bool / 文字列の配列のみ）"
                    ),
                ));
                RawValue::Broken
            }
        }
    };
    match current {
        Some(row) => row.fields.push((key, value, line)),
        None if key == "schema" && schema.is_some() => {
            errors.push(RuleError::new(line, "schema が重複する".to_owned()));
        }
        None if key == "schema" => match value {
            RawValue::One(found) => *schema = Some((line, found)),
            RawValue::List(_) => {
                errors.push(RuleError::new(line, "schema は配列でない".to_owned()));
            }
            // 読めなかった値は scan が報告済み（`schema` は未設定のまま＝
            // `check_schema` が「無い」と言う。値の欠陥と schema の不在は別件である）。
            RawValue::Broken => {}
        },
        None => errors.push(RuleError::new(
            line,
            format!("section の外に未知の key {key} が在る"),
        )),
    }
}

/// TOML の**文字列の配列**を要素へ切る（1 行で閉じること）。
///
/// **空の配列は受けない**——「規則が無い」を空 array で表せてしまうと、書き間違いの
/// `value = []` が allowlist を空のまま効かせる（空の口は「成功」に化ける）。
/// 要素は [`scalar`] で読み、引用符 1 組ちょうどでなければ受けない
/// （`["a" "b"]` の区切り忘れを 1 本の壊れた文字列として黙って通さない・NFR4）。
/// **空文字の要素も受けない**——command 名や verify 行として空を通すと、何もしない口が
/// 規則の顔で並ぶ。
pub fn list(raw: &str) -> Result<Vec<String>, String> {
    if !raw.ends_with(']') {
        return Err("配列が同じ行で閉じていない（要素に改行は置けない）".to_owned());
    }
    let Some(parts) = elements(raw) else {
        return Err("配列の引用符が閉じていない".to_owned());
    };
    if parts.is_empty() {
        return Err("配列が空である（規則が無いことを空の配列で表さない）".to_owned());
    }
    let mut items = Vec::new();
    for part in parts {
        let trimmed = part.trim();
        match scalar(trimmed).filter(|_| quoted_once(trimmed)) {
            Some(Scalar::Str(text)) if !text.is_empty() => items.push(text),
            Some(Scalar::Str(_)) => return Err(format!("配列の要素が空文字である: {trimmed}")),
            _ => return Err(format!("配列の要素が引用符 1 組の文字列でない: {trimmed}")),
        }
    }
    Ok(items)
}

/// 要素が引用符 1 組ちょうどか（中に裸の `"` を含まない）。
///
/// **配列を読む面はここと [`elements`] の 1 組だけ**である（`pipe` の契約 file も
/// これを呼ぶ）。
pub fn quoted_once(text: &str) -> bool {
    text.trim()
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .is_some_and(|body| !body.contains('"'))
}

/// `["a", "b"]` を要素へ切る。要素の中の `,` は quote の内側として扱う。
pub fn elements(raw: &str) -> Option<Vec<String>> {
    let body = raw.strip_prefix('[')?.strip_suffix(']')?;
    let mut parts: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut inside = false;
    for ch in body.chars() {
        match ch {
            '"' => {
                inside = !inside;
                current.push(ch);
            }
            ',' if !inside => parts.push(std::mem::take(&mut current)),
            _ => current.push(ch),
        }
    }
    if inside {
        return None;
    }
    parts.push(current);
    if parts.last().is_some_and(|last| last.trim().is_empty()) {
        parts.pop();
    }
    Some(parts)
}

/// 値 1 つを読む。受理するのは `"..."` / 整数 / `true` / `false` だけ。
pub fn scalar(raw: &str) -> Option<Scalar> {
    if let Some(rest) = raw.strip_prefix('"') {
        return rest.strip_suffix('"').map(|text| Scalar::Str(text.to_owned()));
    }
    match raw {
        "true" => return Some(Scalar::Bool(true)),
        "false" => return Some(Scalar::Bool(false)),
        _ => {}
    }
    let digits: String = raw.chars().filter(|c| *c != '_').collect();
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    digits.parse::<u64>().ok().map(Scalar::Int)
}

/// `schema = 1` が在ることを確かめる。
fn check_schema(schema: Option<(u64, Scalar)>, errors: &mut Vec<RuleError>) {
    match schema {
        Some((_, Scalar::Int(found))) if found == SCHEMA => {}
        Some((line, found)) => errors.push(RuleError::new(
            line,
            format!("schema が {SCHEMA} でない（実 {found:?}）"),
        )),
        None => errors.push(RuleError::new(0, format!("schema = {SCHEMA} が無い"))),
    }
}

/// 生 field から 1 行を組む。欠けや未知 key は全件 `errors` へ積む。
fn build_row(raw: &RawRow, errors: &mut Vec<RuleError>) -> Option<RuleRow> {
    let before = errors.len();
    check_keys(raw, errors);
    // **読めなかった値が 1 つでもあれば、この行はここで打ち切る**。scan が既に
    // 「value が TOML subset の形でない」を 1 件報告しており、続けると同じ欠陥が
    // 「必須 key が無い」「ruling / ruled_at が無い」「id が空である」という
    // **事実でない 2 行目**に化ける（key は在って値が壊れている）。
    if raw
        .fields
        .iter()
        .any(|(_, value, _)| matches!(value, RawValue::Broken))
    {
        return None;
    }
    let id = text_field(raw, "id", errors).unwrap_or_default();
    if id.is_empty() && raw.fields.iter().any(|(key, _, _)| key == "id") {
        errors.push(RuleError::new(raw.line, "id が空である".to_owned()));
    }
    let kind = kind_field(raw, &id, errors);
    // **既定で埋めない**（裁定 `user 2026-09-11T23:59Z`）。欠落は [`check_keys`] が
    // 「必須 key が無い」で 1 件報告済みで、ここで `true` や空文字を代わりに置くと、
    // その行は**書かれていない値**を持ったまま先へ進む。
    let enabled = bool_field(raw, "enabled", errors);
    let ruling = text_field(raw, "ruling", errors);
    let ruled_at = text_field(raw, "ruled_at", errors);
    let value = kind.and_then(|found| value_field(raw, found, &id, errors));
    let (Some(kind), Some(value), Some(enabled), Some(ruling), Some(ruled_at)) =
        (kind, value, enabled, ruling, ruled_at)
    else {
        return None;
    };
    let row = RuleRow {
        id,
        kind,
        value,
        enabled,
        ruling,
        ruled_at,
        line: raw.line,
    };
    if errors.len() > before {
        // key の欠けや未知 key は既に 1 件として報告済みである。ここで validate を
        // 重ねると同じ欠陥が 2 行になるので、報告済みの行はここで打ち切る。
        return None;
    }
    if let Err(error) = row.validate() {
        errors.push(error);
        return None;
    }
    Some(row)
}

/// 文字列の key 1 つだけを持つ表（`[[account]]` の label・`[[plugin]]` の dir・`[[launch-arg]]` の value）1 つ分から
/// 値と見出し行を組む。欠けや未知 key は全件 `errors` へ積む。
///
/// `[[rule]]` と同じ形で**打ち切る**（読めなかった値は scan が 1 件報告済み・key の欠けは
/// [`check_keys`] が 1 件報告済み）——同じ欠陥を 2 行にしないためである。
fn build_single(raw: &RawRow, key: &str, errors: &mut Vec<RuleError>) -> Option<(String, u64)> {
    let before = errors.len();
    check_keys(raw, errors);
    if raw
        .fields
        .iter()
        .any(|(_, value, _)| matches!(value, RawValue::Broken))
    {
        return None;
    }
    let value = text_field(raw, key, errors)?;
    if errors.len() > before {
        return None;
    }
    // **空の値は受けない**（`[[account]]` が 1 件在ることと、その口座を名指せることは
    // 別である。空を通すと credential の置き場が `accounts/` そのものに解けてしまう。
    // plugin dir と起動引数も同じく、空は何もしない口が宣言の顔で並ぶ）。
    if value.is_empty() {
        errors.push(RuleError::new(raw.line, format!("{key} が空である")));
        return None;
    }
    Some((value, raw.line))
}

/// `[[account-group]]` 1 行を組む（設計 account-lifecycle.md §17 の約束 1）。欠けや未知 key は全件 `errors` へ積み、
/// [`build_single`] と同じ形で打ち切る（読めなかった値は scan が 1 件報告済み——**空の列もそこで 1 件になる**）。
///
/// 名が空なら拒む（`[[account]]` の label と同じ理由: 群が 1 件在ることと、その群を名指せることは別である）。
/// 群をまたぐ検査（名の重複・置き場の重複）は [`check_duplicate_groups`]、候補の label が宣言された口座に在るかの
/// 検査は面を合わせる側（[`unknown_candidates`]）が持つ。
fn build_group(raw: &RawRow, errors: &mut Vec<RuleError>) -> Option<AccountGroup> {
    let before = errors.len();
    check_keys(raw, errors);
    if raw
        .fields
        .iter()
        .any(|(_, value, _)| matches!(value, RawValue::Broken))
    {
        return None;
    }
    let name = text_field(raw, "name", errors);
    let anchors = list_field(raw, "anchors", errors);
    let accounts = list_field(raw, "accounts", errors);
    let (Some(name), Some(anchors), Some(accounts)) = (name, anchors, accounts) else {
        return None;
    };
    if errors.len() > before {
        return None;
    }
    if name.is_empty() {
        errors.push(RuleError::new(raw.line, "name が空である".to_owned()));
        return None;
    }
    Some(AccountGroup { name, anchors, accounts, seed: String::new(), line: raw.line })
}

/// `[[tick]]` 1 行を組む（設計 seat-heartbeat.md §5 形 1）。欠けや未知 key は全件 `errors` へ積み、[`build_single`] と同じ形で
/// 打ち切る。2 欄とも**絶対 path**でなければ、その key の行番号で 1 件ずつ拒む（空の字面も相対に数える）。
fn build_tick(raw: &RawRow, errors: &mut Vec<RuleError>) -> Option<TickUnit> {
    let before = errors.len();
    check_keys(raw, errors);
    if raw
        .fields
        .iter()
        .any(|(_, value, _)| matches!(value, RawValue::Broken))
    {
        return None;
    }
    let unit_dir = text_field(raw, "unit-dir", errors);
    let binary = text_field(raw, "binary", errors);
    let (Some(unit_dir), Some(binary)) = (unit_dir, binary) else {
        return None;
    };
    if errors.len() > before {
        return None;
    }
    for (key, value) in [("unit-dir", &unit_dir), ("binary", &binary)] {
        if !Path::new(value).is_absolute() {
            let line = raw.fields.iter().find(|(found, _, _)| found == key).map_or(raw.line, |(_, _, line)| *line);
            errors.push(RuleError::new(line, format!("{key} が絶対 path でない: {value:?}")));
        }
    }
    (errors.len() == before).then_some(TickUnit { unit_dir, binary, line: raw.line })
}

/// `[[contract]]` 1 行を組む。欠けや未知 key は全件 `errors` へ積み、[`build_row`] と同じ形で打ち切る
/// （読めなかった値は scan が 1 件報告済み）。値の形（文字列か配列か）の検査は欄の形を持つ `pipe::table` が行う。
fn build_table(raw: &RawRow, errors: &mut Vec<RuleError>) -> Option<TableRow> {
    let before = errors.len();
    check_keys(raw, errors);
    let mut fields = Vec::new();
    for (key, value, line) in &raw.fields {
        let kept = match value {
            RawValue::One(found) => TableValue::One(found.clone()),
            RawValue::List(items) => TableValue::List(items.clone()),
            RawValue::Broken => return None,
        };
        fields.push((key.clone(), kept, *line));
    }
    (errors.len() == before).then_some(TableRow { line: raw.line, fields })
}

/// label の重複を集める。
///
/// 重複を拒むのは、同じ口座を 2 度読んで同じ枠へ 2 行書く形（`fleet usage` が同じ key の
/// event を重ねる）を塞ぐためであり、行 id の重複と同じ理由である。
fn check_duplicate_labels(accounts: &[AccountLabel], errors: &mut Vec<RuleError>) {
    for (index, account) in accounts.iter().enumerate() {
        let seen = accounts
            .iter()
            .take(index)
            .any(|earlier| earlier.label == account.label);
        if seen {
            errors.push(RuleError::new(
                account.line,
                format!("label {} が重複する", account.label),
            ));
        }
    }
}

/// 群の名の重複と、2 つの群に在る置き場を集める（設計 account-lifecycle.md §17 の約束 3）。
///
/// 名の重複を拒むのは行 id / label と同じ理由（同じ名の群が 2 つ在ると doctor の行も除外の出所も引けない）。
/// 置き場の重複を拒むのは、1 つの置き場の席が 2 つの群に属すと「その席の口座を持つ単位」が決まらないためである。
fn check_duplicate_groups(groups: &[AccountGroup], errors: &mut Vec<RuleError>) {
    for (index, group) in groups.iter().enumerate() {
        if groups.iter().take(index).any(|found| found.name == group.name) {
            errors.push(RuleError::new(group.line, format!("群の名 {} が重複する", group.name)));
        }
        for (at, anchor) in group.anchors.iter().enumerate() {
            // 前の群に在る／同じ群の前の要素に在る、のどちらも「2 度目」である（同じ置き場を 2 度書いた行も、
            // その置き場の席がどの宣言に従うかを決められない点で同じ欠陥である）。
            let seen = groups.iter().take(index).any(|found| found.anchors.contains(anchor))
                || group.anchors.iter().take(at).any(|found| found == anchor);
            if seen {
                errors.push(RuleError::new(group.line, format!("置き場 {anchor} が 2 つの群に在る")));
            }
        }
    }
}

/// 各群の種を宣言順に埋める（設計 account-lifecycle.md §28 形 1）: 前の群の種でない最初の候補。全候補が前の群の種に
/// 使われている群は群の見出し行の欠陥（fail-closed・同じ候補の列を宣言した群どうしが初期状態で同じ口座に乗らない）。
fn seed_groups(groups: &mut [AccountGroup], errors: &mut Vec<RuleError>) {
    let mut taken: Vec<String> = Vec::new();
    for group in groups.iter_mut() {
        match group.accounts.iter().find(|label| !taken.contains(label)) {
            Some(seed) => group.seed.clone_from(seed),
            None => errors.push(RuleError::new(group.line, format!("群 {} の種を決める候補が無い", group.name))),
        }
        taken.push(group.seed.clone());
    }
}

/// 群の候補のうち、合わせた面の口座の表（`known` が真を返す label）に無いものを 1 件ずつ拒む
/// （設計 account-lifecycle.md §17 の約束 3・**面を合わせてからの検査**なので、面の中の欠陥で止まった周は届かない）。
fn unknown_candidates(face: &Manifest, known: impl Fn(&str) -> bool) -> Vec<RuleError> {
    face.groups
        .iter()
        .flat_map(|group| {
            group
                .accounts
                .iter()
                .filter(|label| !known(label))
                .map(|label| {
                    on_host(RuleError::new(
                        group.line,
                        format!("群 {} の候補 {label} が宣言された口座に無い", group.name),
                    ))
                })
                .collect::<Vec<RuleError>>()
        })
        .collect()
}

/// 未知 key・重複 key・必須 key の欠落を集める。
///
/// 重複を拒むのは、同じ key を 2 度書いたとき先勝ちで後の行が**黙って消える**のを
/// 塞ぐためである（SRS AC6「黙って落とす入力 0 件」・NFR4）。とりわけ
/// `enabled = true` の次に `enabled = false` を書くと、不発効の行が有効なまま
/// 機械に読まれてしまう。
fn check_keys(raw: &RawRow, errors: &mut Vec<RuleError>) {
    let known = raw.section.known_keys();
    for (index, (key, _, line)) in raw.fields.iter().enumerate() {
        if !known.contains(&key.as_str()) {
            errors.push(RuleError::new(*line, format!("未知の key {key}")));
        }
        if raw
            .fields
            .iter()
            .take(index)
            .any(|(earlier, _, _)| earlier == key)
        {
            errors.push(RuleError::new(*line, format!("key {key} が重複する")));
        }
    }
    for want in raw.section.required_keys() {
        if !raw.fields.iter().any(|(key, _, _)| key == want) {
            errors.push(RuleError::new(raw.line, format!("必須 key {want} が無い")));
        }
    }
}

/// 文字列 field を取り出す。型違いは error にする。
fn text_field(raw: &RawRow, key: &str, errors: &mut Vec<RuleError>) -> Option<String> {
    let (_, value, line) = raw.fields.iter().find(|(found, _, _)| found == key)?;
    match value {
        RawValue::One(Scalar::Str(text)) => Some(text.clone()),
        other => {
            errors.push(RuleError::new(
                *line,
                format!("{key} は文字列でなければならない（実 {other:?}）"),
            ));
            None
        }
    }
}

/// 文字列の配列の field を取り出す。型違いは error にする（**空の配列は [`list`] が scan の時点で断る**ので、
/// ここへ届く列は 1 要素以上である）。
fn list_field(raw: &RawRow, key: &str, errors: &mut Vec<RuleError>) -> Option<Vec<String>> {
    let (_, value, line) = raw.fields.iter().find(|(found, _, _)| found == key)?;
    match value {
        RawValue::List(items) => Some(items.clone()),
        other => {
            errors.push(RuleError::new(
                *line,
                format!("{key} は文字列の配列でなければならない（実 {other:?}）"),
            ));
            None
        }
    }
}

/// bool field を取り出す。型違いは error にする。
fn bool_field(raw: &RawRow, key: &str, errors: &mut Vec<RuleError>) -> Option<bool> {
    let (_, value, line) = raw.fields.iter().find(|(found, _, _)| found == key)?;
    match value {
        RawValue::One(Scalar::Bool(found)) => Some(*found),
        other => {
            errors.push(RuleError::new(
                *line,
                format!("{key} は bool でなければならない（実 {other:?}）"),
            ));
            None
        }
    }
}

/// `kind` を種類へ引く。未知の字面は error にする。
fn kind_field(raw: &RawRow, id: &str, errors: &mut Vec<RuleError>) -> Option<RuleKind> {
    let text = text_field(raw, "kind", errors)?;
    match RuleKind::parse(&text) {
        Some(kind) => Some(kind),
        None => {
            errors.push(RuleError::new(
                raw.line,
                format!("{id} の kind {text} は未知である"),
            ));
            None
        }
    }
}

/// `value` を種類に応じた値へ写す。bool は value に置けない。
fn value_field(
    raw: &RawRow,
    kind: RuleKind,
    id: &str,
    errors: &mut Vec<RuleError>,
) -> Option<RuleValue> {
    let (_, scalar, line) = raw.fields.iter().find(|(key, _, _)| key == "value")?;
    match scalar {
        // **形の照合はここでしない**（`RuleRow::validate` の 1 箇所が持つ）。ここで
        // 弾くと、kind と value の対応を測る面が 2 つになる。
        RawValue::List(items) => Some(RuleValue::List(items.clone())),
        RawValue::One(Scalar::Int(found)) => Some(RuleValue::Int(*found)),
        RawValue::One(Scalar::Str(text)) => Some(match kind.shape() {
            ValueShape::Policy => RuleValue::Policy(text.clone()),
            ValueShape::Int | ValueShape::Str | ValueShape::List => RuleValue::Str(text.clone()),
        }),
        RawValue::One(Scalar::Bool(_)) => {
            errors.push(RuleError::new(
                *line,
                format!("{id} の value は integer か string でなければならない"),
            ));
            None
        }
        // 網羅のための枝。**到達しない**——読めなかった値を持つ行は
        // [`build_row`] が先に打ち切る（Broken を見る場所は 1 か所である）。
        RawValue::Broken => None,
    }
}

/// 行 id の重複を集める。
fn check_duplicate_ids(rows: &[RuleRow], errors: &mut Vec<RuleError>) {
    for (index, row) in rows.iter().enumerate() {
        let seen = rows
            .iter()
            .take(index)
            .any(|earlier| earlier.id == row.id);
        if seen {
            errors.push(RuleError::new(row.line, format!("id {} が重複する", row.id)));
        }
    }
}

/// クラスの語列表の要素の行を跨ぐ 2 つの崩れ（設計 contract-source.md §48 の 3 の (c)(d)）を 1 件ずつ行番号つきで断る:
/// (c) 語列の先頭語が上限の行（[`CEILING_ROW`]）の値に無い (d) 語列が受付の読む禁じる語列の和集合（`runner.denied_commands`
/// と host の見張りの語列 3 行・読み手は command guard と共有の [`denied_of`] の 1 本）のどれかを含む（語列を 1 本の command と
/// 見て [`denied_in`] に当てる）。どちらも受付が先に断る要素ゆえ当たる行の無い死に値になる。上限の行か和集合の 4 行が揃わない
/// manifest では撃たない（契約表の検査を撃つ各経路は自分が読む行の欠けを既に断る）。要素の形の (a)(b) は行 1 つで決まるので
/// 行の validate が断り、その行はここに届かない。
fn check_class_commands(found: &Manifest, errors: &mut Vec<RuleError>) {
    let Some(RuleValue::List(allowed)) = found.get(CEILING_ROW).filter(|row| row.enabled).map(|row| &row.value) else {
        return;
    };
    let Some(sources) = denied_of(found) else {
        return;
    };
    let denied: Vec<String> = sources.into_iter().flat_map(|(_, sequences)| sequences).collect();
    for row in found.rows.iter().filter(|row| row.kind == RuleKind::RunnerClassCommands) {
        let RuleValue::List(ref elements) = row.value else {
            continue;
        };
        for element in elements {
            let ClassElement::Pair(_, sequence) = class_element(element) else {
                continue;
            };
            let head = sequence.split_whitespace().next().unwrap_or_default();
            if !allowed.iter().any(|command| command == head) {
                let reason = format!("語列の先頭語 {head} が {CEILING_ROW} の値に無い");
                errors.push(RuleError::new(row.line, format!("{} の value の要素 {element:?} の{reason}", row.id)));
            }
            if let Some(hit) = denied_in(&sequence, &denied) {
                let reason = format!("語列が禁じる語列 {} を含む（受付が先に断る死に値）", hit.sequence);
                errors.push(RuleError::new(row.line, format!("{} の value の要素 {element:?} の{reason}", row.id)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    // flip-check: retroactive s2-07l.250
    // host の面の読みの 3 値と `labels_over` の歯（設計 account-lifecycle.md §6 / §7・接頭辞 `rules_host_unit_`）。
    // 現物の挙動を pin する歯なので base でも通る（`.243` run 3 の生存変異を塞ぐ）。

    use super::{collect, finish, Face, HostManifest, Manifest};

    /// host の面の本文を読んだ [`HostManifest::Present`]（`[[account]]` を `labels` の順で持つ・見出し行は 3, 6, …）。
    fn host_of(labels: &[&str]) -> HostManifest {
        let accounts: String = labels.iter().map(|label| format!("\n[[account]]\nlabel = \"{label}\"\n")).collect();
        match finish(collect(&format!("schema = 1\n{accounts}"), Face::Host)) {
            Ok(face) => HostManifest::Present(face),
            Err(errors) => HostManifest::Unreadable(errors),
        }
    }

    /// tracked の面の label 列。
    fn tracked(labels: &[&str]) -> Vec<String> {
        labels.iter().map(|label| (*label).to_owned()).collect()
    }

    /// (a) 在るが読めない（dir を `host.toml` の path に渡す＝権限に依らず読めない）は `Unreadable`（`Absent` に潰さない）。
    #[test]
    fn rules_host_unit_read_of_a_directory_is_unreadable_not_absent() {
        let dir = std::env::temp_dir();
        let read = HostManifest::read(&dir);
        assert_eq!(read.as_str(), "unreadable", "{read:?}");
        let HostManifest::Unreadable(errors) = read else {
            panic!("Unreadable でない");
        };
        assert_eq!(errors.len(), 1, "欠陥 1 件: {errors:?}");
        let first = errors.first().map(ToString::to_string).unwrap_or_default();
        assert!(first.starts_with("rules: host.toml: "), "面の接頭辞: {first}");
        assert!(first.contains(&dir.display().to_string()), "path を名指す: {first}");
        assert!(first.ends_with(" line=0"), "行番号: {first}");
    }

    /// (b) 無い path だけが `Absent`（縮退・0 宣言）。
    #[test]
    fn rules_host_unit_read_of_a_missing_path_is_absent() {
        let missing = std::env::temp_dir().join(format!("scribe2-rules-host-unit-missing-{}", std::process::id())).join("host.toml");
        assert_eq!(HostManifest::read(&missing), HostManifest::Absent);
    }

    /// (c) 重複の無い host の面は tracked の後ろへ宣言順で足す（順序まで）。
    #[test]
    fn rules_host_unit_labels_over_appends_the_host_labels_in_order() {
        assert_eq!(host_of(&["c"]).labels_over(&tracked(&["a", "b"])), Ok(tracked(&["a", "b", "c"])));
    }

    /// (d) 面をまたぐ重複は host の面の行番号で 1 件。tracked が重複の label **だけ**の周も同じく拒む
    /// （重複判定の `==` を `!=` に倒すと、tracked `[a, b]` では `a` が一致しないことで偽の重複が立ち区別がつかない）。
    #[test]
    fn rules_host_unit_labels_over_rejects_a_label_crossing_the_faces() {
        for base in [&["a", "b"][..], &["b"]] {
            let errors = host_of(&["b"]).labels_over(&tracked(base)).expect_err("面をまたぐ重複は拒む");
            assert_eq!(errors.len(), 1, "{base:?}: 1 件: {errors:?}");
            let first = errors.first().map(ToString::to_string).unwrap_or_default();
            assert_eq!(first, "rules: host.toml: label b が面をまたいで重複する（tracked の manifest にも在る） line=3", "{base:?}");
        }
    }

    /// (e) 重複しない label（`d`）は拒まれない＝欠陥はちょうど 1 件。
    #[test]
    fn rules_host_unit_labels_over_rejects_only_the_crossing_label() {
        let errors = host_of(&["b", "d"]).labels_over(&tracked(&["a", "b"])).expect_err("b は重複する");
        let shown: Vec<(u64, String)> = errors.iter().map(|error| (error.line, error.message.clone())).collect();
        assert_eq!(
            shown,
            vec![(3, "host.toml: label b が面をまたいで重複する（tracked の manifest にも在る）".to_owned())],
            "d は拒まない"
        );
    }

    // ─── host の面の `[[tick]]`（設計 seat-heartbeat.md §5 形 1・接頭辞 `host_tick_`） ───

    /// `body` を host の面の規則で読み、欠陥を (行番号, 文言) の列で返す（読めた周は空）。
    fn host_defects(body: &str) -> Vec<(u64, String)> {
        match finish(collect(body, Face::Host)) {
            Ok(_) => Vec::new(),
            Err(errors) => errors.into_iter().map(|error| (error.line, error.message)).collect(),
        }
    }

    /// 1 行の `[[tick]]` は 2 欄と見出し行を運び（round-trip）、tracked の面に合わせても値が残る。表の無い面は `None`。
    #[test]
    fn host_tick_one_row_round_trips_its_two_fields_and_line() {
        let body = "schema = 1\n\n[[account]]\nlabel = \"h1\"\n\n[[tick]]\nunit-dir = \"/u/units\"\nbinary = \"/opt/bin/x\"\n";
        let face = finish(collect(body, Face::Host)).unwrap_or_default();
        let tick = face.tick().map(|found| (found.unit_dir().to_owned(), found.binary().to_owned(), found.line()));
        assert_eq!(tick, Some(("/u/units".to_owned(), "/opt/bin/x".to_owned(), 6)), "2 欄と見出し行");
        let joined = Manifest::default().joined(HostManifest::Present(face.clone())).unwrap_or_default();
        assert_eq!(joined.tick(), face.tick(), "面を合わせても同じ値");
        let bare = finish(collect("schema = 1\n\n[[account]]\nlabel = \"h1\"\n", Face::Host)).unwrap_or_default();
        assert_eq!(bare.tick(), None, "表の無い面は None");
        assert_eq!(Manifest::default().joined(HostManifest::Present(bare)).unwrap_or_default().tick(), None);
    }

    /// 2 行目・相対 path（欄ごと・空の字面も）・欠けた欄・未知 key を行番号つきで 1 件ずつ断り、tracked の面の表は 1 表 1 件で断る。
    #[test]
    fn host_tick_refuses_a_second_row_relative_paths_and_missing_fields_with_line_numbers() {
        let one = "schema = 1\n\n[[tick]]\nunit-dir = \"/u\"\nbinary = \"/b\"\n";
        assert!(host_defects(one).is_empty(), "1 行は読める");
        let cases: [(String, Vec<(u64, &str)>); 6] = [
            (format!("{one}\n[[tick]]\nunit-dir = \"/v\"\nbinary = \"/c\"\n"), vec![(7, "[[tick]] が重複する（最大 1 行）")]),
            ("schema = 1\n\n[[tick]]\nunit-dir = \"units\"\nbinary = \"/b\"\n".to_owned(), vec![(4, "unit-dir が絶対 path でない: \"units\"")]),
            ("schema = 1\n\n[[tick]]\nunit-dir = \"/u\"\nbinary = \"bin/x\"\n".to_owned(), vec![(5, "binary が絶対 path でない: \"bin/x\"")]),
            ("schema = 1\n\n[[tick]]\nunit-dir = \"\"\nbinary = \"\"\n".to_owned(), vec![(4, "unit-dir が絶対 path でない: \"\""), (5, "binary が絶対 path でない: \"\"")]),
            ("schema = 1\n\n[[tick]]\nunit-dir = \"/u\"\n".to_owned(), vec![(3, "必須 key binary が無い")]),
            ("schema = 1\n\n[[tick]]\nunit-dir = \"/u\"\nbinary = \"/b\"\nperiod = 60\n".to_owned(), vec![(6, "未知の key period")]),
        ];
        for (body, want) in cases {
            let want: Vec<(u64, String)> = want.into_iter().map(|(line, text)| (line, text.to_owned())).collect();
            assert_eq!(host_defects(&body), want, "{body}");
        }
        let tracked = finish(collect("schema = 1\n\n[[tick]]\n", Face::Tracked)).map_err(|errors| {
            errors.into_iter().map(|error| (error.line, error.message)).collect::<Vec<(u64, String)>>()
        });
        assert_eq!(
            tracked,
            Err(vec![(3, "[[tick]] は tracked の manifest に置けない（unit の置き場は host の面だけ）".to_owned())]),
            "tracked の面は 1 表 1 件"
        );
    }

    // ─── クラスの語列表の読み（設計 contract-source.md §48 の 3・接頭辞 `class_derive_`） ───

    /// 語列表の行（見出しは 3 行目・値は `value`）と、上限の行・禁じる語列の 4 行のうち `drop` の id でない行を持つ本文。
    fn class_manifest(value: &str, drop: &str) -> String {
        let row = |id: &str, kind: &str, value: &str| {
            format!("\n[[rule]]\nid = \"{id}\"\nkind = \"{kind}\"\nvalue = {value}\nenabled = true\nruling = \"r\"\nruled_at = \"d\"\n")
        };
        let mut text = format!("schema = 1\n{}", row("runner.class_commands", "RunnerClassCommands", value));
        for (id, kind, value) in [
            ("runner.allowed_commands", "RunnerAllowedCommands", "[\"cargo\", \"git\", \"bats\", \"tmux\", \"bd\"]"),
            ("runner.denied_commands", "RunnerDeniedCommands", "[\"cargo mutants\", \"cargo publish\"]"),
            ("host_guard.git", "HostGuardDeniedCommands", "[\"git push --force\", \"git branch -D\"]"),
            ("host_guard.tmux", "HostGuardDeniedCommands", "[\"tmux kill-server\"]"),
            ("host_guard.ledger", "HostGuardDeniedCommands", "[\"bd delete\"]"),
        ] {
            if id != drop {
                text.push_str(&row(id, kind, value));
            }
        }
        text
    }

    /// 崩れ (a)〜(d) を 1 件ずつ語列表の行の見出しの行番号（3）で断る。(d) は `runner.denied_commands` の語列と host の見張りの
    /// 語列 3 行のどれを含む要素でも断る。裁定の 3 要素（禁じる語列 `git push --force` と一部だけ重なる `git push` を含む）は通る。
    #[test]
    fn class_derive_rules_refuse_each_broken_element_once_with_the_row_line() {
        let ruled = "[\"publish git push\", \"delete git push --delete\", \"delete git push -d\"]";
        assert_eq!(Manifest::parse(&class_manifest(ruled, "")).map(|found| found.rows().len()), Ok(6), "裁定の 3 要素は通る");
        for (value, want) in [
            ("[\"publish git push\", \"ship git push\"]", "先頭語 \"ship\" がクラスの名でない"),
            ("[\"publish git push\", \"consume\"]", "クラス consume の語列が空"),
            ("[\"publish sh push\"]", "語列の先頭語 sh が runner.allowed_commands の値に無い"),
            ("[\"publish cargo publish --dry-run\"]", "禁じる語列 cargo publish を含む"),
            ("[\"delete git push origin --force\"]", "禁じる語列 git push --force を含む"),
            ("[\"delete tmux kill-server\"]", "禁じる語列 tmux kill-server を含む"),
            ("[\"delete bd delete x\"]", "禁じる語列 bd delete を含む"),
        ] {
            let errors = Manifest::parse(&class_manifest(value, "")).expect_err("崩れた要素は断る");
            let shown: Vec<(u64, bool)> = errors.iter().map(|error| (error.line, error.message.contains(want))).collect();
            assert_eq!(shown, vec![(3, true)], "{value}: 1 件・行 3・{want}: {errors:?}");
        }
    }

    /// 上限の行か禁じる語列の 4 行のどれかを欠く manifest では (c)(d) を撃たない（揃った manifest では同じ要素を断る＝対）。
    #[test]
    fn class_derive_rules_skip_the_cross_row_checks_without_the_ceiling_or_the_four_denied_rows() {
        let (outside, denied) = ("[\"publish sh push\"]", "[\"delete git push --force\"]");
        for value in [outside, denied] {
            assert!(Manifest::parse(&class_manifest(value, "")).is_err(), "揃った manifest では断る: {value}");
        }
        for drop in ["runner.allowed_commands", "runner.denied_commands", "host_guard.git", "host_guard.tmux", "host_guard.ledger"] {
            for value in [outside, denied] {
                let read = Manifest::parse(&class_manifest(value, drop));
                assert!(read.is_ok(), "{drop} を欠く manifest では撃たない: {value}: {read:?}");
            }
        }
    }
}
