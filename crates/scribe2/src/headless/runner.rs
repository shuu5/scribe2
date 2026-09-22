//! `<NAME> runner`（設計 §6・FR5）。契約を stdin で受け、`claude -p` を worktree で回す。
//!
//! 止める条件はただ 1 つ、**上限 record（`rate_limit_event`）の status が「止める側」の集合に
//! 属すること**である（ADR-0012 §2.1）。それ以外の失敗は claude の rc をそのまま写す——包みが
//! 独自の判定を足すと、呼出側は「誰が止めたか」を見失う。
//!
//! **model は rules 行 `runner.model` から読み、claude に毎回渡す**（`s2-07l.297`・設計 pipeline.md §6）。
//! `--rules PATH` が在ればその manifest・無ければ埋め込み（lens と同じ読み口 [`super::rules_of`]）。
//! manifest から読むのは **model の 1 行だけ**で、allowlist と common-verify は従来どおり便の写し
//! （`--vessel`）から読む（ADR-0010 §2.4 は動かない）。行が解けない周は claude を呼ばず rc 2（lens の
//! cap と同じ極性・版の既定へ黙って倒れない）。
//!
//! **effort も同じ manifest の rules 行 `runner.effort` から読み、claude に毎回渡す**（`s2-07l.322`）。省くと
//! 口座の設定 dir の `settings.json` が決め、同じ便が口座ごとに違う深さで走る。読む順は model → effort
//! （先に落ちた理由 1 つだけを出す）。

use crate::polarity::{OnFailure, Polarity, Timing};
use super::{
    build, feed, fill, flag, need, plugin_dirs, read_stdin_bytes, rules_of, runner_effort, runner_model, Call, Effort,
    Format, DEFAULT_CLAUDE, RC_RATE_LIMIT, RC_UNREACHABLE,
};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_REFUSED};
use crate::fleet::select::Model;
use crate::fleet::Usage;
use crate::pipe::confine;
use crate::pipe::declaration::Effective;
use crate::pipe::gate::last_json_object;
use crate::pipe::RC_QUESTION;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::ExitStatus;
use std::str::CharIndices;

/// prompt の文面（tracked な template・絶対 path も口座名も含まない）。
const TEMPLATE: &str = include_str!("runner.txt");

/// 使い方の 1 行。
pub fn usage() -> String {
    format!(
        "usage: {} runner --worktree D --write-set F --vessel F --plugin-dir D --permission-mode M [--rules PATH] [--account-dir D] [--claude PATH] [--cgroup-root DIR] < contract",
        crate::name::NAME
    )
}

/// runner が rules 行から読む 2 つ: model（`runner.model`）と effort（`runner.effort`）。manifest は `--rules PATH`
/// が在ればそれ・無ければ埋め込み（[`rules_of`]・lens と同じ読み口）。
///
/// **model を先に読む**（model の 4 理由の字面は不変）。どちらも解けない周は先に落ちた理由 1 つだけが出る。
fn rows_of(args: &[String]) -> Result<(Model, Effort), String> {
    let manifest = rules_of(args)?;
    let model = runner_model(&manifest)?;
    let effort = runner_effort(&manifest)?;
    Ok((model, effort))
}

/// `runner` を 1 回。契約本文は stdin から読む。
pub fn dispatch(args: &[String]) -> Outcome {
    let parsed = (|| {
        // `--rules` の値欠けはここで断る（値は [`rules_of`] が後で読む）。
        flag(args, "--rules")?;
        Ok::<_, String>((
            need(args, "--worktree")?.to_owned(),
            need(args, "--write-set")?.to_owned(),
            need(args, "--vessel")?.to_owned(),
            need(args, "--plugin-dir")?.to_owned(),
            need(args, "--permission-mode")?.to_owned(),
            flag(args, "--account-dir")?.map(str::to_owned),
            flag(args, "--claude")?.map(str::to_owned),
            // cgroup の root（claude の scope の peak の置き場・設計 gate-cost.md §13）。省くと typed な既定
            // [`confine::CGROUP_ROOT`]・env は読まない（C2.2）。
            flag(args, "--cgroup-root")?.map(str::to_owned),
        ))
    })();
    let (worktree, write_set, vessel, plugin_dir, mode, account, claude, cgroup_root) = match parsed {
        Ok(found) => found,
        Err(reason) => return Outcome::failed(RC_REFUSED, vec![format!("runner: {reason}"), usage()]),
    };
    // **権限は便の写しから読む**（manifest も repo の宣言も読まない＝便の中で権限が
    // 動かない）。写しが読めない周は claude を起こさずに rc 2 で止める。
    let granted = match Effective::load(Path::new(&vessel)) {
        Ok(found) => found,
        Err(errors) => {
            let mut lines = vec!["runner: vessel の写しを読めない".to_owned()];
            lines.extend(errors.iter().map(ToString::to_string));
            return Outcome::failed(RC_BROKEN, lines);
        }
    };
    // **model と effort は manifest の 2 行だけ**（`--rules` か埋め込み・権限は写しのまま）。行が無い / 不発効 /
    // 文字列でない / 閉じた表に無い周は claude を起こさず rc 2（lens の cap と同じ極性）——版の既定や口座の
    // settings へ黙って倒すと、便が消費するモデル別窓と口座選定の窓がずれ、深さも口座ごとにばらばらになる。
    let (model, effort) = match rows_of(args) {
        Ok(found) => found,
        Err(reason) => return Outcome::failed_line(RC_BROKEN, format!("runner: {reason}")),
    };
    let tools = allowed_tools(granted.allowed());
    let contract = String::from_utf8_lossy(&read_stdin_bytes()).into_owned();
    if contract.trim().is_empty() {
        return refused("契約が stdin に無い".to_owned());
    }
    let listed = match std::fs::read_to_string(&write_set) {
        Ok(found) => found,
        Err(err) => return Outcome::failed_line(RC_BROKEN, format!("runner: write-set を読めない: {err}")),
    };
    // **plugin root は claude を起こす前に検査する**（設計 §6・FR20・憲法 C16.2）。pipe の spawn は
    // 器の plugin を root へ必ず書くので、読めない・配下の dir が 0 の周は root が壊れた印＝
    // guard 0 本の claude を起こさず rc 2 で落とす（fail-closed）。展開は [`build`] が同じ関数で行う。
    if let Err(reason) = plugin_dirs(Path::new(&plugin_dir)) {
        return Outcome::failed_line(RC_BROKEN, format!("runner: {reason}"));
    }
    let prompt = compose(&contract, &listed, granted.allowed());
    // **claude を起こす前に**残す（起きた後に書く形だと、席が止まらない周の prompt が読めない）。
    let unsaved = save_prompt(Path::new(&vessel), &prompt).err();
    let mut outcome = launch(&Call {
        claude: claude.as_deref().unwrap_or(DEFAULT_CLAUDE),
        prompt: &prompt,
        permission_mode: &mode,
        // rules 行の model と effort を**毎回**渡す（claude CLI の字面・lens と同じ 2 行）。
        model: Some(model.alias()),
        effort: Some(effort.alias()),
        plugin_dir: Some(&plugin_dir),
        account_dir: account.as_deref(),
        cwd: Some(Path::new(&worktree)),
        // rate limit を**途中で**見るので逐次で受ける。
        output: Format::StreamJson,
        max_turns: None,
    }, &tools, Path::new(cgroup_root.as_deref().unwrap_or(confine::CGROUP_ROOT)));
    if let Some(reason) = unsaved {
        outcome.err.push(format!("runner: prompt を残せない: {reason}"));
    }
    outcome
}

/// prompt を組む（template の 3 対を **1 走査**で埋める）。
///
/// 重ねて replace すると、先に埋めた契約本文や write-set の中の `{allowed}` / `{write_set}` が
/// 次の走査で展開され、外から来る text が prompt の構造へ触れられる。`{allowed}` は人が読む面の
/// allowlist（`--allowedTools` の `Bash(<cmd>:*)` 形は [`allowed_tools`] が別に組む）で、値の出所は
/// **便の写し**のまま＝manifest も対象 repo の宣言も読まない（ADR-0010 §2.4）。
fn compose(contract: &str, write_set: &str, allowed: &[String]) -> String {
    let listed_allowed = allowed
        .iter()
        .map(|command| format!("- {command}"))
        .collect::<Vec<String>>()
        .join("\n");
    fill(
        TEMPLATE,
        &[
            ("{contract}", contract.trim_end()),
            ("{write_set}", write_set.trim_end()),
            ("{allowed}", &listed_allowed),
        ],
    )
}

/// 残す prompt の file 名。置き場は **vessel の写しの隣**（= run dir・`<state_dir>/pipe/<run>/`）。
const PROMPT_FILE: &str = "prompt.txt";

/// runner が組んだ prompt を便の作業面へ残す（設計 §6・`s2-07l.79`）。
///
/// prompt は stdin で渡すので stream には 1 行も出ない。「何を渡したか」を後から読める口が
/// これで、置き場は写しの隣＝便ごとの run dir である（新しい flag も env も足さない・C2.2）。
/// **tracked な面〔worktree〕には置かない**（契約本文と write-set を PUBLIC 面へ出さない）——
/// 置き場が解けない写しでは cwd へ落とす代わりに残さない側へ倒す。
///
/// **判定の入力ではない**——残せない周も便は続き、rc は claude のものを写す。呼び手は
/// `Err` を stderr の 1 行にするだけで止めない（証跡の欠落で claude を起こさない形にしない）。
fn save_prompt(vessel: &Path, prompt: &str) -> Result<(), String> {
    // **置き場が解けない写し（`vessel.toml` のような裸の名）は残さない**。`Path::parent` は
    // 裸の名に `Some("")` を返すので、そのまま join すると prompt が runner の **cwd**（＝
    // pipeline では便の worktree・tracked 面）へ落ちる（lens 2026-09-11 M2 の実測）。
    let dir = vessel
        .parent()
        .filter(|found| !found.as_os_str().is_empty())
        .ok_or_else(|| format!("{} の置き場を解けない（写しは dir 付きの path で渡す）", vessel.display()))?;
    let path = dir.join(PROMPT_FILE);
    std::fs::write(&path, prompt).map_err(|err| format!("{} を書けない: {err}", path.display()))
}

/// claude を回し、rate limit を見たらその場で止める。
///
/// **権限を与えるのはここだけ**である（lens は判定を受け取るだけ）。settings を 1 つも
/// 読まない起動形（`--setting-sources` の空値 + `--strict-mcp-config`）は runner と lens に
/// 共通なので [`build`] が持ち、この口は**与える権限**だけを足す——器が与えた allow の外は
/// plugin の PermissionRequest hook が deny する（ADR-0011 §2.1 が ADR-0009 §2.1 / ADR-0010
/// §2.4 の起動 flag を部分 supersede・allowlist の形と hook の一律 deny は不変）。
///
/// **claude の scope の peak は走行中に sample する**（設計 gate-cost.md §13・`s2-07l.273`）: stream の
/// 行を読む各周で `memory.peak` を 1 回読み（[`confine::Sampler`]・待ちは足さない）、終端の `scope=` 行に
/// `claude_peak_bytes=` で写す。終端で読む形は、最後の process の終了で scope が消えた正常系を測れない。
fn launch(call: &Call<'_>, tools: &str, cgroup_root: &Path) -> Outcome {
    let (mut command, confinement) = build(call);
    command.arg("--allowedTools").arg(tools);
    let spawned = command.spawn();
    let mut child = match spawned {
        Ok(found) => found,
        Err(err) => return Outcome::failed_line(RC_BROKEN, format!("runner: claude を起動できない: {err}")),
    };
    feed(&mut child, call.prompt);
    let mut seen = Watched::default();
    let mut sampler = confine::Sampler::of(&confinement, cgroup_root);
    if let Some(out) = child.stdout.take() {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            sampler.sample();
            seen.records = seen.records.saturating_add(1);
            // **最終 result の text を覚える**（質問 record の置き場・設計 pipeline-question.md §3）。
            // 種別（`subtype`）と `is_error` も同じ record から読む——claude が失敗で終わった事実は
            // stream の中にしか無く、覚えないと rc≠0 の理由が事後に読めない（`s2-07l.258`）。
            if is_result_record(&line) {
                seen.result_seen = true;
                seen.result_kind = result_subtype(&line);
                seen.result_is_error = result_is_error(&line);
                // 消費の 6 値も同じ record から読む（設計 gate-cost.md §26 形 (2)・揃わない周は `None`）。
                seen.usage = result_usage(&line);
            }
            if let Some(text) = result_text(&line) {
                seen.last_result = Some(text);
            }
            match decide(&line, STOP_STATUSES) {
                Decision::Ignore => continue,
                Decision::Observed(status) => seen.status = Some(status),
                Decision::Stop(status) => {
                    seen.status = Some(status);
                    seen.limited = true;
                    // **待たずに殺す**。上限に当たった席を回し続けても、次の record は
                    // 同じ上限でしか無い。
                    let _ = child.kill();
                    break;
                }
            }
        }
    }
    let status = child.wait();
    // **終端で scope を片付ける**（設計 gate-cost.md §4.4 errata・`s2-07l.234`）。rc と最終行は変えない
    // （stdout は pipeline が読む面なので、結果は stderr の 1 行だけに出す）。
    let mut outcome = conclude(status, &seen);
    outcome.err.extend(scope_line("runner", &confinement, sampler.peak()));
    outcome
}

/// 包めた周の終端の stderr 1 行（`<who>: scope=<片付け> claude_peak_bytes=<n|-> orphans=<n|->`・設計
/// gate-cost.md §13・pipeline.md §20）。
///
/// **Confined の周は `gone` でも必ず出す**——[`confine::release_scope`] は `Gone` を `None` に落とすので
/// [`confine::release`] を直に撃つ（他の呼び手の filter と型は変えない）。包めなかった周は scope が無い＝
/// 行を出さない（`claude_peak_bytes` も `orphans` の語も出ない）。lens も同じ 1 本を使う。
///
/// **孤児は殺す前に数える**（[`confine::orphans_before_release`]・設計 pipeline.md §20 の約束 4）: 片付けの
/// 後の scope は空か dir ごと消えているので、後から読むと「何を殺したか」が 0 と `-` に化ける。数えるのは
/// この口だけで、gate の verify 行ごとの片付け（[`confine::release_scope`]）は従来どおり `kill` 1 本である。
pub(super) fn scope_line(who: &str, confinement: &confine::Confinement, peak: confine::Peak) -> Option<String> {
    match confinement {
        confine::Confinement::Confined { unit } => {
            let orphans = confine::orphans_before_release(unit);
            let released = confine::release(unit);
            Some(scope_words(who, released, peak, orphans))
        }
        confine::Confinement::Unconfined(_) => None,
    }
}

/// 終端の 1 行の字面（pure・in-file の歯が fixture の値で測る・設計 pipeline.md §20 の約束 5 / 6）。
///
/// 3 つの値はどれも**測れなかった周を `-`** で書き、0 と融合しない（C10）——`orphans=0` は「片付けが
/// 1 本も殺さなかった」を測れた周で、`orphans=-` は「数えられなかった」周である。
fn scope_words(who: &str, released: confine::Released, peak: confine::Peak, orphans: confine::Orphans) -> String {
    format!(
        "{who}: scope={} claude_peak_bytes={} orphans={}",
        released.as_str(),
        peak.word(),
        orphans.word()
    )
}

/// stream を読みながら覚えたもの。
#[derive(Debug, Default)]
struct Watched {
    /// 読んだ record の数。
    records: usize,
    /// **観測した上限 status**（ADR-0012 §2.1 末尾）。集合を**実測で育てる**唯一の口で、
    /// これが無いと集合は永久に空のままになる。**判定の入力ではない**ので、残せない周も便は続く。
    status: Option<String>,
    /// 止める側の status を見たか。
    limited: bool,
    /// 最後に見た `result` record の text（質問 record はこの最終行に来る）。
    last_result: Option<String>,
    /// `result` record を 1 つでも見たか（見ていない周は観測行を出さない＝「無い」を `-` に化けさせない）。
    result_seen: bool,
    /// 最後に見た `result` record の種別（`subtype`・key が無い周は `None`）。
    result_kind: Option<ResultKind>,
    /// 最後に見た `result` record の `is_error`（key が無い・bool でない周は `None`）。
    result_is_error: Option<bool>,
    /// 最後に見た `result` record の消費の 6 値（[`result_usage`]・揃わない周は `None`＝要約行に載せない）。
    usage: Option<Usage>,
}

/// claude が終わった後の 1 行と rc を決める。
///
/// **rc は claude のものを写す**（包みが作り替えない）。例外は 1 つ、**正常終了の後**に最終
/// result の text の最終行が質問 record だった周だけで、同じ record を stdout の最終行に
/// **そのまま**写し [`RC_QUESTION`] で終える（観測行はその前・pipeline は最終行を読む）。
/// record が無い・読めない周は claude の rc（0）を写す（FailOpen・[`QUESTION_POLARITY`]）。
/// もう 1 つは最終 `result` record が API に届かず止まったと言う周（[`unreachable`]）で、claude の rc に依らず
/// 停止行（[`halt_line`]）を最終行に足して [`RC_UNREACHABLE`] で終える（設計 account-autonomy.md §17）。
///
/// 要約行の**前**に、最終 `result` record の観測行（[`result_line`]）を 1 本出す（`s2-07l.258`）。
/// rc≠0 の周も同じ——stream は捨てられるので、これが無いと claude が `is_error` で終わった理由は
/// run dir にも残らない（憲法 C11.2 / NFR4）。`result` record を見ていない周は行を出さない。
/// **最終行は変えない**（pipeline は最終行だけを読む）。
fn conclude(status: std::io::Result<ExitStatus>, seen: &Watched) -> Outcome {
    let observed = observed_suffix(seen.status.as_deref());
    if seen.limited {
        return limited(seen.status.as_deref().unwrap_or_default());
    }
    let found = match status {
        Err(err) => return Outcome::failed_line(RC_BROKEN, format!("runner: claude を待てない: {err}")),
        Ok(found) => found,
    };
    let rc = u8::try_from(found.code().unwrap_or(i32::from(RC_BROKEN))).unwrap_or(RC_BROKEN);
    let mut out: Vec<String> = Vec::new();
    if seen.result_seen {
        out.push(result_line(seen.result_kind, seen.result_is_error, seen.last_result.as_deref()));
    }
    out.push(summary_line(rc, seen.records, &observed, seen.usage.as_ref()));
    // **API に届かず止まった周は rc を作り替える**（設計 account-autonomy.md §17 (1)）。pipe は rc だけで段を分ける
    // ので、claude の rc（1）を写すと便が `Failed` に倒れ成果が捨てられる。弁別は [`unreachable`] の 1 本（pure）。
    if unreachable(seen.result_is_error, seen.last_result.as_deref()) {
        out.push(halt_line(seen.last_result.as_deref().unwrap_or_default()));
        return Outcome { out, err: Vec::new(), rc: RC_UNREACHABLE };
    }
    if rc != 0 {
        // 正常終了でない周は最終行を読まない（質問ではなく claude の失敗）。
        return Outcome { out, err: Vec::new(), rc };
    }
    match question_ending(seen.last_result.as_deref().unwrap_or_default()) {
        Ending::Question(record) => {
            out.push(record);
            Outcome { out, err: Vec::new(), rc: RC_QUESTION }
        }
        Ending::Malformed(reason) => Outcome {
            out,
            err: vec![format!("runner: 最終行は質問 record の形でない（{reason}）・claude の rc を写す")],
            rc,
        },
        Ending::Plain => Outcome { out, err: Vec::new(), rc },
    }
}

/// 最終 `result` record の種別（`subtype` の値・**閉じた enum**）。
///
/// 未知の値は [`ResultKind::Unknown`] 1 つに潰し、字面は残さない（観測行の語彙を stream の
/// 語彙に開かない）。採れている値は実 claude の `subtype` から（`success` / `error_max_turns` /
/// `error_during_execution`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultKind {
    /// 正常に終わった。
    Success,
    /// turn 数の上限で終わった。
    ErrorMaxTurns,
    /// 実行中の失敗で終わった。
    ErrorDuringExecution,
    /// 一覧に無い値。
    Unknown,
}

impl ResultKind {
    /// `subtype` の値から（純関数・一覧に無い値は [`ResultKind::Unknown`]）。
    pub fn parse(subtype: &str) -> Self {
        match subtype {
            "success" => Self::Success,
            "error_max_turns" => Self::ErrorMaxTurns,
            "error_during_execution" => Self::ErrorDuringExecution,
            _ => Self::Unknown,
        }
    }

    /// 観測行に載せる字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::ErrorMaxTurns => "error_max_turns",
            Self::ErrorDuringExecution => "error_during_execution",
            Self::Unknown => "unknown",
        }
    }
}

/// 観測行に載せる `result` の text の上限（**字数**・`s2-07l.258`）。
///
/// 観測行の幅であって user 裁定を要する値ではない（rules 行にしない・変えたい周は契約で）。
pub const RESULT_TEXT_CHARS: usize = 400;

/// 最終 `result` record の観測行（純関数・要約行の前に出す）。
///
/// `text` は先頭 [`RESULT_TEXT_CHARS`] 字で切り（`…` は付けない・字数で数える）、改行と tab は空白に
/// する（1 行に収める）。無いものは `-`（呼び手は record を見ていない周にこの行を出さない）。
pub fn result_line(kind: Option<ResultKind>, is_error: Option<bool>, text: Option<&str>) -> String {
    let subtype = kind.map_or("-", ResultKind::as_str);
    let is_error = match is_error {
        Some(true) => "true",
        Some(false) => "false",
        None => "-",
    };
    let text = text.map_or_else(|| "-".to_owned(), |found| one_line(found, RESULT_TEXT_CHARS));
    format!("runner: result subtype={subtype} is_error={is_error} text={text}")
}

/// 本文の先頭 `chars` 字を 1 行に収める（改行と tab は空白・`…` は付けない・観測行と停止行が共有する）。
fn one_line(text: &str, chars: usize) -> String {
    text.chars()
        .take(chars)
        .map(|ch| if matches!(ch, '\n' | '\r' | '\t') { ' ' } else { ch })
        .collect()
}

/// 許す command を `--allowedTools` の 1 本へ組む。
///
/// 値は**便ごとに凍結した写し**から来る＝実装が対象 repo の宣言を書き換えても、走っている
/// 便の権限は変わらない（ADR-0010 §2.4）。
fn allowed_tools(commands: &[String]) -> String {
    commands
        .iter()
        .map(|command| format!("Bash({command}:*)"))
        .collect::<Vec<String>>()
        .join(",")
}

/// **上限を表す status の閉じた集合**（[ADR-0012] §2.1）。
///
/// **実測で採れた値だけ**を入れる（MUST）。記録時点で採れている status は `allowed_warning`
/// （＝「許可されつつ警告」の周で、止める側ではない）1 つだけなので、**この集合は空**である。
/// ゆえに器は当面 rc 75 を一度も立てない——これは ADR の**決定**であって実装の手抜きではない。
///
/// 逆（許可側を列挙して未知を上限へ倒す）を採らない理由は**失敗の重さの非対称**である:
/// rc 75 は判定の名札ではなく**実行の中断**（下の `launch` は待たずに kill する）。取りこぼした
/// 周は「rc 75 という分類が付かない」だけで便は claude 自身の rc で止まるが、誤って上限と読んだ
/// 周は**健全な便が殺される**（ADR-0012 §4 案 (A') の却下理由）。
///
/// 止まる周の status を採取したら、その値をここへ入れる。**推測で足さない**（MUST NOT）。
///
/// [ADR-0012]: ../../../design-intent/decisions/ADR-0012-rate-limit-detection-reads-dedicated-record.html
const STOP_STATUSES: &[&str] = &[];

/// 上限 record の種別（`type` field の値）。
const EVENT_KIND: &str = "rate_limit_event";

/// 構造化された上限情報の key。
const INFO_KEY: &str = "rate_limit_info";

/// 判定の入力になる field の key。
const STATUS_KEY: &str = "status";

/// status が**止める側**か（純関数）。
///
/// 集合が空である以上、production の経路は**止まる側を一度も通らない**。判定をここへ切り出す
/// のは、そうしないと**上限で止まる分岐に歯が 1 本も当たらない**からである（歯は非空の集合を
/// 渡して両向きに測る）。
pub fn stops_on(status: &str, stop_statuses: &[&str]) -> bool {
    stop_statuses.contains(&status)
}

/// この境界の極性（[`Decision::Stop`]）: 上限 record を読んだその場で便を止めるが、**集合に無い status・record が無い周は止めない**（ADR-0012 §2.1「未知は claude の rc へ落ちる」）＝FailOpen。一覧はこれを隠さない。
/// 記録時点の止める側の集合（[`STOP_STATUSES`]）は**空**で production はこの guard を通らない（歯は非空の集合を渡して
/// 両向きに測る）——一覧の in-loop 件数に載るのは**型の事実**であり、実 run で止めた回数ではない。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailOpen,
};

/// 1 行に対する判定（純関数の返り値）。
///
/// **3 値**にするのは、`launch` の loop を薄くして**判定そのものを歯で測れるようにする**ため
/// である。集合が空である以上 production は [`Decision::Stop`] を通らないので、分岐が loop の中に
/// 埋まっていると**上限で止まる側に歯が 1 本も当たらない**（lens 2026-09-11 H1）。
#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    /// 上限 record ではない（何もしない）。
    Ignore,
    /// 上限 record だが**止める側ではない** status（記録だけする）。
    Observed(String),
    /// 止める側の status（rc 75 で中断する）。
    Stop(String),
}

/// 1 行を判定する（純関数）。
pub fn decide(line: &str, stop_statuses: &[&str]) -> Decision {
    let Some(status) = rate_limit_status(line) else {
        return Decision::Ignore;
    };
    if stops_on(status, stop_statuses) {
        return Decision::Stop(status.to_owned());
    }
    Decision::Observed(status.to_owned())
}

/// 最終 result の text の**終わり方**（純関数の 3 値・設計 pipeline-question.md §3）。
///
/// [`Decision`] と同型に切り出すのは、production の loop を薄くして**判定そのものを歯で両向きに
/// 測る**ためである。
#[derive(Debug, PartialEq, Eq)]
pub enum Ending {
    /// 最終行に JSON の object が無い（普通の終わり方・claude の rc を写す）。
    Plain,
    /// JSON らしい最終行が在るが質問 record として読めない（壊れた JSON・`question` が空・
    /// 文字列でない・複数行）＝claude の rc を写し、理由を stderr に 1 行（FailOpen を隠さない）。
    Malformed(String),
    /// 質問 record（`question` 必須非空 1 行・`about` 任意）。中身は**最終行そのもの**（写す用）。
    Question(String),
}

/// この境界の極性（[`Ending`]）: claude が正常に終わった後、最終行の質問 record を読んだその場で便を
/// `Questioned` へ倒す（rc [`RC_QUESTION`]・InLoop）。**record が無い・読めない周は止めず claude の rc へ
/// 落とす**（ADR-0012 §2.1 と同じ「未知は claude の rc へ」＝FailOpen・一覧はこれを隠さない）。
pub const QUESTION_POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailOpen,
};

/// 最終 result の text の最終行を判定する（純関数）。
///
/// 置き場と parse は lens の verdict と**同じ 1 本**（[`last_json_object`]・最終の `{` で始まる行を
/// 1 つの flat object に読む）。`question` key を持たない object は record ではない（[`Ending::Plain`]）。
pub fn question_ending(result: &str) -> Ending {
    let Some(line) = result.lines().rev().find(|line| line.trim_start().starts_with('{')) else {
        return Ending::Plain;
    };
    let pairs = match last_json_object(line) {
        Ok(found) => found,
        Err(reason) => return Ending::Malformed(reason),
    };
    let Some((_, value)) = pairs.iter().find(|(key, _)| key == "question") else {
        return Ending::Plain;
    };
    match value.as_str() {
        Some(question) if question.trim().is_empty() => Ending::Malformed("question が空である".to_owned()),
        Some(question) if question.contains('\n') => Ending::Malformed("question が 1 行でない".to_owned()),
        Some(_) => Ending::Question(line.trim().to_owned()),
        None => Ending::Malformed("question が文字列でない".to_owned()),
    }
}

/// stream-json の 1 行が `result` record なら、その `result`（text）を escape を解いて返す。
///
/// record は入れ子（`usage` 等）を持つので flat parser は使えない。種別は `"type":"result"` の対で見て、
/// `"result"` key の**直後**の文字列 1 つだけを読む（[`find_key`] は `:` を伴う key だけを引くので、
/// `"type":"result"` の値の字面には当たらない）。
pub fn result_text(line: &str) -> Option<String> {
    let body = line.trim_start();
    if !is_result_record(body) {
        return None;
    }
    top_level_string(body, "result")
}

/// stream-json の 1 行が `result` record か（top-level の `"type":"result"` の対で見る）。
pub fn is_result_record(line: &str) -> bool {
    let body = line.trim_start();
    body.starts_with('{') && has_pair(body, "type", "result")
}

/// `result` record の種別（top-level の `subtype`）。record でない・key が無い周は `None`。
pub fn result_subtype(line: &str) -> Option<ResultKind> {
    let body = line.trim_start();
    if !is_result_record(body) {
        return None;
    }
    top_level_string(body, "subtype").map(|subtype| ResultKind::parse(&subtype))
}

/// `result` record の `is_error`（top-level の bool）。record でない・key が無い・bool でない周は `None`。
pub fn result_is_error(line: &str) -> Option<bool> {
    let body = line.trim_start();
    if !is_result_record(body) {
        return None;
    }
    top_level_bool(body, "is_error")
}

/// 要約行の頭（[`summary_line`] が書き、[`summary_usage`] が読む・pipe の spawn は要約行をこの頭で見分ける）。
const SUMMARY_HEAD: &str = "runner: rc=";

/// 要約行（`runner: rc=<rc> records=<n>[ rate-limit-status=<s>][ usage=… turns=<n> wall_ms=<n>]`）。
///
/// 消費の 3 語（[`Usage::words`]）は 6 値が揃った周だけ末尾に足す（設計 gate-cost.md §26 形 (2)）——揃わない周は
/// 語ごと欠く（0 と書かない・C10）。rc は変えない。
fn summary_line(rc: u8, records: usize, observed: &str, usage: Option<&Usage>) -> String {
    let words = usage.map(|found| format!(" {}", found.words())).unwrap_or_default();
    format!("{SUMMARY_HEAD}{rc} records={records}{observed}{words}")
}

/// runner の stdout から最後の要約行の消費の 6 値を読む（[`summary_line`] の対・pipe の spawn が消費の event の
/// 材料にする）。要約行が無い・3 語のどれかが欠けるか数でない周は `None`。
pub fn summary_usage(stdout: &str) -> Option<Usage> {
    let line = stdout.lines().rev().find(|line| line.starts_with(SUMMARY_HEAD))?;
    Usage::from_words(line)
}

/// `result` record の消費の 6 値（`usage` の直下の token 4 値と top-level の `num_turns` / `duration_ms`）。
///
/// `usage` は入れ子の object なので flat parser は使わない: [`find_key`] の深さ guard で `usage` の**直下**だけを読む
/// （`usage.iterations[]` の中の同名の数に釣られない）。record でない・6 値の**どれか 1 つでも**欠けるか非負整数で
/// ない周は `None`＝値を組まない（欠けを 0 に倒すと「測って 0」と「読めなかった」が潰れる・C10）。
pub fn result_usage(line: &str) -> Option<Usage> {
    let body = line.trim_start();
    if !is_result_record(body) {
        return None;
    }
    let inner = immediate_object(body.get(find_key(body, "usage")?..)?)?;
    Some(Usage {
        input: top_level_number(inner, "input_tokens")?,
        output: top_level_number(inner, "output_tokens")?,
        cache_read: top_level_number(inner, "cache_read_input_tokens")?,
        cache_create: top_level_number(inner, "cache_creation_input_tokens")?,
        turns: top_level_number(body, "num_turns")?,
        wall_ms: top_level_number(body, "duration_ms")?,
    })
}

/// JSON object 1 つの **top-level の** `key` の非負整数を読む（[`find_key`] の深さ guard）。数字の列の直後が区切り
/// （`,` `}` 空白・末尾）でない周（小数・指数・文字列・負数・`null`）と u64 に収まらない周は `None`。
fn top_level_number(body: &str, key: &str) -> Option<u64> {
    let value = body.get(find_key(body, key)?..)?.trim_start();
    let rest = value.trim_start_matches(|ch: char| ch.is_ascii_digit());
    let number = value.get(..value.len().saturating_sub(rest.len()))?;
    match rest.chars().next() {
        None => number.parse().ok(),
        Some(next) if next == ',' || next == '}' || next.is_whitespace() => number.parse().ok(),
        Some(_) => None,
    }
}

/// JSON object 1 つが **top-level に** `key` を持つか（[`find_key`] の深さ guard・lens が判定 object へ消費の
/// field を足す前に、同名の key を二重にしないために見る）。
pub(crate) fn has_top_level_key(body: &str, key: &str) -> bool {
    find_key(body, key).is_some()
}

/// JSON object 1 つの **top-level の** `key` の bool 値を読む（入れ子の同名 key は読まない・
/// [`find_key`] の深さ guard）。`true` / `false` のどちらでもない周は `None`（parser を足さない）。
fn top_level_bool(body: &str, key: &str) -> Option<bool> {
    let at = find_key(body, key)?;
    let value = body.get(at..)?.trim_start();
    let (found, rest) = match value.strip_prefix("true") {
        Some(rest) => (true, rest),
        None => (false, value.strip_prefix("false")?),
    };
    // 語の直後は区切り（`,` `}` 空白）でなければ bool ではない（`trueish` のような字面を読まない）。
    match rest.chars().next() {
        None => Some(found),
        Some(next) if next == ',' || next == '}' || next.is_whitespace() => Some(found),
        Some(_) => None,
    }
}

/// JSON object 1 つの **top-level の** `key` の文字列値を escape を解いて読む（入れ子の同名 key は
/// 読まない・[`find_key`] の深さ guard）。値が文字列でない・key が無い周は `None`。
///
/// 読み手は [`result_text`] と、consumer の plugin.json の `name` を読む `pipe/spawn.rs` の 2 つ
/// （JSON の読み手を 2 本に増やさない）。
pub(crate) fn top_level_string(body: &str, key: &str) -> Option<String> {
    let at = find_key(body, key)?;
    json_string(body.get(at..)?)
}

/// colon の後ろの JSON 文字列 1 つを escape を解いて読む（`\uXXXX` の代理対は結合する）。
fn json_string(after_colon: &str) -> Option<String> {
    let mut chars = after_colon.trim_start().strip_prefix('"')?.chars();
    let mut out = String::new();
    let mut pending_high: Option<u32> = None;
    loop {
        let ch = chars.next()?;
        let decoded = match ch {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                'b' => '\u{0008}',
                'f' => '\u{000c}',
                'u' => {
                    let hex: String = chars.by_ref().take(4).collect();
                    let unit = u32::from_str_radix(&hex, 16).ok()?;
                    match (pending_high.take(), unit) {
                        (Some(high), 0xDC00..=0xDFFF) => {
                            char::from_u32(0x10000 + ((high - 0xD800) << 10) + (unit - 0xDC00))?
                        }
                        (None, 0xD800..=0xDBFF) => {
                            pending_high = Some(unit);
                            continue;
                        }
                        (_, unit) => char::from_u32(unit)?,
                    }
                }
                other => other,
            },
            other => other,
        };
        out.push(decoded);
    }
}

/// 上限で止めた周の 1 行（記録面と同じ形で status を載せる）。
pub fn stop_line(status: &str) -> String {
    format!("{STOP_PREFIX}{status}")
}

/// 停止行の status の前までの字面（[`stop_line`] と [`stop_status`] が共有する）。
const STOP_PREFIX: &str = "runner: rate limit の record を見たので止めた rate-limit-status=";

/// 上限で止めた周の結果: 停止行を **stdout** の 1 行に出し rc [`RC_RATE_LIMIT`]。
///
/// pipe は runner の stdout だけを捕らえる（`pipe/spawn.rs`）ので、stderr に出すと段の記帳へ
/// status が届かない（設計 account-autonomy.md §2）。
pub fn limited(status: &str) -> Outcome {
    Outcome { out: vec![stop_line(status)], err: Vec::new(), rc: RC_RATE_LIMIT }
}

/// **API に届かなかったことを表す語の閉じた集合**（設計 account-autonomy.md §17 (1)・宣言順）。
///
/// 到達不能を表す typed な record は claude の stream に無い（上限は `rate_limit_event` の status で来るが、
/// ネットの断は `result` の本文にしか現れない）ので、本文の語で読む。記録時点の語は実測の本文
/// `API Error: Can't reach the API server (EAI_AGAIN)`（host のネット断 2026-09-14）と、同じ層の errno 2 つ。
/// **集合は claude の文面に追随する下界**で、見逃した周は従来どおり `Failed`（fail-safe 側）。語を足すのはここ 1 か所。
pub const UNREACHABLE_WORDS: &[&str] = &["Can't reach the API server", "EAI_AGAIN", "ENETUNREACH", "ECONNREFUSED"];

/// 最終 `result` record が「API に届かず止まった」を言うか（純関数）。
///
/// `is_error` が `true` の record の本文が [`UNREACHABLE_WORDS`] のどれかを含む周**だけ**が真である。
/// `is_error` が `false` / 読めない周は、本文に同じ語が在っても読まない（実装の説明文や tool の出力に語が
/// 混じった正常終了を、到達不能に化けさせない）。
pub fn unreachable(is_error: Option<bool>, text: Option<&str>) -> bool {
    is_error == Some(true) && text.is_some_and(|body| UNREACHABLE_WORDS.iter().any(|word| body.contains(word)))
}

/// 到達不能の停止行に載せる本文の先頭の字数。
const HALT_TEXT_CHARS: usize = 200;

/// API に届かず止めた周の 1 行（`runner: halt reason=unreachable text=<本文の先頭>`・[`stop_line`] と同じ stdout の面）。
pub fn halt_line(text: &str) -> String {
    format!("runner: halt reason=unreachable text={}", one_line(text, HALT_TEXT_CHARS))
}

/// 停止行（[`stop_line`]）から `rate-limit-status=` の値を読む（純関数・[`stop_line`] の対）。
///
/// 読むのは**停止行そのもの**だけで、観測の後置き（`runner: rc=… rate-limit-status=…`）は
/// 停止行ではないので `None`。値が空の行も `None`（呼び手は `unknown` と記す）。
pub fn stop_status(line: &str) -> Option<&str> {
    line.trim_end()
        .strip_prefix(STOP_PREFIX)
        .filter(|status| !status.is_empty())
}

/// 観測を記録面へ載せる後置き（観測していない周は空）。
pub fn observed_suffix(status: Option<&str>) -> String {
    status
        .map(|value| format!(" rate-limit-status={value}"))
        .unwrap_or_default()
}

/// stream-json の 1 行から **`rate_limit_event` の `rate_limit_info.status`** を読む。
///
/// 上限の真の合図は**専用の record 種別と構造化された status** で来る（実 run の raw stream から
/// 採取・ADR-0012 §1）。本文の語彙を探す形はこの便で撤去した——探していたのは合図ではなく
/// 「同じ言葉が混じった別の文」で、識別子の 16 進や tool の出力で 2 度誤爆した。
///
/// 読むのは **`rate_limit_info` の直下の `status`** だけである。`rate_limit_info` は `unifiedWindows` の
/// ような入れ子を持つので、深さを見ずに最初の `"status"` を採る形だと **key の並び次第で別の object の値を
/// 読む**（lens 2026-09-11 H3）。直下の判定は [`find_key`] の深さ guard が担い、切り出し（[`immediate_object`]）は
/// 対応する閉じ brace までを渡す（`s2-07l.126`・入れ子で打ち切る形は key の並び次第で直下の status を取り逃した）。
///
/// key と colon の間・colon と値の間の**空白に寛容**である。実 stream は compact だが（実測）、
/// 表記が変わっただけで記録の口が無音で止まる形にはしない（lens 2026-09-11 H2）。
///
/// 種別と `rate_limit_info` は **top-level の key** で見る（[`find_key`]・s2-07l.123）ので、別の record が
/// 上限 record を入れ子で引用した周は読まない。ADR-0012 §2.2 が撤去したのは本文の語彙走査（その一部と
/// して在った深さ 1 の種別読み）であり、専用 record の種別を marker で見る §2.1 の経路は不変である。
pub fn rate_limit_status(line: &str) -> Option<&str> {
    let body = line.trim_start();
    if !body.starts_with('{') || !has_pair(body, "type", EVENT_KIND) {
        return None;
    }
    let at = find_key(body, INFO_KEY)?;
    let rest = body.get(at..)?;
    let inner = immediate_object(rest)?;
    let status_at = find_key(inner, STATUS_KEY)?;
    let value = inner.get(status_at..)?;
    quoted_value(value)
}

/// `"<key>"` の**直後**（空白と colon を跨いだ先）の位置を返す。
///
/// **同じ深さの key だけ**を見る（s2-07l.123）: 入力が `{` で始まる object ならその直下、
/// [`immediate_object`] が剥がした中身なら先頭の深さである。文字列の中（escape を含む）と
/// 入れ子の object / array の中は**跨ぐ**——実 claude（2.1.268）の `result` record は
/// `usage.iterations[]` の入れ子に `"type":"message"` を持ち、top-level の `"type":"result"` は
/// その後ろに来るので、最初に見える `"type"` を種別と読むと record を取り逃す（.116 実 run）。
fn find_key(body: &str, key: &str) -> Option<usize> {
    let needle = format!("\"{key}\"");
    let target = usize::from(body.trim_start().starts_with('{'));
    let mut scan = Scan::default();
    let mut chars = body.char_indices();
    while let Some((at, ch)) = scan.next_structural(&mut chars) {
        match ch {
            '{' | '[' => scan.depth = scan.depth.saturating_add(1),
            '}' | ']' => scan.depth = scan.depth.saturating_sub(1),
            // 文字列の開き（`next_structural` が `in_string` を立てた直後）＝key の候補。
            // catch-all にしない——原始が返す種類が増えた周に、key でない位置を key の
            // 候補として読む形へ黙って広がる。
            '"' if scan.depth == target => {
                if let Some(after) = key_end(body, at, &needle) {
                    return Some(after);
                }
            }
            _ => {}
        }
    }
    None
}

/// JSON 1 行の走査状態（文字列の中か・escape の直後か・入れ子の深さ）。
#[derive(Default)]
struct Scan {
    in_string: bool,
    escaped: bool,
    depth: usize,
}

impl Scan {
    /// **文字列の外**の次の構造文字（`{` `}` `[` `]` と文字列の開き `"`）を返す。
    ///
    /// 文字列の中（escape された `\"` を跨ぐ）の brace や引用符は構造ではないので返さない
    /// ——`"note":"win {5h}"` の brace で深さを動かすと切り出しが手前で終わる（`s2-07l.126`）。
    /// 文字列の開きを返すときは `in_string` を立てて返す＝呼び手は key の候補として位置だけを
    /// 見ればよく、走査**状態**を自分で持たない（lens-126 MED-1・深さは用途ごとに違うので呼び手）。
    fn next_structural(&mut self, it: &mut CharIndices) -> Option<(usize, char)> {
        for (at, ch) in it.by_ref() {
            if self.skip(ch) {
                continue;
            }
            match ch {
                '{' | '}' | '[' | ']' => return Some((at, ch)),
                '"' => {
                    self.in_string = true;
                    return Some((at, ch));
                }
                _ => {}
            }
        }
        None
    }

    /// 文字列の中の 1 文字を読み飛ばす（true）。文字列の外なら false を返し呼び手が構造を読む。
    fn skip(&mut self, ch: char) -> bool {
        if !self.in_string {
            return false;
        }
        if self.escaped {
            self.escaped = false;
        } else if ch == '\\' {
            self.escaped = true;
        } else if ch == '"' {
            self.in_string = false;
        }
        true
    }
}

/// `at` から `"<key>"` が始まり、その後ろ（空白を跨いで）に `:` が在れば colon の直後の位置を返す。
fn key_end(body: &str, at: usize, needle: &str) -> Option<usize> {
    let rest = body.get(at..)?.strip_prefix(needle)?;
    let value = rest.trim_start().strip_prefix(':')?;
    Some(body.len().saturating_sub(value.len()))
}

/// `"<key>": "<value>"` の対が在るか（空白に寛容）。
fn has_pair(body: &str, key: &str, value: &str) -> bool {
    find_key(body, key)
        .and_then(|at| body.get(at..))
        .and_then(quoted_value)
        .is_some_and(|found| found == value)
}

/// colon の後ろの `"..."` の中身（escape は考えない＝status は識別子である）。
fn quoted_value(after_colon: &str) -> Option<&str> {
    let opened = after_colon.trim_start().strip_prefix('"')?;
    opened.split_once('"').map(|(value, _)| value)
}

/// colon の後ろの `{ ... }` の中身＝**対応する閉じ brace まで**（`s2-07l.126`・planner 裁定 = 案 P）。
///
/// 境界は**文字列の外**で深さ 0 に戻る `}` である。文字列の中の brace（`"note":"win {5h}"`）や、status より前の
/// 入れ子 object で切ると status の手前で終わって上限 record を見逃す（fail-open の向き・`.123` の
/// `usage.iterations[]` と同型）。「直下の key だけを読む」は切り出しではなく [`find_key`] の深さ guard が
/// 担う（入れ子の `status` は深さ 1 以上ゆえ読まれない）。文字列と escape の読み飛ばしは [`find_key`] と同じ
/// [`Scan::next_structural`] に乗せ、走査**状態**を 2 つ持たない（loop の骨格は 2 本在るが、文字列の外の
/// 構造文字を返す原始は 1 本・lens-126 MED-1）。閉じ brace が無い（壊れた行）周は末尾までを返し、読めるかは
/// 下流が決める。
fn immediate_object(after_colon: &str) -> Option<&str> {
    let opened = after_colon.trim_start().strip_prefix('{')?;
    let mut scan = Scan::default();
    let mut end = opened.len();
    let mut chars = opened.char_indices();
    while let Some((at, ch)) = scan.next_structural(&mut chars) {
        match ch {
            '{' => scan.depth = scan.depth.saturating_add(1),
            '}' if scan.depth == 0 => {
                end = at;
                break;
            }
            '}' => scan.depth = scan.depth.saturating_sub(1),
            // `[` `]` と文字列の開きは境界を動かさない（深さは brace だけで数える）。
            _ => {}
        }
    }
    opened.get(..end)
}

/// 前提違反（rc 1 + stderr 1 行・何もしない）。
fn refused(reason: String) -> Outcome {
    Outcome::failed_line(RC_REFUSED, format!("runner: {reason}"))
}

#[cfg(test)]
mod tests {
    use super::{
        conclude, halt_line, observed_suffix, result_usage, scope_words, stop_line, stop_status, summary_line, summary_usage,
        unreachable, Scan, Watched, UNREACHABLE_WORDS,
    };
    use crate::cli_args::RC_USAGE;
    use crate::cli_outcome::{RC_BROKEN, RC_OK, RC_REFUSED};
    use crate::fleet::Usage;
    use crate::headless::{RC_RATE_LIMIT, RC_UNREACHABLE};
    use crate::pipe::approve::RC_BLOCKED;
    use crate::pipe::confine::{Orphans, Peak, Released};
    use crate::pipe::gate::RC_INCONCLUSIVE;
    use crate::pipe::RC_QUESTION;
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;

    /// 実測の本文（host のネット断 2026-09-14 の runner の `result`）。
    const OBSERVED: &str = "API Error: Can't reach the API server (EAI_AGAIN)";

    /// 弁別は `is_error=true` の本文が集合の語を**どれか 1 つ**含む周だけ（語ごとに 1 本ずつ測る・宣言順）。
    #[test]
    fn pipe_unreachable_reads_each_word_of_the_set_on_error_results() {
        assert!(unreachable(Some(true), Some(OBSERVED)), "実測の本文");
        for word in UNREACHABLE_WORDS {
            assert!(unreachable(Some(true), Some(&format!("API Error: {word}"))), "語 {word} を含む error の本文");
        }
        assert_eq!(UNREACHABLE_WORDS.len(), 4, "集合は閉じた 4 語（足すのは設計 §17 の 1 か所）");
    }

    /// **pure**: `is_error=false` / 読めない周は語が在っても読まない。集合に無い error の本文も読まない。
    #[test]
    fn pipe_unreachable_is_pure_on_is_error_and_the_set() {
        assert!(!unreachable(Some(false), Some(OBSERVED)), "is_error=false の本文に語が在っても弁別しない");
        assert!(!unreachable(None, Some(OBSERVED)), "is_error を読めない周も弁別しない");
        assert!(!unreachable(Some(true), Some("API Error: 500 Internal server error")), "集合に無い error");
        assert!(!unreachable(Some(true), None), "本文の無い error");
        assert!(!unreachable(Some(true), Some("can't reach the api server")), "語は字面のまま照合する（大小を畳まない）");
    }

    /// 新しい rc は既存の rc のどれとも衝突しない（母集団 = 器が名乗る rc の定数の全部）。
    #[test]
    fn pipe_unreachable_rc_does_not_collide_with_existing_rcs() {
        let existing = [RC_OK, RC_REFUSED, RC_BROKEN, RC_USAGE, RC_BLOCKED, RC_INCONCLUSIVE, RC_RATE_LIMIT, RC_QUESTION];
        assert!(!existing.contains(&RC_UNREACHABLE), "rc {RC_UNREACHABLE} は既存 {existing:?} と別");
        assert_eq!(RC_UNREACHABLE, 77);
    }

    /// 最終 `result` を見た周の [`Watched`]（claude の rc は呼び手が渡す）。
    fn watched(is_error: bool, text: &str) -> Watched {
        Watched {
            records: 2,
            result_seen: true,
            result_is_error: Some(is_error),
            last_result: Some(text.to_owned()),
            ..Watched::default()
        }
    }

    /// claude が rc 1 で終わり最終 `result` が到達不能を言う周は rc [`RC_UNREACHABLE`]・最終行が停止行。
    /// 同じ本文で `is_error=false` の周と、集合に無い error の周は claude の rc（1）を写す（従来どおり）。
    #[test]
    fn pipe_unreachable_conclude_rewrites_only_the_unreachable_rc() {
        let failed = || Ok(ExitStatus::from_raw(1 << 8));
        let halted = conclude(failed(), &watched(true, OBSERVED));
        assert_eq!(halted.rc, RC_UNREACHABLE);
        assert_eq!(halted.out.last().map(String::as_str), Some(halt_line(OBSERVED).as_str()), "{:?}", halted.out);
        assert!(halted.out.iter().any(|line| line.starts_with("runner: rc=1 ")), "要約行は claude の rc: {:?}", halted.out);
        assert_eq!(conclude(failed(), &watched(false, OBSERVED)).rc, 1, "is_error=false は写す");
        assert_eq!(conclude(failed(), &watched(true, "API Error: 500")).rc, 1, "集合に無い error は写す");
    }

    /// 停止行は本文の先頭を 1 行に収める（改行は空白）。
    #[test]
    fn pipe_unreachable_halt_line_carries_the_head_of_the_text() {
        assert_eq!(
            halt_line("API Error: Can't reach the API server\n(EAI_AGAIN)"),
            "runner: halt reason=unreachable text=API Error: Can't reach the API server (EAI_AGAIN)"
        );
        let long = "x".repeat(500);
        assert_eq!(halt_line(&long), format!("runner: halt reason=unreachable text={}", "x".repeat(200)));
    }

    /// 実 claude の result record と同じ形の 1 行（`usage` の中で `iterations[]` が token 4 値より**前**に在り、その中にも
    /// 同名の key が違う数で在る・`modelUsage` も入れ子で持つ）。`turns` / `wall` は top-level の 2 値の字面そのまま。
    fn record(turns: &str, wall: &str) -> String {
        format!(
            "{{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"duration_ms\":{wall},\"duration_api_ms\":9,\
             {turns}\"result\":\"done\",\"total_cost_usd\":0.25,\
             \"usage\":{{\"iterations\":[{{\"type\":\"message\",\"input_tokens\":901,\"output_tokens\":902}}],\
             \"input_tokens\":11,\"cache_creation_input_tokens\":44,\"cache_read_input_tokens\":33,\"output_tokens\":22,\
             \"server_tool_use\":{{\"web_search_requests\":0}},\"service_tier\":\"standard\"}},\
             \"modelUsage\":{{\"opus\":{{\"inputTokens\":7,\"outputTokens\":8}}}}}}"
        )
    }

    /// 6 値の揃った record から読む値（fixture の数は互いに違う＝取り違えは等号で落ちる）。
    const WANT: Usage = Usage { input: 11, output: 22, cache_read: 33, cache_create: 44, turns: 5, wall_ms: 6000 };

    /// (c) result record の 1 行から usage 4 値と turns / wall_ms を読む: `usage` の直下だけを読み、`usage.iterations[]`
    /// の中の数（901 / 902）と `modelUsage` の数に釣られない（設計 gate-cost.md §26 歯 (c)）。
    #[test]
    fn run_cost_result_usage_reads_the_immediate_usage_and_top_level_turns() {
        assert_eq!(result_usage(&record("\"num_turns\":5,", "6000")), Some(WANT));
    }

    /// `usage` を持たない result record と、result でない record（同じ `usage` を持つ assistant の record）は `None`。
    #[test]
    fn run_cost_result_usage_is_none_without_usage_or_outside_the_result_record() {
        let bare = "{\"type\":\"result\",\"subtype\":\"success\",\"num_turns\":5,\"duration_ms\":6000,\"result\":\"x\"}";
        assert_eq!(result_usage(bare), None, "usage が無い record");
        let assistant = record("\"num_turns\":5,", "6000").replacen("\"type\":\"result\"", "\"type\":\"assistant\"", 1);
        assert_eq!(result_usage(&assistant), None, "result でない record");
    }

    /// (g) 部分欠けと数でない値: `num_turns` だけ欠く周・`duration_ms` が小数 / 文字列の周は `None`（0 を作らない）。
    /// 同じ歯の中で 6 値揃い＝`Some` を対に並べる（欠けを 0 に倒す実装は欠けの側も `Some` になって落ちる）。
    #[test]
    fn run_cost_result_usage_partial_or_non_number_builds_nothing() {
        assert_eq!(result_usage(&record("\"num_turns\":5,", "6000")), Some(WANT), "6 値揃い＝値が在る");
        assert_eq!(result_usage(&record("", "6000")), None, "num_turns だけ欠く");
        assert_eq!(result_usage(&record("\"num_turns\":5,", "60.5")), None, "duration_ms が小数");
        assert_eq!(result_usage(&record("\"num_turns\":5,", "\"6000\"")), None, "duration_ms が文字列");
        assert_eq!(result_usage(&record("\"num_turns\":null,", "6000")), None, "num_turns が null");
    }

    /// 要約行は 6 値が揃った周だけ `usage=` の 3 語を末尾に運び、[`summary_usage`] が同じ値へ読み戻す。揃わない周は
    /// 語ごと欠き（従来の字面のまま）・読み手は `None`。
    #[test]
    fn run_cost_summary_line_carries_usage_only_when_present() {
        let with = summary_line(0, 3, " rate-limit-status=allowed_warning", Some(&WANT));
        assert_eq!(
            with,
            "runner: rc=0 records=3 rate-limit-status=allowed_warning \
             usage=in:11,out:22,cache_read:33,cache_create:44 turns=5 wall_ms=6000"
        );
        assert_eq!(summary_usage(&format!("runner: result subtype=success is_error=false text=x\n{with}\n")), Some(WANT));
        let without = summary_line(0, 3, "", None);
        assert_eq!(without, "runner: rc=0 records=3", "揃わない周は従来の字面");
        assert_eq!(summary_usage(&without), None);
        assert_eq!(summary_usage("runner: rc=0 records=3 usage=in:1,out:2,cache_read:3,cache_create:4 turns=5"), None);
    }

    /// 停止行の読み手は [`stop_line`] と往復し、観測行と空行は停止行として読まない。
    #[test]
    fn stop_status_reads_back_the_stop_line_only() {
        assert_eq!(stop_status(&stop_line("allowed_warning")), Some("allowed_warning"));
        let observed = format!("runner: rc=0 records=3{}", observed_suffix(Some("allowed_warning")));
        assert_eq!(stop_status(&observed), None, "観測の後置きは停止行ではない: {observed}");
        assert_eq!(stop_status(""), None, "空行は停止行ではない");
        assert_eq!(stop_status(&stop_line("")), None, "値が空の停止行は読めない側");
    }

    /// 片付けが殺した数は **0 も数として書く**（設計 pipeline.md §20 の約束 5）。
    ///
    /// 語の位置は `claude_peak_bytes` の後ろで、既存の 2 つの値は動かさない（fixture の 3 つの数は
    /// 互いに違う＝値の取り違えが両方の assert を通らない）。
    #[test]
    fn runner_orphans_zero_is_written_as_the_counted_number() {
        assert_eq!(
            scope_words("runner", Released::Killed, Peak::Bytes(4096), Orphans::Count(0)),
            "runner: scope=killed claude_peak_bytes=4096 orphans=0",
            "1 本も殺さなかった周も 0 を書く"
        );
        assert_eq!(
            scope_words("lens", Released::Gone, Peak::Bytes(4096), Orphans::Count(3)),
            "lens: scope=gone claude_peak_bytes=4096 orphans=3",
            "殺した本数をそのまま書く（lens も同じ 1 本）"
        );
    }

    /// 数えられなかった周は `-`（設計 pipeline.md §20 の約束 5・C10）＝**0 と融合しない**。
    ///
    /// 同じ片付け・同じ peak の 2 本を並べて、`Unreadable` を 0 に倒す変異が**字面の差**で落ちることまで
    /// 測る（片側だけを見る歯は、`-` を `0` と書く実装でも緑になる）。
    #[test]
    fn runner_orphans_unreadable_is_written_as_a_dash() {
        let unreadable = scope_words("runner", Released::Killed, Peak::Unreadable, Orphans::Unreadable);
        assert_eq!(unreadable, "runner: scope=killed claude_peak_bytes=- orphans=-", "読めない周は -");
        let zero = scope_words("runner", Released::Killed, Peak::Unreadable, Orphans::Count(0));
        assert_ne!(unreadable, zero, "「0 本」と「測れなかった」を同じ字面にしない: {zero}");
        assert!(zero.ends_with(" orphans=0"), "0 の側は数で終わる: {zero}");
    }

    /// 文字列の外の構造文字だけを、位置ごと順に集める。
    fn structurals(body: &str) -> Vec<(usize, char)> {
        let mut scan = Scan::default();
        let mut chars = body.char_indices();
        let mut seen = Vec::new();
        while let Some(found) = scan.next_structural(&mut chars) {
            seen.push(found);
        }
        seen
    }

    /// 文字列の中の brace は（escape された引用符を跨いでも）構造文字ではない。
    /// 返るのは開き `"` と、文字列の外の `{` `}` `[` `]` だけである。
    #[test]
    fn scan_next_structural_skips_braces_inside_strings() {
        // 文字列の中に `{` `}` と escape された `"` を持つ 1 行（期待する位置は
        // **この字面のまま**数えた値である＝空白を足すと index がずれる）。
        let body = r#"{"a{b\"c}":[1]}"#;
        assert_eq!(
            structurals(body),
            [(0, '{'), (1, '"'), (11, '['), (13, ']'), (14, '}')]
        );
    }
}
