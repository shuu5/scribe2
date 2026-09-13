//! 席の役割と登録（設計 docs/design/seat-roles.md §2 / §6・ADR-0022 §2.1 / §2.5・SRS FR40）。役割の解決は
//! [`role_of_target`] の 1 本で**登録 row だけ**を読む（env・window 名の慣習・pane の字面は読まない・C2.2 / N3）。

use crate::fleet::store::{self, LockPolicy};
use crate::fleet::{cli, replay, Event, EventKind, Registration, State, ACTOR_MACHINE, SCHEMA};
use crate::polarity::{OnFailure, Polarity, Timing};
use std::path::Path;

/// 席の役割。**variant の列挙は core が持つ**（文書は写さない・ADR-0013 §2.1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    /// 計画と裁定の持ち込み先の席。
    Planner,
    /// 便を流す管理席。
    Admin,
}

/// [`Role`] の全 variant（宣言順）。
pub const ALL: &[Role] = &[Role::Planner, Role::Admin];

impl Role {
    /// 行と引数に使う字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planner => "planner",
            Self::Admin => "admin",
        }
    }

    /// 字面から引く。未知なら `None`（variant 名の字面も受けない）。
    pub fn parse(text: &str) -> Option<Self> {
        ALL.iter().copied().find(|role| role.as_str() == text)
    }
}

/// 登録の受付の極性（設計 §6）: 受付の時点で止め、打刻を読めない周は登録しない。
pub const POLARITY: Polarity = Polarity { timing: Timing::InLoop, on_failure: OnFailure::FailClosed };

/// 登録を断る理由（閉じた enum）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterRefusal {
    /// 撃った target に打刻が無い・読めない・`sid` が空（hooks を積んだ席の証拠が無い）。
    NoStamp,
    /// `--launch` の file を読めない・`--anchor` 無しで cwd の repo root を解けない。
    Input,
    /// event log へ書けない（理由の本文）。
    Store(String),
}

impl RegisterRefusal {
    /// 行に出す字面（store の断りは理由の本文を添える）。
    pub fn render(&self, target: &str) -> String {
        let reason = match self {
            Self::NoStamp => "no-stamp".to_owned(),
            Self::Input => "input-unreadable".to_owned(),
            Self::Store(text) => format!("store detail={text}"),
        };
        format!("seat register: refused reason={reason} target={target}")
    }
}

/// 登録を 1 件追記する。`draft` の `role` / `target` / `account` / `model` を使い、`sid` は打刻から・`launch` は
/// file の本文・`anchor` は明示の絶対化か cwd の repo root で埋める。**打刻を先に測る**（断る周は event を書かない）。
pub fn register(state_dir: &Path, draft: Registration, launch: &Path, anchor: Option<&Path>) -> Result<Registration, RegisterRefusal> {
    let seat = super::seat_dir(state_dir, &draft.target);
    let text = std::fs::read_to_string(super::state::path(&seat)).unwrap_or_default();
    let stamp = text.lines().rev().find(|line| !line.trim().is_empty()).and_then(|line| super::state::Stamp::from_line(line).ok());
    let sid = stamp.map(|found| found.sid.trim().to_owned()).filter(|sid| !sid.is_empty()).ok_or(RegisterRefusal::NoStamp)?;
    let cwd_root = || std::env::current_dir().ok().and_then(|cwd| crate::hook::vessel::repo_root(&cwd));
    let root = anchor.map_or_else(cwd_root, |found| std::path::absolute(found).ok());
    let (Ok(launch), Some(root)) = (std::fs::read_to_string(launch), root) else {
        return Err(RegisterRefusal::Input);
    };
    append_row(state_dir, Registration { sid, launch, anchor: root.display().to_string(), ..draft })
}

/// 登録 row の口座を `account` に更新する（account-autonomy.md §5 の立て直し・同じ鍵で `SeatRegistered` 1 件）。
/// `seat register` を経由せず**打刻の条件は課さない**（`sid` は登録時の証拠であって現在の session の識別子では
/// ない）: `role` / `anchor` / `target` / `sid` / `launch` / `model` は既存 row から写す。
pub fn relabel(state_dir: &Path, row: &Registration, account: &str) -> Result<Registration, RegisterRefusal> {
    append_row(state_dir, Registration { account: account.to_owned(), ..row.clone() })
}

/// 登録 row を 1 件積む（**書き手 2 つの同じ 1 関数**・設計 seat-roles.md §2）: `seat register`（打刻の条件を
/// 先に測る）と tick の口座更新（[`relabel`]）がここを通る。
fn append_row(state_dir: &Path, registration: Registration) -> Result<Registration, RegisterRefusal> {
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
    };
    let store_err = |err: store::StoreError| RegisterRefusal::Store(err.to_string());
    store::append(state_dir, &event, LockPolicy::embedded().map_err(store_err)?).map_err(store_err)?;
    Ok(registration)
}

/// target の登録 row（**役割の解決の 1 本の隣**・設計 §2 / §9 (e)）: 鍵ごとに最新へ畳んだ行のうち `target` が
/// 一致するものを引き、複数の鍵が同じ target なら log の後の行が勝つ（同じ鍵の旧 row は畳まれて旧 target では
/// 解けない）。row の `model` / `account` はここから運ぶ（account-autonomy.md §3 / §5 の読み手・契約 (d) / (e)）。
pub fn registration_of_target<'a>(state: &'a State, target: &str) -> Option<&'a Registration> {
    let rows = state.registrations.values().filter(|latest| latest.registration.target == target);
    rows.max_by_key(|latest| latest.seq).map(|latest| &latest.registration)
}

/// target の役割（**役割の解決の 1 本**・設計 §2）: [`registration_of_target`] の row の `role`。
pub fn role_of_target(state: &State, target: &str) -> Option<Role> {
    registration_of_target(state, target).map(|row| row.role)
}

/// 登録 row の一覧（pure・鍵の順・1 row 1 行）。`model` の無い row は `-`（契約 (e)・doctor の欄）。
pub fn render_rows(state: &State) -> Vec<String> {
    let row = |found: &Registration| {
        let model = found.model.as_deref().unwrap_or("-");
        format!("seat: role={} anchor={} target={} account={} model={model}", found.role.as_str(), found.anchor, found.target, found.account)
    };
    state.registrations.values().map(|latest| row(&latest.registration)).collect()
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
/// `model` の欄つき・log を読めない周は 0 行）の後に突合の 1 行。
pub fn doctor_lines(state_dir: &Path, socket: Option<&str>) -> Vec<String> {
    let state = store::read_all(state_dir).ok().map(|events| replay(&events));
    let panes = super::tmux_stdout(socket, &["list-panes", "-a", "-F", "#{session_name}:#{window_name}"]);
    let live: Option<Vec<String>> = panes.map(|out| out.lines().map(str::to_owned).collect());
    let mut lines = state.as_ref().map(render_rows).unwrap_or_default();
    lines.push(render_reconcile(state.as_ref(), live.as_deref()));
    lines
}
