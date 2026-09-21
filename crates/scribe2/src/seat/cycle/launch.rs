//! 席の**起動**（`seat launch`・[`launch`]・設計 account-lifecycle.md §4・ADR-0026 §2.3・SRS FR59）と、起動行の純関数
//! （[`derive_launch`] / [`fill_launch`] / [`with_agent_view_off`] / [`with_anchor_cd`] / [`with_flags`] / [`with_defaults`]・雛形 file を持たない）。
//! [`super`] から純移動（`s2-07l.319`）。起動の注入は立て直しと**同じ 1 本**（[`super::relaunch::boot`]）を通る。

use super::relaunch::{boot, choose, input_gate, launch_line, Boot, Booted};
use super::{
    EFFORT_FLAG, HOLE, MODEL_FLAG, NEXT_AFTER_NOT_SHELL, REASON_ACCOUNT_UNKNOWN, REASON_EFFORT_DUPLICATED,
    REASON_LOG_UNREADABLE, REASON_MODEL_DUPLICATED, REASON_MODEL_MISMATCH, REASON_MODEL_UNKNOWN, REASON_NOT_SHELL,
    REASON_NO_ACCOUNT, REASON_REGISTER, REASON_REPLACE, REASON_RESTORE, REASON_RESTORE_SAME_WINDOW,
    REASON_SESSION_MISSING, REASON_WINDOW, WHEN_LAUNCH, WHO_LAUNCH,
};
use crate::fleet::select::{Model, NoCandidate, Selection};
use crate::fleet::store;
use crate::fleet::{replay, Registration};
use crate::headless::{ACCOUNT_ENV, AGENT_VIEW_ENV, AGENT_VIEW_OFF, DEFAULT_CLAUDE};
use crate::hook::{seat_name, InjectionRecord, SCHEMA};
use crate::rules::manifest::{LaunchArg, Manifest, PluginDir};
use crate::seat::role::{Role, RoleDefaults};
use crate::seat::{inject, role, sanitize_target, state, tmux_ok, RuleRead, StateDir};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

/// 起動の雛形の穴の数えが 1 でない理由（**閉じた 2 値**・憲法 C11）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Holes {
    /// 穴が無い（口座を渡せない雛形）。
    Missing,
    /// 穴が 2 つ以上（どれを埋めるか決まらない）。
    Many,
}

/// [`Holes`] の全 variant（宣言順）。
pub const HOLES: &[Holes] = &[Holes::Missing, Holes::Many];

impl Holes {
    /// 断りの字面（tick の判定行は `relaunch-` を前置きする）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "launch-no-hole",
            Self::Many => "launch-many-holes",
        }
    }
}

/// 起動の雛形の穴 [`HOLE`] **ちょうど 1 つ**を `account_dir` で埋める（pure・文字列の置換だけ）。env を読まず、
/// 雛形の中の host 名や絶対 path を解釈しない（C2.2）: 器が知るのは穴の位置だけである。
pub fn fill_launch(template: &str, account_dir: &str) -> Result<String, Holes> {
    match template.matches(HOLE).count() {
        0 => Err(Holes::Missing),
        1 => Ok(template.replacen(HOLE, account_dir, 1)),
        _ => Err(Holes::Many),
    }
}

/// 起動行の先頭に agent view を切る env（`CLAUDE_CODE_DISABLE_AGENT_VIEW=1 `）を前置する（pure・設計 account-autonomy.md
/// §5「agent view の前提」・`s2-07l.239`）。器が起こす claude は常に agent view 無しで動く——有効な session は background
/// work が残る周の `/exit` で dialog を出して止まり、器は描画を読まない（C3.3）ので答えられない。雛形は user の物で
/// 書き換えず（[`fill_launch`] は不変）、子へ設定するだけで env は読まない（C2.2）。既に同じ前置で始まる行は二重にせず、
/// 空の行はそのまま返す。
pub fn with_agent_view_off(line: &str) -> String {
    let prefix = format!("{AGENT_VIEW_ENV}={AGENT_VIEW_OFF} ");
    if line.trim().is_empty() || line.trim_start().starts_with(&prefix) {
        return line.to_owned();
    }
    format!("{prefix}{line}")
}

/// 起動行の先頭に登録 row の anchor への `cd '<anchor>' && ` を前置する（pure・account-lifecycle.md §4・`s2-07l.324`）。
/// 起こす claude の cwd は pane の shell の cwd を継ぐ（project の CLAUDE.md と hook は cwd 由来）ので、pane の cwd
/// （host の再起動後の復元で home に戻る・真実でない C3）でなく row の `anchor` から写す（env も cwd も読まない・C2.2）。
/// agent view の前置（[`with_agent_view_off`]）より**前**＝`cd … && ENV=… claude …` の順。既に同じ前置で始まる行は
/// 二重にせず、空の行はそのまま返す。
pub(super) fn with_anchor_cd(line: &str, anchor: &str) -> String {
    let prefix = format!("cd '{anchor}' && ");
    if line.trim().is_empty() || line.trim_start().starts_with(&prefix) {
        return line.to_owned();
    }
    format!("{prefix}{line}")
}

/// 宣言の model（表示名か別名・登録 row の `model` / `--model`）を型にする: 無しは `Ok(None)`・表に無い字面は `Err`。
pub(super) fn model_of(text: Option<&str>) -> Result<Option<Model>, ()> {
    text.map_or(Ok(None), |found| Model::parse(found).map(Some).ok_or(()))
}

/// 席の既定（model と effort の対）を**渡された manifest** の行から導き、宣言の model（`--model`）を照合する（**pure**・設計
/// seat-roles.md §20 の約束 3 / 7）: 行を読めない周は [`crate::seat::RuleRead`] の字面（`no-rule:<variant>`）で断り
/// （口座の設定 file の既定で黙って起こさない・C10）、`declared` が行の model と食い違う周は [`REASON_MODEL_MISMATCH`]
/// （`--model` は照合であって宣言ではない）。埋め込み manifest を渡すのは `seat launch` の口の 1 か所だけ。
pub(super) fn seat_defaults(rules: &Manifest, role: Role, declared: Option<Model>) -> Result<RoleDefaults, &'static str> {
    let defaults = role::defaults_of(rules, role).map_err(RuleRead::no_rule)?;
    match declared {
        Some(model) if model != defaults.model => Err(REASON_MODEL_MISMATCH),
        _ => Ok(defaults),
    }
}

/// 起動行 `line` に旗と値の対の列 `flags` を運ばせる（pure・C10「宣言値を起動へ効かせる」・settings の層に依らない C2.2）:
/// `claude` の語の直後に `flags` の宣言順のまま挟む（`claude` の語が無い雛形は末尾）。空の列は行をそのまま（雛形は書き換えない）。
pub fn with_flags(line: &str, flags: &[(&str, &str)]) -> String {
    if flags.is_empty() {
        return line.to_owned();
    }
    let mut words: Vec<&str> = line.split(' ').collect();
    let at = words.iter().position(|word| *word == DEFAULT_CLAUDE).map_or(words.len(), |at| at + 1);
    words.splice(at..at, flags.iter().flat_map(|(flag, value)| [*flag, *value]));
    words.join(" ")
}

/// 起動行 `line` に役割の既定を運ばせる（[`with_flags`] の 1 本・設計 seat-roles.md §20 の約束 1）: `--model <別名>` → `--effort <値>`
/// の順（この関数の列の宣言順）で 1 つずつ。`None` は行をそのまま（登録 row の雛形は `None`）。
pub fn with_defaults(line: &str, defaults: Option<RoleDefaults>) -> String {
    let Some(defaults) = defaults else { return line.to_owned() };
    with_flags(line, &[(MODEL_FLAG, defaults.model.alias()), (EFFORT_FLAG, defaults.effort.alias())])
}

/// 送る起動行 `line` の**末尾**に語の列 `tail` を足す（pure・設計 account-lifecycle.md §18・会話の引き継ぎ）。空の列は行をそのまま。
/// 語の字の集合は呼び手（短い形の口）が絞る＝ここは引用も解釈もしない。
fn with_tail(line: &str, tail: &[&str]) -> String {
    if tail.is_empty() {
        return line.to_owned();
    }
    format!("{line} {}", tail.join(" "))
}

/// 起動行の `--model` と `--effort` は旗ごとに高々 1 つ: 雛形の literal と器の 1 つが重なる周は後勝ちにせず、旗ごとに違う理由
/// （[`REASON_MODEL_DUPLICATED`] / [`REASON_EFFORT_DUPLICATED`]・宣言は行の 1 か所）で断る。
pub fn single_model(line: &str) -> Result<(), &'static str> {
    let count = |flag: &str| line.split(' ').filter(|word| *word == flag).count();
    [(MODEL_FLAG, REASON_MODEL_DUPLICATED), (EFFORT_FLAG, REASON_EFFORT_DUPLICATED)]
        .into_iter()
        .find(|(flag, _)| count(flag) > 1)
        .map_or(Ok(()), |(_, reason)| Err(reason))
}

/// 起動行の導出（**pure**・設計 account-lifecycle.md §4・ADR-0026 §2.3）:
/// `CLAUDE_CODE_DISABLE_AGENT_VIEW=1 CLAUDE_CONFIG_DIR={account_dir} claude [--model <別名> --effort <値>] --plugin-dir <anchor> [--plugin-dir <dir>…] [<value>…]`。
///
/// 穴は [`HOLE`] の 1 つだけ（[`fill_launch`] / [`Holes`] は不変）。`claude` は語（shell の PATH が解く・器は claude の
/// 場所を持たない）。`defaults` が在れば `claude` の直後に運ぶ（[`with_defaults`]・登録 row の雛形は `None`＝旗無し）。器自身の plugin は
/// anchor（main checkout・`plugin.json` を持つ）を積み、host 固有の plugin dir と起動引数は host の面の宣言（`[[plugin]]` /
/// `[[launch-arg]]`・宣言順）から写す（C10.2）。雛形 file は読まない・書かない。値の中の空白は解釈しない（shell が読む字面のまま）。
pub fn derive_launch(anchor: &Path, plugins: &[PluginDir], args: &[LaunchArg], defaults: Option<RoleDefaults>) -> String {
    let mut words = vec![
        format!("{AGENT_VIEW_ENV}={AGENT_VIEW_OFF}"),
        format!("{ACCOUNT_ENV}={HOLE}"),
        DEFAULT_CLAUDE.to_owned(),
        "--plugin-dir".to_owned(),
        anchor.display().to_string(),
    ];
    for plugin in plugins {
        words.push("--plugin-dir".to_owned());
        words.push(plugin.dir().to_owned());
    }
    words.extend(args.iter().map(|arg| arg.value().to_owned()));
    with_defaults(&words.join(" "), defaults)
}

/// 席の起動 1 回の入力（[`launch`]・`seat launch`・account-lifecycle.md §4）。
pub struct Launch<'a> {
    /// tmux target（`session:window`・window は無ければ作る・session は作らない）。
    pub target: &'a str,
    /// tmux の socket。
    pub socket: Option<&'a str>,
    /// 解決済みの置き場（credential dir・event log・打刻の置き場）。
    pub state_dir: &'a StateDir,
    /// 立ち上がった後に送る復元 command（`--restore`・無ければ送らない）。
    pub restore: Option<&'a str>,
    /// 立ち上がりと復元の確認上限（rules 行 `seat.cycle_settle_s`）。
    pub settle: Duration,
    /// 確認の周期（rules 行 `seat.cycle_poll_ms`）。
    pub step: Duration,
    /// 席の役割（登録 row の鍵の片方・`--role`）。
    pub role: Role,
    /// 登録 row の anchor（絶対 path・`--anchor` か cwd の repo root・起動行の `--plugin-dir` の 1 つ目）。
    pub anchor: &'a Path,
    /// 明示の口座（`--account`・無ければ session 用の選定）。
    pub account: Option<&'a str>,
    /// 宣言の model（`--model`・役割の既定の行との照合と選定に渡す・登録 row には行から導いた値を書く）。
    pub model: Option<&'a str>,
    /// 開いた manifest（tracked + host の面・`[[account]]` / `[[plugin]]` / `[[launch-arg]]` の出所）。
    pub manifest: &'a Manifest,
    /// 役割の既定の行（`seat.model.<役割名>` / `seat.effort.<役割名>`）を引く manifest（`seat launch` の口は埋め込みを渡す・
    /// [`seat_defaults`]）。
    pub rules: &'a Manifest,
    /// R-C9-1 の値（session 用の閾値）。
    pub threshold_pct: u64,
    /// 注入する起動行の**末尾**に足す会話の引き継ぎの語（短い形の `-c` = `--continue`・`-r ID` = `--resume ID`・空は足さない・
    /// 設計 account-lifecycle.md §18）。登録 row の `launch`（雛形）には載せない（会話の id は 1 回きりの値）。
    pub carry: &'a [&'a str],
}

/// 席の起動 1 回の結果。**「送っていない」と「送ったが確かめられない」を分ける**（[`super::Relaunched`] と同じ）。
pub enum Launched {
    /// 起動行を注入して立ち上がりを確かめた（選んだ label・`--restore` を送った周はその消費）。
    Done(String, Option<inject::Settled>),
    /// 選べる口座が無い（**1 key も送らず row も書かない**）。
    None(NoCandidate),
    /// **1 key も送っていない**（前提の断りは全部ここで、**row を 1 件も書いていない**＝設計 seat-roles.md §26 の約束 5）。
    Refused(&'static str),
    /// 送った／置き換えたが確かめられない（row は書き終えている・約束 6）。
    Failed(&'static str),
}

/// 席を起こす（設計 account-lifecycle.md §4 / seat-roles.md §26 / §20・ADR-0026 §2.3・SRS FR59 / FR40 / FR36）: `--model` を型にし（表に無い値は
/// `launch-model-unknown`）→ 役割の既定の行から model と effort を導き `--model` を照合する（[`seat_defaults`]・行を読めない周は
/// `no-rule:<variant>`・食い違いは `launch-model-mismatch`）→ 口座を決め（`--account` か session 用の選定 [`choose`]）→ 起動行（導出した行に
/// 既定の 2 つの旗を運ばせ穴を埋め row の anchor への `cd` を前置・二重は旗ごとの理由・row の雛形は旗無し＝宣言は行の 1 か所）→ **呼び手の pane が
/// target の pane そのものか**を `-t` の無い 1 問いで測り（[`crate::seat::target_of_caller`]）→ 前提の断りを**全部**済ませて登録 row を書く
/// （[`prepare`]）→ 一致する周は key を 1 つも送らず自分の process を起動行へ置き換え（[`replace_with`]）、一致しない周は立て直しと同じ
/// 1 本（[`boot`]）で shell へ注入して `--restore` が在れば復元を送る → `inject.jsonl` に `kind=launch` を 1 行。lock も cycle-stamp も
/// 取らない（起動は user の手番）。
pub fn launch(request: &Launch) -> Launched {
    let started_at = Instant::now();
    let Ok(declared) = model_of(request.model) else { return Launched::Refused(REASON_MODEL_UNKNOWN) };
    let defaults = match seat_defaults(request.rules, request.role, declared) {
        Ok(found) => found,
        Err(reason) => return Launched::Refused(reason),
    };
    let label = match pick_account(request) {
        Ok(label) => label,
        Err(refused) => return refused,
    };
    let derived = derive_launch(request.anchor, request.manifest.plugins(), request.manifest.launch_args(), None);
    let anchor = request.anchor.display().to_string();
    // 呼び手の窓そのものへ起こす周（約束 7）: 前面の判定も入力欄の門も掛けず、送らずに置き換える。
    let same = crate::seat::target_of_caller(request.socket).as_deref() == Some(request.target);
    let carried = with_defaults(&derived, Some(defaults));
    let line = match launch_line(request.state_dir, &carried, &label, &anchor).and_then(|line| prepare(request, (&label, defaults.model), derived, same).map(|()| line)) {
        Ok(found) => with_tail(&found, request.carry),
        Err(reason) => return Launched::Refused(reason),
    };
    if same {
        // 記帳まで済ませてから置き換える（成功する周は返らない＝以後、器の行は 1 つも出ない）。
        record_launch(request, &label, started_at);
        return Launched::Failed(replace_with(&line));
    }
    let common = Boot {
        target: request.target,
        socket: request.socket,
        state_dir: request.state_dir,
        restore: request.restore,
        settle: request.settle,
        step: request.step,
    };
    let dir = crate::seat::seat_dir(&request.state_dir.path, request.target);
    let booted = boot(&common, &dir, (&line, WHEN_LAUNCH), || Ok(()));
    record_launch(request, &label, started_at);
    match booted {
        Booted::Done(Some(inject::Settled::Consumed)) => Launched::Done(label, Some(inject::Settled::Consumed)),
        Booted::Done(None) => Launched::Done(label, None),
        Booted::Done(Some(_)) => Launched::Failed(REASON_RESTORE),
        Booted::Failed(reason) => Launched::Failed(reason),
    }
}

/// 自分の process を `sh -c <起動行>` の **1 枚**で置き換える（約束 7・shell は 1 枚だけ＝どの層が起動行を解くかを曖昧に
/// しない）。key は 1 つも送らない（送る口はここを通らない）。
///
/// 成功する周は**返らない**ので、戻り値は失敗の理由 [`REASON_REPLACE`] だけである（`exec` が返る＝置き換えられなかった）。
fn replace_with(line: &str) -> &'static str {
    let failed = std::process::Command::new("sh").arg("-c").arg(line).exec();
    let _ = failed;
    REASON_REPLACE
}

/// 口座を決める: `--account` は宣言（開いた manifest の `[[account]]`）に在る label だけ（無ければ `account-unknown`）・
/// 無ければ session 用の選定（除外 = 他の席の登録 row の口座・候補なしは [`Launched::None`]）。event log を読めない周は
/// 選定に入らず断る。**ここまでは row も key も書かない**。
fn pick_account(request: &Launch) -> Result<String, Launched> {
    let labels: Vec<String> = request.manifest.accounts().iter().map(|account| account.label().to_owned()).collect();
    if let Some(label) = request.account {
        return labels.iter().any(|found| found == label).then(|| label.to_owned()).ok_or(Launched::Refused(REASON_ACCOUNT_UNKNOWN));
    }
    let events = store::read_all(&request.state_dir.path).map_err(|_| Launched::Refused(REASON_LOG_UNREADABLE))?;
    let state = replay(&events);
    let anchor = request.anchor.display().to_string();
    match choose((request.role, anchor.as_str(), None), &state, &labels, request.model, request.threshold_pct) {
        Selection::Chosen(label) => Ok(label),
        Selection::None(found) => Err(Launched::None(found)),
    }
}

/// 起動行を送る前の手（順序固定・設計 seat-roles.md §26 の約束 5 / 6 / 8）: session の実在（無ければ `session-missing`）→
/// **前提の断りを全部**（同じ窓の周は `--restore` を `restore-in-the-same-window`・違う窓で窓が既に在る周だけ前面が shell か
/// 〔`not-a-shell`〕と入力欄の門〔[`input_gate`]・`pane-missing` / `input-busy` / `input-unknown`〕）→ 登録 row を書く
/// （`sid` 無し・`launch` = 導出した行 `derived`〔穴を埋める前・旗無し〕・`model` = 行から導いた値の表示名＝実測の行と同じ語彙）→
/// 窓がまだ無い周だけ作る（[`create_window`]）。
///
/// **断りの 5 つは全部 row の前**で、row の後に残るのは `window-unwritable` だけである（`s2-07l.488` の実測＝断った周に
/// 点検の口が `registered=1 live=1` と出る形を塞ぐ）。判定の並びは従来のまま（session の有無 → 窓の前面 → 入力欄）で、
/// **窓がまだ無い周と同じ窓の周は前面と入力欄の判定を 2 つとも飛ばす**——前者は作る前の窓に pane が無く、後者は前面が
/// 起動の口自身で門が必ず閉じて見える（key を 1 つも送らないので門が守る「打ちかけの入力」も無い）。
fn prepare(request: &Launch, (label, model): (&str, Model), derived: String, same: bool) -> Result<(), &'static str> {
    let Some((session, window)) = request.target.split_once(':') else {
        return Err(REASON_SESSION_MISSING);
    };
    if crate::seat::tmux_stdout(request.socket, &["has-session", "-t", &format!("={session}")]).is_none() {
        return Err(REASON_SESSION_MISSING);
    }
    let exists = window_exists(request, session, window);
    if same {
        if request.restore.is_some() {
            return Err(REASON_RESTORE_SAME_WINDOW);
        }
    } else if exists {
        if !crate::seat::pane_is_shell(request.socket, request.target) {
            return Err(REASON_NOT_SHELL);
        }
        input_gate(request.socket, request.target)?;
    }
    let row = Registration {
        role: request.role,
        anchor: request.anchor.display().to_string(),
        target: request.target.to_owned(),
        sid: None,
        account: label.to_owned(),
        launch: derived,
        model: Some(model.display().to_owned()),
    };
    role::register(&request.state_dir.path, row).map_err(|_| REASON_REGISTER)?;
    if exists {
        return Ok(());
    }
    create_window(request, session, window)
}

/// target の window が既に在るか（`list-windows -F '#{window_name}'` を exact の session 名で引く）。**撃てない周は「無い」側**
/// （`new-window` へ進み、それも撃てなければ `window-unwritable`）。[`prepare`] が 1 回だけ測り、前面の判定と窓を作るかの
/// 両方がこの 1 つの値を読む（同じ事実を 2 度撃たない）。
fn window_exists(request: &Launch, session: &str, window: &str) -> bool {
    crate::seat::tmux_stdout(request.socket, &["list-windows", "-t", &format!("={session}"), "-F", "#{window_name}"])
        .unwrap_or_default()
        .lines()
        .any(|found| found == window)
}

/// window を作る（まだ無い周だけ・登録 row の後）: `new-window` が失敗すれば `window-unwritable`・作れたら shell の prompt が
/// 描かれるまで窓（`settle`）の内で待つ（作った直後の空の pane へ起動行を送らない）。
///
/// session は `=<session>:` で名指す: `=` は前方一致でない exact の名・末尾の `:` は「その session の次の空き index」
/// （`-t <session>` の裸の名は、session と同じ名の window が在る周に **window** として解決され `index in use` で落ちる・
/// 実測 2026-09-14 tmux 3.6b）。
fn create_window(request: &Launch, session: &str, window: &str) -> Result<(), &'static str> {
    if !tmux_ok(request.socket, &["new-window", "-t", &format!("={session}:"), "-n", window]) {
        return Err(REASON_WINDOW);
    }
    let deadline = Instant::now().checked_add(request.settle);
    while before_deadline(Instant::now(), deadline) {
        let pane = crate::seat::tmux_stdout(request.socket, &["capture-pane", "-p", "-J", "-t", request.target]);
        if pane.is_some_and(|found| crate::seat::shell_input_empty(&found).is_ok()) {
            break;
        }
        sleep(request.step);
    }
    Ok(())
}

/// 窓の判定（**pure**・C10「時計は測定値・判定は pure」・`s2-07l.344`）: 測った `now` が `deadline` の**手前**なら true。
/// 等しい周は手前ではない（`<` は strict・境界は歯で pin する）。`deadline` が無い（`checked_add` が溢れた）周は窓が
/// 閉じている側（false）＝待たない。呼び手（[`open_window`] / [`super::relaunch`] の復元の窓）は `Instant::now()` を渡す。
pub(super) fn before_deadline(now: Instant, deadline: Option<Instant>) -> bool {
    deadline.is_some_and(|at| now < at)
}

/// 起動を `inject.jsonl` に 1 行記録する（`who=seat-launch`・`what` は `decision=inject … kind=launch` の形・
/// `when=launch`）。**置き場へ書けない周も結果を変えない**。
fn record_launch(request: &Launch, label: &str, started: Instant) {
    let kind = super::KIND_LAUNCH;
    let what = format!("decision=inject target={} kind={kind} account={label}", sanitize_target(request.target));
    let entry = InjectionRecord {
        schema: SCHEMA,
        who: WHO_LAUNCH.to_owned(),
        bytes: what.len() as u64,
        what,
        when: WHEN_LAUNCH.to_owned(),
        // 数えていないことを 0 と書かない。
        tokens: None,
        wall_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        seat: seat_name(request.target),
        ts: state::now_secs(),
    };
    let _ = crate::hook::append(&request.state_dir.path, &entry);
}

/// `seat launch` の 1 行（成立・断り・失敗）。置き場の 2 語を末尾に載せる（cycle と同じ規律）。
///
/// `not-a-shell` の断りだけ**次の 1 手**（[`NEXT_AFTER_NOT_SHELL`]・約束 9）を置き場の 2 語の**前**に足す
/// （path は行末のまま＝出所を偽れない）。その窓には生きた席が在り器は殺さないので、手は人が選ぶ。
pub fn render_launched(target: &str, result: &Launched, state: &StateDir) -> String {
    let suffix = state.suffix();
    let target = sanitize_target(target);
    match result {
        Launched::Done(label, None) => format!("seat launch: launched target={target} account={label}{suffix}"),
        Launched::Done(label, Some(settled)) => {
            format!("seat launch: launched target={target} account={label} consumed={}{suffix}", settled.as_str())
        }
        Launched::None(found) => {
            format!("seat launch: refused reason={REASON_NO_ACCOUNT} detail={} target={target}{suffix}", found.reason.as_str())
        }
        Launched::Refused(reason) => {
            let next = if *reason == REASON_NOT_SHELL { format!(" next={NEXT_AFTER_NOT_SHELL}") } else { String::new() };
            format!("seat launch: refused reason={reason} target={target}{next}{suffix}")
        }
        Launched::Failed(reason) => format!("seat launch: failed reason={reason} target={target}{suffix}"),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        before_deadline, derive_launch, fill_launch, model_of, seat_defaults, single_model, with_agent_view_off, with_anchor_cd,
        with_defaults, with_flags, Holes, Model, Role, RoleDefaults, RuleRead, HOLE, HOLES, REASON_EFFORT_DUPLICATED,
        REASON_MODEL_DUPLICATED, REASON_MODEL_MISMATCH,
    };
    use crate::headless::Effort;
    use crate::order::is_declaration_order;
    use crate::rules::manifest::Manifest;
    use std::path::Path;
    use std::time::{Duration, Instant};

    /// 窓の判定は pure な 4 値表（`s2-07l.344`・.319 の検出線の生存 `<` × 3 を潰す）: `now < at` は手前（true）・
    /// `now == at` は手前ではない（strict・壁時計の等号を待って測らない）・`now > at` は過ぎている・`deadline = None`
    /// は窓が無い（false）。`now` は測定値を 1 回だけ取り、表は Instant の算術だけで作る（sleep しない）。
    #[test]
    fn cycle_deadline_before_is_strict_and_none_is_expired() {
        let now = Instant::now();
        let later = now + Duration::from_millis(1);
        assert!(before_deadline(now, Some(later)), "now < at は手前");
        assert!(!before_deadline(now, Some(now)), "now == at は手前ではない（strict）");
        assert!(!before_deadline(later, Some(now)), "now > at は過ぎている");
        assert!(!before_deadline(now, None), "deadline が無い周は待たない");
    }

    /// 起動行の導出（契約 (6f)・account-lifecycle.md §4）: 穴は `{account_dir}` の 1 つ（[`fill_launch`] がそのまま埋める）・
    /// 順序は agent view off → 口座の env → `claude` → anchor の `--plugin-dir` → `[[plugin]]` の dir（宣言順）→
    /// `[[launch-arg]]` の value（宣言順）。plugin 0 件・引数 0 件は anchor の `--plugin-dir` だけで終わる。後半は model の運び（`s2-07l.313`）。
    #[test]
    fn seat_launch_derive_line_orders_anchor_plugins_and_args_with_one_hole() {
        let host = "schema = 1\n\n[[plugin]]\ndir = \"/opt/p2\"\n\n[[launch-arg]]\nvalue = \"--permission-mode\"\n\n\
                    [[plugin]]\ndir = \"/opt/p1\"\n\n[[launch-arg]]\nvalue = \"bypassPermissions\"\n";
        let manifest = Manifest::parse(host).unwrap_or_default();
        assert_eq!(manifest.plugins().len(), 2, "fixture が読める");
        let line = derive_launch(Path::new("/repo/main"), manifest.plugins(), manifest.launch_args(), None);
        assert_eq!(
            line,
            "CLAUDE_CODE_DISABLE_AGENT_VIEW=1 CLAUDE_CONFIG_DIR={account_dir} claude --plugin-dir /repo/main \
             --plugin-dir /opt/p2 --plugin-dir /opt/p1 --permission-mode bypassPermissions",
            "宣言順（p2 → p1・--permission-mode → bypassPermissions）"
        );
        assert_eq!(line.matches(HOLE).count(), 1, "穴は 1 つ");
        assert_eq!(
            fill_launch(&line, "/state/accounts/a2").as_deref(),
            Ok("CLAUDE_CODE_DISABLE_AGENT_VIEW=1 CLAUDE_CONFIG_DIR=/state/accounts/a2 claude --plugin-dir /repo/main \
                --plugin-dir /opt/p2 --plugin-dir /opt/p1 --permission-mode bypassPermissions"),
            "穴は既存の fill_launch で埋まる"
        );
        assert_eq!(with_agent_view_off(&line), line, "前置は既に在る（二重にしない）");
        let bare = derive_launch(Path::new("/repo/main"), &[], &[], None);
        assert_eq!(bare, "CLAUDE_CODE_DISABLE_AGENT_VIEW=1 CLAUDE_CONFIG_DIR={account_dir} claude --plugin-dir /repo/main");
        // 既定の対（C10・§20 の約束 1）: `claude` の直後に `--model <別名>` → `--effort <値>` の順で 1 つずつ・None は従来の行と同一・
        // 雛形へ挟むのも同じ位置（`claude` の語が無い雛形は末尾）・旗ごとに 2 つの行だけを旗ごとの理由で断る（約束 2）。
        let pair = RoleDefaults { model: Model::Fable, effort: Effort::High };
        let fable = derive_launch(Path::new("/repo/main"), &[], &[], Some(pair));
        assert_eq!(
            fable,
            "CLAUDE_CODE_DISABLE_AGENT_VIEW=1 CLAUDE_CONFIG_DIR={account_dir} claude --model fable --effort high --plugin-dir /repo/main"
        );
        assert_eq!(with_defaults(&bare, Some(pair)), fable, "導出の後に運んでも同じ行");
        assert_eq!(
            with_defaults("sh l.sh {account_dir}", Some(RoleDefaults { model: Model::Opus, effort: Effort::Xhigh })),
            "sh l.sh {account_dir} --model opus --effort xhigh",
            "`claude` の語が無い雛形は末尾"
        );
        assert_eq!(with_flags(&bare, &[]), bare, "空の列は行をそのまま");
        let twice_model = with_flags(&fable, &[("--model", "opus")]);
        let twice_effort = with_flags(&fable, &[("--effort", "low")]);
        assert_eq!(
            (single_model(&fable), single_model(&twice_model), single_model(&twice_effort)),
            (Ok(()), Err(REASON_MODEL_DUPLICATED), Err(REASON_EFFORT_DUPLICATED))
        );
        assert_ne!(REASON_MODEL_DUPLICATED, REASON_EFFORT_DUPLICATED, "旗ごとに違う理由");
        assert_eq!((model_of(None), model_of(Some("Fable")), model_of(Some("opus")), model_of(Some("nope"))), (Ok(None), Ok(Some(Model::Fable)), Ok(Some(Model::Opus)), Err(())));
    }

    /// `[[rule]]` 2 行（役割の既定の対）の manifest。kind / 値 / 発効は引数で崩せる（`None` は行を置かない）。
    fn rules_with(model: Option<(&str, &str, bool)>, effort: Option<(&str, &str, bool)>) -> Manifest {
        let role = Role::Orchestrator;
        let row = |id: String, (kind, value, enabled): (&str, &str, bool)| {
            format!("[[rule]]\nid = \"{id}\"\nkind = \"{kind}\"\nvalue = {value}\nenabled = {enabled}\nruling = \"r\"\nruled_at = \"d\"\n\n")
        };
        let text = format!(
            "schema = 1\n\n{}{}",
            model.map(|found| row(role.model_row(), found)).unwrap_or_default(),
            effort.map(|found| row(role.effort_row(), found)).unwrap_or_default()
        );
        match Manifest::parse(&text) {
            Ok(found) => found,
            Err(errors) => panic!("fixture の manifest を読める: {errors:?}"),
        }
    }

    /// 約束 7（§20・fail-closed・C10）: 行なし・不発効・表に無い の 3 形の manifest を pure 側に渡すと既定を導かず（＝起動行を
    /// 組む入力が無い）、断りの理由は [`RuleRead`] の字面で 3 形ごとに違う。読める manifest は対を返し、`--model` は照合になる
    /// （一致は通り・食い違いは `launch-model-mismatch`・無しは行の値）。埋め込み manifest は常に読めるので e2e では測れない。
    #[test]
    fn seat_defaults_unreadable_rows_refuse_with_the_rule_read_reason() {
        let role = Role::Orchestrator;
        let good = rules_with(Some(("RoleModel", "\"fable\"", true)), Some(("RoleEffort", "\"high\"", true)));
        let cases = [
            (rules_with(None, Some(("RoleEffort", "\"high\"", true))), RuleRead::Missing, "行なし"),
            (rules_with(Some(("RoleModel", "\"fable\"", false)), Some(("RoleEffort", "\"high\"", true))), RuleRead::Disabled, "不発効"),
            (rules_with(Some(("RoleModel", "\"fable\"", true)), Some(("DialogueSurface", "\"orchestrator\"", true))), RuleRead::NotInTable, "表に無い"),
        ];
        let mut reasons = Vec::new();
        for (rules, read, what) in &cases {
            let refused = seat_defaults(rules, role, None);
            assert_eq!(refused, Err(read.no_rule()), "{what}");
            assert!(refused.is_err_and(|reason| reason.starts_with("no-rule:") && reason.ends_with(read.as_str())), "{what}");
            reasons.push(read.no_rule());
        }
        reasons.dedup();
        assert_eq!(reasons.len(), cases.len(), "3 形ごとに違う理由: {reasons:?}");
        let pair = RoleDefaults { model: Model::Fable, effort: Effort::High };
        assert_eq!(seat_defaults(&good, role, None), Ok(pair), "無しは行の値");
        assert_eq!(seat_defaults(&good, role, Some(Model::Fable)), Ok(pair), "一致は通る");
        assert_eq!(seat_defaults(&good, role, Some(Model::Opus)), Err(REASON_MODEL_MISMATCH), "食い違いは断る");
        assert_eq!(
            seat_defaults(&good, role, None).map(|found| derive_launch(Path::new("/r"), &[], &[], Some(found))).as_deref(),
            Ok("CLAUDE_CODE_DISABLE_AGENT_VIEW=1 CLAUDE_CONFIG_DIR={account_dir} claude --model fable --effort high --plugin-dir /r"),
            "読める周だけ起動行を組む"
        );
    }

    /// 起動行の先頭に agent view を切る env を 1 つだけ前置する: 行の中身は変えず、既に前置済みの行は二重にせず、空の行は
    /// そのまま（契約 (c)・`s2-07l.239`）。
    #[test]
    fn seat_agent_view_off_prefix_is_single_and_keeps_blank_lines() {
        let line = "CLAUDE_CONFIG_DIR=/state/accounts/a2 claude --resume";
        let once = with_agent_view_off(line);
        assert_eq!(once, "CLAUDE_CODE_DISABLE_AGENT_VIEW=1 CLAUDE_CONFIG_DIR=/state/accounts/a2 claude --resume");
        assert_eq!(with_agent_view_off(&once), once, "前置済みの行は二重にしない");
        assert_eq!(
            with_agent_view_off("sh l.sh /state/accounts/a2"),
            "CLAUDE_CODE_DISABLE_AGENT_VIEW=1 sh l.sh /state/accounts/a2",
            "env で始まらない雛形にも前置する"
        );
        assert_eq!(with_agent_view_off(""), "", "空の行はそのまま");
        assert_eq!(with_agent_view_off("  "), "  ", "空白だけの行もそのまま");
    }

    /// 起動行の先頭に row の anchor への `cd '<anchor>' && ` を 1 つだけ前置する（`s2-07l.324`）: agent view の env より前
    /// （`cd … && ENV=… claude …` の順）・行の中身は変えず・既に同じ前置で始まる行は二重にせず・空の行はそのまま。
    /// anchor は引数の値をそのまま写す（env も cwd も読まない・C2.2）。
    #[test]
    fn seat_launch_anchor_cd_prefix_is_single_and_keeps_blank_lines() {
        let line = with_agent_view_off("CLAUDE_CONFIG_DIR=/state/accounts/a2 claude --plugin-dir /repo/main");
        let once = with_anchor_cd(&line, "/repo/main");
        assert_eq!(once, "cd '/repo/main' && CLAUDE_CODE_DISABLE_AGENT_VIEW=1 CLAUDE_CONFIG_DIR=/state/accounts/a2 claude --plugin-dir /repo/main");
        assert_eq!(with_anchor_cd(&once, "/repo/main"), once, "前置済みの行は二重にしない");
        assert_eq!(with_anchor_cd("sh l.sh /state/accounts/a2", "/repo/acct"), "cd '/repo/acct' && sh l.sh /state/accounts/a2", "env で始まらない雛形にも前置する");
        assert_eq!(with_anchor_cd("", "/repo/main"), "", "空の行はそのまま");
        assert_eq!(with_anchor_cd("  ", "/repo/main"), "  ", "空白だけの行もそのまま");
    }

    /// 雛形の穴はちょうど 1 つだけが埋まり（文字列の置換だけ・env の字面も path の字面も解釈しない）、無い・2 つ
    /// 以上は typed に断る（account-autonomy.md §5・契約 (d)）。
    #[test]
    fn seat_account_fill_launch_fills_exactly_one_hole() {
        let dir = "/state/accounts/a2";
        assert_eq!(
            fill_launch("CLAUDE_CONFIG_DIR={account_dir} claude --resume", dir),
            Ok("CLAUDE_CONFIG_DIR=/state/accounts/a2 claude --resume".to_owned())
        );
        assert_eq!(
            fill_launch("$CONFIG_ROOT/cld --host box {account_dir}", dir),
            Ok("$CONFIG_ROOT/cld --host box /state/accounts/a2".to_owned()),
            "env の字面と host 名は解釈しない（そのまま残す）"
        );
        assert_eq!(fill_launch("claude --resume", dir), Err(Holes::Missing));
        assert_eq!(fill_launch("{account-dir} {credential-dir}", dir), Err(Holes::Missing), "似た字面は穴ではない");
        assert_eq!(fill_launch("{account_dir} {account_dir}", dir), Err(Holes::Many));
        assert_eq!(fill_launch("{account_dir}{account_dir}{account_dir}", dir), Err(Holes::Many));
        let names: Vec<&str> = HOLES.iter().map(|holes| holes.as_str()).collect();
        assert_eq!(names, ["launch-no-hole", "launch-many-holes"]);
        assert!(is_declaration_order(HOLES, |holes| holes as usize));
    }
}
