//! 席の役割と登録（設計 docs/design/seat-roles.md §2 / §6・ADR-0022 §2.1 / §2.5・SRS FR40）。役割の解決は
//! [`role_of_target`] の 1 本で**登録 row だけ**を読む（env・window 名の慣習・pane の字面は読まない・C2.2 / N3）。

use crate::fleet::select::Model;
use crate::fleet::store::{self, LockPolicy};
use crate::fleet::{cli, replay, Event, EventKind, Registration, State, ACTOR_MACHINE, SCHEMA};
use crate::headless::Effort;
use crate::pipe::declaration::path_kinds::PathKinds;
use crate::polarity::{OnFailure, Polarity, Timing};
use crate::rules::manifest::{HostManifest, Manifest};
use crate::rules::RuleValue;
use std::path::Path;

use super::tick::install::{doctor_word, Probe};
use super::tick::{self, ROW_INTERVAL};
use super::RuleRead;

/// 席の役割。**variant の列挙は core が持つ**（文書は写さない・ADR-0013 §2.1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    /// 人と話す唯一の席（ADR-0045 §2 (1)）。契約・設計・落ちる歯を書き、実装は自分で行わない。
    Orchestrator,
}

/// [`Role`] の全 variant（宣言順）。
pub const ALL: &[Role] = &[Role::Orchestrator];

impl Role {
    /// 行と引数に使う字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Orchestrator => "orchestrator",
        }
    }

    /// 字面から引く。未知なら `None`（variant 名の字面も受けない）。
    pub fn parse(text: &str) -> Option<Self> {
        ALL.iter().copied().find(|role| role.as_str() == text)
    }

    /// 役割ごとの既定の model の行 id（`seat.model.<役割名>`・設計 seat-roles.md §19）。
    ///
    /// 前置きが `role.` でないのは、その前置きが**権能の行**（役割ごとに雛形を 1 枚ずつ要る行）の印
    /// だからである（`role.<役割名>`・設計 §5）。既定の対は雛形を要らない。
    pub fn model_row(self) -> String {
        format!("seat.model.{}", self.as_str())
    }

    /// 役割ごとの既定の effort の行 id（`seat.effort.<役割名>`・設計 seat-roles.md §19）。
    pub fn effort_row(self) -> String {
        format!("seat.effort.{}", self.as_str())
    }
}

/// 役割ごとの既定（設計 seat-roles.md §19・裁定 id `user 2026-09-17T04:23Z`）。**対で持つ**——
/// model だけを運ぶと effort が口座の設定 dir 任せに戻り、同じ役割の席が口座ごとに違う深さで走る。
/// 値の正本は rules 行（`seat.model.<役割名>` / `seat.effort.<役割名>`）で、ここは型だけを持つ（C1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoleDefaults {
    /// 既定の model（閉じた表 [`Model`]）。
    pub model: Model,
    /// 既定の effort（閉じた表 [`Effort`]）。
    pub effort: Effort,
}

/// **渡された manifest** から役割の既定の対を引く（pure・[`super::int_rule_of`] と同じ 2 段の下段）。
///
/// 極性は **fail-closed**: 行が無い・不発効・値が文字列でない・字面が閉じた表に無い のどれも既定へ
/// 倒さず、理由（[`RuleRead`] の 4 variant）を名指して `Err`（C1「行の無さを既定に倒さない」）。
/// model と effort のどちらが読めなくても対は返らない（片肺で起こさない）。
pub fn defaults_of(manifest: &Manifest, role: Role) -> Result<RoleDefaults, RuleRead> {
    let model = table_row(manifest, &role.model_row(), Model::parse)?;
    let effort = table_row(manifest, &role.effort_row(), Effort::parse)?;
    Ok(RoleDefaults { model, effort })
}

/// 埋め込み manifest から役割の既定の対を引く（[`super::int_rule`] と同じ 2 段の上段・読めない周は
/// [`RuleRead::ManifestUnreadable`]）。
pub fn defaults(role: Role) -> Result<RoleDefaults, RuleRead> {
    defaults_of(&super::embedded_manifest()?, role)
}

/// 発効した行の文字列を閉じた表で引く（**4 つの読みを別の variant で返す**）。
fn table_row<T>(manifest: &Manifest, id: &str, parse: impl Fn(&str) -> Option<T>) -> Result<T, RuleRead> {
    let row = manifest.get(id).ok_or(RuleRead::Missing)?;
    if !row.enabled {
        return Err(RuleRead::Disabled);
    }
    let RuleValue::Str(text) = &row.value else {
        return Err(RuleRead::NotStr);
    };
    parse(text).ok_or(RuleRead::NotInTable)
}

/// 権能＝操作の種別（設計 §3・ADR-0022 §2.2・SRS FR41）。**variant の列挙は core が持つ**（文書は写さない）。
///
/// どの役割がどの権能を持つかは rules 行 `role.<役割名>`（`RuleKind::RoleCapabilities`・値は名の列・裁定 id
/// 付き）が持ち、ここは名の集合だけを閉じる。列に無い名は manifest の読み込みで `RuleError` になる
/// （[`Capability::parse`] の失敗）。`Go` / `EditContract` は記録時点で対応する subcommand も path 種別も無い
/// （go の記帳の口は後続・契約は台帳の write）＝行の値には在るが Bash 面では照合されない宣言だけの
/// 権能である。`Launch` / `Merge` は器の dispatcher だけが行う操作で、席の行には並ばない（ADR-0045 §2 (1)）。
/// `Stop` は便 1 本を名指す停止（`pipe stop --run <id>`）だけに結び、`--all` と名指しの無い停止は `Launch` の
/// まま（ADR-0048 §2・設計 seat-roles.md §25）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Capability {
    /// 回答の記帳（`pipe answer`）。
    Answer,
    /// 承認の記帳（`pipe approve`）。
    Approve,
    /// go の記帳（merge の許可・記帳の口は後続）。
    Go,
    /// 便の起動（`pipe intake` / `run` / `resume` / `retire`・名指しでない `stop`）。
    Launch,
    /// go 後の merge（`pipe land`）。
    Merge,
    /// 便 1 本を名指す停止（`pipe stop --run <id>`・ADR-0048）。
    Stop,
    /// 契約の編集（台帳の write・path 種別を持たない）。
    EditContract,
    /// `design-intent/` の編集。
    EditDesignIntent,
    /// `docs/design/` の編集。
    EditDesignDoc,
    /// 歯（`crates/<crate>/tests/` 配下）の編集。
    EditTests,
    /// 上記以外の repo 内の編集。
    EditCode,
    /// repo root の外の編集。
    EditOutside,
}

/// [`Capability`] の全 variant（宣言順）。
pub const CAPABILITIES: &[Capability] = &[
    Capability::Answer,
    Capability::Approve,
    Capability::Go,
    Capability::Launch,
    Capability::Merge,
    Capability::Stop,
    Capability::EditContract,
    Capability::EditDesignIntent,
    Capability::EditDesignDoc,
    Capability::EditTests,
    Capability::EditCode,
    Capability::EditOutside,
];

impl Capability {
    /// rules 行の値と記録に使う字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Answer => "answer",
            Self::Approve => "approve",
            Self::Go => "go",
            Self::Launch => "launch",
            Self::Merge => "merge",
            Self::Stop => "stop",
            Self::EditContract => "edit-contract",
            Self::EditDesignIntent => "edit-design-intent",
            Self::EditDesignDoc => "edit-design-doc",
            Self::EditTests => "edit-tests",
            Self::EditCode => "edit-code",
            Self::EditOutside => "edit-outside",
        }
    }

    /// 字面から引く。未知なら `None`（variant 名の字面も受けない）。
    pub fn parse(text: &str) -> Option<Self> {
        CAPABILITIES.iter().copied().find(|found| found.as_str() == text)
    }
}

/// 登録の受付の極性（設計 §6）: 受付の時点で止め、打刻を読めない周は登録しない。
pub const POLARITY: Polarity = Polarity { timing: Timing::InLoop, on_failure: OnFailure::FailClosed };

/// 登録を断る理由（閉じた enum）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterRefusal {
    /// 撃った target に打刻が無い・読めない・`sid` が空（hooks を積んだ席の証拠が無い）。`seat register` の口にだけ掛かる
    /// （`seat launch` の row は `sid` 無しで積む・account-lifecycle.md §4）。
    NoStamp,
    /// `--launch` の file を読めない・`--anchor` 無しで cwd の repo root を解けない。
    Input,
    /// event log へ書けない（理由の本文）。
    Store(String),
    /// `--model` が役割の既定の行と食い違う（[`REASON_MODEL_MISMATCH`]）・行を読めない（`no-rule:<variant>`）＝
    /// 行から導いた値を row に書けない（設計 seat-roles.md §20 の約束 5・受付の極性は fail-closed のまま）。
    Model(&'static str),
}

/// `seat register --model` が役割の既定の行と食い違う（[`RegisterRefusal::Model`] の字面の 1 つ）。
pub const REASON_MODEL_MISMATCH: &str = "model-mismatch";

impl RegisterRefusal {
    /// 行に出す字面（store の断りは理由の本文を添える）。
    pub fn render(&self, target: &str) -> String {
        let reason = match self {
            Self::NoStamp => "no-stamp".to_owned(),
            Self::Input => "input-unreadable".to_owned(),
            Self::Store(text) => format!("store detail={text}"),
            Self::Model(text) => (*text).to_owned(),
        };
        format!("seat register: refused reason={reason} target={target}")
    }
}

/// `seat register` の口の登録（設計 seat-roles.md §2）: `draft` の `role` / `target` / `account` / `model` を使い、`sid` は
/// 打刻から（`Some`・無ければ [`RegisterRefusal::NoStamp`]＝この口にだけ掛かる条件）・`launch` は file の本文・`anchor` は
/// [`anchor_of`] で埋め、`model` は役割の既定の行から導いた値（[`derived_model`]）で [`register`] へ渡す。**打刻を先に測る**
/// （断る周は event を書かない）。
pub fn register_stamped(state_dir: &Path, draft: Registration, launch: &Path, anchor: Option<&Path>) -> Result<Registration, RegisterRefusal> {
    let sid = stamped_sid(state_dir, &draft.target).ok_or(RegisterRefusal::NoStamp)?;
    let (Ok(launch), Some(root)) = (std::fs::read_to_string(launch), anchor_of(anchor)) else {
        return Err(RegisterRefusal::Input);
    };
    let model = derived_model(defaults(draft.role), draft.model.as_deref())?;
    register(state_dir, Registration { sid: Some(sid), launch, anchor: root.display().to_string(), model: Some(model), ..draft })
}

/// 登録 row に書く `model`（pure・設計 seat-roles.md §20 の約束 5 / 6）: 役割の既定の行の model の**表示名**（実測の行と同じ語彙）。
/// 宣言 `declared`（`--model`）は照合で、行と食い違う・表に無い周は [`REASON_MODEL_MISMATCH`]、行を読めない周は
/// `no-rule:<variant>` を [`RegisterRefusal::Model`] で返す（既定へ倒さない）。
fn derived_model(read: Result<RoleDefaults, RuleRead>, declared: Option<&str>) -> Result<String, RegisterRefusal> {
    let row = read.map_err(|failed| RegisterRefusal::Model(failed.no_rule()))?.model;
    match declared {
        Some(text) if Model::parse(text) != Some(row) => Err(RegisterRefusal::Model(REASON_MODEL_MISMATCH)),
        _ => Ok(row.display().to_owned()),
    }
}

/// target の打刻の最終行の `sid`（hooks を積んだ session の証拠）。打刻が無い・読めない・`sid` が空なら `None`。
fn stamped_sid(state_dir: &Path, target: &str) -> Option<String> {
    let seat = super::seat_dir(state_dir, target);
    let text = std::fs::read_to_string(super::state::path(&seat)).unwrap_or_default();
    let stamp = text.lines().rev().find(|line| !line.trim().is_empty()).and_then(|line| super::state::Stamp::from_line(line).ok());
    stamp.map(|found| found.sid.trim().to_owned()).filter(|sid| !sid.is_empty())
}

/// 登録 row の `anchor`（**`seat register` と `seat launch` の同じ 1 つの解き方**・account-lifecycle.md §4）: 明示の
/// `--anchor` は絶対化（symlink も存在も見ない）・無ければ cwd の repo root（`current_dir` は syscall であって env では
/// ない・C2.2）。解けなければ `None`。
pub fn anchor_of(anchor: Option<&Path>) -> Option<std::path::PathBuf> {
    let cwd_root = || std::env::current_dir().ok().and_then(|cwd| crate::hook::vessel::repo_root(&cwd));
    anchor.map_or_else(cwd_root, |found| std::path::absolute(found).ok())
}

/// 登録 row の口座を `account` に更新する（account-autonomy.md §5 の立て直し・同じ鍵で `SeatRegistered` 1 件）。
/// `seat register` を経由せず**打刻の条件は課さない**（`sid` は登録時の証拠であって現在の session の識別子では
/// ない）: `role` / `anchor` / `target` / `sid` / `launch` / `model` は既存 row から写す。
pub fn relabel(state_dir: &Path, row: &Registration, account: &str) -> Result<Registration, RegisterRefusal> {
    register(state_dir, Registration { account: account.to_owned(), ..row.clone() })
}

/// 登録 row を 1 件積む（**書き手 3 つの同じ 1 関数**・設計 seat-roles.md §2・account-lifecycle.md §4）: `seat register`
/// （[`register_stamped`]・打刻の条件を先に測り `sid` は `Some`）・tick の口座更新（[`relabel`]）・`seat launch`
/// （[`crate::seat::cycle::launch`]・`sid` は `None`・打刻の条件は掛けない）がここを通る。**`sid` は任意**。
pub fn register(state_dir: &Path, registration: Registration) -> Result<Registration, RegisterRefusal> {
    let event = Event {
        schema: SCHEMA,
        ts: cli::now_utc(),
        kind: EventKind::SeatRegistered,
        run: String::new(),
        bead: String::new(),
        host: cli::host(),
        actor: ACTOR_MACHINE.to_owned(),
        stage: None, seat: None, pid: None, detail: None, allowance: None,
        registration: Some(registration.clone()),
        mark: None,
        account: None,
        cost: None,
        rule: None,
    };
    let store_err = |err: store::StoreError| RegisterRefusal::Store(err.to_string());
    store::append(state_dir, &event, LockPolicy::embedded().map_err(store_err)?).map_err(store_err)?;
    Ok(registration)
}

/// 登録 row の退役を断る理由（閉じた enum・設計 account-lifecycle.md §24 形 1）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetireRefusal {
    /// target の登録 row が無い（退役済みを含む＝event を書かない）。
    NoRow,
    /// event log を読めない・書けない（理由の本文）。
    Store(String),
}

impl RetireRefusal {
    /// 行に出す字面（store の断りは理由の本文を添える）。
    pub fn render(&self, target: &str) -> String {
        let reason = match self {
            Self::NoRow => "no-row".to_owned(),
            Self::Store(text) => format!("store detail={text}"),
        };
        format!("seat retire: refused reason={reason} target={target}")
    }
}

/// target の登録 row を退役させる（設計 account-lifecycle.md §24 形 1）: [`registration_of_target`] の row を引き、その写しを本体に
/// `SeatRetired` を 1 件積む（`detail` = `reason`・actor は human）。row が無い周は [`RetireRefusal::NoRow`] で event を書かない。
/// 退役の後の読み手は全部 replay 経由で row を見なくなる（読み手の側は変えない・形 3）。
pub fn retire(state_dir: &Path, target: &str, reason: Option<&str>) -> Result<Registration, RetireRefusal> {
    let events = store::read_all(state_dir).map_err(|errors| RetireRefusal::Store(errors.iter().map(ToString::to_string).collect::<Vec<_>>().join("; ")))?;
    let state = replay(&events);
    let row = registration_of_target(&state, target).cloned().ok_or(RetireRefusal::NoRow)?;
    let event = Event {
        schema: SCHEMA,
        ts: cli::now_utc(),
        kind: EventKind::SeatRetired,
        run: String::new(),
        bead: String::new(),
        host: cli::host(),
        actor: EventKind::SeatRetired.default_actor().to_owned(),
        stage: None, seat: None, pid: None,
        detail: reason.map(str::to_owned),
        allowance: None,
        registration: Some(row.clone()),
        mark: None,
        account: None,
        cost: None,
        rule: None,
    };
    let store_err = |err: store::StoreError| RetireRefusal::Store(err.to_string());
    store::append(state_dir, &event, LockPolicy::embedded().map_err(store_err)?).map_err(store_err)?;
    Ok(row)
}

/// target の登録 row（**役割の解決の 1 本の隣**・設計 §2 / §9 (e)）: 鍵ごとに最新へ畳んだ行のうち `target` が
/// 一致するものを引き、複数の鍵が同じ target なら log の後の行が勝つ（同じ鍵の旧 row は畳まれて旧 target では
/// 解けない）。row の `model` / `account` はここから運ぶ（account-autonomy.md §3 / §5 の読み手・契約 (d) / (e)）。
pub fn registration_of_target<'a>(state: &'a State, target: &str) -> Option<&'a Registration> {
    let rows = state.registrations.values().filter(|latest| latest.registration.target == target);
    rows.max_by_key(|latest| latest.seq).map(|latest| &latest.registration)
}

/// 鍵（役割 × anchor）の登録 row（[`registration_of_target`] の隣・account-lifecycle.md §14 の短い形の既定の出所）: 鍵ごとに
/// 最新へ畳んだ行をそのまま引く（`target` / `model` はここから運ぶ・無ければ `None`＝呼び手が flag の欠けを名指す）。
pub fn registration_of_key<'a>(state: &'a State, role: Role, anchor: &str) -> Option<&'a Registration> {
    state.registrations.get(&(role, anchor.to_owned())).map(|latest| &latest.registration)
}

/// target の役割（**役割の解決の 1 本**・設計 §2）: [`registration_of_target`] の row の `role`。
pub fn role_of_target(state: &State, target: &str) -> Option<Role> {
    registration_of_target(state, target).map(|row| row.role)
}

/// 登録 row の一覧（pure・鍵の順・1 row 1 行）。`model` の無い row は `-`（契約 (e)・doctor の欄）。末尾の欄
/// `paths=` は anchor の path の種別の宣言の state（`paths_of(anchor)`・`default` / `declared:<書かれた key の数>` /
/// `invalid:<理由>`・設計 seat-roles.md §24）＝row は anchor ごとなので anchor ごとの state を 1 行で名乗る。`model=` の直後の欄
/// `default=` は役割の既定の行（`default_of(role)`・[`render_default`]）＝宣言と row の突合を doctor の 1 面にだけ出す（§20 の約束 8）。
pub fn render_rows(state: &State, paths_of: impl Fn(&str) -> PathKinds, default_of: impl Fn(Role) -> Result<RoleDefaults, RuleRead>) -> Vec<String> {
    let row = |found: &Registration| {
        let model = found.model.as_deref().unwrap_or("-");
        format!(
            "seat: role={} anchor={} target={} account={} model={model} default={} paths={}",
            found.role.as_str(),
            found.anchor,
            found.target,
            found.account,
            render_default(default_of(found.role)),
            paths_of(&found.anchor).render()
        )
    };
    state.registrations.values().map(|latest| row(&latest.registration)).collect()
}

/// `default=` の欄の 1 語（pure）: 読める周は `<model の表示名>/<effort>`（row の `model` と同じ語彙）・読めない周は既定の語を
/// 出さず理由の字面 `no-rule:<variant>`（判定しない＝rc を変えない）。
pub fn render_default(read: Result<RoleDefaults, RuleRead>) -> String {
    read.map_or_else(|failed| failed.no_rule().to_owned(), |found| format!("{}/{}", found.model.display(), found.effort.alias()))
}

/// doctor の登録 row の一覧（[`render_rows`] に anchor の HEAD の宣言の読み手と、`rules` の manifest の既定の読み手を渡した形・
/// row ごとに git を 1 回撃つ）。
pub fn doctor_rows(state: &State, rules: &Result<Manifest, RuleRead>) -> Vec<String> {
    render_rows(
        state,
        |anchor| PathKinds::read_at_head(Path::new(anchor)),
        |role| rules.as_ref().map_err(|failed| *failed).and_then(|manifest| defaults_of(manifest, role)),
    )
}

/// 登録 row と実在の target の突合の 1 行（pure・`seats: registered=N live=K missing=M`）。
/// log を読めない周・tmux を撃てない周は数えられない値を**0 と書かない**。
pub fn render_reconcile(state: Option<&State>, live: Option<&[String]>) -> String {
    let registered = state.map_or("unreadable".to_owned(), |found| found.registrations.len().to_string());
    let (Some(state), Some(targets)) = (state, live) else {
        return format!("seats: registered={registered} live=unmeasurable missing=unmeasurable");
    };
    let found = state.registrations.values().filter(|latest| targets.contains(&latest.registration.target)).count();
    format!("seats: registered={registered} live={found} missing={}", state.registrations.len().saturating_sub(found))
}

/// doctor の項目（event log を読み、tmux の `list-panes` を 1 回撃つ・C3.2）: 登録 row の一覧（1 row 1 行・
/// `model` と `default` と `paths` の欄つき・log を読めない周は 0 行）の後に突合の 1 行。`rules` は `--rules` の値（口座の行と
/// 同じ形・無ければ埋め込み）で、役割の既定の行を引く manifest（読めない周は `default=` の欄が理由を名乗る・rc は変えない）。
/// `units`（`--unit-dir` と `--binary` がそろった周だけ）が在る周は row の行の末尾に `tick-unit=` の 1 語を足し
/// （[`crate::seat::tick::install::doctor_word`]・設計 seat-heartbeat.md §3）、flag が無く host の面に `[[tick]]` が在る周は面の値で
/// 同じ 1 語を足す（flag が勝つ・§5 形 3）。どちらも無い周は `tick-unit=` を足さない。`paths=` の直後には常に
/// `heartbeat=` / `tick=` の 2 項目（[`tick_words`]・§12 行 p 形 3）。
pub fn doctor_lines(state_dir: &Path, socket: Option<&str>, rules: Option<&str>, units: Option<&Probe>) -> Vec<String> {
    let state = store::read_all(state_dir).ok().map(|events| replay(&events));
    let panes = super::tmux_stdout(socket, &["list-panes", "-a", "-F", "#{session_name}:#{window_name}"]);
    let live: Option<Vec<String>> = panes.map(|out| out.lines().map(str::to_owned).collect());
    let manifest = super::manifest_read(rules.map_or_else(Manifest::embedded, |path| Manifest::load(Path::new(path))));
    let declared = match HostManifest::read(&crate::rules::host_manifest_path(state_dir)) {
        HostManifest::Present(face) => face.tick().cloned(),
        HostManifest::Absent | HostManifest::Unreadable(_) => None,
    };
    let face = declared.as_ref().map(|tick| Probe {
        unit_dir: Path::new(tick.unit_dir()),
        binary: Path::new(tick.binary()),
        rules: rules.map(Path::new),
    });
    let units = units.or(face.as_ref());
    let rows = |found: &State| {
        let beats = found.registrations.values().map(|latest| tick_words(state_dir, &latest.registration.target, &manifest));
        let lines: Vec<String> = doctor_rows(found, &manifest).into_iter().zip(beats).map(|(line, words)| format!("{line} {words}")).collect();
        let Some(probe) = units else {
            return lines;
        };
        let words = found.registrations.values().map(|latest| doctor_word(state_dir, &latest.registration.target, probe, &manifest));
        lines.into_iter().zip(words).map(|(line, word)| format!("{line} {word}")).collect()
    };
    let mut lines = state.as_ref().map(rows).unwrap_or_default();
    lines.push(render_reconcile(state.as_ref(), live.as_deref()));
    lines
}

/// doctor の席の行の 2 項目 `heartbeat=on|off tick=<語>`（設計 seat-heartbeat.md §12 行 p 形 3）: `tick=` は `seat tick status` と
/// 同じ健全の 1 関数（[`crate::seat::tick::health`]）の語で、周期の行を読めない周は `tick-unit=` と同じ no-rule の語（rc は変えない）。
/// 梯子の行は読まない。
fn tick_words(state_dir: &Path, target: &str, manifest: &Result<Manifest, RuleRead>) -> String {
    let seat = super::seat_dir(state_dir, target);
    let interval = manifest.as_ref().map_err(|failed| *failed).and_then(|found| super::int_rule_of(found, ROW_INTERVAL));
    let tick = interval.map_or_else(RuleRead::no_rule, |secs| tick::health(&seat, secs).as_str());
    format!("heartbeat={} tick={tick}", tick::switch_word(&seat))
}

#[cfg(test)]
mod tests {
    use super::{defaults, defaults_of, Role, RoleDefaults, ALL};
    use crate::fleet::select::Model;
    use crate::headless::Effort;
    use crate::rules::manifest::Manifest;
    use crate::seat::RuleRead;

    /// `[[rule]]` 2 行（役割の既定の対）の fixture。kind / 値 / 発効は引数で崩せる。
    fn manifest_with(role: Role, model: (&str, &str, bool), effort: (&str, &str, bool)) -> Manifest {
        let row = |id: String, kind: &str, value: &str, enabled: bool| {
            format!("[[rule]]\nid = \"{id}\"\nkind = \"{kind}\"\nvalue = {value}\nenabled = {enabled}\nruling = \"r\"\nruled_at = \"d\"\n\n")
        };
        let text = format!(
            "schema = 1\n\n{}{}",
            row(role.model_row(), model.0, model.1, model.2),
            row(role.effort_row(), effort.0, effort.1, effort.2)
        );
        match Manifest::parse(&text) {
            Ok(found) => found,
            Err(errors) => panic!("fixture の manifest を読める: {errors:?}"),
        }
    }

    /// 読める周の既定の対（`RoleModel` = fable・`RoleEffort` = high）。
    fn good(role: Role) -> Manifest {
        manifest_with(role, ("RoleModel", "\"fable\"", true), ("RoleEffort", "\"high\"", true))
    }

    /// 歯 (c): 読み手は行から**対を型で返す**（model と effort の 2 field・どちらも閉じた型）。
    ///
    /// 対の片方だけを読む変異（effort を見ない・model を捨てる）は値の assert で落ちる。
    #[test]
    fn seat_role_defaults_of_returns_the_pair_as_closed_types() {
        for role in ALL.iter().copied() {
            assert_eq!(
                defaults_of(&good(role), role),
                Ok(RoleDefaults { model: Model::Fable, effort: Effort::High }),
                "{role:?}"
            );
        }
        // 表示名（実測行の語彙）も別名と同じ表で引ける・effort は字面だけ。
        let display = manifest_with(Role::Orchestrator, ("RoleModel", "\"Opus\"", true), ("RoleEffort", "\"xhigh\"", true));
        assert_eq!(
            defaults_of(&display, Role::Orchestrator),
            Ok(RoleDefaults { model: Model::Opus, effort: Effort::Xhigh }),
            "値は閉じた表の字面で引く"
        );
    }

    /// 歯 (c): 行なし / 不発効 / 値が文字列でない / 字面が表に無い の 4 周を**それぞれ別の理由**で名指す
    /// （fail-closed・既定へ倒さない）。4 つを 1 つに潰す変異（全部 `Missing`・`enabled` を見ない・形を
    /// 見ない・`parse` の失敗を握り潰す）はどれかの assert で落ちる。model 側と effort 側の両方で測る。
    #[test]
    fn seat_role_defaults_of_names_each_failure_for_both_rows() {
        let role = Role::Orchestrator;
        let empty = match Manifest::parse("schema = 1\n") {
            Ok(found) => found,
            Err(errors) => panic!("空の manifest を読める: {errors:?}"),
        };
        assert_eq!(defaults_of(&empty, role), Err(RuleRead::Missing), "行が無い");
        let cases = [
            (("RoleModel", "\"fable\"", false), ("RoleEffort", "\"high\"", true), RuleRead::Disabled, "model が不発効"),
            (("RoleModel", "\"fable\"", true), ("RoleEffort", "\"high\"", false), RuleRead::Disabled, "effort が不発効"),
            (("CoreLines", "7", true), ("RoleEffort", "\"high\"", true), RuleRead::NotStr, "model が文字列でない"),
            (("RoleModel", "\"fable\"", true), ("CoreLines", "7", true), RuleRead::NotStr, "effort が文字列でない"),
            (("DialogueSurface", "\"orchestrator\"", true), ("RoleEffort", "\"high\"", true), RuleRead::NotInTable, "model が表に無い"),
            (("RoleModel", "\"fable\"", true), ("DialogueSurface", "\"orchestrator\"", true), RuleRead::NotInTable, "effort が表に無い"),
        ];
        for (model, effort, want, what) in cases {
            assert_eq!(defaults_of(&manifest_with(role, model, effort), role), Err(want), "{what}");
        }
        // 片方だけ在る周は残りの行の不在で止まる（片肺で対を返さない）。
        let only_model = match Manifest::parse(&format!(
            "schema = 1\n\n[[rule]]\nid = \"{}\"\nkind = \"RoleModel\"\nvalue = \"fable\"\nenabled = true\nruling = \"r\"\nruled_at = \"d\"\n",
            role.model_row()
        )) {
            Ok(found) => found,
            Err(errors) => panic!("fixture の manifest を読める: {errors:?}"),
        };
        assert_eq!(defaults_of(&only_model, role), Err(RuleRead::Missing), "effort の行が無い");
    }

    /// 歯 (c): 埋め込みの薄い口は tracked の行（`fable` / `high`・裁定 `user 2026-09-17T04:23Z`）を返し、
    /// 閉じた列のどの役割でも読める（行が 1 本でも欠ければここが落ちる）。
    #[test]
    fn seat_role_defaults_reads_every_role_from_the_embedded_manifest() {
        for role in ALL.iter().copied() {
            assert_eq!(
                defaults(role),
                Ok(RoleDefaults { model: Model::Fable, effort: Effort::High }),
                "{role:?} の既定は裁定 user 2026-09-17T04:23Z の対"
            );
        }
    }
}
