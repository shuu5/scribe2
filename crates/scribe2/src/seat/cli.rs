//! `seat` に続く引数の面（設計 §3）。
//!
//! **env も HOME も読まない**（憲法 C2.2）: 出所（pane / transcript）も置き場も
//! 引数で明示されたものだけを見る。値欠けの flag は黙って落とさず使い方で断る。

use super::cycle;
use super::role;
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_REFUSED};
use crate::fleet::select::Model;
use crate::rules::RuleError;
use std::path::Path;

/// `seat` の使い方。
pub fn usage() -> String {
    "usage: seat <register --state-dir S --target T --role R --account L --launch FILE [--anchor DIR]|launch --state-dir S --role R --target S:W [--account L] [--anchor DIR] [--model M] [--restore CMD]|<label> --orchestrator [--target S:W] [--model M] [--anchor DIR] [--restore CMD] [--state-dir S]> [--tmux-socket PATH] [--capture-file PATH] [--state-dir PATH]".to_owned()
}

/// `seat` に続く引数を捌く。
pub fn dispatch(args: &[String]) -> Outcome {
    match args.first().map(String::as_str) {
        Some("register") => register_of(args),
        Some("launch") => launch_of(args),
        // 既知の verb でなく `--` で始まらない第 1 token は口座 label（短い形・account-lifecycle.md §14）。
        Some(label) if !label.starts_with("--") && !label.trim().is_empty() => short_of(label, args.get(1..).unwrap_or_default()),
        _ => refused_usage(),
    }
}

/// 使い方を stderr へ出して rc 1。
fn refused_usage() -> Outcome {
    Outcome::failed(RC_REFUSED, vec![usage()])
}

/// `--<name> <value>` の読み取り結果。
enum Flag<'a> {
    /// flag そのものが無い。
    Absent,
    /// 値が在る。
    Value(&'a str),
    /// flag は在るが値が無い（末尾か、次が別の flag）。
    Missing,
}

/// `--<name>` を読む。
fn flag<'a>(args: &'a [String], name: &str) -> Flag<'a> {
    let Some(at) = args.iter().position(|arg| arg == name) else {
        return Flag::Absent;
    };
    match args.get(at.saturating_add(1)) {
        Some(found) if !found.starts_with("--") => Flag::Value(found),
        _ => Flag::Missing,
    }
}

/// 任意の flag。**値欠けは黙って落とさず断る**（SRS NFR4）。
fn optional<'a>(args: &'a [String], name: &str) -> Result<Option<&'a str>, ()> {
    match flag(args, name) {
        Flag::Absent => Ok(None),
        Flag::Value(found) => Ok(Some(found)),
        Flag::Missing => Err(()),
    }
}

/// 空文字を**使い方の誤り**として断る任意の flag。
///
/// 空の text は `send-keys -l ""` が rc 0 で終わるので、渡し忘れが「送った」に化ける
/// （実測 2026-09-10・空の口は成功に化ける）。断る側へ倒し、1 key も送らない。
fn nonempty<'a>(args: &'a [String], name: &str) -> Result<Option<&'a str>, ()> {
    match optional(args, name)? {
        Some(found) if found.trim().is_empty() => Err(()),
        other => Ok(other),
    }
}

/// 必須の flag のうち**空文字を断る**もの。
fn required_nonempty<'a>(args: &'a [String], name: &str) -> Result<&'a str, ()> {
    nonempty(args, name)?.ok_or(())
}

/// 壊れた manifest の断り: defect を 1 件 1 行（`rules validate` と 1 byte 同じ）で全件並べ、
/// 末尾に口の既存の断り行 `judged` を足す（設計 rules-manifest.md §5「同じ拒否 5 形」・
/// seat-autonomy.md §3 の 1 周 1 判定行・`s2-07l.154`）。rc は `rules` / `pipe` / `fleet` と同じ 1。
fn broken_rules(errors: &[RuleError], mut judged: Vec<String>) -> Outcome {
    let mut lines = crate::rules::cli::render_defects(errors);
    lines.append(&mut judged);
    Outcome::failed(RC_REFUSED, lines)
}

/// `seat register`（設計 seat-roles.md §2）。未知の `--role` は使い方の誤りとして断る。`--model M` は任意
/// （席が使う model の表示名か別名・[`Model::parse`] の表に無い値は `--role` と同じく使い方の誤り＝未知の値を row に書かない・
/// `--account` と同じ受け方＝空文字も使い方の誤り・契約 (e) / `s2-07l.313`）。
fn register_of(args: &[String]) -> Outcome {
    let [state_dir, target, role, account, launch] = ["--state-dir", "--target", "--role", "--account", "--launch"].map(|name| required_nonempty(args, name));
    let (Ok(state_dir), Ok(target), Ok(Some(role)), Ok(account), Ok(launch), Ok(anchor), Ok(model)) =
        (state_dir, target, role.map(role::Role::parse), account, launch, nonempty(args, "--anchor"), nonempty(args, "--model"))
    else {
        return refused_usage();
    };
    if model.is_some_and(|found| Model::parse(found).is_none()) {
        return refused_usage();
    }
    let draft = crate::fleet::Registration {
        role,
        target: target.to_owned(),
        account: account.to_owned(),
        anchor: String::new(),
        sid: None,
        launch: String::new(),
        model: model.map(str::to_owned),
    };
    let refused = |rc, err: role::RegisterRefusal| Outcome::failed_line(rc, err.render(target));
    match role::register_stamped(Path::new(state_dir), draft, Path::new(launch), anchor.map(Path::new)) {
        Ok(done) => {
            // `model` の無い row は従来の行のまま（既存の外形を変えない）・在る row は末尾に 1 語足す。この口の row は
            // 打刻から解いた `sid` を必ず持つ（無い形は launch の row だけ・その行は `none` を名乗る）。
            let model = done.model.as_ref().map(|found| format!(" model={found}")).unwrap_or_default();
            let sid = done.sid.as_deref().unwrap_or("none");
            Outcome::ok_line(format!("seat register: registered role={} target={} sid={sid} account={} anchor={}{model}", done.role.as_str(), done.target, done.account, done.anchor))
        }
        Err(err @ role::RegisterRefusal::Store(_)) => refused(RC_BROKEN, err),
        Err(err) => refused(RC_REFUSED, err),
    }
}

/// `seat launch` の起動の入力（設計 account-lifecycle.md §4 / §14）: 長い形は flag から・短い形は登録 row の既定を埋めて
/// **同じこの 1 つ**を組み、同じ [`launch_with`] を通る（同じ Registration・同じ起動行）。読むのはここ・処理は [`cycle::launch`]。
struct LaunchFlags<'a> {
    /// `--target S:W`（`session:window` の両方が非空・短い形は row の値が既定）。
    target: &'a str,
    /// `--role`（閉じた [`role::Role`]・短い形は `--orchestrator`）。
    role: role::Role,
    /// `--account` / `--model` / `--restore` / `--tmux-socket`（任意・空文字は使い方の誤り・`--model` の表に無い値は
    /// [`cycle::launch`] が `launch-model-unknown` で断る・短い形の口座は第 1 token の label）。
    account: Option<&'a str>,
    model: Option<&'a str>,
    restore: Option<&'a str>,
    socket: Option<&'a str>,
}

/// 長い形の引数: 解く前の置き場と anchor の flag（[`launch_place`] が解く）と起動の入力。
struct LaunchArgs<'a> {
    /// `--state-dir`（必須・空文字は使い方の誤り）。
    state_dir: &'a str,
    /// `--anchor`（任意）。
    anchor: Option<&'a str>,
    flags: LaunchFlags<'a>,
}

/// 起動の置き場と anchor（長い形・短い形が [`launch_place`] の同じ解き方で先に解く・`seat register` とも同じ）。
struct LaunchPlace {
    /// 解決済みの置き場（出所付き）。
    state: super::StateDir,
    /// 登録 row の anchor（絶対 path）。
    anchor: std::path::PathBuf,
}

/// `--target` の形（`session:window` の両方が非空）。
fn target_well_formed(target: &str) -> bool {
    target.split_once(':').is_some_and(|(session, window)| !session.is_empty() && !window.is_empty())
}

/// `seat launch` の flag を読む（長い形）。値欠け・空文字・未知の `--role`・`S:W` でない `--target` は `None`（使い方で断る）。
fn launch_flags(args: &[String]) -> Option<LaunchArgs<'_>> {
    let [state_dir, target, role] = ["--state-dir", "--target", "--role"].map(|name| required_nonempty(args, name).ok());
    let [account, anchor, model, restore, socket] =
        ["--account", "--anchor", "--model", "--restore", "--tmux-socket"].map(|name| nonempty(args, name).ok());
    let (Some(state_dir), Some(target), Some(role)) = (state_dir, target, role.and_then(role::Role::parse)) else {
        return None;
    };
    if !target_well_formed(target) {
        return None;
    }
    Some(LaunchArgs {
        state_dir,
        anchor: anchor?,
        flags: LaunchFlags {
            target,
            role,
            account: account?,
            model: model?,
            restore: restore?,
            socket: socket?,
        },
    })
}

/// `seat launch`（設計 account-lifecycle.md §4・ADR-0026 §2.3・SRS FR59）: 置き場と anchor（`seat register` と同じ解き方）を
/// 解き、[`launch_with`] へ渡す（長い形）。
fn launch_of(args: &[String]) -> Outcome {
    let Some(args) = launch_flags(args) else {
        return refused_usage();
    };
    match launch_place(Some(args.state_dir), args.anchor, Some(args.flags.target)) {
        Ok(place) => launch_with(&args.flags, &place),
        Err(refused) => refused,
    }
}

/// 短い形の既定が解けない理由（account-lifecycle.md §14・row も flag も無い・`cycle::launch` の前で終わる断り＝
/// [`cycle::Launched`] の variant ではない）。
const REASON_DEFAULTS_UNRESOLVED: &str = "defaults-unresolved";

/// 短い形 `seat <label> --orchestrator [--target S:W] [--model M] [--anchor DIR] [--restore CMD] [--state-dir S]`
/// （設計 account-lifecycle.md §14・SRS FR59 / FR40）: 役割の flag は**ちょうど 1 つ**（0 か 2 は使い方の誤り）・置き場と anchor は
/// 長い形と同じ解き方・target と model は明示の flag が無ければ同じ鍵（役割 × anchor）の登録 row の値（[`short_defaults`]）。
/// 解けた周は長い形と同じ [`LaunchFlags`]（口座 = label）を組んで同じ [`launch_with`] を通る。
fn short_of(label: &str, args: &[String]) -> Outcome {
    let Some(role) = short_role_of(args) else {
        return refused_usage();
    };
    // 値欠け・空文字は使い方の誤り（他の flag と同じ極性・SRS NFR4）。
    let [target, model, anchor, restore, state_dir, socket] =
        ["--target", "--model", "--anchor", "--restore", "--state-dir", "--tmux-socket"].map(|name| nonempty(args, name).ok());
    let (Some(target), Some(model), Some(anchor), Some(restore), Some(state_dir), Some(socket)) = (target, model, anchor, restore, state_dir, socket) else {
        return refused_usage();
    };
    if target.is_some_and(|found| !target_well_formed(found)) {
        return refused_usage();
    }
    let place = match launch_place(state_dir, anchor, target) {
        Ok(found) => found,
        Err(refused) => return refused,
    };
    let (target, model) = match short_defaults(&place, role, target, model) {
        Ok(found) => found,
        Err(refused) => return refused,
    };
    let flags = LaunchFlags { target: &target, role, account: Some(label), model: Some(&model), restore, socket };
    launch_with(&flags, &place)
}

/// 短い形の役割の flag（`--orchestrator`＝[`role::Role`] の字面に `--` を前置した形）。**ちょうど 1 つ**の周だけ `Some`
/// （0 個・同じ flag の重複は `None`＝使い方の誤り）。
fn short_role_of(args: &[String]) -> Option<role::Role> {
    let mut roles = args.iter().filter_map(|arg| arg.strip_prefix("--").and_then(role::Role::parse));
    let role = roles.next()?;
    roles.next().is_none().then_some(role)
}

/// 短い形の既定を **1 関数で導く**（§14）: 明示の flag は row の値に勝ち、無ければ同じ鍵（役割 × anchor）の登録 row の
/// `target` / `model`（[`role::registration_of_key`]）。両方が明示の周は log を読まない。row の `model` が無い周は `--model` が
/// 要る。足りない flag は宣言順（`--target` → `--model`）で `missing=` に載せて `defaults-unresolved` で断る（1 key も送らず
/// row も書かない）。log を読めない周は `log-unreadable`（「row が無い」と混ぜない・fail-closed）。
fn short_defaults(place: &LaunchPlace, role: role::Role, target: Option<&str>, model: Option<&str>) -> Result<(String, String), Outcome> {
    let row = match (target, model) {
        (Some(_), Some(_)) => None,
        _ => {
            let events = crate::fleet::store::read_all(&place.state.path)
                .map_err(|_| refused_before_launch(cycle::REASON_LOG_UNREADABLE, target, Some(&place.state)))?;
            let state = crate::fleet::replay(&events);
            role::registration_of_key(&state, role, &place.anchor.display().to_string()).cloned()
        }
    };
    let target = target.map(str::to_owned).or_else(|| row.as_ref().map(|found| found.target.clone()));
    let model = model.map(str::to_owned).or_else(|| row.as_ref().and_then(|found| found.model.clone()));
    match (target, model) {
        (Some(target), Some(model)) => Ok((target, model)),
        (target, model) => {
            let missing: Vec<&str> = [("--target", target.is_none()), ("--model", model.is_none())]
                .into_iter()
                .filter_map(|(name, absent)| absent.then_some(name))
                .collect();
            Err(Outcome::failed_line(RC_REFUSED, render_defaults_unresolved(&missing)))
        }
    }
}

/// 既定が解けない周の断りの 1 行（§14 が pin する字面）。**`target=` も置き場の 2 語も載せない**: target が解けない周にも出る
/// 断りに未確定の値を置かない。
fn render_defaults_unresolved(missing: &[&str]) -> String {
    format!("seat launch: refused reason={REASON_DEFAULTS_UNRESOLVED} missing={}", missing.join(","))
}

/// 置き場と anchor を解く（長い形・短い形の同じ 1 本・`seat register` と同じ解き方）: `--state-dir` > git 設定（解けなければ
/// `state-dir`）・`--anchor` か cwd の repo root（解けなければ `anchor-unresolvable`）。
fn launch_place(state_dir: Option<&str>, anchor: Option<&str>, target: Option<&str>) -> Result<LaunchPlace, Outcome> {
    let Some(state) = super::state_dir_of(state_dir) else {
        return Err(refused_before_launch(cycle::REASON_STATE_DIR, target, None));
    };
    let Some(anchor) = role::anchor_of(anchor.map(Path::new)) else {
        return Err(refused_before_launch(cycle::REASON_ANCHOR, target, Some(&state)));
    };
    Ok(LaunchPlace { state, anchor })
}

/// [`cycle::launch`] の手前の断り（置き場・anchor・log）: target が既知の周だけ `target=` を載せ、置き場が解けた周だけ 2 語を
/// 載せる（解けていない値を行に置かない・長い形の行は従来の字面のまま）。
fn refused_before_launch(reason: &'static str, target: Option<&str>, state: Option<&super::StateDir>) -> Outcome {
    let line = match (target, state) {
        (Some(target), Some(state)) => cycle::render_launched(target, &cycle::Launched::Refused(reason), state),
        (Some(target), None) => format!("seat launch: refused reason={reason} target={target}"),
        (None, Some(state)) => format!("seat launch: refused reason={reason}{}", state.suffix()),
        (None, None) => format!("seat launch: refused reason={reason}"),
    };
    Outcome::failed_line(RC_REFUSED, line)
}

/// 起動の本体（長い形・短い形の同じ 1 本）: manifest（tracked + 置き場の host の面）を 1 回だけ開いて確認の刻みと R-C9-1 の値を
/// 読み、[`cycle::launch`] へ渡す。
fn launch_with(flags: &LaunchFlags, place: &LaunchPlace) -> Outcome {
    let state = &place.state;
    let refused = |reason: &'static str| {
        Outcome::failed_line(RC_REFUSED, cycle::render_launched(flags.target, &cycle::Launched::Refused(reason), state))
    };
    // 宣言（`[[account]]` / `[[plugin]]` / `[[launch-arg]]`）と確認の刻みは同じ 1 つの manifest から読む（第 2 の parser を作らない）。
    let manifest = match crate::rules::read(None, Some(&state.path)) {
        Ok(manifest) => manifest,
        Err(errors) => return broken_rules(&errors, vec![cycle::render_launched(flags.target, &cycle::Launched::Refused(cycle::REASON_NO_RULE), state)]),
    };
    let Some((settle, step)) = cycle::pace_of(&manifest) else {
        return refused(cycle::REASON_NO_RULE);
    };
    let threshold_pct = match super::int_rule_of(&manifest, super::ID_THRESHOLD) {
        Ok(found) => found,
        Err(read) => return refused(read.no_rule()),
    };
    let result = cycle::launch(&cycle::Launch {
        target: flags.target,
        socket: flags.socket,
        state_dir: state,
        restore: flags.restore,
        settle,
        step,
        role: flags.role,
        anchor: &place.anchor,
        account: flags.account,
        model: flags.model,
        manifest: &manifest,
        threshold_pct,
    });
    let line = cycle::render_launched(flags.target, &result, state);
    match result {
        cycle::Launched::Done(..) => Outcome::ok_line(line),
        cycle::Launched::None(_) | cycle::Launched::Refused(_) | cycle::Launched::Failed(_) => Outcome::failed_line(RC_REFUSED, line),
    }
}
