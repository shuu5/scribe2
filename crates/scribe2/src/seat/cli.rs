//! `seat` に続く引数の面（設計 §3）。
//!
//! **env も HOME も読まない**（憲法 C2.2）: 出所（pane / transcript）も置き場も
//! 引数で明示されたものだけを見る。値欠けの flag は黙って落とさず使い方で断る。

use super::cycle;
use super::role;
use super::tick::install::Verb;
use crate::cli_args::{self, Allowed};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_REFUSED};
use crate::fleet::select::Model;
use crate::rules::RuleError;
use std::path::Path;

/// `seat` の使い方。
pub fn usage() -> String {
    "usage: seat <register --state-dir S --target T --role R --account L --launch FILE [--anchor DIR]|launch --state-dir S --role R --target S:W [--account L] [--anchor DIR] [--model M] [--restore CMD]|ruling add --state-dir S --target T --words W [--bead B] [--rule ID]|ruling ls --state-dir S|tick --state-dir S --target S:W [--rules F]|tick install --state-dir S --target S:W --unit-dir U --binary PATH [--rules F]|tick uninstall --state-dir S --target S:W --unit-dir U --binary PATH [--rules F]|<label> [--orchestrator] [-c|-r ID] [--target S:W] [--model M] [--anchor DIR] [--restore CMD] [--state-dir S]> [--tmux-socket PATH] [--capture-file PATH] [--state-dir PATH]".to_owned()
}

/// `seat` の既知の verb（閉じた語・宣言順・設計 contract-source.md §17 の形 (vii)）。短い形の第 1 token（口座 label）は
/// 閉じた集合を持たないので語にしない＝[`SeatCommand::parse`] が `None` を返した token は [`dispatch`] の label の腕へ落ちる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeatCommand {
    /// `seat register`。
    Register,
    /// `seat launch`。
    Launch,
    /// `seat ruling add` / `seat ruling ls`（run 無しの裁定・設計 fleet-event-log.md §9）。
    Ruling,
    /// `seat tick`（管理 tick・設計 seat-heartbeat.md §2）。
    Tick,
}

/// [`SeatCommand`] の全部（宣言順・件数は既知の verb の本数で dispatch の腕の本数ではない）。
pub const SEAT_COMMANDS: &[SeatCommand] = &[SeatCommand::Register, SeatCommand::Launch, SeatCommand::Ruling, SeatCommand::Tick];

impl SeatCommand {
    /// 引数の字面。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Register => "register",
            Self::Launch => "launch",
            Self::Ruling => "ruling",
            Self::Tick => "tick",
        }
    }

    /// 字面から読む（既知の verb でなければ `None`）。
    pub fn parse(token: &str) -> Option<Self> {
        SEAT_COMMANDS.iter().copied().find(|command| command.as_str() == token)
    }
}

/// 値を取る seat の flag。**値欠け・空文字・重なりは閉包の断りにしない**: 本体の [`optional`] / [`nonempty`] が従来どおり
/// 使い方の rc 1 で断り、重なりは最初の 1 つを読む（設計 pipeline.md §14 約束 9・既存の歯の字面と rc は不変）。
const fn value(name: &'static str) -> Allowed {
    Allowed::maybe(name)
}

/// `seat register` が受ける flag（設計 pipeline.md §14 約束 5・[`SeatCommand`] の宣言順・usage の末尾の 3 つを含む）。
const ALLOWED_REGISTER: &[cli_args::Allowed] = &[
    value("--state-dir"),
    value("--target"),
    value("--role"),
    value("--account"),
    value("--launch"),
    value("--anchor"),
    value("--model"),
    value("--tmux-socket"),
    value("--capture-file"),
];
/// `seat launch`。
const ALLOWED_LAUNCH: &[cli_args::Allowed] = &[
    value("--state-dir"),
    value("--role"),
    value("--target"),
    value("--account"),
    value("--anchor"),
    value("--model"),
    value("--restore"),
    value("--tmux-socket"),
    value("--capture-file"),
];
/// `seat ruling`（`add` / `ls` の 2 語は positional・`ls` は `--state-dir` だけを読む）。
const ALLOWED_RULING: &[cli_args::Allowed] = &[
    value("--state-dir"),
    value("--target"),
    value("--words"),
    value("--bead"),
    value("--rule"),
];
/// `seat tick`（席の口で `--rules` を受けるのはこの口だけ・歯の seam＝行の写しを差し替える）。
const ALLOWED_TICK: &[cli_args::Allowed] = &[
    value("--state-dir"),
    value("--target"),
    value("--rules"),
    value("--tmux-socket"),
    value("--capture-file"),
];
/// `seat tick install` / `uninstall`（unit の導出と書き・設計 seat-heartbeat.md §3・pane を読まないので tmux の flag は受けない）。
const ALLOWED_TICK_UNIT: &[cli_args::Allowed] = &[
    value("--state-dir"),
    value("--target"),
    value("--unit-dir"),
    value("--binary"),
    value("--rules"),
];

/// [`SeatCommand`] が受ける flag の集合（[`dispatch`] が verb を選んだ直後に [`crate::cli_args::parse`] へ渡す・`rest` は verb の後ろ）。
fn allowed_of(command: SeatCommand, rest: &[String]) -> &'static [Allowed] {
    match command {
        SeatCommand::Register => ALLOWED_REGISTER,
        SeatCommand::Launch => ALLOWED_LAUNCH,
        SeatCommand::Ruling => ALLOWED_RULING,
        SeatCommand::Tick if unit_verb(rest).is_some() => ALLOWED_TICK_UNIT,
        SeatCommand::Tick => ALLOWED_TICK,
    }
}

/// `seat tick` の後ろの第 1 token が unit の動詞（`install` / `uninstall`）か。
fn unit_verb(rest: &[String]) -> Option<Verb> {
    rest.first().and_then(|token| Verb::parse(token))
}

/// `seat` に続く引数を捌く。既知の verb は選んだ直後に閉包の検査を 1 回撃つ（未知の flag と `--help` を typed に断る）。
pub fn dispatch(args: &[String]) -> Outcome {
    let first = args.first().map(String::as_str);
    let verb = first.and_then(SeatCommand::parse);
    if let Some(command) = verb {
        let rest = args.get(1..).unwrap_or_default();
        if let Err(error) = crate::cli_args::parse(rest, allowed_of(command, rest)) {
            return crate::cli_args::refusal("seat", &error, usage());
        }
    }
    match (verb, first) {
        (Some(SeatCommand::Register), _) => register_of(args),
        (Some(SeatCommand::Launch), _) => launch_of(args),
        (Some(SeatCommand::Ruling), _) => ruling_of(args.get(1..).unwrap_or_default()),
        (Some(SeatCommand::Tick), _) => tick_of(args.get(1..).unwrap_or_default()),
        // 既知の verb でなく `--` で始まらない第 1 token は口座 label（短い形・account-lifecycle.md §14）。label は閉じた語を
        // 持たない＝[`SeatCommand`] の subcommand ではないので閉包の検査の外（消えた口の名もこの腕で従来どおり断る）。
        (None, Some(label)) if !label.starts_with("--") && !label.trim().is_empty() => short_of(label, args.get(1..).unwrap_or_default()),
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

/// `seat ruling add|ls`（設計 fleet-event-log.md §9 (2)）。`rest` は `ruling` の後ろ。`--state-dir` は必須（空文字は使い方の誤り）。
/// `add` の `--words` は**空文字を使い方の誤りにしない**——空の逐語は typed な断り（`reason=empty-words`・rc 1・書かない）で名乗る。
fn ruling_of(rest: &[String]) -> Outcome {
    let (Some(verb), Ok(state_dir)) = (rest.first().map(String::as_str), required_nonempty(rest, "--state-dir")) else {
        return refused_usage();
    };
    match verb {
        "add" => ruling_add(rest, Path::new(state_dir)),
        "ls" => match super::ruling::ls(Path::new(state_dir)) {
            Ok(lines) => Outcome::ok(lines),
            Err(errors) => Outcome::failed(RC_BROKEN, errors.iter().map(ToString::to_string).collect()),
        },
        _ => refused_usage(),
    }
}

/// `seat tick`（設計 seat-heartbeat.md §2）: `--state-dir` と `S:W` の `--target` は必須・値欠けと空文字は使い方の誤り
/// （`--rules` も同じ）。rules は `rules` の口と同じ 1 本（`--rules` か埋め込み）で読み、判定は [`super::tick::run`] が持つ。
fn tick_of(args: &[String]) -> Outcome {
    if let Some(verb) = unit_verb(args) {
        return tick_unit_of(verb, args.get(1..).unwrap_or_default());
    }
    let [state_dir, target] = ["--state-dir", "--target"].map(|name| required_nonempty(args, name));
    let [socket, capture, rules] = ["--tmux-socket", "--capture-file", "--rules"].map(|name| nonempty(args, name));
    let (Ok(state_dir), Ok(target), Ok(socket), Ok(capture), Ok(_)) = (state_dir, target, socket, capture, rules) else {
        return refused_usage();
    };
    if !target_well_formed(target) {
        return refused_usage();
    }
    let flags = super::tick::Flags { state_dir, target, socket, capture };
    super::tick::run(&flags, crate::rules::cli::open(args))
}

/// `seat tick install|uninstall`（設計 seat-heartbeat.md §3）: `--state-dir` / `S:W` の `--target` / `--unit-dir` / `--binary` は必須で、
/// 値欠けと空文字は使い方の誤り（`--rules` も同じ）。撤去も同じ引数で導出し直して比べる＝どちらの口も同じ引数の形。
fn tick_unit_of(verb: Verb, args: &[String]) -> Outcome {
    let [state_dir, target, unit_dir, binary] = ["--state-dir", "--target", "--unit-dir", "--binary"].map(|name| required_nonempty(args, name));
    let (Ok(state_dir), Ok(target), Ok(unit_dir), Ok(binary), Ok(rules)) = (state_dir, target, unit_dir, binary, nonempty(args, "--rules")) else {
        return refused_usage();
    };
    if !target_well_formed(target) {
        return refused_usage();
    }
    let flags = super::tick::install::Flags { state_dir, target, unit_dir, binary, rules };
    super::tick::install::run(verb, &flags, crate::rules::cli::open(args))
}

/// `seat ruling add`: 対話面の席の逐語を `RulingReceived` 1 件として書き、ts（裁定 id）を stdout の 1 行で返す。
fn ruling_add(rest: &[String], state_dir: &Path) -> Outcome {
    let (Ok(target), Ok(Some(words)), Ok(bead), Ok(rule)) =
        (required_nonempty(rest, "--target"), optional(rest, "--words"), nonempty(rest, "--bead"), nonempty(rest, "--rule"))
    else {
        return refused_usage();
    };
    let draft = super::ruling::Draft { target, words, bead, rule };
    match super::ruling::add(state_dir, &draft) {
        Ok(event) => Outcome::ok_line(format!(
            "seat ruling: recorded ts={} target={target} bead={} rule={}",
            event.ts,
            bead.unwrap_or("-"),
            rule.unwrap_or("-")
        )),
        Err(err @ super::ruling::RulingRefusal::Store(_)) => Outcome::failed_line(RC_BROKEN, err.render(target)),
        Err(err) => Outcome::failed_line(RC_REFUSED, err.render(target)),
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
    /// 注入する起動行の末尾に足す会話の引き継ぎ（短い形の `-c` / `-r ID` だけ・[`short_carry_of`]・長い形は空）。
    carry: &'a [&'a str],
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
            carry: &[],
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

/// 短い形 `seat <label> [--orchestrator] [-c|-r ID] [--target S:W] [--model M] [--anchor DIR] [--restore CMD] [--state-dir S]`
/// （設計 account-lifecycle.md §14・seat-roles.md §26・SRS FR59 / FR40）: 役割の flag は**多くとも 1 つ**（0 個は既定の
/// orchestrator・2 つは使い方の誤り・[`short_role_of`]）・置き場と anchor は長い形と同じ解き方・target と model は明示の
/// flag が無ければ同じ鍵（役割 × anchor）の登録 row の値、target は更に呼び手の pane の session（[`short_defaults`]）。
/// 解けた周は長い形と同じ [`LaunchFlags`]（口座 = label）を組んで同じ [`launch_with`] を通る。
fn short_of(label: &str, args: &[String]) -> Outcome {
    let (Some(role), Some(carry)) = (short_role_of(args), short_carry_of(args)) else {
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
    let (target, model) = match short_defaults(&place, role, target, model, socket) {
        Ok(found) => found,
        Err(refused) => return refused,
    };
    let flags = LaunchFlags { target: &target, role, account: Some(label), model: Some(&model), restore, socket, carry: &carry };
    launch_with(&flags, &place)
}

/// 短い形の会話の引き継ぎ（設計 account-lifecycle.md §18）: `-c`（別名 `--continue`）は `["--continue"]`・`-r ID`（別名
/// `--resume ID`）は `["--resume", ID]`・どちらも無ければ空。**多くとも 1 つ**の周だけ `Some`: 両方・同じ flag の重複・値の無い
/// `-r`・会話 id の形（[`conversation_id`]）でない値は `None`＝使い方の誤り（注入する行は pane の shell が読むので、値を
/// 引用で包むのでなく字の集合を絞る）。
fn short_carry_of(args: &[String]) -> Option<Vec<&str>> {
    let at: Vec<usize> = args.iter().enumerate().filter(|(_, arg)| matches!(arg.as_str(), "-c" | "--continue" | "-r" | "--resume")).map(|(at, _)| at).collect();
    match at.as_slice() {
        [] => Some(Vec::new()),
        [at] => match args.get(*at).map(String::as_str) {
            Some("-c" | "--continue") => Some(vec!["--continue"]),
            _ => args.get(at.saturating_add(1)).map(String::as_str).filter(|id| conversation_id(id)).map(|id| vec!["--resume", id]),
        },
        _ => None,
    }
}

/// 会話 id の形（claude の session id・UUID の `8-4-4-4-12`・16 進小文字と `-` だけ・36 字）。
fn conversation_id(text: &str) -> bool {
    let groups: Vec<&str> = text.split('-').collect();
    groups.iter().map(|group| group.len()).eq([8, 4, 4, 4, 12])
        && groups.iter().all(|group| group.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')))
}

/// 短い形の役割の flag（`--orchestrator`＝[`role::Role`] の字面に `--` を前置した形）。**多くとも 1 つ**の周だけ `Some`:
/// 0 個は既定の [`role::Role::Orchestrator`]（席の入口の 1 語・設計 seat-roles.md §26 の約束 1）・1 つはその役割・
/// 2 つ以上（同じ flag の重複を含む）は `None`＝使い方の誤り（どれを採るか決まらない形は黙って選ばない）。
fn short_role_of(args: &[String]) -> Option<role::Role> {
    let mut roles = args.iter().filter_map(|arg| arg.strip_prefix("--").and_then(role::Role::parse));
    let role = roles.next().unwrap_or(role::Role::Orchestrator);
    roles.next().is_none().then_some(role)
}

/// 短い形の既定を **1 関数で導く**（§14・seat-roles.md §26 の約束 3 / 4）: 明示の flag は row の値に勝ち、無ければ同じ鍵
/// （役割 × anchor）の登録 row の `target` / `model`（[`role::registration_of_key`]）。両方が明示の周は log を読まない。
///
/// `target` は row でも解けなければ**呼び手の pane の session**（[`super::session_of_caller`]・`-t` を付けない 1 問い）と
/// **役割の字面**を `:` で繋いだ形にする（`seat <label>` の 1 語で打った窓の session に、役割の名の窓を開ける）。
/// 問いが撃てない周・session の名が空の周は解けないまま。`model` は明示も row も無ければ**役割の既定の行**から導く
/// （[`role::defaults`]・設計 seat-roles.md §20 の約束 4）＝`--model` が欠けるのは行を読めない周だけ。足りない名は宣言順
/// （`--target` → `--model`）で `missing=` に載せて `defaults-unresolved` で断る（1 key も送らず row も書かない）。log を読めない
/// 周は `log-unreadable`（「row が無い」と混ぜない・fail-closed）。
fn short_defaults(place: &LaunchPlace, role: role::Role, target: Option<&str>, model: Option<&str>, socket: Option<&str>) -> Result<(String, String), Outcome> {
    let row = match (target, model) {
        (Some(_), Some(_)) => None,
        _ => {
            let events = crate::fleet::store::read_all(&place.state.path)
                .map_err(|_| refused_before_launch(cycle::REASON_LOG_UNREADABLE, target, Some(&place.state)))?;
            let state = crate::fleet::replay(&events);
            role::registration_of_key(&state, role, &place.anchor.display().to_string()).cloned()
        }
    };
    let target = target
        .map(str::to_owned)
        .or_else(|| row.as_ref().map(|found| found.target.clone()))
        .or_else(|| super::session_of_caller(socket).map(|session| format!("{session}:{}", role.as_str())));
    let model = model
        .map(str::to_owned)
        .or_else(|| row.as_ref().and_then(|found| found.model.clone()))
        .or_else(|| role::defaults(role).ok().map(|found| found.model.display().to_owned()));
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
    // 役割の既定の行（model と effort）は埋め込み manifest から引く＝起動の口に `--rules` は作らない（設計 seat-roles.md §20 の約束 7）。
    let rules = match super::embedded_manifest() {
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
        rules: &rules,
        threshold_pct,
        carry: flags.carry,
        replace_own: true,
    });
    let line = cycle::render_launched(flags.target, &result, state);
    match result {
        cycle::Launched::Done(..) => Outcome::ok_line(line),
        cycle::Launched::None(_) | cycle::Launched::Refused(_) | cycle::Launched::Failed(_) => Outcome::failed_line(RC_REFUSED, line),
    }
}
