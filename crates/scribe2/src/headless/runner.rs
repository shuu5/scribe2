//! `<NAME> runner`（設計 §6・FR5）。契約を stdin で受け、`claude -p` を worktree で回す。
//!
//! 止める条件はただ 1 つ、**上限 record（`rate_limit_event`）の status が「止める側」の集合に
//! 属すること**である（ADR-0012 §2.1）。それ以外の失敗は claude の rc をそのまま写す——包みが
//! 独自の判定を足すと、呼出側は「誰が止めたか」を見失う。

use crate::polarity::{OnFailure, Polarity, Timing};
use super::{build, feed, fill, flag, need, read_stdin_bytes, Call, DEFAULT_CLAUDE, RC_RATE_LIMIT};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_REFUSED};
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
        "usage: {} runner --worktree D --write-set F --vessel F --plugin-dir D --permission-mode M [--account-dir D] [--claude PATH] < contract",
        crate::name::NAME
    )
}

/// `runner` を 1 回。契約本文は stdin から読む。
pub fn dispatch(args: &[String]) -> Outcome {
    let parsed = (|| {
        Ok::<_, String>((
            need(args, "--worktree")?.to_owned(),
            need(args, "--write-set")?.to_owned(),
            need(args, "--vessel")?.to_owned(),
            need(args, "--plugin-dir")?.to_owned(),
            need(args, "--permission-mode")?.to_owned(),
            flag(args, "--account-dir")?.map(str::to_owned),
            flag(args, "--claude")?.map(str::to_owned),
        ))
    })();
    let (worktree, write_set, vessel, plugin_dir, mode, account, claude) = match parsed {
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
    let tools = allowed_tools(granted.allowed());
    let contract = String::from_utf8_lossy(&read_stdin_bytes()).into_owned();
    if contract.trim().is_empty() {
        return refused("契約が stdin に無い".to_owned());
    }
    let listed = match std::fs::read_to_string(&write_set) {
        Ok(found) => found,
        Err(err) => return Outcome::failed_line(RC_BROKEN, format!("runner: write-set を読めない: {err}")),
    };
    let prompt = compose(&contract, &listed, granted.allowed());
    // **claude を起こす前に**残す（起きた後に書く形だと、席が止まらない周の prompt が読めない）。
    let unsaved = save_prompt(Path::new(&vessel), &prompt).err();
    let mut outcome = launch(&Call {
        claude: claude.as_deref().unwrap_or(DEFAULT_CLAUDE),
        prompt: &prompt,
        permission_mode: &mode,
        plugin_dir: Some(&plugin_dir),
        account_dir: account.as_deref(),
        cwd: Some(Path::new(&worktree)),
        // rate limit を**途中で**見るので逐次で受ける。
        streaming: true,
    },
    &tools);
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
fn launch(call: &Call<'_>, tools: &str) -> Outcome {
    let mut command = build(call);
    command.arg("--allowedTools").arg(tools);
    let spawned = command.spawn();
    let mut child = match spawned {
        Ok(found) => found,
        Err(err) => return Outcome::failed_line(RC_BROKEN, format!("runner: claude を起動できない: {err}")),
    };
    feed(&mut child, call.prompt);
    let mut seen = Watched::default();
    if let Some(out) = child.stdout.take() {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            seen.records = seen.records.saturating_add(1);
            // **最終 result の text を覚える**（質問 record の置き場・設計 pipeline-question.md §3）。
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
    conclude(child.wait(), &seen)
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
}

/// claude が終わった後の 1 行と rc を決める。
///
/// **rc は claude のものを写す**（包みが作り替えない）。例外は 1 つ、**正常終了の後**に最終
/// result の text の最終行が質問 record だった周だけで、同じ record を stdout の最終行に
/// **そのまま**写し [`RC_QUESTION`] で終える（観測行はその前・pipeline は最終行を読む）。
/// record が無い・読めない周は claude の rc（0）を写す（FailOpen・[`QUESTION_POLARITY`]）。
fn conclude(status: std::io::Result<ExitStatus>, seen: &Watched) -> Outcome {
    let observed = observed_suffix(seen.status.as_deref());
    if seen.limited {
        let reason = seen.status.as_deref().unwrap_or_default();
        return Outcome::failed_line(RC_RATE_LIMIT, stop_line(reason));
    }
    let found = match status {
        Err(err) => return Outcome::failed_line(RC_BROKEN, format!("runner: claude を待てない: {err}")),
        Ok(found) => found,
    };
    let rc = u8::try_from(found.code().unwrap_or(i32::from(RC_BROKEN))).unwrap_or(RC_BROKEN);
    let line = format!("runner: rc={rc} records={}{observed}", seen.records);
    if rc != 0 {
        // 正常終了でない周は最終行を読まない（質問ではなく claude の失敗）。
        return Outcome { out: vec![line], err: Vec::new(), rc };
    }
    match question_ending(seen.last_result.as_deref().unwrap_or_default()) {
        Ending::Question(record) => Outcome { out: vec![line, record], err: Vec::new(), rc: RC_QUESTION },
        Ending::Malformed(reason) => Outcome {
            out: vec![line],
            err: vec![format!("runner: 最終行は質問 record の形でない（{reason}）・claude の rc を写す")],
            rc,
        },
        Ending::Plain => Outcome::ok_line(line),
    }
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
    if !body.starts_with('{') || !has_pair(body, "type", "result") {
        return None;
    }
    let at = find_key(body, "result")?;
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
    format!("runner: rate limit の record を見たので止めた rate-limit-status={status}")
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
    use super::Scan;

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
